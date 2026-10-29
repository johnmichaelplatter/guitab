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
    pub fn score_title(&self) -> Option<&str> {
        self.global.title.as_deref().or_else(|| self.ext.title.as_deref())
    }

    pub fn score_subtitle(&self) -> Option<&str> {
        self.global.subtitle.as_deref().or_else(|| self.ext.subtitle.as_deref())
    }

    pub fn score_composer(&self) -> Option<&str> {
        self.global.composer.as_deref().or_else(|| self.ext.composer.as_deref())
    }

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

pub fn deserialize_ext<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ExtOrVendor<T> {
        Vendor { ursine: T },
        Direct(T),
    }
    Ok(Option::<ExtOrVendor<T>>::deserialize(d)?
        .map(|v| match v {
            ExtOrVendor::Vendor { ursine } => ursine,
            ExtOrVendor::Direct(t) => t,
        })
        .unwrap_or_default())
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct DocumentExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<ScoreLayout>,
}

impl DocumentExt {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.subtitle.is_none()
            && self.composer.is_none()
            && self.layout.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MnxDocument {
    pub mnx: MnxMeta,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "DocumentExt::is_empty"
    )]
    pub ext: DocumentExt,
    pub global: Global,
    pub parts: Vec<Part>,
}

impl std::ops::Deref for MnxDocument {
    type Target = DocumentExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for MnxDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
}

fn default_page_size() -> String {
    "letter".to_string()
}
fn default_margin_top_bottom() -> f32 {
    40.0
}
fn default_margin_left_right() -> f32 {
    45.0
}
fn default_measure_numbers() -> String {
    "systemStart".to_string()
}
fn default_false() -> bool {
    false
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ScoreLayout {
    #[serde(default = "default_page_size", rename = "pageSize")]
    pub page_size: String,
    #[serde(default)]
    pub margins: PageMargins,
    #[serde(default = "default_measure_numbers", rename = "measureNumbers")]
    pub measure_numbers: String,
    #[serde(default = "default_false", rename = "justifyLastSystem")]
    pub justify_last_system: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub offsets: std::collections::HashMap<String, Offset2D>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "lockedSystems")]
    pub locked_systems: Vec<[usize; 2]>,
}

impl Default for ScoreLayout {
    fn default() -> Self {
        Self {
            page_size: default_page_size(),
            margins: PageMargins::default(),
            measure_numbers: default_measure_numbers(),
            justify_last_system: false,
            title: None,
            subtitle: None,
            composer: None,
            offsets: std::collections::HashMap::new(),
            locked_systems: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PageMargins {
    #[serde(default = "default_margin_top_bottom")]
    pub top: f32,
    #[serde(default = "default_margin_top_bottom")]
    pub bottom: f32,
    #[serde(default = "default_margin_left_right")]
    pub left: f32,
    #[serde(default = "default_margin_left_right")]
    pub right: f32,
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top: default_margin_top_bottom(),
            bottom: default_margin_top_bottom(),
            left: default_margin_left_right(),
            right: default_margin_left_right(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
pub struct Offset2D {
    pub dx: f32,
    pub dy: f32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MnxMeta {
    pub version: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ReferenceNoteSpec {
    #[serde(with = "flow_req")]
    pub pitch: Pitch,
    #[serde(rename = "frequencyHz")]
    pub frequency_hz: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct GlobalTuningExt {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "referenceNote"
    )]
    pub reference_note: Option<ReferenceNoteSpec>,
}

impl GlobalTuningExt {
    pub fn is_empty(&self) -> bool {
        self.reference_note.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct GlobalExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<GlobalTuningExt>,
}

impl GlobalExt {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.subtitle.is_none()
            && self.composer.is_none()
            && self.tuning.as_ref().map(|t| t.is_empty()).unwrap_or(true)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct Global {
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "GlobalExt::is_empty"
    )]
    pub ext: GlobalExt,
    #[serde(default)]
    pub measures: Vec<GlobalMeasure>,
}

impl std::ops::Deref for Global {
    type Target = GlobalExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for Global {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
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
pub struct GlobalMeasureExt {
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "tempoChange")]
    pub tempo_changes: Vec<TempoChange>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "systemBreak"
    )]
    pub system_break: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "pageBreak"
    )]
    pub page_break: Option<bool>,
}

impl GlobalMeasureExt {
    pub fn is_empty(&self) -> bool {
        self.tempo_changes.is_empty()
            && self.system_break.is_none()
            && self.page_break.is_none()
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "GlobalMeasureExt::is_empty"
    )]
    pub ext: GlobalMeasureExt,
}

impl std::ops::Deref for GlobalMeasure {
    type Target = GlobalMeasureExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for GlobalMeasure {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
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
    #[serde(with = "flow_req")]
    pub value: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub location: Option<Position>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub count: u32,
    pub unit: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct PartExt {
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
    #[serde(
        rename = "pitchBendRange",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pitch_bend_range: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capo: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<Vec<StringTuning>>,
    #[serde(
        rename = "customFrets",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_frets: Option<Vec<crate::fretboard_editor::CustomFret>>,
}

impl PartExt {
    pub fn is_empty(&self) -> bool {
        self.micro_fret_divisions.is_none()
            && self.fret_step_multiplier.is_none()
            && self.pitch_bend_range.is_none()
            && self.capo.is_none()
            && self.tuning.is_none()
            && self.custom_frets.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "shortName", default, skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "PartExt::is_empty"
    )]
    pub ext: PartExt,
    #[serde(default)]
    pub measures: Vec<Measure>,
}

impl std::ops::Deref for Part {
    type Target = PartExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for Part {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct StringTuning {
    pub string: u8,
    #[serde(with = "flow_req")]
    pub pitch: Pitch,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Position {
    #[serde(with = "flow_seq_req")]
    pub fraction: [u32; 2],
}

impl Position {
    pub fn from_quarter_beats(offset: f64) -> Option<Self> {
        if offset > 0.001 {
            let num = (offset * 1000.0).round() as u32;
            let den = 4000;
            let mut a = num;
            let mut b = den;
            while b != 0 {
                let t = b;
                b = a % b;
                a = t;
            }
            Some(Self {
                fraction: [num / a, den / a],
            })
        } else {
            None
        }
    }

    pub fn from_quarter_beats_including_zero(offset: f64) -> Self {
        Self::from_quarter_beats(offset).unwrap_or(Self { fraction: [0, 1] })
    }

    pub fn to_quarter_beats(&self) -> f64 {
        if self.fraction[1] == 0 {
            0.0
        } else {
            (self.fraction[0] as f64 / self.fraction[1] as f64) * 4.0
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Span {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Arpeggio {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub position: Option<Position>,
    #[serde(with = "flow_req")]
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Measure {
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beams: Option<Vec<BeamGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arpeggios: Option<Vec<Arpeggio>>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Sequence {
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default)]
    pub content: Vec<SequenceItem>,
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

fn default_tuplet_type() -> Option<String> {
    Some("tuplet".to_string())
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Tuplet {
    #[serde(
        rename = "type",
        default = "default_tuplet_type",
        skip_serializing_if = "Option::is_none"
    )]
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct LyricLineExt {
    #[serde(default, alias = "extend", skip_serializing_if = "Option::is_none")]
    pub extender: Option<bool>,
}

impl LyricLineExt {
    pub fn is_empty(&self) -> bool {
        self.extender.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct LyricLine {
    pub text: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<String>,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "LyricLineExt::is_empty"
    )]
    pub ext: LyricLineExt,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct EventLyrics {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub lines: std::collections::BTreeMap<String, LyricLine>,
}

impl EventLyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct TextMarking {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "fontFamily"
    )]
    pub font_family: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "fontSize"
    )]
    pub font_size: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct EventExt {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "bowDirection"
    )]
    pub bow_direction: Option<BowDirection>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "textMarkings"
    )]
    pub text_markings: Vec<TextMarking>,
}

impl EventExt {
    pub fn is_empty(&self) -> bool {
        self.bow_direction.is_none() && self.text_markings.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<EventLyrics>,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "EventExt::is_empty"
    )]
    pub ext: EventExt,
}

impl std::ops::Deref for Event {
    type Target = EventExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for Event {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Rest {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Tap {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Slide {}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct AccidentalDisplay {
    pub show: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cautionary: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct NoteExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fret: Option<i32>,
    #[serde(
        rename = "decimalFret",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub decimal_fret: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub tap: Option<Tap>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub slide: Option<Slide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harmonic: Option<Harmonic>,
}

impl NoteExt {
    pub fn is_empty(&self) -> bool {
        self.muted.is_none()
            && self.string.is_none()
            && self.fret.is_none()
            && self.decimal_fret.is_none()
            && self.tap.is_none()
            && self.slide.is_none()
            && self.bend.is_none()
            && self.harmonic.is_none()
    }

    pub fn effective_fret(&self) -> Option<f64> {
        self.decimal_fret.or_else(|| self.fret.map(|f| f as f64))
    }

    pub fn formatted_fret(&self) -> Option<String> {
        if let Some(df) = self.decimal_fret {
            Some(crate::fretboard_editor::format_fret_number(df))
        } else {
            self.fret.map(|f| f.to_string())
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(with = "flow_req")]
    pub pitch: Pitch,
    #[serde(
        rename = "accidentalDisplay",
        default,
        skip_serializing_if = "Option::is_none",
        with = "flow_opt"
    )]
    pub accidental_display: Option<AccidentalDisplay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ties: Option<Vec<Tie>>,
    #[serde(rename = "_c", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(
        rename = "_x",
        default,
        deserialize_with = "deserialize_ext",
        skip_serializing_if = "NoteExt::is_empty"
    )]
    pub ext: NoteExt,
}

impl std::ops::Deref for Note {
    type Target = NoteExt;
    fn deref(&self) -> &Self::Target {
        &self.ext
    }
}

impl std::ops::DerefMut for Note {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ext
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Harmonic {
    pub touch: f64,
    #[serde(rename = "soundingNote")]
    pub sounding_note: Box<Note>,
}

/// Allows `alter` to be parsed from YAML as either a raw float (0.632)
/// or an EDO fraction string ('1\19') or ratio string ('5/4').
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
                if let Some((num, den)) = s.split_once('\\') {
                    let steps = num.trim().parse::<f64>().unwrap_or(0.0);
                    let edo = den.trim().parse::<f64>().unwrap_or(12.0);
                    if edo != 0.0 {
                        return steps * (12.0 / edo);
                    }
                } else if let Some((num, den)) = s.split_once('/') {
                    let p = num.trim().parse::<f64>().unwrap_or(0.0);
                    let q = den.trim().parse::<f64>().unwrap_or(1.0);
                    if p > 0.0 && q > 0.0 {
                        // Ratio offset from 1/1 in semitones: 12 * log2(p/q)
                        return 12.0 * (p / q).log2();
                    }
                }
                0.0
            }
        }
    }

    /// Converts an EDO fraction (`m\n`) or ratio (`p/q`) or float alteration into a 3-digit decimal
    /// `Alteration::Value` using `12 * ln(2^(m/n)) / ln(2)`.
    pub fn to_three_digit_decimal(&self) -> Alteration {
        let raw = self.as_semitones();
        let rounded = (raw * 1000.0).round() / 1000.0;
        let clean = if rounded == 0.0 { 0.0 } else { rounded };
        Alteration::Value(clean)
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct PitchExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl PitchExt {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Pitch {
    #[serde(with = "step_serde")]
    pub step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alter: Option<Alteration>,
    pub octave: i32,
    #[serde(
        rename = "_x",
        default,
        with = "flow_opt_pitch_ext",
        skip_serializing_if = "PitchExt::is_empty"
    )]
    pub ext: PitchExt,
}

pub mod step_serde {
    use super::*;
    use serde::de::{self, Visitor};

    pub fn serialize<S: Serializer>(val: &str, s: S) -> Result<S::Ok, S::Error> {
        if !val.contains('/') && !val.contains('\\') {
            let trimmed = val.trim();
            if !trimmed.is_empty()
                && trimmed
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
            {
                if let Ok(num) = trimmed.parse::<f64>() {
                    return s.serialize_f64(num);
                }
            }
        }
        s.serialize_str(val)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        struct StepVisitor;
        impl<'de> Visitor<'de> for StepVisitor {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a pitch step letter, ratio string, or cents number")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
                Ok(v.to_string())
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
                Ok(v)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<String, E> {
                if v.fract() == 0.0 {
                    Ok(format!("{:.1}", v))
                } else {
                    Ok(format!("{}", v))
                }
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
                Ok(format!("{:.1}", v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
                Ok(format!("{:.1}", v as f64))
            }
        }
        d.deserialize_any(StepVisitor)
    }
}

pub mod flow_opt_pitch_ext {
    use super::*;
    pub fn serialize<S: Serializer>(val: &PitchExt, s: S) -> Result<S::Ok, S::Error> {
        FlowMap(val).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PitchExt, D::Error> {
        super::deserialize_ext(d)
    }
}

impl Pitch {
    /// Returns true if `step` is specified as a Just Intonation ratio (e.g. `"4/3"`).
    pub fn is_ratio_step(&self) -> bool {
        self.step.contains('/')
    }

    /// Returns true if `step` is specified as a cents offset (e.g. `"498.0"`).
    pub fn is_cents_step(&self) -> bool {
        !self.step.contains('/')
            && !self.step.contains('\\')
            && self
                .step
                .trim()
                .trim_end_matches('c')
                .parse::<f64>()
                .is_ok()
    }

    /// Returns true if `step` is either a ratio (`"4/3"`) or cents (`"498.0"`) rather than `"A"`..`"G"`.
    pub fn is_ratio_or_cents_step(&self) -> bool {
        self.is_ratio_step() || self.is_cents_step()
    }

    /// Returns true if `alter` is a ratio string (`"p/q"`).
    pub fn has_ratio_alter(&self) -> bool {
        matches!(&self.alter, Some(Alteration::Fraction(s)) if s.contains('/'))
    }

    /// If `step` is a ratio (`"4/3"`) or cents (`"498.0"`), returns its cents offset above `1/1`.
    pub fn step_cents_above_reference(&self) -> Option<f64> {
        let s = self.step.trim();
        if let Some((num_s, den_s)) = s.split_once('/') {
            let p = num_s.trim().parse::<f64>().ok()?;
            let q = den_s.trim().parse::<f64>().ok()?;
            if p > 0.0 && q > 0.0 {
                return Some(1200.0 * (p / q).log2());
            }
            None
        } else {
            let clean = s.trim_end_matches('c').trim();
            clean.parse::<f64>().ok()
        }
    }

    #[allow(dead_code)]
    pub fn to_midi_note(&self) -> f64 {
        self.to_midi_note_with_ref(None)
    }

    /// Computes the continuous 12-EDO MIDI note number (where `C4` = 60.0),
    /// resolving ratio/cents `step` values against `ref_note` when provided.
    pub fn to_midi_note_with_ref(&self, ref_note: Option<&ReferenceNoteSpec>) -> f64 {
        let step_val = if let Some(cents) = self.step_cents_above_reference() {
            let ref_semi = if let Some(rn) = ref_note {
                let nom = crate::fretboard_editor::extract_nominal_from_note_name(&rn.pitch.step);
                let alter_semi = rn
                    .pitch
                    .alter
                    .as_ref()
                    .map(|a| a.as_semitones())
                    .unwrap_or(0.0);
                crate::fretboard_editor::nominal_12edo_semitones(nom) + alter_semi
            } else {
                0.0
            };
            ref_semi + (cents / 100.0)
        } else {
            match self.step.as_str() {
                "C" => 0.0,
                "D" => 2.0,
                "E" => 4.0,
                "F" => 5.0,
                "G" => 7.0,
                "A" => 9.0,
                "B" => 11.0,
                other => {
                    let nom = crate::fretboard_editor::extract_nominal_from_note_name(other);
                    crate::fretboard_editor::nominal_12edo_semitones(nom)
                }
            }
        };
        let alter_val = self.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);

        ((self.octave + 1) as f64) * 12.0 + step_val + alter_val
    }

    /// Converts any `Pitch` (including ratio/cents `step` or ratio `alter`) into a standard
    /// 12-EDO MNX `Pitch` with `step` in `"A"`..`"G"` and `alter` rounded to 3 decimal places.
    pub fn to_standard_12edo_pitch(&self, ref_note: Option<&ReferenceNoteSpec>) -> Pitch {
        if !self.is_ratio_or_cents_step() && !self.has_ratio_alter() {
            return Pitch {
                step: self.step.clone(),
                alter: self.alter.as_ref().map(|a| a.to_three_digit_decimal()),
                octave: self.octave,
                ext: self.ext.clone(),
            };
        }

        let total_semitones_from_c0 = self.to_midi_note_with_ref(ref_note) - 12.0;
        let nom = if let Some(name) = &self.ext.name {
            crate::fretboard_editor::extract_nominal_from_note_name(name)
        } else if !self.is_ratio_or_cents_step() {
            crate::fretboard_editor::extract_nominal_from_note_name(&self.step)
        } else {
            // Find nearest natural 12-EDO pitch class
            let semitone_in_oct = total_semitones_from_c0.rem_euclid(12.0);
            let candidates = [
                ('C', 0.0),
                ('D', 2.0),
                ('E', 4.0),
                ('F', 5.0),
                ('G', 7.0),
                ('A', 9.0),
                ('B', 11.0),
            ];
            candidates
                .iter()
                .min_by(|(_, a), (_, b)| {
                    let da = (semitone_in_oct - *a)
                        .abs()
                        .min(12.0 - (semitone_in_oct - *a).abs());
                    let db = (semitone_in_oct - *b)
                        .abs()
                        .min(12.0 - (semitone_in_oct - *b).abs());
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(c, _)| *c)
                .unwrap_or('C')
        };

        let nom_semi = crate::fretboard_editor::nominal_12edo_semitones(nom);
        let octave = ((total_semitones_from_c0 - nom_semi) / 12.0).round() as i32;
        let base_semi = (octave as f64) * 12.0 + nom_semi;
        let raw_alter = total_semitones_from_c0 - base_semi;
        let rounded = (raw_alter * 1000.0).round() / 1000.0;
        let alter = if rounded.abs() < 1e-6 {
            None
        } else {
            Some(Alteration::Value(if rounded == 0.0 {
                0.0
            } else {
                rounded
            }))
        };

        Pitch {
            step: nom.to_string(),
            alter,
            octave,
            ext: self.ext.clone(),
        }
    }
}
impl Part {
    pub fn get_string_tuning(&self, string: u8) -> Option<Pitch> {
        if let Some(tunings) = &self.ext.tuning {
            if !tunings.is_empty() {
                return tunings
                    .iter()
                    .find(|t| t.string == string)
                    .map(|t| t.pitch.clone());
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
            ext: PitchExt::default(),
        })
    }

    pub fn capo_fret(&self) -> u8 {
        self.ext.capo.unwrap_or(0)
    }

    pub fn edo(&self) -> u32 {
        self.ext.micro_fret_divisions.unwrap_or(12) as u32
    }

    pub fn step_multiplier(&self) -> i32 {
        self.ext.fret_step_multiplier.unwrap_or(1) as i32
    }

    pub fn num_strings(&self) -> usize {
        self.ext
            .tuning
            .as_ref()
            .and_then(|tuning| tuning.iter().map(|t| t.string).max())
            .unwrap_or(6)
            .max(1) as usize
    }

    pub fn has_custom_frets(&self) -> bool {
        self.ext
            .custom_frets
            .as_ref()
            .map(|f| !f.is_empty())
            .unwrap_or(false)
    }

    pub fn clean_unneeded_fret_fields(&mut self) {
        if self.has_custom_frets() {
            self.ext.micro_fret_divisions = None;
            self.ext.fret_step_multiplier = None;
        }
    }
}

impl MnxDocument {
    pub fn clean_unneeded_fret_fields(&mut self) {
        for part in &mut self.parts {
            part.clean_unneeded_fret_fields();
        }
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

    /// Converts all `Pitch` objects across the document (tuning, notes, and harmonic sounding notes)
    /// into standard 12-EDO pitch classes (`A`–`G`) with `alter` rounded to 3 decimal places.
    pub fn prepare_for_mnx_export(&mut self) {
        let ref_note = self
            .global
            .ext
            .tuning
            .as_ref()
            .and_then(|t| t.reference_note.as_ref())
            .cloned();

        fn convert_note(note: &mut Note, ref_note: Option<&ReferenceNoteSpec>) {
            note.pitch = note.pitch.to_standard_12edo_pitch(ref_note);
            if let Some(h) = &mut note.ext.harmonic {
                convert_note(&mut h.sounding_note, ref_note);
            }
        }

        fn convert_items(items: &mut [SequenceItem], ref_note: Option<&ReferenceNoteSpec>) {
            for item in items {
                match item {
                    SequenceItem::Event(ev) => {
                        for note in &mut ev.notes {
                            convert_note(note, ref_note);
                        }
                    }
                    SequenceItem::Tuplet(t) => convert_items(&mut t.content, ref_note),
                    SequenceItem::Grace(g) => convert_items(&mut g.content, ref_note),
                }
            }
        }

        for part in &mut self.parts {
            if let Some(tunings) = &mut part.ext.tuning {
                for t in tunings {
                    t.pitch = t.pitch.to_standard_12edo_pitch(ref_note.as_ref());
                }
            }
            for measure in &mut part.measures {
                for seq in &mut measure.sequences {
                    convert_items(&mut seq.content, ref_note.as_ref());
                }
            }
        }
    }
}

/// Parses `yaml_text` (automatically stripping any `#comments`), converts all `alter` fields
/// (`m\n`) to 3-digit decimals via `12 * ln(2^(m/n)) / ln(2)`, and serializes to pretty-printed `.mnx` JSON.
pub fn export_yaml_to_mnx_json(yaml_text: &str) -> Result<String, String> {
    let mut doc: MnxDocument =
        serde_saphyr::from_str(yaml_text).map_err(|e| format!("YAML parse error: {}", e))?;
    doc.prepare_for_mnx_export();
    to_mnx_json_string(&doc)
}

/// Serializes an `MnxDocument` to pretty-printed JSON.
pub fn to_mnx_json_string<T: Serialize>(value: &T) -> Result<String, String> {
    let mut ser = JsonSerializer {
        out: String::new(),
        indent: 0,
    };
    value.serialize(&mut ser).map_err(|e| e.0)?;
    ser.out.push('\n');
    Ok(ser.out)
}

#[derive(Debug)]
struct JsonError(String);

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JsonError {}

impl serde::ser::Error for JsonError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        JsonError(msg.to_string())
    }
}

struct JsonSerializer {
    out: String,
    indent: usize,
}

impl JsonSerializer {
    fn push_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    fn write_escaped_str(&mut self, v: &str) {
        self.out.push('"');
        for ch in v.chars() {
            match ch {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\x08' => self.out.push_str("\\b"),
                '\x0c' => self.out.push_str("\\f"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    self.out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }
}

struct JsonSeqSerializer<'a> {
    ser: &'a mut JsonSerializer,
    first: bool,
}

struct JsonMapSerializer<'a> {
    ser: &'a mut JsonSerializer,
    first: bool,
}

impl<'a> Serializer for &'a mut JsonSerializer {
    type Ok = ();
    type Error = JsonError;
    type SerializeSeq = JsonSeqSerializer<'a>;
    type SerializeTuple = JsonSeqSerializer<'a>;
    type SerializeTupleStruct = JsonSeqSerializer<'a>;
    type SerializeTupleVariant = serde::ser::Impossible<(), JsonError>;
    type SerializeMap = JsonMapSerializer<'a>;
    type SerializeStruct = JsonMapSerializer<'a>;
    type SerializeStructVariant = serde::ser::Impossible<(), JsonError>;

    fn serialize_bool(self, v: bool) -> Result<(), Self::Error> {
        self.out.push_str(if v { "true" } else { "false" });
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<(), Self::Error> {
        self.out.push_str(&v.to_string());
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<(), Self::Error> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f64(self, v: f64) -> Result<(), Self::Error> {
        let rounded = (v * 1000.0).round() / 1000.0;
        let clean = if rounded == 0.0 { 0.0 } else { rounded };
        if (clean - clean.round()).abs() < 1e-9 {
            self.out.push_str(&(clean.round() as i64).to_string());
        } else {
            self.out.push_str(&clean.to_string());
        }
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<(), Self::Error> {
        let mut buf = [0u8; 4];
        self.write_escaped_str(v.encode_utf8(&mut buf));
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<(), Self::Error> {
        self.write_escaped_str(v);
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<(), Self::Error> {
        Err(JsonError("bytes unsupported".to_string()))
    }

    fn serialize_none(self) -> Result<(), Self::Error> {
        self.out.push_str("null");
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Self::Error> {
        self.out.push_str("{}");
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Self::Error> {
        self.out.push_str("{}");
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<(), Self::Error> {
        self.write_escaped_str(variant);
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.out.push('[');
        self.indent += 1;
        Ok(JsonSeqSerializer {
            ser: self,
            first: true,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(JsonError("tuple variant unsupported".to_string()))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.out.push('{');
        self.indent += 1;
        Ok(JsonMapSerializer {
            ser: self,
            first: true,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(JsonError("struct variant unsupported".to_string()))
    }
}

impl<'a> serde::ser::SerializeSeq for JsonSeqSerializer<'a> {
    type Ok = ();
    type Error = JsonError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        if !self.first {
            self.ser.out.push(',');
        }
        self.first = false;
        self.ser.out.push('\n');
        self.ser.push_indent();
        value.serialize(&mut *self.ser)
    }

    fn end(self) -> Result<(), Self::Error> {
        self.ser.indent -= 1;
        if !self.first {
            self.ser.out.push('\n');
            self.ser.push_indent();
        }
        self.ser.out.push(']');
        Ok(())
    }
}

impl<'a> serde::ser::SerializeTuple for JsonSeqSerializer<'a> {
    type Ok = ();
    type Error = JsonError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl<'a> serde::ser::SerializeTupleStruct for JsonSeqSerializer<'a> {
    type Ok = ();
    type Error = JsonError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl<'a> serde::ser::SerializeMap for JsonMapSerializer<'a> {
    type Ok = ();
    type Error = JsonError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        if !self.first {
            self.ser.out.push(',');
        }
        self.first = false;
        self.ser.out.push('\n');
        self.ser.push_indent();
        key.serialize(&mut *self.ser)?;
        self.ser.out.push_str(": ");
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.ser)
    }

    fn end(self) -> Result<(), Self::Error> {
        self.ser.indent -= 1;
        if !self.first {
            self.ser.out.push('\n');
            self.ser.push_indent();
        }
        self.ser.out.push('}');
        Ok(())
    }
}

impl<'a> serde::ser::SerializeStruct for JsonMapSerializer<'a> {
    type Ok = ();
    type Error = JsonError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        if key == "_x" {
            serde::ser::SerializeMap::serialize_key(self, key)?;
            self.ser.out.push_str("{\n");
            self.ser.indent += 1;
            self.ser.push_indent();
            self.ser.out.push_str("\"ursine\": ");
            value.serialize(&mut *self.ser)?;
            self.ser.indent -= 1;
            self.ser.out.push('\n');
            self.ser.push_indent();
            self.ser.out.push('}');
            Ok(())
        } else {
            serde::ser::SerializeMap::serialize_entry(self, key, value)
        }
    }

    fn end(self) -> Result<(), Self::Error> {
        serde::ser::SerializeMap::end(self)
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
                    ties:
                      - target: n2
                    _x:
                      string: 6
                      fret: 8
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
                    _x:
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
                    ties:
                      - target: rest_ev
                    _x:
                      string: 1
                      fret: 0
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
                    _x:
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

    #[test]
    fn test_lyrics_yaml_roundtrip() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - {}
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes: []
                lyrics:
                  lines:
                    "1":
                      text: Ooh
                      type: whole
                      _x:
                        extender: true
              - duration: { base: eighth }
                notes: []
                lyrics:
                  lines:
                    "1":
                      text: "con-"
                      type: start
              - duration: { base: eighth }
                notes: []
                lyrics:
                  lines:
                    "1":
                      text: cert
                      type: end
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).expect("Failed to deserialize MNX with lyrics");
        let content = &doc.parts[0].measures[0].sequences[0].content;
        assert_eq!(content.len(), 3);
        if let SequenceItem::Event(ev) = &content[0] {
            let lyr = ev.lyrics.as_ref().unwrap().lines.get("1").unwrap();
            assert_eq!(lyr.text, "Ooh");
            assert_eq!(lyr.ext.extender, Some(true));
        } else {
            panic!("Expected Event");
        }
        if let SequenceItem::Event(ev) = &content[1] {
            let lyr = ev.lyrics.as_ref().unwrap().lines.get("1").unwrap();
            assert_eq!(lyr.text, "con-");
            assert_eq!(lyr.line_type.as_deref(), Some("start"));
        } else {
            panic!("Expected Event");
        }
        let out = serde_saphyr::to_string(&doc).expect("Failed to serialize MNX with lyrics");
        let doc2: MnxDocument = serde_saphyr::from_str(&out).expect("Failed to deserialize serialized lyrics");
        assert_eq!(doc, doc2);
    }

    #[test]
    fn test_export_yaml_to_mnx_json() {
        let yaml = r#"# Top-level comment to strip
mnx:
  version: 1 # inline comment
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Kite Guitar"
    _x:
      microFretDivisions: 41
      fretStepMultiplier: 2
      tuning:
        - string: 1
          pitch: { step: E, alter: '1\41', octave: 4 }
    measures:
      - sequences:
          - content:
              # Event comment
              - duration: { base: quarter }
                notes:
                  - pitch: { step: F, alter: '-8\41', octave: 4 }
                    _x:
                      string: 1
                      fret: 2
                      harmonic:
                        touch: 12
                        soundingNote:
                          pitch: { step: F, alter: '2\41', octave: 5 }
"#;
        let json_str = export_yaml_to_mnx_json(yaml).expect("Should export to MNX JSON");
        assert!(!json_str.contains('#'), "Comments must be stripped");
        assert!(!json_str.contains("\\41"), "EDO fractions must be converted to decimals");
        assert!(json_str.contains("\"ursine\": {"), "_x must be wrapped under vendor 'ursine': {}", json_str);
        assert!(json_str.contains("\"alter\": 0.293"), "1\\41 -> 0.293 in JSON: {}", json_str);
        assert!(json_str.contains("\"alter\": -2.341"), "-8\\41 -> -2.341 in JSON: {}", json_str);
        assert!(json_str.contains("\"alter\": 0.585"), "2\\41 -> 0.585 in JSON: {}", json_str);

        // JSON is valid YAML 1.2, so serde_saphyr can parse the exported .mnx JSON back into MnxDocument!
        let parsed: MnxDocument = serde_saphyr::from_str(&json_str).expect("Exported .mnx JSON must parse cleanly");
        assert_eq!(
            parsed.parts[0].tuning.as_ref().unwrap()[0].pitch.alter,
            Some(Alteration::Value(0.293))
        );
        let ev = parsed.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes[0].pitch.alter, Some(Alteration::Value(-2.341)));
        assert_eq!(
            ev.notes[0].harmonic.as_ref().unwrap().sounding_note.pitch.alter,
            Some(Alteration::Value(0.585))
        );
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct EventMarkings {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "flow_opt")]
    pub accent: Option<Accent>,
}

impl EventMarkings {
    pub fn is_empty(&self) -> bool {
        self.accent.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct Accent {}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BowDirection {
    pub direction: String,
}

impl Serialize for BowDirection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Helper<'a> {
            direction: &'a str,
        }
        FlowMap(Helper {
            direction: &self.direction,
        })
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BowDirection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Map { direction: String },
            Str(String),
        }
        match Helper::deserialize(deserializer)? {
            Helper::Map { direction } => Ok(BowDirection { direction }),
            Helper::Str(direction) => Ok(BowDirection { direction }),
        }
    }
}

