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
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            pitch_bend_range: 2,
            string_to_channel: HashMap::new(), // Populated dynamically if empty
            default_bpm: 120.0,
            ppq: u15::from(480),
            start_event_idx: None,
        }
    }
}

/// Represents an absolute-time MIDI event before converting to delta-times.
#[derive(Debug, Clone)]
struct AbsoluteEvent {
    time_ticks: u32,
    kind: TrackEventKind<'static>,
}

pub fn generate_midi<'a>(doc: &MnxDocument, mut options: MidiOptions) -> Smf<'a> {
    let header = Header::new(Format::SingleTrack, Timing::Metrical(options.ppq));
    let mut smf = Smf::new(header);
    
    let mut track = Vec::new();
    let mut abs_events = Vec::new();
    
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
    
    // Add tempo event
    let micros_per_beat = (60_000_000.0 / current_bpm).round() as u32;
    abs_events.push(AbsoluteEvent {
        time_ticks: 0,
        kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(micros_per_beat))),
    });

    if let Some(part) = doc.parts.first() {
        // Setup channel mappings for strings
        let num_strings = part.num_strings() as u8;
        if options.string_to_channel.is_empty() {
            for s in 1..=num_strings {
                // Modulo 16 for safety, though string index is typically 1-8. Note: ch 9 is drums.
                let ch = (s - 1) % 16;
                let ch = if ch == 9 { (ch + 1) % 16 } else { ch };
                options.string_to_channel.insert(s, u4::from(ch));
            }
        }

        // Setup pitch bend ranges for used channels
        for &ch in options.string_to_channel.values() {
            // RPN Pitch Bend Sensitivity (MSB = 0, LSB = 0)
            abs_events.push(AbsoluteEvent {
                time_ticks: 0,
                kind: TrackEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(101), value: u7::from(0) } }
            });
            abs_events.push(AbsoluteEvent {
                time_ticks: 0,
                kind: TrackEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(100), value: u7::from(0) } }
            });
            // Set value
            abs_events.push(AbsoluteEvent {
                time_ticks: 0,
                kind: TrackEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::from(6), value: u7::from(options.pitch_bend_range.min(127)) } }
            });
        }

        let freq_table = FretboardFrequencyTable::from_part_default(part);
        let mut current_time_ticks = 0;
        let mut current_event_idx = 0;
        let start_event = options.start_event_idx.unwrap_or(0);

        for measure in &part.measures {
            for seq in &measure.sequences {
                let mut seq_time_ticks = current_time_ticks;
                
                for event in &seq.content {
                    let beats = duration_to_quarter_beats(&event.duration);
                    let duration_ticks = (beats * (u16::from(options.ppq) as f64)).round() as u32;

                    if current_event_idx >= start_event {
                        for note in &event.notes {
                            if let (Some(string), Some(fret)) = (note.string, note.fret) {
                                let channel = options.string_to_channel.get(&string).copied().unwrap_or(u4::from(0));
                                
                                if let Some(pos_state) = freq_table.get(string, fret) {
                                    let midi_note_frac = pos_state.midi_note_number;
                                    let base_note = midi_note_frac.round() as u8;
                                    let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;
                                    
                                    // Pitch bend range calculation
                                    let pitch_bend_val = (8192.0 + (semitone_diff / options.pitch_bend_range as f32) * 8192.0)
                                        .round()
                                        .clamp(0.0, 16383.0) as u16;

                                    // Send Pitch Bend right before Note On
                                    abs_events.push(AbsoluteEvent {
                                        time_ticks: seq_time_ticks,
                                        kind: TrackEventKind::Midi {
                                            channel,
                                            message: MidiMessage::PitchBend {
                                                bend: midly::PitchBend(midly::num::u14::from(pitch_bend_val)),
                                            },
                                        },
                                    });

                                    // Note On
                                    abs_events.push(AbsoluteEvent {
                                        time_ticks: seq_time_ticks,
                                        kind: TrackEventKind::Midi {
                                            channel,
                                            message: MidiMessage::NoteOn {
                                                key: u7::from(base_note.min(127)),
                                                vel: u7::from(100), // Default velocity
                                            },
                                        },
                                    });

                                    // Note Off
                                    abs_events.push(AbsoluteEvent {
                                        time_ticks: seq_time_ticks + duration_ticks,
                                        kind: TrackEventKind::Midi {
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
                        
                        seq_time_ticks += duration_ticks;
                    }

                    current_event_idx += 1;
                }
                current_time_ticks = seq_time_ticks; // Advance measure time (assumes 1 sequence per measure for simple monophonic/polyphonic tab)
            }
        }
    }

    // Sort by absolute time
    abs_events.sort_by_key(|e| e.time_ticks);

    // Convert to relative delta times
    let mut last_time = 0;
    for abs_e in abs_events {
        let delta = abs_e.time_ticks - last_time;
        last_time = abs_e.time_ticks;
        track.push(TrackEvent {
            delta: u28::from_int_lossy(delta),
            kind: abs_e.kind,
        });
    }

    // End of Track
    track.push(TrackEvent {
        delta: u28::from(0),
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
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MidiPlayer {
    pub fn new() -> Self {
        Self {
            stop_signal: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

    pub fn stop(&mut self) {
        self.stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);
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

        let smf = generate_midi(doc, options);
        let track = smf.tracks.into_iter().next().ok_or_else(|| "No MIDI tracks generated".to_string())?;

        let midi_out = midir::MidiOutput::new("mnx_tab_cli_player")
            .map_err(|e| format!("Failed to create MidiOutput: {}", e))?;
        let ports = midi_out.ports();
        let port = ports.get(port_index).ok_or_else(|| format!("Selected MIDI port index {} not found", port_index))?;

        let mut conn_out = midi_out.connect(port, "mnx_playback")
            .map_err(|e| format!("Failed to connect to MIDI port: {}", e))?;

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_signal = stop_flag.clone();

        // Pulses per quarter note
        let ppq = match smf.header.timing {
            midly::Timing::Metrical(p) => u16::from(p) as f64,
            _ => 480.0,
        };

        let handle = std::thread::spawn(move || {
            // Default 120 bpm = 500_000 microseconds per beat
            let mut tempo_micros_per_beat = 500_000.0_f64;

            for event in track {
                if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                // Sleep for delta time
                let delta_ticks = event.delta.as_int() as f64;
                if delta_ticks > 0.0 {
                    let micros = delta_ticks * (tempo_micros_per_beat / ppq);
                    let sleep_dur = std::time::Duration::from_micros(micros.round() as u64);

                    // Sleep in small chunks of 10ms to stay responsive to stop requests
                    let start_sleep = std::time::Instant::now();
                    while start_sleep.elapsed() < sleep_dur {
                        if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let rem = sleep_dur.saturating_sub(start_sleep.elapsed());
                        std::thread::sleep(rem.min(std::time::Duration::from_millis(10)));
                    }

                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                }

                match event.kind {
                    TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                        tempo_micros_per_beat = t.as_int() as f64;
                    }
                    TrackEventKind::Midi { channel, message } => {
                        let bytes = serialize_midi_message(channel, &message);
                        if !bytes.is_empty() {
                            let _ = conn_out.send(&bytes);
                        }
                    }
                    _ => {}
                }
            }

            // Send All Notes Off (CC 123) and Reset All Controllers (CC 121) to all 16 channels on stop/finish
            for ch in 0..16 {
                let status_cc = 0xB0 | ch;
                let _ = conn_out.send(&[status_cc, 123, 0]); // All notes off
                let _ = conn_out.send(&[status_cc, 121, 0]); // Reset controllers
            }
        });

        self.handle = Some(handle);
        Ok(())
    }

    pub fn play_preview(
        &mut self,
        notes: Vec<NotePreview>,
        port_index: usize,
        duration_ms: u64,
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

            // Send Note Off for all notes
            for n in &notes {
                let note_off = midly::MidiMessage::NoteOff {
                    key: n.key,
                    vel: midly::num::u7::from(64),
                };
                let bytes = serialize_midi_message(n.channel, &note_off);
                if !bytes.is_empty() {
                    let _ = conn_out.send(&bytes);
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
}
