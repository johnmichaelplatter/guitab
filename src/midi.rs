use crate::mnx::MnxDocument;
use crate::tuning_table::FretboardFrequencyTable;
use crate::validate::duration_to_quarter_beats;
use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MidiOptions {
    pub pitch_bend_range: u8,
    pub string_to_channel: HashMap<u8, u4>,
    pub default_bpm: f64,
    pub ppq: u15,
    pub start_event_idx: Option<usize>,
    pub is_mpe: bool,
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            pitch_bend_range: 2,
            string_to_channel: HashMap::new(), // Populated dynamically if empty
            default_bpm: 120.0,
            ppq: u15::from(480),
            start_event_idx: None,
            is_mpe: false,
        }
    }
}

impl MidiOptions {
    pub fn mpe_default() -> Self {
        Self {
            pitch_bend_range: 48,
            string_to_channel: HashMap::new(),
            default_bpm: 120.0,
            ppq: u15::from(480),
            start_event_idx: None,
            is_mpe: true,
        }
    }
}

/// Helper to calculate 14-bit MIDI pitch bend value from semitone difference and pitch bend range
pub fn calculate_pitch_bend(semitone_diff: f32, pb_range: u8) -> u16 {
    let range = pb_range.max(1) as f32;
    (8192.0 + (semitone_diff / range) * 8192.0)
        .round()
        .clamp(0.0, 16383.0) as u16
}

/// Send MPE Configuration Message (MCM) on Master Channel 1 (0-indexed channel 0)
pub fn send_mpe_configuration(out_conn: &mut midir::MidiOutputConnection, member_channels: u8) {
    let messages = [
        [0xB0, 101, 0],               // RPN MSB: 0
        [0xB0, 100, 6],               // RPN LSB: 6 (MPE Configuration)
        [0xB0, 6, member_channels],   // Data Entry MSB: number of member channels
        [0xB0, 38, 0],                // Data Entry LSB: 0
        [0xB0, 101, 127],             // Null RPN MSB
        [0xB0, 100, 127],             // Null RPN LSB
    ];
    for msg in messages.iter() {
        let _ = out_conn.send(msg);
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackEvent {
    pub delta_ticks: u32,
    pub kind: PlaybackEventKind,
}

#[derive(Debug, Clone)]
pub enum PlaybackEventKind {
    Tempo(u32),
    Midi { channel: u4, message: MidiMessage },
    EventMarker(usize),
}

#[derive(Debug, Clone)]
struct InternalAbsoluteEvent {
    time_ticks: u32,
    kind: InternalEventKind,
}

#[derive(Debug, Clone)]
enum InternalEventKind {
    Tempo(u32),
    Midi { channel: u4, message: MidiMessage },
    EventMarker(usize),
}

pub fn generate_playback_track(
    doc: &MnxDocument,
    mut options: MidiOptions,
) -> (Vec<PlaybackEvent>, f64) {
    let ppq = u16::from(options.ppq) as f64;
    let mut abs_events: Vec<InternalAbsoluteEvent> = Vec::new();

    // Determine BPM from MNX or fallback
    let mut current_bpm = options.default_bpm;
    if !doc.global.measures.is_empty() {
        if let Some(first_measure) = doc.global.measures.first() {
            for dir in &first_measure.directions {
                if let Some(tempo) = &dir.tempo {
                    current_bpm = tempo.bpm;
                }
            }
        }
    }

    // Add tempo event at tick 0
    let micros_per_beat = (60_000_000.0 / current_bpm).round() as u32;
    abs_events.push(InternalAbsoluteEvent {
        time_ticks: 0,
        kind: InternalEventKind::Tempo(micros_per_beat),
    });

    if let Some(part) = doc.parts.first() {
        let num_strings = part.num_strings() as u8;

        // Setup channel mappings for strings if not explicitly provided
        if options.string_to_channel.is_empty() {
            for s in 1..=num_strings {
                if options.is_mpe {
                    let ch = ((s - 1) % 15 + 1) as u8;
                    options.string_to_channel.insert(s, u4::from(ch));
                } else {
                    let ch = (s - 1) % 16;
                    let ch = if ch == 9 { (ch + 1) % 16 } else { ch };
                    options.string_to_channel.insert(s, u4::from(ch));
                }
            }
        }

        // If MPE mode is enabled, emit MPE Configuration Message on Master Channel 1 (0-indexed 0)
        if options.is_mpe {
            let member_channels = num_strings.min(15);
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(101), value: u7::from(0) },
                },
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(100), value: u7::from(6) },
                },
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(6), value: u7::from(member_channels) },
                },
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(38), value: u7::from(0) },
                },
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(101), value: u7::from(127) },
                },
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi {
                    channel: u4::from(0),
                    message: MidiMessage::Controller { controller: u7::from(100), value: u7::from(127) },
                },
            });
        }

        // Setup pitch bend ranges (RPN 0) for all used string member channels
        for &ch in options.string_to_channel.values() {
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(101), value: u7::from(0) } }
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(100), value: u7::from(0) } }
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(6), value: u7::from(options.pitch_bend_range.min(127)) } }
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(38), value: u7::from(0) } }
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(101), value: u7::from(127) } }
            });
            abs_events.push(InternalAbsoluteEvent {
                time_ticks: 0,
                kind: InternalEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(100), value: u7::from(127) } }
            });
        }

        let freq_table = FretboardFrequencyTable::from_part_default(part);
        let mut current_time_ticks = 0;
        let mut current_event_idx = 0;
        let start_event = options.start_event_idx.unwrap_or(0);

        // Find time tick offset of start_event so playback from selected note starts immediately
        let mut start_tick_offset = 0;
        'find_offset: for measure in &part.measures {
            for seq in &measure.sequences {
                let mut seq_time = current_time_ticks;
                for event in &seq.content {
                    if current_event_idx == start_event {
                        start_tick_offset = seq_time;
                        break 'find_offset;
                    }
                    let beats = duration_to_quarter_beats(&event.duration);
                    let duration_ticks = (beats * ppq).round() as u32;
                    seq_time += duration_ticks;
                    current_event_idx += 1;
                }
                current_time_ticks = seq_time;
            }
        }

        current_time_ticks = 0;
        current_event_idx = 0;

        for measure in &part.measures {
            for seq in &measure.sequences {
                let mut seq_time_ticks = current_time_ticks;

                for event in &seq.content {
                    let beats = duration_to_quarter_beats(&event.duration);
                    let duration_ticks = (beats * ppq).round() as u32;

                    if current_event_idx >= start_event {
                        let event_start_ticks = seq_time_ticks.saturating_sub(start_tick_offset);

                        // Emit score event marker for synchronization with the UI
                        abs_events.push(InternalAbsoluteEvent {
                            time_ticks: event_start_ticks,
                            kind: InternalEventKind::EventMarker(current_event_idx),
                        });

                        for note in &event.notes {
                            if let (Some(string), Some(fret)) = (note.string, note.fret) {
                                let default_ch = if options.is_mpe {
                                    u4::from(((string - 1) % 15 + 1) as u8)
                                } else {
                                    u4::from((string - 1) % 16)
                                };
                                let channel = options.string_to_channel.get(&string).copied().unwrap_or(default_ch);

                                if let Some(pos_state) = freq_table.get(string, fret) {
                                    let midi_note_frac = pos_state.midi_note_number;
                                    let base_note = midi_note_frac.round() as u8;
                                    let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;

                                    let pitch_bend_val = calculate_pitch_bend(semitone_diff, options.pitch_bend_range);

                                    // Pitch Bend
                                    abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: event_start_ticks,
                                        kind: InternalEventKind::Midi {
                                            channel,
                                            message: MidiMessage::PitchBend {
                                                bend: midly::PitchBend(midly::num::u14::from(pitch_bend_val)),
                                            },
                                        },
                                    });

                                    // Note On
                                    abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: event_start_ticks,
                                        kind: InternalEventKind::Midi {
                                            channel,
                                            message: MidiMessage::NoteOn {
                                                key: u7::from(base_note.min(127)),
                                                vel: u7::from(100),
                                            },
                                        },
                                    });

                                    // Note Off
                                    abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: event_start_ticks + duration_ticks,
                                        kind: InternalEventKind::Midi {
                                            channel,
                                            message: MidiMessage::NoteOff {
                                                key: u7::from(base_note.min(127)),
                                                vel: u7::from(64),
                                            },
                                        },
                                    });
                                }
                            }
                        }
                    }

                    seq_time_ticks += duration_ticks;
                    current_event_idx += 1;
                }
                current_time_ticks = seq_time_ticks;
            }
        }
    }

    // Sort by absolute time
    abs_events.sort_by_key(|e| e.time_ticks);

    // Convert to relative delta times
    let mut playback_events = Vec::new();
    let mut last_time = 0;
    for abs_e in abs_events {
        let delta = abs_e.time_ticks - last_time;
        last_time = abs_e.time_ticks;
        let kind = match abs_e.kind {
            InternalEventKind::Tempo(t) => PlaybackEventKind::Tempo(t),
            InternalEventKind::Midi { channel, message } => PlaybackEventKind::Midi { channel, message },
            InternalEventKind::EventMarker(idx) => PlaybackEventKind::EventMarker(idx),
        };
        playback_events.push(PlaybackEvent {
            delta_ticks: delta,
            kind,
        });
    }

    (playback_events, ppq)
}

pub fn generate_midi<'a>(doc: &MnxDocument, options: MidiOptions) -> Smf<'a> {
    let ppq = options.ppq;
    let (playback_events, _) = generate_playback_track(doc, options);

    let header = Header::new(Format::SingleTrack, Timing::Metrical(ppq));
    let mut smf = Smf::new(header);
    let mut track = Vec::new();

    let mut accumulated_delta = 0;
    for ev in playback_events {
        accumulated_delta += ev.delta_ticks;
        match ev.kind {
            PlaybackEventKind::Tempo(t) => {
                track.push(TrackEvent {
                    delta: u28::from_int_lossy(accumulated_delta),
                    kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(t))),
                });
                accumulated_delta = 0;
            }
            PlaybackEventKind::Midi { channel, message } => {
                track.push(TrackEvent {
                    delta: u28::from_int_lossy(accumulated_delta),
                    kind: TrackEventKind::Midi { channel, message },
                });
                accumulated_delta = 0;
            }
            PlaybackEventKind::EventMarker(_) => {
                // Internal to player sequencer; omit from standard SMF
            }
        }
    }

    // End of Track
    track.push(TrackEvent {
        delta: u28::from_int_lossy(accumulated_delta),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    smf.tracks.push(track);
    smf
}

/// Helper to serialize a midly MidiMessage into standard MIDI byte arrays for midir
pub fn serialize_midi_message(channel: u4, message: &MidiMessage) -> Vec<u8> {
    let status_nibble = match message {
        MidiMessage::NoteOff { .. } => 0x80,
        MidiMessage::NoteOn { .. } => 0x90,
        MidiMessage::Aftertouch { .. } => 0xA0,
        MidiMessage::Controller { .. } => 0xB0,
        MidiMessage::ProgramChange { .. } => 0xC0,
        MidiMessage::ChannelAftertouch { .. } => 0xD0,
        MidiMessage::PitchBend { .. } => 0xE0,
    };
    let status_byte = status_nibble | channel.as_int();

    match message {
        MidiMessage::NoteOn { key, vel } | MidiMessage::NoteOff { key, vel } => {
            vec![status_byte, key.as_int(), vel.as_int()]
        }
        MidiMessage::Controller { controller, value } => {
            vec![status_byte, controller.as_int(), value.as_int()]
        }
        MidiMessage::ProgramChange { program } => {
            vec![status_byte, program.as_int()]
        }
        MidiMessage::PitchBend { bend } => {
            let val = bend.0.as_int();
            let lsb = (val & 0x7F) as u8;
            let msb = ((val >> 7) & 0x7F) as u8;
            vec![status_byte, lsb, msb]
        }
        MidiMessage::Aftertouch { key, vel } => {
            vec![status_byte, key.as_int(), vel.as_int()]
        }
        MidiMessage::ChannelAftertouch { vel } => {
            vec![status_byte, vel.as_int()]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPortInfo {
    pub id: usize,
    pub name: String,
}

pub fn get_midi_output_ports() -> Vec<MidiPortInfo> {
    match midir::MidiOutput::new("mnx_tab_cli_probe") {
        Ok(out) => {
            let mut ports = Vec::new();
            for (idx, p) in out.ports().iter().enumerate() {
                let name = out.port_name(p).unwrap_or_else(|_| format!("Port {}", idx));
                ports.push(MidiPortInfo { id: idx, name });
            }
            ports
        }
        Err(_) => Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub struct NotePreview {
    pub channel: midly::num::u4,
    pub key: midly::num::u7,
    pub pitch_bend: Option<u16>,
    pub velocity: midly::num::u7,
}

pub struct MidiPlayer {
    stop_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    current_event: std::sync::Arc<std::sync::atomic::AtomicI64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MidiPlayer {
    pub fn new() -> Self {
        Self {
            stop_signal: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            current_event: std::sync::Arc::new(std::sync::atomic::AtomicI64::new(-1)),
            handle: None,
        }
    }

    pub fn is_playing(&self) -> bool {
        if let Some(h) = &self.handle {
            if h.is_finished() {
                false
            } else {
                !self.stop_signal.load(std::sync::atomic::Ordering::Relaxed)
            }
        } else {
            false
        }
    }

    pub fn current_event_idx(&self) -> Option<usize> {
        if !self.is_playing() {
            return None;
        }
        let val = self.current_event.load(std::sync::atomic::Ordering::Relaxed);
        if val >= 0 {
            Some(val as usize)
        } else {
            None
        }
    }

    pub fn stop(&mut self) {
        self.stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);
        self.current_event.store(-1, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn play(
        &mut self,
        doc: &MnxDocument,
        options: MidiOptions,
        port_index: usize,
    ) -> Result<(), String> {
        self.stop();

        let (track, ppq) = generate_playback_track(doc, options);

        let midi_out = midir::MidiOutput::new("mnx_tab_cli_player")
            .map_err(|e| format!("Failed to create MidiOutput: {}", e))?;
        let ports = midi_out.ports();
        let port = ports.get(port_index).ok_or_else(|| format!("Selected MIDI port index {} not found", port_index))?;

        let mut conn_out = midi_out.connect(port, "mnx_playback")
            .map_err(|e| format!("Failed to connect to MIDI port: {}", e))?;

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_signal = stop_flag.clone();

        let current_event_tracker = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(-1));
        self.current_event = current_event_tracker.clone();

        let handle = std::thread::spawn(move || {
            let mut tempo_micros_per_beat = 500_000.0_f64;

            for event in track {
                if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                // Sleep for delta time
                let delta_ticks = event.delta_ticks as f64;
                if delta_ticks > 0.0 {
                    let micros = delta_ticks * (tempo_micros_per_beat / ppq);
                    let sleep_dur = std::time::Duration::from_micros(micros.round() as u64);

                    // Sleep in small chunks of 5ms to stay responsive to stop requests
                    let start_sleep = std::time::Instant::now();
                    while start_sleep.elapsed() < sleep_dur {
                        if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let rem = sleep_dur.saturating_sub(start_sleep.elapsed());
                        std::thread::sleep(rem.min(std::time::Duration::from_millis(5)));
                    }

                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }

                match event.kind {
                    PlaybackEventKind::Tempo(t) => {
                        tempo_micros_per_beat = t as f64;
                    }
                    PlaybackEventKind::Midi { channel, message } => {
                        let bytes = serialize_midi_message(channel, &message);
                        if !bytes.is_empty() {
                            let _ = conn_out.send(&bytes);
                        }
                    }
                    PlaybackEventKind::EventMarker(idx) => {
                        current_event_tracker.store(idx as i64, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }

            // Send All Notes Off (CC 123) and Reset All Controllers (CC 121) to all 16 channels on stop/finish
            for ch in 0..16 {
                let status_cc = 0xB0 | ch;
                let _ = conn_out.send(&[status_cc, 123, 0]); // All notes off
                let _ = conn_out.send(&[status_cc, 121, 0]); // Reset controllers
            }

            current_event_tracker.store(-1, std::sync::atomic::Ordering::Relaxed);
        });

        self.handle = Some(handle);
        Ok(())
    }


    pub fn play_preview(
        &mut self,
        notes: Vec<NotePreview>,
        port_index: usize,
        duration_ms: u64,
        is_mpe: bool,
        member_channels: u8,
        pb_range: u8,
    ) -> Result<(), String> {
        self.stop();

        if notes.is_empty() {
            return Ok(());
        }

        let midi_out = midir::MidiOutput::new("mnx_tab_cli_preview")
            .map_err(|e| format!("Failed to create MidiOutput: {}", e))?;
        let ports = midi_out.ports();
        let port = ports.get(port_index).ok_or_else(|| format!("Selected MIDI port index {} not found", port_index))?;

        let mut conn_out = midi_out.connect(port, "mnx_preview")
            .map_err(|e| format!("Failed to connect to MIDI port: {}", e))?;

        if is_mpe {
            send_mpe_configuration(&mut conn_out, member_channels.min(15));
        }

        // Send pitch bend sensitivity RPN to channels involved
        for n in &notes {
            let ch = n.channel.as_int();
            let _ = conn_out.send(&[0xB0 | ch, 101, 0]);
            let _ = conn_out.send(&[0xB0 | ch, 100, 0]);
            let _ = conn_out.send(&[0xB0 | ch, 6, pb_range.min(127)]);
            let _ = conn_out.send(&[0xB0 | ch, 38, 0]);
        }

        // Send Pitch Bend and Note On messages immediately
        for n in &notes {
            if let Some(pb_val) = n.pitch_bend {
                let pb_msg = midly::MidiMessage::PitchBend {
                    bend: midly::PitchBend(midly::num::u14::from(pb_val)),
                };
                let bytes = serialize_midi_message(n.channel, &pb_msg);
                if !bytes.is_empty() {
                    let _ = conn_out.send(&bytes);
                }
            }
            let note_on = midly::MidiMessage::NoteOn {
                key: n.key,
                vel: n.velocity,
            };
            let bytes = serialize_midi_message(n.channel, &note_on);
            if !bytes.is_empty() {
                let _ = conn_out.send(&bytes);
            }
        }

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_signal = stop_flag.clone();

        let handle = std::thread::spawn(move || {
            let sleep_dur = std::time::Duration::from_millis(duration_ms);
            let start_sleep = std::time::Instant::now();
            while start_sleep.elapsed() < sleep_dur {
                if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let rem = sleep_dur.saturating_sub(start_sleep.elapsed());
                std::thread::sleep(rem.min(std::time::Duration::from_millis(10)));
            }

            // Send Note Off for all notes and reset pitch bend
            for n in &notes {
                let note_off = midly::MidiMessage::NoteOff {
                    key: n.key,
                    vel: midly::num::u7::from(64),
                };
                let bytes = serialize_midi_message(n.channel, &note_off);
                if !bytes.is_empty() {
                    let _ = conn_out.send(&bytes);
                }
                // Reset pitch bend to center (8192)
                let reset_pb = midly::MidiMessage::PitchBend {
                    bend: midly::PitchBend(midly::num::u14::from(8192)),
                };
                let bytes_pb = serialize_midi_message(n.channel, &reset_pb);
                if !bytes_pb.is_empty() {
                    let _ = conn_out.send(&bytes_pb);
                }
            }
        });

        self.handle = Some(handle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnx::MnxDocument;
    
    #[test]
    fn test_generate_midi_output() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let smf = generate_midi(&doc, MidiOptions::default());
        assert_eq!(smf.tracks.len(), 1);
        let track = &smf.tracks[0];
        
        let has_pitch_bend = track.iter().any(|ev| matches!(ev.kind, TrackEventKind::Midi { message: MidiMessage::PitchBend { .. }, .. }));
        assert!(has_pitch_bend, "Should generate pitch bend events for microtonal tuning");
    }

    #[test]
    fn test_calculate_pitch_bend() {
        // Neutral (0 semitone diff) -> 8192
        assert_eq!(calculate_pitch_bend(0.0, 48), 8192);
        assert_eq!(calculate_pitch_bend(0.0, 2), 8192);

        // Max upward (+48 semitones on +/-48 range) -> 16383 (clamped)
        assert_eq!(calculate_pitch_bend(48.0, 48), 16383);

        // Max downward (-48 semitones on +/-48 range) -> 0
        assert_eq!(calculate_pitch_bend(-48.0, 48), 0);

        // Half semitone on +/- 48 range -> 8192 + (0.5 / 48) * 8192 = 8192 + 85 = 8277
        assert_eq!(calculate_pitch_bend(0.5, 48), 8277);
    }

    #[test]
    fn test_generate_midi_mpe_output() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let mpe_opts = MidiOptions::mpe_default();
        assert_eq!(mpe_opts.pitch_bend_range, 48);
        assert!(mpe_opts.is_mpe);

        let smf = generate_midi(&doc, mpe_opts);
        assert_eq!(smf.tracks.len(), 1);
        let track = &smf.tracks[0];

        // Verify MPE Configuration message is sent on channel 0 (Master channel): CC 100 = 6, CC 6 = 8 (for 8-string Kite Guitar)
        let has_mpe_config = track.iter().any(|ev| {
            if let TrackEventKind::Midi { channel, message } = &ev.kind {
                if channel.as_int() == 0 {
                    if let MidiMessage::Controller { controller, value } = message {
                        return controller.as_int() == 100 && value.as_int() == 6;
                    }
                }
            }
            false
        });
        assert!(has_mpe_config, "MPE track must include MPE Configuration Message (RPN 6) on Master Channel 1");

        // Verify member channels use pitch bend sensitivity of 48: CC 6 = 48
        let has_pb_range_48 = track.iter().any(|ev| {
            if let TrackEventKind::Midi { channel, message } = &ev.kind {
                if channel.as_int() > 0 {
                    if let MidiMessage::Controller { controller, value } = message {
                        return controller.as_int() == 6 && value.as_int() == 48;
                    }
                }
            }
            false
        });
        assert!(has_pb_range_48, "MPE member channels must have pitch bend range set to 48");

        // Verify notes are assigned to MPE member channels (Channels 1..8, i.e. MIDI channels 2..9)
        let member_note_on_found = track.iter().any(|ev| {
            if let TrackEventKind::Midi { channel, message } = &ev.kind {
                if channel.as_int() >= 1 && channel.as_int() <= 8 {
                    return matches!(message, MidiMessage::NoteOn { .. });
                }
            }
            false
        });
        assert!(member_note_on_found, "NoteOn events must be sent on MPE member channels (>= 1)");
    }

    #[test]
    fn test_serialize_midi_message() {
        let note_on = MidiMessage::NoteOn { key: midly::num::u7::from(60), vel: midly::num::u7::from(100) };
        let bytes = serialize_midi_message(midly::num::u4::from(2), &note_on);
        assert_eq!(bytes, vec![0x92, 60, 100]);

        let note_off = MidiMessage::NoteOff { key: midly::num::u7::from(60), vel: midly::num::u7::from(64) };
        let bytes_off = serialize_midi_message(midly::num::u4::from(0), &note_off);
        assert_eq!(bytes_off, vec![0x80, 60, 64]);

        let pb = MidiMessage::PitchBend { bend: midly::PitchBend(midly::num::u14::from(8192)) };
        let bytes_pb = serialize_midi_message(midly::num::u4::from(5), &pb);
        assert_eq!(bytes_pb, vec![0xE5, 0x00, 0x40]);
    }

    #[test]
    fn test_midi_player_initial_state() {
        let player = MidiPlayer::new();
        assert!(!player.is_playing());
    }

    #[test]
    fn test_generate_midi_start_event_idx() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let full_smf = generate_midi(&doc, MidiOptions::default());
        let full_note_ons = full_smf.tracks[0].iter().filter(|ev| matches!(ev.kind, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. })).count();

        let partial_options = MidiOptions {
            start_event_idx: Some(1),
            ..Default::default()
        };
        let partial_smf = generate_midi(&doc, partial_options);
        let partial_note_ons = partial_smf.tracks[0].iter().filter(|ev| matches!(ev.kind, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. })).count();

        assert!(partial_note_ons < full_note_ons, "Partial playback should have fewer NoteOn events");
        assert!(partial_note_ons > 0, "Partial playback should have remaining NoteOn events");
    }

    #[test]
    fn test_generate_playback_track_event_markers() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let markers: Vec<usize> = track.iter().filter_map(|ev| {
            if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                Some(idx)
            } else {
                None
            }
        }).collect();

        // example.yaml has 2 measures of notes
        assert!(!markers.is_empty(), "Playback track must contain score event markers");
        assert_eq!(markers[0], 0, "First marker must be event 0");
        // Verify monotonically increasing event indices 0, 1, 2, ...
        for (i, &m) in markers.iter().enumerate() {
            assert_eq!(m, i, "Markers must match sequential event indices");
        }
    }

    #[test]
    fn test_generate_playback_track_partial_start() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        // Start playback from event 3
        let options = MidiOptions {
            start_event_idx: Some(3),
            ..Default::default()
        };
        let (track, _) = generate_playback_track(&doc, options);

        let markers: Vec<usize> = track.iter().filter_map(|ev| {
            if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                Some(idx)
            } else {
                None
            }
        }).collect();

        assert!(!markers.is_empty());
        assert_eq!(markers[0], 3, "First event marker must be the requested start_event_idx");

        // The first playback event (or marker) for event 3 must start at delta 0
        let first_marker_event = track.iter().find(|ev| matches!(ev.kind, PlaybackEventKind::EventMarker(3))).unwrap();
        assert_eq!(first_marker_event.delta_ticks, 0, "First note/marker of partial playback must start at tick 0");

        // Subsequent markers must have non-zero total delta advance
        if markers.len() > 1 {
            let second_marker_idx = track.iter().position(|ev| matches!(ev.kind, PlaybackEventKind::EventMarker(4))).unwrap();
            let accumulated_ticks: u32 = track[..=second_marker_idx].iter().map(|ev| ev.delta_ticks).sum();
            assert!(accumulated_ticks > 0, "Subsequent events must have positive accumulated delta ticks");
        }
    }
}

