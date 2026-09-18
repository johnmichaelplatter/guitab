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
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            pitch_bend_range: 2,
            string_to_channel: HashMap::new(), // Populated dynamically if empty
            default_bpm: 120.0,
            ppq: u15::from(480),
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

        for measure in &part.measures {
            for seq in &measure.sequences {
                let mut seq_time_ticks = current_time_ticks;
                
                for event in &seq.content {
                    let beats = duration_to_quarter_beats(&event.duration);
                    let duration_ticks = (beats * (u16::from(options.ppq) as f64)).round() as u32;

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
}
