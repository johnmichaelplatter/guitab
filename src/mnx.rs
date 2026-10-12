use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_saphyr::{FlowMap, FlowSeq};

pub mod flow_opt {
    use super::*;
    pub fn serialize<T: Serialize, S: Serializer>(
        val: &Option<T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match val {
            Some(v) => FlowMap(v).serialize(s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<T>, D::Error> {
        Option::<T>::deserialize(d)
    }
}

pub mod flow_req {
    use super::*;
    pub fn serialize<T: Serialize, S: Serializer>(val: &T, s: S) -> Result<S::Ok, S::Error> {
        FlowMap(val).serialize(s)
    }
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<T, D::Error> {
        T::deserialize(d)
    }
}

pub mod flow_seq_req {
    use super::*;
    pub fn serialize<T: Serialize, S: Serializer>(val: &T, s: S) -> Result<S::Ok, S::Error> {
        FlowSeq(val).serialize(s)
    }
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<T, D::Error> {
        T::deserialize(d)
    }
}
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstPath {
    TopLevel(usize, usize, usize),
    InTuplet(usize, usize, usize, usize),
}

impl AstPath {
    pub fn m_idx(&self) -> usize {
        match self {
            AstPath::TopLevel(m, _, _) => *m,
            AstPath::InTuplet(m, _, _, _) => *m,
        }
    }
}

impl MnxDocument {
    pub fn get_flattened_event_paths(&self) -> Vec<AstPath> {
        let mut flattened_events = Vec::new();
        if let Some(part) = self.parts.get(0) {
            for (m_idx, measure) in part.measures.iter().enumerate() {
                for (s_idx, seq) in measure.sequences.iter().enumerate() {
                    for (i_idx, item) in seq.content.iter().enumerate() {
                        match item {
                            crate::mnx::SequenceItem::Event(_) => {
                                flattened_events.push(AstPath::TopLevel(m_idx, s_idx, i_idx));
                            }
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                for (t_idx, inner) in t.content.iter().enumerate() {
                                    if let crate::mnx::SequenceItem::Event(_) = inner {
                                        flattened_events
                                            .push(AstPath::InTuplet(m_idx, s_idx, i_idx, t_idx));
                                    }
                                }
                            }
                            crate::mnx::SequenceItem::Grace(g) => {
                                for (g_idx, inner) in g.content.iter().enumerate() {
                                    if let crate::mnx::SequenceItem::Event(_) = inner {
                                        flattened_events
                                            .push(AstPath::InTuplet(m_idx, s_idx, i_idx, g_idx));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        flattened_events
    }

    pub fn get_event_at_path(&self, path: AstPath) -> Option<&Event> {
        match path {
            AstPath::TopLevel(m, s, i) => {
                if let SequenceItem::Event(e) = self
                    .parts
                    .get(0)?
                    .measures
                    .get(m)?
                    .sequences
                    .get(s)?
                    .content
                    .get(i)?
                {
                    Some(e)
                } else {
                    None
                }
            }
            AstPath::InTuplet(m, s, i, t) => {
                let item = self
                    .parts
                    .get(0)?
                    .measures
                    .get(m)?
                    .sequences
                    .get(s)?
                    .content
                    .get(i)?;
                match item {
                    SequenceItem::Tuplet(tu) => {
                        if let SequenceItem::Event(e) = tu.content.get(t)? {
                            Some(e)
                        } else {
                            None
                        }
                    }
                    SequenceItem::Grace(gr) => {
                        if let SequenceItem::Event(e) = gr.content.get(t)? {
                            Some(e)
                        } else {
                            None
                        }
                    }
                    SequenceItem::Event(_) => None,
                }
            }
        }
    }

    pub fn get_event_mut_at_path(&mut self, path: AstPath) -> Option<&mut Event> {
        match path {
            AstPath::TopLevel(m, s, i) => {
                if let SequenceItem::Event(e) = self
                    .parts
                    .get_mut(0)?
                    .measures
                    .get_mut(m)?
                    .sequences
                    .get_mut(s)?
                    .content
                    .get_mut(i)?
                {
                    Some(e)
                } else {
                    None
                }
            }
            AstPath::InTuplet(m, s, i, t) => {
                let item = self
                    .parts
                    .get_mut(0)?
                    .measures
                    .get_mut(m)?
                    .sequences
                    .get_mut(s)?
                    .content
                    .get_mut(i)?;
                match item {
                    SequenceItem::Tuplet(tu) => {
                        if let SequenceItem::Event(e) = tu.content.get_mut(t)? {
                            Some(e)
                        } else {
                            None
                        }
                    }
                    SequenceItem::Grace(gr) => {
                        if let SequenceItem::Event(e) = gr.content.get_mut(t)? {
                            Some(e)
                        } else {
                            None
                        }
                    }
                    SequenceItem::Event(_) => None,
                }
            }
        }
    }
}

impl MnxDocument {
    pub fn clean_dangling_ties(&mut self) {
        let mut all_note_ids = std::collections::HashSet::new();

        for part in &self.parts {
            for measure in &part.measures {
                for seq in &measure.sequences {
                    for ev in seq.flattened_events() {
                        for note in &ev.notes {
                            if let Some(id) = &note.id {
                                all_note_ids.insert(id.clone());
                            }
                        }
                    }
                }
            }
        }

        macro_rules! traverse_mut {
            ($content:expr) => {
                fn walk(
                    content: &mut Vec<SequenceItem>,
                    all_note_ids: &std::collections::HashSet<String>,
                ) {
                    for item in content {
                        match item {
                            SequenceItem::Event(ev) => {
                                for note in &mut ev.notes {
                                    if let Some(ties) = &mut note.ties {
                                        ties.retain(|tie| all_note_ids.contains(&tie.target));
                                        if ties.is_empty() {
                                            note.ties = None;
                                        }
                                    }
                                }
                            }
                            SequenceItem::Tuplet(t) => {
                                walk(&mut t.content, all_note_ids);
                            }
                            SequenceItem::Grace(g) => {
                                walk(&mut g.content, all_note_ids);
                            }
                        }
                    }
                }
                walk($content, &all_note_ids);
            };
        }

        for part in &mut self.parts {
            for measure in &mut part.measures {
                for seq in &mut measure.sequences {
                    traverse_mut!(&mut seq.content);

                    // Merge adjacent SequenceItem::Grace items
                    let mut i = 0;
                    while i + 1 < seq.content.len() {
                        if matches!((&seq.content[i], &seq.content[i + 1]), (SequenceItem::Grace(_), SequenceItem::Grace(_))) {
                            let next_item = seq.content.remove(i + 1);
                            if let (SequenceItem::Grace(g1), SequenceItem::Grace(g2)) = (&mut seq.content[i], next_item) {
                                g1.content.extend(g2.content);
                            }
                        } else {
                            i += 1;
                        }
                    }
                }
            }
        }
    }
}

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

fn default_repeat_times() -> u32 {
    2
}

fn default_ending_duration() -> u32 {
    1
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Ending {
    #[serde(default = "default_ending_duration")]
    pub duration: u32,
    #[serde(with = "flow_seq_req")]
    pub numbers: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct GlobalMeasure {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub time: Option<TimeSignature>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "repeatStart"
    )]
    pub repeat_start: Option<RepeatStart>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "repeatEnd"
    )]
    pub repeat_end: Option<RepeatEnd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barline: Option<Barline>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub ending: Option<Ending>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tempos: Vec<Tempo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "tempoChange")]
    pub tempo_changes: Vec<TempoChange>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RepeatStart {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RepeatEnd {
    #[serde(default = "default_repeat_times")]
    pub times: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Barline {
    #[serde(rename = "type")]
    pub barline_type: String,
}


#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TempoChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tempo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub bpm: f64,
    pub value: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub count: u32,
    pub unit: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<Instrument>,
    pub measures: Vec<Measure>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Instrument {
    pub name: String,
    #[serde(
        rename = "microFretDivisions",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub micro_fret_divisions: Option<u8>,
    #[serde(
        rename = "fretStepMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fret_step_multiplier: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capo: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<Vec<StringTuning>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct StringTuning {
    pub string: u8,
    #[serde(with = "flow_req")]
    pub pitch: Pitch,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Position {
    pub fraction: [u32; 2],
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Span {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Arpeggio {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(default)]
    pub arrow: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct BeamGroup {
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "flow_seq_req")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arpeggios: Option<Vec<Arpeggio>>,
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
    Grace(Grace),
    Event(Event),
}


#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Grace {
    #[serde(rename = "type")]
    pub item_type: String, // will always be "grace"
    pub content: Vec<SequenceItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tuplet {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    pub inner: TupletDuration,
    pub outer: TupletDuration,
    pub content: Vec<SequenceItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TupletDuration {
    #[serde(with = "flow_req")]
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
    pub markings: Option<EventMarkings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(with = "flow_req")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dots: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tie {
    pub target: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tap {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Slide {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(with = "flow_req")]
    pub pitch: Pitch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fret: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accidental: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ties: Option<Vec<Tie>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub tap: Option<Tap>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub slide: Option<Slide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harmonic: Option<Harmonic>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Harmonic {
    pub touch: f64,
    #[serde(rename = "soundingNote")]
    pub sounding_note: Box<Note>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
            SequenceItem::Grace(_) => 0.0,
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
            SequenceItem::Grace(g) => {
                for inner in &g.content {
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

#[derive(Debug, Clone)]
pub struct GraceInfo<'a> {
    pub grace: &'a Grace,
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
                        SequenceItem::Grace(g) => {
                            let mut grace_events = Vec::new();
                            for inner_item in &g.content {
                                inner_item.collect_events(&mut grace_events);
                            }
                            let count = grace_events.len();
                            let start = cur_e;
                            let end = cur_e + count;
                            if flat_event_idx >= start && flat_event_idx < end {
                                return None; // Grace is not a Tuplet
                            }
                            cur_e = end;
                        }
                    }
                }
            }
        }
        None
    }

    pub fn find_event_grace_info(&self, flat_event_idx: usize) -> Option<GraceInfo<'_>> {
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
                                return None; // Tuplet is not a Grace
                            }
                            cur_e = end;
                        }
                        SequenceItem::Grace(g) => {
                            let mut grace_events = Vec::new();
                            for inner_item in &g.content {
                                inner_item.collect_events(&mut grace_events);
                            }
                            let count = grace_events.len();
                            let start = cur_e;
                            let end = cur_e + count;
                            if flat_event_idx >= start && flat_event_idx < end {
                                return Some(GraceInfo {
                                    grace: g,
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

    pub fn get_prior_ending_max(&self, start_m: usize) -> Option<u32> {
        if start_m == 0 {
            return None;
        }
        for k in (0..start_m).rev() {
            if let Some(gm) = self.global.measures.get(k) {
                if let Some(ending) = &gm.ending {
                    let end_cov = k + ending.duration as usize;
                    if end_cov == start_m {
                        return ending.numbers.iter().max().copied();
                    }
                }
            }
        }
        if let Some(gm) = self.global.measures.get(start_m - 1) {
            if let Some(ending) = &gm.ending {
                return ending.numbers.iter().max().copied();
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
            assert_eq!(ev1.notes[0].ties.as_ref().unwrap()[0].target, "n2");
            assert_eq!(ev1.slurs.as_ref().unwrap()[0].target, "ev2");
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

    #[test]
    fn test_parse_and_serialize_tap() {
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
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    string: 1
                    fret: 12
                    tap: {}
"#;
        let doc: MnxDocument =
            serde_saphyr::from_str(yaml).expect("Failed to deserialize MNX with tap");
        let note = &doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes[0];
        assert!(note.tap.is_some());

        let out = serde_saphyr::to_string(&doc).expect("Failed to serialize MNX with tap");
        assert!(out.contains("tap: {}"));
    }

    #[test]
    fn test_parse_and_serialize_endings() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - repeatStart: {}
      time: { count: 3, unit: 4 }
    - ending: { duration: 2, numbers: [1, 2] }
    - repeatEnd: {}
    - ending: { duration: 2, numbers: [3], open: true }
    - {}
    - {}
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: half, dots: 1 }
                notes: []
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).expect("Failed to deserialize MNX with endings");
        assert_eq!(doc.global.measures.len(), 6);
        assert!(doc.global.measures[0].repeat_start.is_some());
        
        let e1 = doc.global.measures[1].ending.as_ref().unwrap();
        assert_eq!(e1.duration, 2);
        assert_eq!(e1.numbers, vec![1, 2]);
        assert_eq!(e1.open, None);

        let r_end = doc.global.measures[2].repeat_end.as_ref().unwrap();
        assert_eq!(r_end.times, 2); // defaulted to 2

        let e2 = doc.global.measures[3].ending.as_ref().unwrap();
        assert_eq!(e2.duration, 2);
        assert_eq!(e2.numbers, vec![3]);
        assert_eq!(e2.open, Some(true));

        let out = serde_saphyr::to_string(&doc).expect("Failed to serialize MNX with endings");
        let doc2: MnxDocument = serde_saphyr::from_str(&out).expect("Failed to deserialize serialized endings");
        assert_eq!(doc, doc2);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct EventMarkings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<Accent>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "bowDirection"
    )]
    pub bow_direction: Option<BowDirection>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Accent {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct BowDirection {
    pub direction: String,
}
