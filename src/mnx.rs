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
    pub capo: Option<u8>,
    #[serde(rename = "microFretDivisions")]
    pub micro_fret_divisions: Option<u8>, // Determines frets per octave
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
    pub accidental: Option<String>, // Mirrors MusicXML accidentals (e.g., "quarter-flat")
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Pitch {
    pub step: String,
    pub alter: Option<f64>, // Now supports floats for microtonal precision
    pub octave: i32,
}

impl Pitch {
    /// Converts semantic pitch to continuous MIDI float (C4 = 60.0)
    pub fn to_midi_note(&self) -> f64 {
        let step_val = match self.step.as_str() {
            "C" => 0.0, "D" => 2.0, "E" => 4.0, "F" => 5.0, "G" => 7.0, "A" => 9.0, "B" => 11.0,
            _ => 0.0, 
        };
        let alter_val = self.alter.unwrap_or(0.0);
        
        ((self.octave + 1) as f64) * 12.0 + step_val + alter_val
    }
}