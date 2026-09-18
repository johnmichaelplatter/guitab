use serde::{Deserialize, Serialize};
use std::fmt;

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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Global {
    #[serde(default)]
    pub measures: Vec<GlobalMeasure>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GlobalMeasure {
    #[serde(default)]
    pub time: Option<TimeSignature>,
    #[serde(default)]
    pub directions: Vec<Direction>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Direction {
    pub tempo: Option<Tempo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Tempo {
    pub bpm: f64,
    pub value: Option<String>, // e.g. "quarter"
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
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
    pub micro_fret_divisions: Option<u8>, 
    #[serde(rename = "fretStepMultiplier")]
    pub fret_step_multiplier: Option<u8>,
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Duration {
    pub base: String,
    #[serde(default)]
    pub dots: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Note {
    pub pitch: Pitch,
    pub string: Option<u8>,
    pub fret: Option<u8>,
    pub accidental: Option<String>, 
}

/// Allows `alter` to be parsed from YAML as either a raw float (0.632) 
/// or an EDO fraction string ('1\19').
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum Alteration {
    Value(f64),
    Fraction(String),
}

impl Alteration {
    /// Evaluates the alteration into continuous standard semitones
    pub fn as_semitones(&self) -> f64 {
        match self {
            Alteration::Value(v) => *v,
            Alteration::Fraction(s) => {
                if let Some((num, den)) = s.split_once(|c| c == '\\' || c == '/') {
                    let steps = num.trim().parse::<f64>().unwrap_or(0.0);
                    let edo = den.trim().parse::<f64>().unwrap_or(12.0);
                    if edo != 0.0 {
                        return steps * (12.0 / edo);
                    }
                }
                0.0
            }
        }
    }
}

impl fmt::Display for Alteration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Alteration::Value(v) => write!(f, "{}", v),
            Alteration::Fraction(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Pitch {
    pub step: String,
    pub alter: Option<Alteration>, 
    pub octave: i32,
}

impl Pitch {
    #[allow(dead_code)]
    pub fn to_midi_note(&self) -> f64 {
        let step_val = match self.step.as_str() {
            "C" => 0.0, "D" => 2.0, "E" => 4.0, "F" => 5.0, "G" => 7.0, "A" => 9.0, "B" => 11.0,
            _ => 0.0, 
        };
        let alter_val = self.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);
        
        ((self.octave + 1) as f64) * 12.0 + step_val + alter_val
    }
}
impl Part {
    pub fn get_string_tuning(&self, string: u8) -> Option<Pitch> {
        if let Some(inst) = &self.instrument {
            if let Some(tunings) = &inst.tuning {
                if let Some(t) = tunings.iter().find(|t| t.string == string) {
                    return Some(t.pitch.clone());
                }
            }
        }
        
        let (step, octave) = match string {
            1 => ("E", 4),
            2 => ("B", 3),
            3 => ("G", 3),
            4 => ("D", 3),
            5 => ("A", 2),
            6 => ("E", 2),
            _ => return None,
        };
        
        Some(Pitch {
            step: step.to_string(),
            alter: None,
            octave,
        })
    }

    pub fn capo_fret(&self) -> u8 {
        self.instrument.as_ref().and_then(|i| i.capo).unwrap_or(0)
    }

    pub fn edo(&self) -> u32 {
        self.instrument.as_ref().and_then(|i| i.micro_fret_divisions).unwrap_or(12) as u32
    }

    pub fn step_multiplier(&self) -> i32 {
        self.instrument.as_ref().and_then(|i| i.fret_step_multiplier).unwrap_or(1) as i32
    }
    
    pub fn num_strings(&self) -> usize {
        self.instrument
            .as_ref()
            .and_then(|inst| inst.tuning.as_ref())
            .and_then(|tuning| tuning.iter().map(|t| t.string).max())
            .unwrap_or(6)
            .max(1) as usize
    }
}
