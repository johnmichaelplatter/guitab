use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MnxDocument {
    pub mnx: MnxMeta,
    pub global: Global,
    pub parts: Vec<Part>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MnxMeta {
    pub version: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct Global {
    #[serde(default)]
    pub measures: Vec<GlobalMeasure>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct GlobalMeasure {
    #[serde(default)]
    pub time: Option<TimeSignature>,
    #[serde(default)]
    pub directions: Vec<Direction>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Direction {
    pub tempo: Option<Tempo>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub value: Option<String>, // e.g. "quarter"
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub count: u32,
    pub unit: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Part {
    pub instrument: Option<Instrument>,
    pub measures: Vec<Measure>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Instrument {
    pub name: String,
    pub tuning: Option<Vec<StringTuning>>,
    pub capo: Option<u8>,
    #[serde(rename = "microFretDivisions")]
    pub micro_fret_divisions: Option<u8>,
    #[serde(rename = "fretStepMultiplier")]
    pub fret_step_multiplier: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct StringTuning {
    pub string: u8,
    pub pitch: Pitch,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct BeamGroup {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beams: Option<Vec<BeamGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Measure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beams: Option<Vec<BeamGroup>>,
    pub sequences: Vec<Sequence>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Sequence {
    pub content: Vec<SequenceItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beams: Option<Vec<BeamGroup>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum SequenceItem {
    Tuplet(Tuplet),
    Event(Event),
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tuplet {
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub inner: TupletDuration,
    pub outer: TupletDuration,
    pub content: Vec<SequenceItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TupletDuration {
    pub duration: Duration,
    #[serde(default = "default_multiple")]
    pub multiple: u32,
}

fn default_multiple() -> u32 {
    1
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Slur {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(rename = "startNote", default, skip_serializing_if = "Option::is_none")]
    pub start_note: Option<String>,
    #[serde(rename = "endNote", default, skip_serializing_if = "Option::is_none")]
    pub end_note: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Event {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub duration: Duration,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Rest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slurs: Option<Vec<Slur>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Rest {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Duration {
    pub base: String,
    #[serde(default)]
    pub dots: Option<u32>,
    #[serde(default)]
    pub multiple: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tie {
    pub target: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub pitch: Pitch,
    pub string: Option<u8>,
    pub fret: Option<i32>,
    pub accidental: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ties: Option<Vec<Tie>>,
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
            "C" => 0.0,
            "D" => 2.0,
            "E" => 4.0,
            "F" => 5.0,
            "G" => 7.0,
            "A" => 9.0,
            "B" => 11.0,
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
        self.instrument
            .as_ref()
            .and_then(|i| i.micro_fret_divisions)
            .unwrap_or(12) as u32
    }

    pub fn step_multiplier(&self) -> i32 {
        self.instrument
            .as_ref()
            .and_then(|i| i.fret_step_multiplier)
            .unwrap_or(1) as i32
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

impl Sequence {
    pub fn total_beats(&self) -> f64 {
        let mut total = 0.0;
        for item in &self.content {
            total += item.total_beats();
        }
        total
    }
    pub fn flattened_events(&self) -> Vec<&Event> {
        let mut result = Vec::new();
        for item in &self.content {
            item.collect_events(&mut result);
        }
        result
    }
}

impl SequenceItem {
    pub fn total_beats(&self) -> f64 {
        match self {
            SequenceItem::Event(e) => crate::validate::duration_to_quarter_beats(&e.duration),
            SequenceItem::Tuplet(t) => {
                crate::validate::duration_to_quarter_beats(&t.outer.duration)
                    * (t.outer.multiple as f64)
            }
        }
    }
    pub fn collect_events<'a>(&'a self, out: &mut Vec<&'a Event>) {
        match self {
            SequenceItem::Event(e) => out.push(e),
            SequenceItem::Tuplet(t) => {
                for inner in &t.content {
                    inner.collect_events(out);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TupletInfo<'a> {
    pub tuplet: &'a Tuplet,
    pub event_range: std::ops::Range<usize>,
    pub inner_event_idx: usize,
}

impl MnxDocument {
    pub fn find_event_tuplet_info(&self, flat_event_idx: usize) -> Option<TupletInfo<'_>> {
        let part = self.parts.first()?;
        let mut cur_e = 0;
        for measure in &part.measures {
            for seq in &measure.sequences {
                for item in &seq.content {
                    match item {
                        SequenceItem::Event(_) => {
                            if cur_e == flat_event_idx {
                                return None;
                            }
                            cur_e += 1;
                        }
                        SequenceItem::Tuplet(t) => {
                            let mut tuplet_events = Vec::new();
                            for inner_item in &t.content {
                                inner_item.collect_events(&mut tuplet_events);
                            }
                            let count = tuplet_events.len();
                            let start = cur_e;
                            let end = cur_e + count;
                            if flat_event_idx >= start && flat_event_idx < end {
                                return Some(TupletInfo {
                                    tuplet: t,
                                    event_range: start..end,
                                    inner_event_idx: flat_event_idx - start,
                                });
                            }
                            cur_e = end;
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mnx_ties_slurs_beams() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - beams:
          - events: [ev1, ev2]
        sequences:
          - content:
              - id: ev1
                duration: { base: eighth }
                notes:
                  - id: n1
                    pitch: { step: C, octave: 3 }
                    string: 6
                    fret: 8
                    ties:
                      - target: n2
                slurs:
                  - target: ev2
                    side: up
                    startNote: n1
                    endNote: n2
              - id: ev2
                duration: { base: eighth }
                notes:
                  - id: n2
                    pitch: { step: D, octave: 3 }
                    string: 6
                    fret: 10
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).expect("Failed to deserialize MNX");
        let measure = &doc.parts[0].measures[0];
        assert_eq!(
            measure.beams.as_ref().unwrap()[0].events,
            vec!["ev1".to_string(), "ev2".to_string()]
        );
        let seq = &measure.sequences[0];
        if let SequenceItem::Event(ev1) = &seq.content[0] {
            assert_eq!(ev1.id.as_deref(), Some("ev1"));
            assert_eq!(ev1.notes[0].id.as_deref(), Some("n1"));
            assert_eq!(
                ev1.notes[0].ties.as_ref().unwrap()[0].target,
                "n2"
            );
            assert_eq!(
                ev1.slurs.as_ref().unwrap()[0].target,
                "ev2"
            );
            assert_eq!(
                ev1.slurs.as_ref().unwrap()[0].start_note.as_deref(),
                Some("n1")
            );
            assert_eq!(
                ev1.slurs.as_ref().unwrap()[0].end_note.as_deref(),
                Some("n2")
            );
        } else {
            panic!("Expected Event at index 0");
        }
    }

    #[test]
    fn test_parse_tie_to_rest_let_ring() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: E, octave: 4 }
                    string: 1
                    fret: 0
                    ties:
                      - target: rest_ev
              - id: rest_ev
                duration: { base: quarter }
                rest: {}
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).expect("Failed to deserialize MNX");
        let seq = &doc.parts[0].measures[0].sequences[0];
        if let SequenceItem::Event(ev1) = &seq.content[0] {
            assert_eq!(ev1.notes[0].ties.as_ref().unwrap()[0].target, "rest_ev");
        } else {
            panic!("Expected Event at index 0");
        }
        if let SequenceItem::Event(ev2) = &seq.content[1] {
            assert!(ev2.rest.is_some());
            assert_eq!(ev2.id.as_deref(), Some("rest_ev"));
        } else {
            panic!("Expected Event at index 1");
        }
    }
}

