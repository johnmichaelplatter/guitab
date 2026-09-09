use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct MnxDocument {
    pub mnx: MnxMeta,
    pub global: Global,
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MnxMeta {
    pub version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Global {
    pub measures: Vec<GlobalMeasure>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalMeasure {
    pub time: TimeSignature,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TimeSignature {
    pub count: u32,
    pub unit: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Part {
    pub instrument: Option<Instrument>,
    pub measures: Vec<Measure>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Instrument {
    pub name: String,
    pub tuning: Option<Vec<StringTuning>>,
    pub capo: Option<u8>, // Added capo support
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StringTuning {
    pub string: u8,
    pub pitch: Pitch,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Measure {
    pub sequences: Vec<Sequence>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Sequence {
    pub content: Vec<Event>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Event {
    pub duration: Duration,
    pub notes: Vec<Note>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Duration {
    pub base: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Note {
    pub pitch: Pitch,
    pub string: Option<u8>,
    pub fret: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Pitch {
    pub step: String,
    pub alter: Option<i32>, // Important for physical matching of non-natural notes
    pub octave: i32,
}

impl Pitch {
    /// Converts the MNX semantic pitch to an absolute MIDI note number (C4 = 60)
    pub fn to_midi_note(&self) -> i32 {
        let step_val = match self.step.as_str() {
            "C" => 0, "D" => 2, "E" => 4, "F" => 5, "G" => 7, "A" => 9, "B" => 11,
            _ => 0, 
        };
        let accidental = self.alter.unwrap_or(0);
        
        // MIDI standard: octave 0 starts at C0 (MIDI 12). So C4 = (4 + 1) * 12 + 0 = 60
        (self.octave + 1) * 12 + step_val + accidental
    }

    /// Helper for error messages and future autofix routines
    pub fn from_midi_note(note: i32) -> Self {
        let octave = (note / 12) - 1;
        let rem = note % 12;
        let (step, alter) = match rem {
            0 => ("C", 0), 1 => ("C", 1), 2 => ("D", 0), 3 => ("E", -1),
            4 => ("E", 0), 5 => ("F", 0), 6 => ("F", 1), 7 => ("G", 0),
            8 => ("A", -1), 9 => ("A", 0), 10 => ("B", -1), 11 => ("B", 0),
            _ => ("C", 0)
        };
        Pitch {
            step: step.to_string(),
            alter: if alter == 0 { None } else { Some(alter) },
            octave,
        }
    }
}