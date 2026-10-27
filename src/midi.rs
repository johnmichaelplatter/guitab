use crate::mnx::MnxDocument;

use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
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
        [0xB0, 101, 0],             // RPN MSB: 0
        [0xB0, 100, 6],             // RPN LSB: 6 (MPE Configuration)
        [0xB0, 6, member_channels], // Data Entry MSB: number of member channels
        [0xB0, 38, 0],              // Data Entry LSB: 0
        [0xB0, 101, 127],           // Null RPN MSB
        [0xB0, 100, 127],           // Null RPN LSB
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
    EndOfTrack,
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
    EndOfTrack,
}

pub struct PlaybackTrackGenerator<'a> {
    doc: &'a MnxDocument,
    options: MidiOptions,
    abs_events: Vec<InternalAbsoluteEvent>,
    ppq: f64,
}

impl<'a> PlaybackTrackGenerator<'a> {
    pub fn new(doc: &'a MnxDocument, options: MidiOptions) -> Self {
        let ppq = u16::from(options.ppq) as f64;
        Self {
            doc,
            options,
            abs_events: Vec::new(),
            ppq,
        }
    }



    fn setup_channels_and_mpe(&mut self, num_strings: u8) {
        if self.options.string_to_channel.is_empty() {
            for s in 1..=num_strings {
                if self.options.is_mpe {
                    let ch = ((s - 1) % 15 + 1) as u8;
                    self.options.string_to_channel.insert(s, u4::from(ch));
                } else {
                    let ch = (s - 1) % 16;
                    let ch = if ch == 9 { (ch + 1) % 16 } else { ch };
                    self.options.string_to_channel.insert(s, u4::from(ch));
                }
            }
        }

        if self.options.is_mpe {
            let member_channels = num_strings.min(15);
            let mpe_ccs = [
                (101, 0),
                (100, 6),
                (6, member_channels),
                (38, 0),
                (101, 127),
                (100, 127),
            ];
            for (cc, val) in mpe_ccs {
                self.abs_events.push(InternalAbsoluteEvent {
                    time_ticks: 0,
                    kind: InternalEventKind::Midi {
                        channel: u4::from(0),
                        message: MidiMessage::Controller {
                            controller: u7::from(cc),
                            value: u7::from(val),
                        },
                    },
                });
            }
        }

        for &ch in self.options.string_to_channel.values() {
            let pb_range = self.options.pitch_bend_range.min(127);
            let pb_ccs = [
                (101, 0),
                (100, 0),
                (6, pb_range),
                (38, 0),
                (101, 127),
                (100, 127),
            ];
            for (cc, val) in pb_ccs {
                self.abs_events.push(InternalAbsoluteEvent {
                    time_ticks: 0,
                    kind: InternalEventKind::Midi {
                        channel: ch,
                        message: MidiMessage::Controller {
                            controller: u7::from(cc),
                            value: u7::from(val),
                        },
                    },
                });
            }
        }
    }

    fn compute_event_schedule(
        &self,
        part: &crate::mnx::Part,
        flattened_measures: &[usize],
    ) -> (Vec<u32>, HashMap<(usize, usize, usize), (u32, u32)>) {
        let mut measure_start_ticks = Vec::with_capacity(flattened_measures.len());
        let mut cur_measure_start = 0;
        for &m_idx in flattened_measures {
            measure_start_ticks.push(cur_measure_start);
            let measure = &part.measures[m_idx];
            let mut max_seq_dur = 0;
            for seq in &measure.sequences {
                let mut seq_dur = 0;
                let mut events_scaled = Vec::new();
                walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                for (event, scale) in events_scaled {
                    if scale > 0.0 {
                        let beats = crate::validate::duration_to_quarter_beats(&event.duration);
                        seq_dur += ((beats * scale) * self.ppq).round() as u32;
                    }
                }
                max_seq_dur = max_seq_dur.max(seq_dur);
            }
            cur_measure_start += max_seq_dur;
        }

        struct VoiceEvent {
            fm_idx: usize,
            s_idx: usize,
            e_idx: usize,
            is_grace: bool,
            nominal_start: u32,
            nominal_dur: u32,
        }

        let max_seqs = part
            .measures
            .iter()
            .map(|m| m.sequences.len())
            .max()
            .unwrap_or(0);
        let mut schedule_map = HashMap::new();

        for s_idx in 0..max_seqs {
            let mut voice_events: Vec<VoiceEvent> = Vec::new();
            for (fm_idx, &m_idx) in flattened_measures.iter().enumerate() {
                let measure = &part.measures[m_idx];
                if let Some(seq) = measure.sequences.get(s_idx) {
                    let m_start = measure_start_ticks[fm_idx];
                    let mut seq_offset = 0;
                    let mut events_scaled = Vec::new();
                    walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                    for (e_idx, (event, scale)) in events_scaled.into_iter().enumerate() {
                        let is_grace = scale == 0.0;
                        let nominal_dur = if is_grace {
                            40
                        } else {
                            let beats = crate::validate::duration_to_quarter_beats(&event.duration);
                            ((beats * scale) * self.ppq).round() as u32
                        };
                        let nominal_start = m_start + seq_offset;
                        if !is_grace {
                            seq_offset += nominal_dur;
                        }
                        voice_events.push(VoiceEvent {
                            fm_idx,
                            s_idx,
                            e_idx,
                            is_grace,
                            nominal_start,
                            nominal_dur,
                        });
                    }
                }
            }

            let n = voice_events.len();
            let mut actual_start: Vec<u32> = voice_events.iter().map(|e| e.nominal_start).collect();
            let mut actual_dur: Vec<u32> = voice_events.iter().map(|e| e.nominal_dur).collect();

            let mut i = 0;
            while i < n {
                if voice_events[i].is_grace {
                    let g_start = i;
                    while i < n && voice_events[i].is_grace {
                        i += 1;
                    }
                    let g_end = i;
                    let grace_count = (g_end - g_start) as u32;
                    let total_grace = grace_count * 40;

                    if g_end < n {
                        let target_nominal_start = voice_events[g_end].nominal_start;
                        let target_nominal_end =
                            target_nominal_start + voice_events[g_end].nominal_dur;

                        let (grace_start, target_start, target_dur) = if g_start > 0 {
                            let prior_idx = g_start - 1;
                            let prior_start = actual_start[prior_idx];
                            let prior_dur = actual_dur[prior_idx];
                            let min_prior_dur = 20.min(prior_dur);
                            let desired_grace_start =
                                target_nominal_start.saturating_sub(total_grace);

                            if desired_grace_start >= prior_start + min_prior_dur {
                                actual_dur[prior_idx] =
                                    (desired_grace_start - prior_start).min(prior_dur);
                                (
                                    desired_grace_start,
                                    target_nominal_start,
                                    voice_events[g_end].nominal_dur,
                                )
                            } else {
                                actual_dur[prior_idx] = min_prior_dur;
                                let g_st = prior_start + min_prior_dur;
                                let t_st = g_st + total_grace;
                                let t_dur = target_nominal_end
                                    .saturating_sub(t_st)
                                    .max(20.min(voice_events[g_end].nominal_dur));
                                (g_st, t_st, t_dur)
                            }
                        } else {
                            let g_st = target_nominal_start;
                            let t_st = g_st + total_grace;
                            let t_dur = target_nominal_end
                                .saturating_sub(t_st)
                                .max(20.min(voice_events[g_end].nominal_dur));
                            (g_st, t_st, t_dur)
                        };

                        for (k, g_idx) in (g_start..g_end).enumerate() {
                            actual_start[g_idx] = grace_start + (k as u32) * 40;
                            actual_dur[g_idx] = 40;
                        }
                        actual_start[g_end] = target_start;
                        actual_dur[g_end] = target_dur;
                    } else {
                        for (k, g_idx) in (g_start..g_end).enumerate() {
                            actual_start[g_idx] =
                                voice_events[g_idx].nominal_start + (k as u32) * 40;
                            actual_dur[g_idx] = 40;
                        }
                    }
                } else {
                    i += 1;
                }
            }

            for (idx, ev) in voice_events.iter().enumerate() {
                schedule_map.insert(
                    (ev.fm_idx, ev.s_idx, ev.e_idx),
                    (actual_start[idx], actual_dur[idx]),
                );
            }
        }

        (measure_start_ticks, schedule_map)
    }

    fn find_start_offset(
        &self,
        part: &crate::mnx::Part,
        start_event: usize,
        flattened_measures: &[usize],
        raw_event_idx_map: &[Vec<Vec<usize>>],
        schedule_map: &HashMap<(usize, usize, usize), (u32, u32)>,
    ) -> u32 {
        for (fm_idx, &m_idx) in flattened_measures.iter().enumerate() {
            let measure = &part.measures[m_idx];
            for (s_idx, seq) in measure.sequences.iter().enumerate() {
                let mut events_scaled = Vec::new();
                walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                for (e_idx, _) in events_scaled.into_iter().enumerate() {
                    let raw_idx = raw_event_idx_map[m_idx][s_idx][e_idx];
                    if raw_idx == start_event {
                        if let Some(&(st, _dur)) = schedule_map.get(&(fm_idx, s_idx, e_idx)) {
                            return st;
                        }
                    }
                }
            }
        }
        0
    }

    fn process_events(
        &mut self,
        part: &crate::mnx::Part,
        start_tick_offset: u32,
        flattened_measures: &[usize],
        raw_event_idx_map: &[Vec<Vec<usize>>],
        measure_start_ticks: &[u32],
        schedule_map: &HashMap<(usize, usize, usize), (u32, u32)>,
    ) {
        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_part_default(part);
        let mut string_notes_timeline: std::collections::HashMap<u8, Vec<(u32, f64)>> =
            std::collections::HashMap::new();
        for (fm_idx, &m_idx) in flattened_measures.iter().enumerate() {
            let measure = &part.measures[m_idx];
            for (s_idx, seq) in measure.sequences.iter().enumerate() {
                let mut events_scaled = Vec::new();
                walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                for (e_idx, (event, _scale)) in events_scaled.into_iter().enumerate() {
                    let event_start = schedule_map
                        .get(&(fm_idx, s_idx, e_idx))
                        .map(|&(st, _)| st)
                        .unwrap_or(0);
                    for note in &event.notes {
                        if note.muted.unwrap_or(false)
                            || note
                                .harmonic
                                .as_ref()
                                .and_then(|h| h.sounding_note.muted)
                                .unwrap_or(false)
                        {
                            continue;
                        }
                        let string_opt = note
                            .string
                            .or_else(|| note.harmonic.as_ref().and_then(|h| h.sounding_note.string));
                        if let Some(string) = string_opt {
                            let midi_frac_opt = if let Some(harmonic) = &note.harmonic {
                                Some(freq_table.pitch_to_midi_note_number(&harmonic.sounding_note.pitch))
                            } else if let Some(fret) = note.fret {
                                freq_table.get(string, fret).map(|s| s.midi_note_number)
                            } else {
                                Some(freq_table.pitch_to_midi_note_number(&note.pitch))
                            };
                            if let Some(midi_frac) = midi_frac_opt {
                                string_notes_timeline
                                    .entry(string)
                                    .or_default()
                                    .push((event_start, midi_frac));
                            }
                        }
                    }
                }
            }
        }

        let mut pending_ties: std::collections::HashMap<String, (midly::num::u4, u8)> =
            std::collections::HashMap::new();
        let mut final_ticks = 0;
        let mut current_bpm;

        let mut tempo_at_start = self.options.default_bpm;
        
        // Scan to find tempo at start_tick_offset
        for (fm_idx, &m_idx) in flattened_measures.iter().enumerate() {
            if m_idx < self.doc.global.measures.len() {
                let gm = &self.doc.global.measures[m_idx];
                let parse_pos = |loc: &Option<crate::mnx::Position>| -> f64 {
                    loc.as_ref().map(|p| p.to_quarter_beats()).unwrap_or(0.0)
                };
                let m_start = measure_start_ticks[fm_idx];
                for tempo in &gm.tempos {
                    let offset_beats = parse_pos(&tempo.location);
                    let tempo_ticks = m_start + (offset_beats * self.options.ppq.as_int() as f64) as u32;
                    if tempo_ticks <= start_tick_offset {
                        tempo_at_start = tempo.bpm * crate::validate::duration_to_quarter_beats(&tempo.value);
                    }
                }
            }
        }

        self.abs_events.push(InternalAbsoluteEvent {
            time_ticks: 0,
            kind: InternalEventKind::Tempo((60_000_000.0 / tempo_at_start).round() as u32),
        });
        current_bpm = tempo_at_start;

        for (fm_idx, &m_idx) in flattened_measures.iter().enumerate() {
            let measure = &part.measures[m_idx];
            let current_time_ticks = measure_start_ticks[fm_idx];
            
            if m_idx < self.doc.global.measures.len() {
                let gm = &self.doc.global.measures[m_idx];
                
                // Helper to parse location
                let parse_loc = |loc: &Option<String>| -> f64 {
                    if let Some(s) = loc {
                        if let Some((n, d)) = s.split_once('/') {
                            if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                                if den > 0.0 { return (num / den) * 4.0; }
                            }
                        }
                    }
                    0.0
                };
                let parse_pos = |loc: &Option<crate::mnx::Position>| -> f64 {
                    loc.as_ref().map(|p| p.to_quarter_beats()).unwrap_or(0.0)
                };

                for tempo in &gm.tempos {
                    let offset_beats = parse_pos(&tempo.location);
                    let tempo_ticks = current_time_ticks + (offset_beats * self.options.ppq.as_int() as f64) as u32;
                    let eff_bpm = tempo.bpm * crate::validate::duration_to_quarter_beats(&tempo.value);
                    
                    if tempo_ticks > start_tick_offset && eff_bpm != current_bpm {
                        current_bpm = eff_bpm;
                        let event_start_ticks = tempo_ticks.saturating_sub(start_tick_offset);
                        let micros_per_beat = (60_000_000.0 / current_bpm).round() as u32;
                        self.abs_events.push(InternalAbsoluteEvent {
                            time_ticks: event_start_ticks,
                            kind: InternalEventKind::Tempo(micros_per_beat),
                        });
                    }
                }

                for tc in &gm.tempo_changes {
                    let start_offset = parse_loc(&tc.location);
                    let start_ticks = current_time_ticks + (start_offset * self.options.ppq.as_int() as f64) as u32;
                    
                    if start_ticks >= start_tick_offset {
                        // find target
                        let mut target_bpm = current_bpm;
                        let mut target_ticks = start_ticks + self.options.ppq.as_int() as u32; // fallback 1 beat
                        if let Some(tid) = &tc.target {
                            let mut found = false;
                            for (fm2, &m2) in flattened_measures.iter().enumerate() {
                                if m2 < self.doc.global.measures.len() {
                                    for t in &self.doc.global.measures[m2].tempos {
                                        if t.id.as_ref() == Some(tid) {
                                            let off = parse_pos(&t.location);
                                            target_ticks = measure_start_ticks[fm2] + (off * self.options.ppq.as_int() as f64) as u32;
                                            target_bpm = t.bpm * crate::validate::duration_to_quarter_beats(&t.value);
                                            found = true;
                                            break;
                                        }
                                    }
                                }
                                if found { break; }
                            }
                        }
                        
                        let duration_ticks = target_ticks.saturating_sub(start_ticks).max(1);
                        let start_bpm = current_bpm;
                        let bpm_diff = target_bpm - start_bpm;
                        
                        let curve_editor = if let Some(bend_str) = &tc.bend {
                            Some(crate::curve_editor::parse_bend_string(
                                bend_str,
                                eframe::egui::Rect::from_min_size(
                                    eframe::egui::pos2(0.0, 0.0),
                                    eframe::egui::vec2(100.0, 100.0),
                                ),
                            ))
                        } else { None };
                        
                        let num_samples = 32.min(duration_ticks as usize);
                        for i in 1..=num_samples {
                            let progress = i as f64 / num_samples as f64;
                            let tick_offset = ((duration_ticks as f64) * progress) as u32;
                            let ev_ticks = start_ticks + tick_offset;
                            
                            let mut interp_prog = progress;
                            if let Some(ce) = &curve_editor {
                                let y_screen = ce.sample_y_at_x((progress * 100.0) as f32);
                                let y_norm = (100.0 - y_screen).clamp(0.0, 100.0);
                                interp_prog = (y_norm / 100.0) as f64;
                            }
                            
                            let cur_bpm = start_bpm + bpm_diff * interp_prog;
                            if ev_ticks > start_tick_offset {
                                let ev_start = ev_ticks.saturating_sub(start_tick_offset);
                                let micros_per_beat = (60_000_000.0 / cur_bpm).round() as u32;
                                self.abs_events.push(InternalAbsoluteEvent {
                                    time_ticks: ev_start,
                                    kind: InternalEventKind::Tempo(micros_per_beat),
                                });
                            }
                        }
                        // Set current_bpm to target
                        current_bpm = target_bpm;
                    }
                }
            }

            for (s_idx, seq) in measure.sequences.iter().enumerate() {
                let mut events_scaled = Vec::new();
                walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                for (e_idx, (event, _scale)) in events_scaled.into_iter().enumerate() {
                    let &(effective_start_ticks, duration_ticks) = schedule_map
                        .get(&(fm_idx, s_idx, e_idx))
                        .unwrap_or(&(0, 0));
                    let raw_idx = raw_event_idx_map[m_idx][s_idx][e_idx];

                    if effective_start_ticks >= start_tick_offset {
                        let event_start_ticks = effective_start_ticks.saturating_sub(start_tick_offset);
                        final_ticks = final_ticks.max(event_start_ticks + duration_ticks);

                        self.abs_events.push(InternalAbsoluteEvent {
                            time_ticks: event_start_ticks,
                            kind: InternalEventKind::EventMarker(raw_idx),
                        });

                        let mut arp_dir = None;
                        if let Some(arpeggios) = &measure.arpeggios {
                            let first_id = event
                                .notes
                                .first()
                                .and_then(|n| n.id.as_deref())
                                .unwrap_or("");
                            let last_id = event
                                .notes
                                .last()
                                .and_then(|n| n.id.as_deref())
                                .unwrap_or("");
                            for arp in arpeggios {
                                if arp.span.start == first_id && arp.span.end == last_id {
                                    arp_dir = arp.direction.clone();
                                    break;
                                }
                            }
                        }
                        let num_notes = event
                            .notes
                            .iter()
                            .filter(|n| {
                                let s = n.string.or_else(|| {
                                    n.harmonic
                                        .as_ref()
                                        .and_then(|h| h.sounding_note.string)
                                });
                                s.is_some() && (n.fret.is_some() || n.harmonic.is_some())
                            })
                            .count();
                        let total_strum_delay_ticks = (0.125 * (self.ppq as f64)).round() as u32; // 32nd note
                        let strum_delay_per_note = if num_notes > 1 {
                            total_strum_delay_ticks.min(duration_ticks / 2) / (num_notes as u32 - 1)
                        } else {
                            0
                        };

                        let mut ordered_notes = Vec::new();
                        for (i, note) in event.notes.iter().enumerate() {
                            if note.muted.unwrap_or(false)
                                || note
                                    .harmonic
                                    .as_ref()
                                    .and_then(|h| h.sounding_note.muted)
                                    .unwrap_or(false)
                            {
                                continue;
                            }
                            let string_opt = note
                                .string
                                .or_else(|| note.harmonic.as_ref().and_then(|h| h.sounding_note.string));
                            if let Some(string) = string_opt {
                                if note.fret.is_some() || note.harmonic.is_some() {
                                    ordered_notes.push((i, note, string));
                                }
                            }
                        }
                        if arp_dir.as_deref() == Some("up") {
                            ordered_notes.sort_by(|a, b| b.2.cmp(&a.2));
                        } else if arp_dir.as_deref() == Some("down") {
                            ordered_notes.sort_by(|a, b| a.2.cmp(&b.2));
                        }

                        for (strum_idx, (_idx, note, string)) in
                            ordered_notes.into_iter().enumerate()
                        {
                            let mut note_start_ticks = event_start_ticks;
                            if arp_dir.is_some() {
                                note_start_ticks += (strum_idx as u32) * strum_delay_per_note;
                            }

                            let default_ch = if self.options.is_mpe {
                                u4::from(((string - 1) % 15 + 1) as u8)
                            } else {
                                u4::from((string - 1) % 16)
                            };
                            let channel = self
                                .options
                                .string_to_channel
                                .get(&string)
                                .copied()
                                .unwrap_or(default_ch);

                            let midi_note_frac_opt = if let Some(harmonic) = &note.harmonic {
                                Some(freq_table.pitch_to_midi_note_number(&harmonic.sounding_note.pitch))
                            } else if let Some(fret) = note.fret {
                                freq_table.get(string, fret).map(|s| s.midi_note_number)
                            } else {
                                Some(freq_table.pitch_to_midi_note_number(&note.pitch))
                            };

                            if let Some(midi_note_frac) = midi_note_frac_opt {
                                let base_note = midi_note_frac.round() as u8;
                                let semitone_diff =
                                    (midi_note_frac - (base_note as f64)) as f32;
                                let pitch_bend_val = calculate_pitch_bend(
                                    semitone_diff,
                                    self.options.pitch_bend_range,
                                );

                                let bend_str_opt = note.bend.as_ref().or_else(|| {
                                    note.harmonic
                                        .as_ref()
                                        .and_then(|h| h.sounding_note.bend.as_ref())
                                });
                                let bend_editor = if let Some(bend_str) = bend_str_opt {
                                    Some(crate::curve_editor::parse_bend_string(
                                        bend_str,
                                        eframe::egui::Rect::from_min_size(
                                            eframe::egui::pos2(0.0, 0.0),
                                            eframe::egui::vec2(100.0, 100.0),
                                        ),
                                    ))
                                } else {
                                    None
                                };

                                let has_slide = note.slide.is_some()
                                    || note
                                        .harmonic
                                        .as_ref()
                                        .map_or(false, |h| h.sounding_note.slide.is_some());
                                let mut has_forward_slide = false;
                                let mut slide_target_midi_frac = 0.0;
                                if has_slide {
                                    has_forward_slide = true;
                                    if let Some(timeline) = string_notes_timeline.get(&string) {
                                        for &(t, midi_frac) in timeline {
                                            if t > effective_start_ticks {
                                                slide_target_midi_frac = midi_frac;
                                                break;
                                            }
                                        }
                                    }
                                }

                                let note_id_opt = note.id.as_ref().or_else(|| {
                                    note.harmonic
                                        .as_ref()
                                        .and_then(|h| h.sounding_note.id.as_ref())
                                });
                                let is_tied = note_id_opt
                                    .map_or(false, |id| pending_ties.contains_key(id));

                                let (ch, note_val) = if is_tied {
                                    let id = note_id_opt.unwrap();
                                    pending_ties.remove(id).unwrap()
                                } else {
                                    self.abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: note_start_ticks,
                                        kind: InternalEventKind::Midi {
                                            channel,
                                            message: MidiMessage::PitchBend {
                                                bend: midly::PitchBend(midly::num::u14::from(
                                                    pitch_bend_val,
                                                )),
                                            },
                                        },
                                    });

                                    if let Some(editor) = &bend_editor {
                                        let num_samples = 32;
                                        for i in 1..=num_samples {
                                            let progress = i as f32 / num_samples as f32;
                                            let tick_offset =
                                                ((duration_ticks as f32) * progress) as u32;
                                            // Normalized X is 0.0 to 100.0 for the evaluator
                                            // sample_y_at_x returns 0.0 (top) to 100.0 (bottom) because the editor rect was 0.0 to 100.0
                                            let y_screen =
                                                editor.sample_y_at_x(progress * 100.0);
                                            let y_norm = (100.0 - y_screen).clamp(0.0, 100.0); // invert so 100.0 is MAX bend
                                            let bend_semitones = (y_norm / 100.0)
                                                * (self.options.pitch_bend_range as f32);

                                            let total_bend_val = calculate_pitch_bend(
                                                semitone_diff + bend_semitones,
                                                self.options.pitch_bend_range,
                                            );

                                            self.abs_events.push(InternalAbsoluteEvent {
                                                time_ticks: note_start_ticks + tick_offset,
                                                kind: InternalEventKind::Midi {
                                                    channel,
                                                    message: MidiMessage::PitchBend {
                                                        bend: midly::PitchBend(
                                                            midly::num::u14::from(
                                                                total_bend_val,
                                                            ),
                                                        ),
                                                    },
                                                },
                                            });
                                        }
                                    } else if has_forward_slide && slide_target_midi_frac != 0.0
                                    {
                                        let num_samples = 32;
                                        let total_dist =
                                            (slide_target_midi_frac - midi_note_frac) as f32;
                                        let clipped_dist = total_dist.clamp(
                                            -(self.options.pitch_bend_range as f32),
                                            self.options.pitch_bend_range as f32,
                                        );

                                        // Instantiate the exact curve from the user's screenshot for the slide
                                        // Starts flat at bottom-left (0,0), handle pulled 50% to the right (50,0). Ends top-right (100,100).
                                        let slide_editor = crate::curve_editor::MonotonicCurveEditor::from_svg_path_normalized(
                                            "M 0.0 0.0 C 50.0 0.0 100.0 100.0 100.0 100.0",
                                            eframe::egui::Rect::from_min_max(eframe::egui::pos2(0.0, 0.0), eframe::egui::pos2(100.0, 100.0))
                                        );

                                        for i in 1..=num_samples {
                                            let progress = i as f32 / num_samples as f32;
                                            let tick_offset =
                                                ((duration_ticks as f32) * progress) as u32;

                                            // Evaluate the bezier curve
                                            let y_screen =
                                                slide_editor.sample_y_at_x(progress * 100.0);
                                            // y_screen goes from 100.0 (bottom) to 0.0 (top)
                                            // We want normalized progression from 0.0 to 1.0
                                            let curve_progress = (100.0 - y_screen) / 100.0;
                                            let curve_progress = curve_progress.clamp(0.0, 1.0);

                                            let bend_semitones = clipped_dist * curve_progress;
                                            let total_bend_val = calculate_pitch_bend(
                                                semitone_diff + bend_semitones,
                                                self.options.pitch_bend_range,
                                            );

                                            self.abs_events.push(InternalAbsoluteEvent {
                                                time_ticks: note_start_ticks + tick_offset,
                                                kind: InternalEventKind::Midi {
                                                    channel,
                                                    message: MidiMessage::PitchBend {
                                                        bend: midly::PitchBend(
                                                            midly::num::u14::from(
                                                                total_bend_val,
                                                            ),
                                                        ),
                                                    },
                                                },
                                            });
                                        }
                                    }

                                    let is_accented = event.markings.as_ref().map_or(false, |m| m.accent.is_some());
                                    let velocity = if is_accented { 127 } else { 100 };

                                    self.abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: note_start_ticks,
                                        kind: InternalEventKind::Midi {
                                            channel,
                                            message: MidiMessage::NoteOn {
                                                key: u7::from(base_note.min(127)),
                                                vel: u7::from(velocity),
                                            },
                                        },
                                    });
                                    (channel, base_note)
                                };

                                let forward_ties = note
                                    .ties
                                    .as_ref()
                                    .or_else(|| note.harmonic.as_ref().and_then(|h| h.sounding_note.ties.as_ref()));
                                let has_forward_tie = forward_ties.map_or(false, |t| !t.is_empty());

                                let mut has_forward_slur = false;
                                if let (Some(slurs), Some(n_id)) = (&event.slurs, note_id_opt) {
                                    has_forward_slur = slurs
                                        .iter()
                                        .any(|s| s.start_note.as_ref() == Some(n_id));
                                }

                                if has_forward_tie {
                                    let target = forward_ties.unwrap()[0].target.clone();
                                    pending_ties.insert(target, (ch, note_val));
                                } else {
                                    // Legato overlap: if slur or slide, delay NoteOff by ~5% of a quarter note
                                    let overlap = if has_forward_slur || has_forward_slide {
                                        (self.ppq as f64 * 0.05) as u32
                                    } else {
                                        0
                                    };

                                    self.abs_events.push(InternalAbsoluteEvent {
                                        time_ticks: event_start_ticks
                                            + duration_ticks
                                            + overlap,
                                        kind: InternalEventKind::Midi {
                                            channel: ch,
                                            message: MidiMessage::NoteOff {
                                                key: u7::from(note_val.min(127)),
                                                vel: u7::from(64),
                                            },
                                        },
                                    });
                                }
                            }
                        }

                        if let Some(ev_id) = &event.id {
                            if let Some((ch, note_val)) = pending_ties.remove(ev_id) {
                                self.abs_events.push(InternalAbsoluteEvent {
                                    time_ticks: event_start_ticks + duration_ticks,
                                    kind: InternalEventKind::Midi {
                                        channel: ch,
                                        message: MidiMessage::NoteOff {
                                            key: u7::from(note_val.min(127)),
                                            vel: u7::from(64),
                                        },
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }

        for (_, (ch, note_val)) in pending_ties.drain() {
            self.abs_events.push(InternalAbsoluteEvent {
                time_ticks: final_ticks,
                kind: InternalEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::NoteOff {
                        key: u7::from(note_val.min(127)),
                        vel: u7::from(64),
                    },
                },
            });
        }

        if final_ticks > 0 {
            self.abs_events.push(InternalAbsoluteEvent {
                time_ticks: final_ticks,
                kind: InternalEventKind::EndOfTrack,
            });
        }
    }

    fn resolve_deltas(mut self) -> Vec<PlaybackEvent> {
        self.abs_events.sort_by_key(|e| e.time_ticks);
        let mut playback_events = Vec::new();
        let mut last_time = 0;
        for abs_e in self.abs_events {
            let delta = abs_e.time_ticks.saturating_sub(last_time);
            last_time = abs_e.time_ticks;
            match abs_e.kind {
                InternalEventKind::Midi { channel, message } => {
                    playback_events.push(PlaybackEvent {
                        delta_ticks: delta,
                        kind: PlaybackEventKind::Midi { channel, message },
                    });
                }
                InternalEventKind::Tempo(mpb) => {
                    playback_events.push(PlaybackEvent {
                        delta_ticks: delta,
                        kind: PlaybackEventKind::Tempo(mpb),
                    });
                }
                InternalEventKind::EventMarker(idx) => {
                    playback_events.push(PlaybackEvent {
                        delta_ticks: delta,
                        kind: PlaybackEventKind::EventMarker(idx),
                    });
                }
                InternalEventKind::EndOfTrack => {
                    playback_events.push(PlaybackEvent {
                        delta_ticks: delta,
                        kind: PlaybackEventKind::EndOfTrack,
                    });
                }
            }
        }
        playback_events
    }

    pub fn generate(mut self) -> (Vec<PlaybackEvent>, f64) {
        let total_part_measures = self.doc.parts.first().map(|p| p.measures.len()).unwrap_or(0);
        let flattened_measures = compute_flattened_measures(&self.doc.global, total_part_measures);

        if let Some(part) = self.doc.parts.first() {
            let num_strings = part.num_strings() as u8;
            self.setup_channels_and_mpe(num_strings);
            
            let mut raw_event_idx_map = Vec::new();
            let mut current_raw_idx = 0;
            for measure in &part.measures {
                let mut seq_map = Vec::new();
                for seq in &measure.sequences {
                    let mut ev_map = Vec::new();
                    let mut events_scaled = Vec::new();
                    walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                    for _ in events_scaled {
                        ev_map.push(current_raw_idx);
                        current_raw_idx += 1;
                    }
                    seq_map.push(ev_map);
                }
                raw_event_idx_map.push(seq_map);
            }

            let (measure_start_ticks, schedule_map) =
                self.compute_event_schedule(part, &flattened_measures);

            let start_event = self.options.start_event_idx.unwrap_or(0);
            let start_tick_offset = self.find_start_offset(
                part,
                start_event,
                &flattened_measures,
                &raw_event_idx_map,
                &schedule_map,
            );
            self.process_events(
                part,
                start_tick_offset,
                &flattened_measures,
                &raw_event_idx_map,
                &measure_start_ticks,
                &schedule_map,
            );
        }
        let ppq = self.ppq;
        (self.resolve_deltas(), ppq)
    }
}

/// Computes the flattened playback order of measures respecting repeats (repeatStart, repeatEnd)
/// and alternate endings (voltas).
pub fn compute_flattened_measures(
    global: &crate::mnx::Global,
    num_part_measures: usize,
) -> Vec<usize> {
    let mut flattened_measures = Vec::new();
    let num_measures = if global.measures.is_empty() {
        num_part_measures
    } else {
        global.measures.len().min(num_part_measures)
    };

    if num_measures == 0 {
        return flattened_measures;
    }

    if global.measures.is_empty() {
        for i in 0..num_measures {
            flattened_measures.push(i);
        }
        return flattened_measures;
    }

    struct EndingRange {
        start_m: usize,
        duration: usize,
        numbers: Vec<u32>,
    }

    let mut endings: Vec<EndingRange> = Vec::new();
    for (idx, gm) in global.measures.iter().enumerate().take(num_measures) {
        if let Some(ending) = &gm.ending {
            endings.push(EndingRange {
                start_m: idx,
                duration: ending.duration.max(1) as usize,
                numbers: ending.numbers.clone(),
            });
        }
    }

    let mut m = 0;
    let mut active_repeat_start = 0;
    let mut pass_count: u32 = 1;
    let mut steps_count = 0;
    const MAX_STEPS: usize = 10_000;

    while m < num_measures && steps_count < MAX_STEPS {
        steps_count += 1;
        let gm = &global.measures[m];

        // If this measure starts a repeat block
        if gm.repeat_start.is_some() && pass_count == 1 {
            active_repeat_start = m;
        }

        // Determine the next repeat_start boundary after active_repeat_start
        let next_repeat_start = (active_repeat_start + 1..num_measures)
            .find(|&i| global.measures[i].repeat_start.is_some())
            .unwrap_or(num_measures);

        // Collect all endings belonging to this repeat section
        let section_endings: Vec<&EndingRange> = endings
            .iter()
            .filter(|e| e.start_m >= active_repeat_start && e.start_m < next_repeat_start)
            .collect();

        // End of the last ending in this section (if any)
        let section_endings_end = section_endings
            .iter()
            .map(|e| e.start_m + e.duration)
            .max();

        // If we have advanced past all endings of this section, reset pass_count and active_repeat_start
        if let Some(end_m) = section_endings_end {
            if m >= end_m && pass_count > 1 {
                pass_count = 1;
                active_repeat_start = m;
            }
        }

        // Compute max_pass for this repeat section
        let max_pass: u32 = if !section_endings.is_empty() {
            section_endings
                .iter()
                .flat_map(|e| e.numbers.iter())
                .copied()
                .max()
                .unwrap_or(2)
        } else {
            // Check if there is a repeat_end in this section
            (active_repeat_start..next_repeat_start)
                .filter_map(|i| global.measures[i].repeat_end.as_ref())
                .map(|re| re.times.max(1))
                .next()
                .unwrap_or(2)
        };

        // Check if measure m is the start of an ending
        if let Some(cur_ending) = section_endings.iter().find(|e| e.start_m == m) {
            if !cur_ending.numbers.contains(&pass_count) {
                // Skip this ending! Find an ending in this section that matches pass_count
                if let Some(next_matching_ending) = section_endings
                    .iter()
                    .find(|e| e.start_m > m && e.numbers.contains(&pass_count))
                {
                    m = next_matching_ending.start_m;
                    continue;
                } else {
                    // No matching ending found for this pass; jump past all endings in this section
                    let last_ending_end = section_endings_end.unwrap_or(m + cur_ending.duration);
                    m = last_ending_end;
                    continue;
                }
            }
        }

        // Play this measure
        flattened_measures.push(m);

        // Check if this measure triggers a repeat
        let is_repeat_end = gm.repeat_end.is_some()
            || (pass_count < max_pass
                && section_endings.iter().any(|e| {
                    e.start_m + e.duration - 1 == m && e.numbers.contains(&pass_count)
                })
                && section_endings.iter().any(|e| e.start_m == m + 1));

        if is_repeat_end {
            if pass_count < max_pass {
                pass_count += 1;
                m = active_repeat_start;
                continue;
            } else {
                // Finished all passes for this repeat block
                pass_count = 1;
                active_repeat_start = m + 1;
                m += 1;
                continue;
            }
        }

        m += 1;
    }

    flattened_measures
}

pub fn generate_playback_track(
    doc: &MnxDocument,
    options: MidiOptions,
) -> (Vec<PlaybackEvent>, f64) {
    PlaybackTrackGenerator::new(doc, options).generate()
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
            PlaybackEventKind::EventMarker(_) | PlaybackEventKind::EndOfTrack => {
                // Internal to player sequencer; omit from standard SMF (accumulated_delta is carried into EndOfTrack below)
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
    match midir::MidiOutput::new("guitab_probe") {
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

struct CachedMidiConnection {
    port_index: usize,
    conn: midir::MidiOutputConnection,
}

pub struct MidiPlayer {
    stop_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    current_event: std::sync::Arc<std::sync::atomic::AtomicI64>,
    connection: std::sync::Arc<std::sync::Mutex<Option<CachedMidiConnection>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MidiPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MidiPlayer {
    pub fn new() -> Self {
        Self {
            stop_signal: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            current_event: std::sync::Arc::new(std::sync::atomic::AtomicI64::new(-1)),
            connection: std::sync::Arc::new(std::sync::Mutex::new(None)),
            handle: None,
        }
    }

    fn ensure_connection(&self, port_index: usize) -> Result<(), String> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| String::from("MIDI connection lock poisoned"))?;
        if let Some(cached) = guard.as_ref() {
            if cached.port_index == port_index {
                return Ok(());
            }
        }
        let midi_out = midir::MidiOutput::new("guitab_player")
            .map_err(|e| format!("Failed to create MidiOutput: {}", e))?;
        let ports = midi_out.ports();
        let port = ports
            .get(port_index)
            .ok_or_else(|| format!("Selected MIDI port index {} not found", port_index))?;
        let conn = midi_out
            .connect(port, "mnx_output")
            .map_err(|e| format!("Failed to connect to MIDI port: {}", e))?;
        *guard = Some(CachedMidiConnection { port_index, conn });
        Ok(())
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
        let val = self
            .current_event
            .load(std::sync::atomic::Ordering::Relaxed);
        if val >= 0 { Some(val as usize) } else { None }
    }

    pub fn stop(&mut self) {
        self.stop_signal
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.current_event
            .store(-1, std::sync::atomic::Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
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
        self.ensure_connection(port_index)?;

        let (track, ppq) = generate_playback_track(doc, options);

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_signal = stop_flag.clone();

        let current_event_tracker = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(-1));
        self.current_event = current_event_tracker.clone();

        let conn_arc = self.connection.clone();

        let handle = std::thread::spawn(move || {
            let mut tempo_micros_per_beat = 500_000.0_f64;
            let mut next_deadline = std::time::Instant::now();

            for event in track {
                if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                let delta_ticks = event.delta_ticks as f64;
                if delta_ticks > 0.0 {
                    let micros = delta_ticks * (tempo_micros_per_beat / ppq);
                    next_deadline += std::time::Duration::from_micros(micros.round() as u64);

                    loop {
                        if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                        let now = std::time::Instant::now();
                        if now >= next_deadline {
                            break;
                        }
                        std::thread::park_timeout(next_deadline - now);
                    }

                    if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
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
                            if let Ok(mut guard) = conn_arc.lock() {
                                if let Some(cached) = guard.as_mut() {
                                    let _ = cached.conn.send(&bytes);
                                }
                            }
                        }
                    }
                    PlaybackEventKind::EventMarker(idx) => {
                        current_event_tracker
                            .store(idx as i64, std::sync::atomic::Ordering::Relaxed);
                    }
                    PlaybackEventKind::EndOfTrack => {}
                }
            }

            if let Ok(mut guard) = conn_arc.lock() {
                if let Some(cached) = guard.as_mut() {
                    for ch in 0..16 {
                        let status_cc = 0xB0 | ch;
                        let _ = cached.conn.send(&[status_cc, 123, 0]);
                        let _ = cached.conn.send(&[status_cc, 121, 0]);
                    }
                }
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

        self.ensure_connection(port_index)?;

        if let Ok(mut guard) = self.connection.lock() {
            if let Some(cached) = guard.as_mut() {
                let conn_out = &mut cached.conn;
                if is_mpe {
                    send_mpe_configuration(conn_out, member_channels.min(15));
                }

                for n in &notes {
                    let ch = n.channel.as_int();
                    let _ = conn_out.send(&[0xB0 | ch, 101, 0]);
                    let _ = conn_out.send(&[0xB0 | ch, 100, 0]);
                    let _ = conn_out.send(&[0xB0 | ch, 6, pb_range.min(127)]);
                    let _ = conn_out.send(&[0xB0 | ch, 38, 0]);
                }

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
            }
        }

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_signal = stop_flag.clone();
        let conn_arc = self.connection.clone();

        let handle = std::thread::spawn(move || {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(duration_ms);
            loop {
                if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let now = std::time::Instant::now();
                if now >= deadline {
                    break;
                }
                std::thread::park_timeout(deadline - now);
            }

            if let Ok(mut guard) = conn_arc.lock() {
                if let Some(cached) = guard.as_mut() {
                    for n in &notes {
                        let note_off = midly::MidiMessage::NoteOff {
                            key: n.key,
                            vel: midly::num::u7::from(64),
                        };
                        let bytes = serialize_midi_message(n.channel, &note_off);
                        if !bytes.is_empty() {
                            let _ = cached.conn.send(&bytes);
                        }
                    }
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
        let yaml = include_str!("../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let smf = generate_midi(&doc, MidiOptions::default());
        assert_eq!(smf.tracks.len(), 1);
        let track = &smf.tracks[0];

        let has_pitch_bend = track.iter().any(|ev| {
            matches!(
                ev.kind,
                TrackEventKind::Midi {
                    message: MidiMessage::PitchBend { .. },
                    ..
                }
            )
        });
        assert!(
            has_pitch_bend,
            "Should generate pitch bend events for microtonal tuning"
        );
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
        let yaml = include_str!("../examples/example.yaml");
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
        assert!(
            has_mpe_config,
            "MPE track must include MPE Configuration Message (RPN 6) on Master Channel 1"
        );

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
        assert!(
            has_pb_range_48,
            "MPE member channels must have pitch bend range set to 48"
        );

        // Verify notes are assigned to MPE member channels (Channels 1..8, i.e. MIDI channels 2..9)
        let member_note_on_found = track.iter().any(|ev| {
            if let TrackEventKind::Midi { channel, message } = &ev.kind {
                if channel.as_int() >= 1 && channel.as_int() <= 8 {
                    return matches!(message, MidiMessage::NoteOn { .. });
                }
            }
            false
        });
        assert!(
            member_note_on_found,
            "NoteOn events must be sent on MPE member channels (>= 1)"
        );
    }

    #[test]
    fn test_serialize_midi_message() {
        let note_on = MidiMessage::NoteOn {
            key: midly::num::u7::from(60),
            vel: midly::num::u7::from(100),
        };
        let bytes = serialize_midi_message(midly::num::u4::from(2), &note_on);
        assert_eq!(bytes, vec![0x92, 60, 100]);

        let note_off = MidiMessage::NoteOff {
            key: midly::num::u7::from(60),
            vel: midly::num::u7::from(64),
        };
        let bytes_off = serialize_midi_message(midly::num::u4::from(0), &note_off);
        assert_eq!(bytes_off, vec![0x80, 60, 64]);

        let pb = MidiMessage::PitchBend {
            bend: midly::PitchBend(midly::num::u14::from(8192)),
        };
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
        let yaml = include_str!("../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let full_smf = generate_midi(&doc, MidiOptions::default());
        let full_note_ons = full_smf.tracks[0]
            .iter()
            .filter(|ev| {
                matches!(
                    ev.kind,
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOn { .. },
                        ..
                    }
                )
            })
            .count();

        let partial_options = MidiOptions {
            start_event_idx: Some(1),
            ..Default::default()
        };
        let partial_smf = generate_midi(&doc, partial_options);
        let partial_note_ons = partial_smf.tracks[0]
            .iter()
            .filter(|ev| {
                matches!(
                    ev.kind,
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOn { .. },
                        ..
                    }
                )
            })
            .count();

        assert!(
            partial_note_ons < full_note_ons,
            "Partial playback should have fewer NoteOn events"
        );
        assert!(
            partial_note_ons > 0,
            "Partial playback should have remaining NoteOn events"
        );
    }

    #[test]
    fn test_generate_playback_track_event_markers() {
        let yaml = include_str!("../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let markers: Vec<usize> = track
            .iter()
            .filter_map(|ev| {
                if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        // example.yaml has 2 measures of notes
        assert!(
            !markers.is_empty(),
            "Playback track must contain score event markers"
        );
        assert_eq!(markers[0], 0, "First marker must be event 0");
        // Verify monotonically increasing event indices 0, 1, 2, ...
        for (i, &m) in markers.iter().enumerate() {
            assert_eq!(m, i, "Markers must match sequential event indices");
        }
    }

    #[test]
    fn test_generate_playback_track_partial_start() {
        let yaml = include_str!("../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        // Start playback from event 3
        let options = MidiOptions {
            start_event_idx: Some(3),
            ..Default::default()
        };
        let (track, _) = generate_playback_track(&doc, options);

        let markers: Vec<usize> = track
            .iter()
            .filter_map(|ev| {
                if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        assert!(!markers.is_empty());
        assert_eq!(
            markers[0], 3,
            "First event marker must be the requested start_event_idx"
        );

        // The first playback event (or marker) for event 3 must start at delta 0
        let first_marker_event = track
            .iter()
            .find(|ev| matches!(ev.kind, PlaybackEventKind::EventMarker(3)))
            .unwrap();
        assert_eq!(
            first_marker_event.delta_ticks, 0,
            "First note/marker of partial playback must start at tick 0"
        );

        // Subsequent markers must have non-zero total delta advance
        if markers.len() > 1 {
            let second_marker_idx = track
                .iter()
                .position(|ev| matches!(ev.kind, PlaybackEventKind::EventMarker(4)))
                .unwrap();
            let accumulated_ticks: u32 = track[..=second_marker_idx]
                .iter()
                .map(|ev| ev.delta_ticks)
                .sum();
            assert!(
                accumulated_ticks > 0,
                "Subsequent events must have positive accumulated delta ticks"
            );
        }
    }

    #[test]
    fn test_playback_track_grace_notes_timing() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - id: part1
    name: Guitar
    measures:
      - sequences:
          - content:
              - type: grace
                content:
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: G, octave: 4 }
                        string: 1
                        fret: 3
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: A, octave: 4 }
                        string: 1
                        fret: 5
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 4 }
                    string: 1
                    fret: 7
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 5 }
                    string: 1
                    fret: 8
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let mut marker_ticks: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        let mut curr_tick = 0;
        for ev in &track {
            curr_tick += ev.delta_ticks;
            if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                marker_ticks.insert(idx, curr_tick);
            }
        }

        assert_eq!(marker_ticks.get(&0), Some(&0), "Grace 1 at tick 0");
        assert_eq!(marker_ticks.get(&1), Some(&40), "Grace 2 at tick 40");
        assert_eq!(marker_ticks.get(&2), Some(&80), "Quarter 1 at tick 80");
        assert_eq!(marker_ticks.get(&3), Some(&480), "Quarter 2 at tick 480 - unaffected by grace notes!");
    }

    #[test]
    fn test_playback_track_grace_notes_steals_from_prior_note() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - id: part1
    name: Guitar
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 0
              - type: grace
                content:
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: G, octave: 4 }
                        _x:
                          string: 1
                          fret: 3
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: A, octave: 4 }
                        _x:
                          string: 1
                          fret: 5
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 4 }
                    _x:
                      string: 1
                      fret: 7
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 5 }
                    _x:
                      string: 1
                      fret: 8
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let mut marker_ticks: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        let mut curr_tick = 0;
        let mut note_offs: Vec<(u32, u8)> = Vec::new();
        for ev in &track {
            curr_tick += ev.delta_ticks;
            match &ev.kind {
                PlaybackEventKind::EventMarker(idx) => {
                    marker_ticks.insert(*idx, curr_tick);
                }
                PlaybackEventKind::Midi { message: midly::MidiMessage::NoteOff { key, .. }, .. } => {
                    note_offs.push((curr_tick, key.as_int()));
                }
                _ => {}
            }
        }

        // Quarter 1 (prior note) starts at tick 0
        assert_eq!(marker_ticks.get(&0), Some(&0), "Quarter 1 starts at tick 0");
        // Grace 1 starts at tick 400 (steals 80 ticks from Quarter 1: 480 - 80 = 400)
        assert_eq!(marker_ticks.get(&1), Some(&400), "Grace 1 starts at tick 400");
        // Grace 2 starts at tick 440
        assert_eq!(marker_ticks.get(&2), Some(&440), "Grace 2 starts at tick 440");
        // Quarter 2 starts at tick 480 (EXACT on-beat nominal start!)
        assert_eq!(marker_ticks.get(&3), Some(&480), "Quarter 2 starts at tick 480 on beat");
        // Quarter 3 starts at tick 960 (EXACT on-beat nominal start!)
        assert_eq!(marker_ticks.get(&4), Some(&960), "Quarter 3 starts at tick 960");

        // Quarter 1 (E4 = 64) has NoteOff at tick 400!
        // Grace 1 (G4 = 67) NoteOff at tick 440
        // Grace 2 (A4 = 69) NoteOff at tick 480
        // Quarter 2 (B4 = 71) NoteOff at tick 960 (full 480 duration!)
        assert!(note_offs.iter().any(|&(t, k)| t == 400 && k == 64), "Quarter 1 NoteOff at 400");
        assert!(note_offs.iter().any(|&(t, k)| t == 440 && k == 67), "Grace 1 NoteOff at 440");
        assert!(note_offs.iter().any(|&(t, k)| t == 480 && k == 69), "Grace 2 NoteOff at 480");
        assert!(note_offs.iter().any(|&(t, k)| t == 960 && k == 71), "Quarter 2 NoteOff at 960");
    }

    #[test]
    fn test_playback_track_grace_notes_across_measure_boundary() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - time: { count: 4, unit: 4 }
parts:
  - id: part1
    name: Guitar
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
      - sequences:
          - content:
              - type: grace
                content:
                  - duration: { base: eighth }
                    notes: [{ pitch: { step: G, octave: 4 }, _x: { string: 1, fret: 3 } }]
                  - duration: { base: eighth }
                    notes: [{ pitch: { step: A, octave: 4 }, _x: { string: 1, fret: 5 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: B, octave: 4 }, _x: { string: 1, fret: 7 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: C, octave: 5 }, _x: { string: 1, fret: 8 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: D, octave: 5 }, _x: { string: 1, fret: 10 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 5 }, _x: { string: 1, fret: 12 } }]
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let mut marker_ticks: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        let mut curr_tick = 0;
        let mut note_offs: Vec<(u32, u8)> = Vec::new();
        for ev in &track {
            curr_tick += ev.delta_ticks;
            match &ev.kind {
                PlaybackEventKind::EventMarker(idx) => {
                    marker_ticks.insert(*idx, curr_tick);
                }
                PlaybackEventKind::Midi { message: midly::MidiMessage::NoteOff { key, .. }, .. } => {
                    note_offs.push((curr_tick, key.as_int()));
                }
                _ => {}
            }
        }

        // Measure 1 notes: 0, 480, 960, 1440
        // Measure 1 note 4: nominal 1440..1920. Stolen 80 ticks: ends at 1840!
        // Grace 1 (index 4): starts at 1840
        // Grace 2 (index 5): starts at 1880
        // Measure 2 note 1 (index 6): starts at EXACTLY 1920 (measure 2 downbeat!)
        assert_eq!(marker_ticks.get(&3), Some(&1440), "M1 Note 4 starts at 1440");
        assert_eq!(marker_ticks.get(&4), Some(&1840), "Grace 1 starts at 1840");
        assert_eq!(marker_ticks.get(&5), Some(&1880), "Grace 2 starts at 1880");
        assert_eq!(marker_ticks.get(&6), Some(&1920), "M2 Note 1 starts on downbeat at 1920");

        assert!(note_offs.iter().any(|&(t, k)| t == 1840 && k == 64), "M1 Note 4 NoteOff at 1840");
        assert!(note_offs.iter().any(|&(t, k)| t == 1880 && k == 67), "Grace 1 NoteOff at 1880");
        assert!(note_offs.iter().any(|&(t, k)| t == 1920 && k == 69), "Grace 2 NoteOff at 1920");
        assert!(note_offs.iter().any(|&(t, k)| t == 2400 && k == 71), "M2 Note 1 NoteOff at 2400 (full 480 duration)");
    }

    #[test]
    fn test_harmonic_midi_rendering() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Standard Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: E, octave: 4 }
    measures:
      - sequences:
          - content:
              # Harmonic 1: touch 12.0 -> sounding E5 (MIDI 76). Without harmonic, fret 12 is E5 (MIDI 76).
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 12
                      harmonic:
                        touch: 12.0
                        soundingNote:
                          pitch: { step: E, octave: 5 }
              # Harmonic 2: touch 7.0 -> sounding B5 (MIDI 83). Without harmonic, fret 7 is B4 (MIDI 71)!
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 7
                      harmonic:
                        touch: 7.0
                        soundingNote:
                          pitch: { step: B, alter: 0.02, octave: 5 }
              # Harmonic 3: touch 5.0 -> sounding E6 (MIDI 88). Without harmonic, fret 5 is A4 (MIDI 69)!
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 5
                      harmonic:
                        touch: 5.0
                        soundingNote:
                          pitch: { step: E, octave: 6 }
              # Harmonic 4: touch 3.9 -> sounding G6 (alter 0.863, MIDI ~92). Without harmonic, fret 4 is G#4 (MIDI 68)!
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 4
                      harmonic:
                        touch: 3.9
                        soundingNote:
                          pitch: { step: G, alter: 0.863, octave: 6 }
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let mut note_ons: Vec<(u32, u8)> = Vec::new();
        let mut note_offs: Vec<(u32, u8)> = Vec::new();
        let mut curr_tick = 0;
        for ev in &track {
            curr_tick += ev.delta_ticks;
            match &ev.kind {
                PlaybackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { key, vel },
                    ..
                } => {
                    if vel.as_int() > 0 {
                        note_ons.push((curr_tick, key.as_int()));
                    } else {
                        note_offs.push((curr_tick, key.as_int()));
                    }
                }
                PlaybackEventKind::Midi {
                    message: midly::MidiMessage::NoteOff { key, .. },
                    ..
                } => {
                    note_offs.push((curr_tick, key.as_int()));
                }
                _ => {}
            }
        }

        // 4 quarter notes: starts at 0, 480, 960, 1440
        assert_eq!(note_ons.len(), 4);
        assert_eq!(note_ons[0], (0, 76), "Harmonic 1 sounds E5 (MIDI 76)");
        assert_eq!(
            note_ons[1],
            (480, 83),
            "Harmonic 2 sounds B5 (MIDI 83, not fretted B4 71)"
        );
        assert_eq!(
            note_ons[2],
            (960, 88),
            "Harmonic 3 sounds E6 (MIDI 88, not fretted A4 69)"
        );
        assert_eq!(
            note_ons[3],
            (1440, 92),
            "Harmonic 4 sounds G#6 (MIDI 92, not fretted G#4 68)"
        );

        // Durations: each quarter note has 480 ticks duration
        assert_eq!(note_offs.len(), 4);
        assert_eq!(note_offs[0], (480, 76), "Harmonic 1 ends at 480");
        assert_eq!(note_offs[1], (960, 83), "Harmonic 2 ends at 960");
        assert_eq!(note_offs[2], (1440, 88), "Harmonic 3 ends at 1440");
        assert_eq!(note_offs[3], (1920, 92), "Harmonic 4 ends at 1920");
    }

    #[test]
    fn test_harmonic_tied_midi_rendering() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Standard Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: E, octave: 4 }
    measures:
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - id: h1
                    pitch: { step: E, octave: 4 }
                    ties: [{ target: h2 }]
                    _x:
                      string: 1
                      fret: 7
                      harmonic:
                        touch: 7.0
                        soundingNote:
                          pitch: { step: B, alter: 0.02, octave: 5 }
              - duration: { base: half }
                notes:
                  - id: h2
                    pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 7
                      harmonic:
                        touch: 7.0
                        soundingNote:
                          pitch: { step: B, alter: 0.02, octave: 5 }
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, _) = generate_playback_track(&doc, MidiOptions::default());

        let mut note_ons: Vec<(u32, u8)> = Vec::new();
        let mut note_offs: Vec<(u32, u8)> = Vec::new();
        let mut curr_tick = 0;
        for ev in &track {
            curr_tick += ev.delta_ticks;
            match &ev.kind {
                PlaybackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { key, vel },
                    ..
                } => {
                    if vel.as_int() > 0 {
                        note_ons.push((curr_tick, key.as_int()));
                    } else {
                        note_offs.push((curr_tick, key.as_int()));
                    }
                }
                PlaybackEventKind::Midi {
                    message: midly::MidiMessage::NoteOff { key, .. },
                    ..
                } => {
                    note_offs.push((curr_tick, key.as_int()));
                }
                _ => {}
            }
        }

        // Tied harmonic: Only one NoteOn at tick 0 (key 83 = B5), and one NoteOff at tick 1920 (end of measure / both half notes)
        assert_eq!(note_ons.len(), 1, "Only one NoteOn for tied harmonic");
        assert_eq!(note_ons[0], (0, 83));
        assert_eq!(note_offs.len(), 1, "Only one NoteOff for tied harmonic");
        assert_eq!(
            note_offs[0],
            (1920, 83),
            "NoteOff sustained until end of tied note"
        );
    }

    #[test]
    fn test_example_harmonics_yaml_midi_rendering() {
        let yaml = include_str!("../examples/example_harmonics.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let (track, ppq) = generate_playback_track(&doc, MidiOptions::default());
        assert_eq!(ppq, 480.0);

        let note_ons: Vec<(u32, u8)> = track
            .iter()
            .scan(0, |curr_tick, ev| {
                *curr_tick += ev.delta_ticks;
                Some((*curr_tick, ev))
            })
            .filter_map(|(tick, ev)| {
                if let PlaybackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { key, vel },
                    ..
                } = ev.kind
                {
                    if vel.as_int() > 0 {
                        Some((tick, key.as_int()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        // Measure 1 notes (first 4 NoteOns):
        // ev1: E5 (76)
        // ev2: B5 (83)
        // ev3: E6 (88)
        // ev4: G#6 (92)
        assert_eq!(note_ons[0], (0, 76));
        assert_eq!(note_ons[1], (480, 83));
        assert_eq!(note_ons[2], (960, 88));
        assert_eq!(note_ons[3], (1440, 92));
    }

    #[test]
    fn test_compute_flattened_measures_endings() {
        let yaml = include_str!("../examples/example_endings.yaml");
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let flattened = compute_flattened_measures(&doc.global, 6);
        assert_eq!(flattened, vec![0, 1, 2, 0, 1, 2, 0, 3, 4, 5]);
    }

    #[test]
    fn test_compute_flattened_measures_simple_endings() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - repeatStart: {}
      time: { count: 4, unit: 4 }
    - ending: { duration: 1, numbers: [1] }
      repeatEnd: {}
    - ending: { duration: 1, numbers: [2] }
      repeatEnd: {}
    - ending: { duration: 1, numbers: [3], open: true }
parts:
  - measures:
      - sequences: []
      - sequences: []
      - sequences: []
      - sequences: []
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let flattened = compute_flattened_measures(&doc.global, 4);
        assert_eq!(flattened, vec![0, 1, 0, 2, 0, 3]);
    }

    #[test]
    fn test_compute_flattened_measures_standard_repeat() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - repeatStart: {}
      time: { count: 4, unit: 4 }
    - {}
    - repeatEnd: { times: 2 }
    - {}
parts:
  - measures:
      - sequences: []
      - sequences: []
      - sequences: []
      - sequences: []
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let flattened = compute_flattened_measures(&doc.global, 4);
        assert_eq!(flattened, vec![0, 1, 2, 0, 1, 2, 3]);
    }

    #[test]
    fn test_playback_track_endings_example() {
        let yaml = include_str!("../examples/example_endings.yaml");
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let (track, _) = generate_playback_track(&doc, MidiOptions::default());

        let markers: Vec<usize> = track
            .iter()
            .filter_map(|ev| {
                if let PlaybackEventKind::EventMarker(idx) = ev.kind {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(markers, vec![0, 1, 2, 3, 0, 1, 2, 3, 0, 4, 5, 6, 7]);
    }
}

pub fn walk_events_with_scaling<'a>(
    content: &'a [crate::mnx::SequenceItem],
    out: &mut Vec<(&'a crate::mnx::Event, f64)>,
    scale: f64,
) {
    for item in content {
        match item {
            crate::mnx::SequenceItem::Event(e) => out.push((e, scale)),
            crate::mnx::SequenceItem::Grace(g) => {
                walk_events_with_scaling(&g.content, out, 0.0);
            }
            crate::mnx::SequenceItem::Tuplet(t) => {
                let inner_beats = crate::validate::duration_to_quarter_beats(&t.inner.duration)
                    * (t.inner.multiple as f64);
                let outer_beats = crate::validate::duration_to_quarter_beats(&t.outer.duration)
                    * (t.outer.multiple as f64);
                let new_scale = if inner_beats > 0.0 {
                    scale * (outer_beats / inner_beats)
                } else {
                    scale
                };
                walk_events_with_scaling(&t.content, out, new_scale);
            }
        }
    }
}
