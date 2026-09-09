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

#[derive(Debug, Deserialize, Serialize)]
pub struct Pitch {
    pub step: String,
    pub octave: i32,
}