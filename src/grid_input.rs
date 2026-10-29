//! Grid Controller MIDI Input Engine & 4-Corner Calibration
//!
//! Supports up to 4 simultaneous MIDI input ports for grid controllers (Launchpad, LinnStrument,
//! Lumatone, Push, Deluge, etc.), 4-corner pad calibration (`upper_left`, `lower_left`,
//! `upper_right`, `lower_right`), on-the-fly `fret_offset` per controller, and live pad state
//! tracking with a 500ms release latch for Grid Controller Input Mode.

use crate::midi::MidiPortInfo;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

pub const MAX_GRID_CONTROLLERS: usize = 4;
pub const GRID_RELEASE_LATCH_MS: u64 = 500;

/// Discover available MIDI input ports on the system.
pub fn get_midi_input_ports() -> Vec<MidiPortInfo> {
    match midir::MidiInput::new("guitab_grid_probe") {
        Ok(midi_in) => {
            let mut ports = Vec::new();
            for (idx, p) in midi_in.ports().iter().enumerate() {
                let name = midi_in
                    .port_name(p)
                    .unwrap_or_else(|_| format!("Input Port {}", idx));
                ports.push(MidiPortInfo { id: idx, name });
            }
            ports
        }
        Err(_) => Vec::new(),
    }
}

/// 4-Corner calibration data for a grid controller.
///
/// The user presses:
/// 1. `upper_left`  -> Top string (String 1), leftmost column (column 0)
/// 2. `lower_left`  -> Bottom physical row, leftmost column (column 0)
/// 3. `upper_right` -> Top string (String 1), rightmost column
/// 4. `lower_right` -> Bottom physical row, rightmost column
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridCalibration {
    pub upper_left: u8,
    pub lower_left: u8,
    pub upper_right: u8,
    pub lower_right: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_rows: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_cols: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_stride: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col_stride: Option<i32>,
}

impl Default for GridCalibration {
    fn default() -> Self {
        // Default 6-row x 8-col grid starting at note 36 (Launchpad / standard drum/grid layout:
        // upper_left = 76 (row 0), lower_left = 36 (row 5 for 6 strings), col stride = +1, row stride = -8)
        Self::new_corners(76, 36, 83, 43)
    }
}

impl GridCalibration {
    /// Creates a new calibration from the 4 corner MIDI notes, automatically
    /// inferring the physical `(row_stride, col_stride, num_rows, num_cols)`.
    pub fn new_corners(upper_left: u8, lower_left: u8, upper_right: u8, lower_right: u8) -> Self {
        let mut cal = Self {
            upper_left,
            lower_left,
            upper_right,
            lower_right,
            num_rows: None,
            num_cols: None,
            row_stride: None,
            col_stride: None,
        };
        if let Some((r_stride, c_stride, n_rows, n_cols)) = cal.infer_physical_axes() {
            cal.row_stride = Some(r_stride);
            cal.col_stride = Some(c_stride);
            cal.num_rows = Some(n_rows);
            cal.num_cols = Some(n_cols);
        }
        cal
    }

    /// Solves the 2D affine grid parameters `(row_stride, col_stride, num_rows, num_cols)`
    /// for the physical grid controller hardware.
    ///
    /// This is an intrinsic property of the physical controller and is invariant to the
    /// number of strings on any specific instrument or score.
    pub fn solve_axes(&self) -> Option<(i32, i32, u8, u8)> {
        if let (Some(rs), Some(cs), Some(nr), Some(nc)) =
            (self.row_stride, self.col_stride, self.num_rows, self.num_cols)
        {
            return Some((rs, cs, nr, nc));
        }
        self.infer_physical_axes()
    }

    /// Infers the physical `(row_stride, col_stride, num_rows, num_cols)` from the 4 corners.
    pub fn infer_physical_axes(&self) -> Option<(i32, i32, u8, u8)> {
        let ul = self.upper_left as i32;
        let ll = self.lower_left as i32;
        let ur = self.upper_right as i32;
        let _lr = self.lower_right as i32;

        let d_down = ll - ul;
        let d_right = ur - ul;

        if d_down == 0 && d_right == 0 {
            return None;
        }

        // 1. Column stride & count inference:
        // On grid controllers, adjacent columns are almost universally chromatic (+/-1)
        // or small uniform strides (e.g. whole tones +/-2).
        let (col_stride, num_cols) = if d_right == 0 {
            (1, 1)
        } else if d_right.abs() <= 32 {
            (d_right.signum(), (d_right.abs() + 1) as u8)
        } else {
            let candidates = [1, 2, 3, 4, 5, 8, 10, 12, 16];
            let mut found = (d_right.signum(), (d_right.abs() + 1).min(32) as u8);
            for &step in &candidates {
                if d_right.abs() >= step && d_right % step == 0 {
                    let gaps = d_right.abs() / step;
                    if (1..=32).contains(&gaps) {
                        found = (d_right.signum() * step, (gaps + 1) as u8);
                        break;
                    }
                }
            }
            found
        };

        // 2. Row stride & count inference:
        let (row_stride, num_rows) = if d_down == 0 {
            (1, 1)
        } else {
            // First: check 7 gaps (8 rows) — overwhelmingly the dominant physical hardware
            // height for grid controllers (Launchpad, Push, LinnStrument, APC, and 8-row x 9-col grids).
            let step_7 = (d_down / 7).abs();
            if d_down % 7 == 0 && [10, 8, 9, 5, 4, 12, 16, 1, 2, 3, 6, 7].contains(&step_7) {
                (d_down / 7, 8u8)
            } else {
                // Check candidate row strides:
                let candidates = [8, 10, 9, 5, 12, 16, 4, 3, 2, 1, 7, 6];
                let mut found = (d_down, 2u8);
                for &step in &candidates {
                    if d_down.abs() >= step && d_down % step == 0 {
                        let gaps = d_down.abs() / step;
                        if (1..=24).contains(&gaps) {
                            found = (d_down.signum() * step, (gaps + 1) as u8);
                            break;
                        }
                    }
                }
                found
            }
        };

        Some((row_stride, col_stride, num_rows, num_cols))
    }

    /// Maps a raw MIDI note number to `(string_1_indexed, fret_column_0_indexed)`
    /// using this calibration. Returns `None` if the note does not land on the calibrated grid
    /// or falls on a bottom row truncated beyond `target_num_strings`.
    pub fn map_note(&self, note: u8, target_num_strings: u8) -> Option<(u8, i32)> {
        let (row_stride, col_stride, num_rows, num_cols) = self.solve_axes()?;
        let delta = (note as i32) - (self.upper_left as i32);

        // Search all valid `(row, col)` in `0..num_rows` x `0..num_cols`
        for r in 0..(num_rows as i32) {
            for c in 0..(num_cols as i32) {
                if r * row_stride + c * col_stride == delta {
                    let string_num = (r + 1) as u8;
                    if string_num >= 1 && string_num <= target_num_strings {
                        return Some((string_num, c));
                    } else {
                        // Any row beyond target_num_strings is truncated and unmapped
                        return None;
                    }
                }
            }
        }
        None
    }
}

/// Interactive 4-corner calibration step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStep {
    UpperLeft,
    LowerLeft,
    UpperRight,
    LowerRight,
}

impl CalibrationStep {
    pub fn prompt_text(self) -> &'static str {
        match self {
            CalibrationStep::UpperLeft => "Step 1/4: Press the UPPER-LEFT pad (Top row, leftmost column)",
            CalibrationStep::LowerLeft => "Step 2/4: Press the LOWER-LEFT pad (Bottom row, leftmost column)",
            CalibrationStep::UpperRight => "Step 3/4: Press the UPPER-RIGHT pad (Top row, rightmost column)",
            CalibrationStep::LowerRight => "Step 4/4: Press the LOWER-RIGHT pad (Bottom row, rightmost column)",
        }
    }
}

/// Persisted configuration for a single Grid Controller slot (0..4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridControllerSlotConfig {
    pub enabled: bool,
    pub port_name: Option<String>,
    pub fret_offset: i32,
    pub calibration: GridCalibration,
}

impl Default for GridControllerSlotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port_name: None,
            fret_offset: 0,
            calibration: GridCalibration::default(),
        }
    }
}

/// Incoming event from any connected MIDI grid controller thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridMidiEvent {
    pub slot_idx: usize,
    pub note: u8,
    pub is_note_on: bool,
    pub velocity: u8,
}

/// Manages up to 4 live `midir::MidiInputConnection`s and tracks held pads + 500ms release latch.
pub struct GridInputManager {
    pub slots: [GridControllerSlotConfig; MAX_GRID_CONTROLLERS],
    pub selected_port_ids: [Option<usize>; MAX_GRID_CONTROLLERS],
    pub available_ports: Vec<MidiPortInfo>,
    connections: [Option<midir::MidiInputConnection<()>>; MAX_GRID_CONTROLLERS],
    tx: Sender<GridMidiEvent>,
    rx: Receiver<GridMidiEvent>,
    /// Currently held raw pads: `(slot_idx, midi_note) -> (string, fret)`
    pub held_pads: BTreeMap<(usize, u8), (u8, i32)>,
    /// Whether the editor is currently in Grid Controller Input Mode (light-red cursor).
    pub is_grid_input_mode: bool,
    /// Timestamp when the last held pad was released (`held_pads` transitioned from non-empty to empty).
    pub release_latch_start: Option<Instant>,
    /// Active 4-corner calibration wizard state: `(slot_idx, step, partial_calibration)`
    pub active_calibration: Option<(usize, CalibrationStep, GridCalibration)>,
}

impl Default for GridInputManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GridInputManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let available_ports = if cfg!(test) {
            Vec::new()
        } else {
            get_midi_input_ports()
        };
        Self {
            slots: Default::default(),
            selected_port_ids: [None; MAX_GRID_CONTROLLERS],
            available_ports,
            connections: [None, None, None, None],
            tx,
            rx,
            held_pads: BTreeMap::new(),
            is_grid_input_mode: false,
            release_latch_start: None,
            active_calibration: None,
        }
    }

    pub fn refresh_ports(&mut self) {
        self.available_ports = get_midi_input_ports();
        for slot_idx in 0..MAX_GRID_CONTROLLERS {
            if let Some(id) = self.selected_port_ids[slot_idx] {
                if !self.available_ports.iter().any(|p| p.id == id) {
                    self.disconnect_slot(slot_idx);
                }
            }
        }
    }

    pub fn connect_slot(&mut self, slot_idx: usize, port_id: Option<usize>) -> Result<(), String> {
        if slot_idx >= MAX_GRID_CONTROLLERS {
            return Err("Invalid controller slot".to_string());
        }
        self.disconnect_slot(slot_idx);
        let Some(pid) = port_id else {
            return Ok(());
        };

        let midi_in = midir::MidiInput::new(&format!("guitab_grid_in_{}", slot_idx))
            .map_err(|e| format!("Failed to initialize MIDI input: {}", e))?;
        let ports = midi_in.ports();
        let port = ports
            .get(pid)
            .ok_or_else(|| format!("MIDI input port {} not found", pid))?;
        let port_name = midi_in
            .port_name(port)
            .unwrap_or_else(|_| format!("Input Port {}", pid));

        let tx = self.tx.clone();
        let conn = midi_in
            .connect(
                port,
                &format!("guitab_grid_conn_{}", slot_idx),
                move |_stamp, message, _| {
                    if message.len() >= 3 {
                        let status = message[0] & 0xF0;
                        let note = message[1];
                        let velocity = message[2];
                        if status == 0x90 && velocity > 0 {
                            let _ = tx.send(GridMidiEvent {
                                slot_idx,
                                note,
                                is_note_on: true,
                                velocity,
                            });
                        } else if status == 0x80 || (status == 0x90 && velocity == 0) {
                            let _ = tx.send(GridMidiEvent {
                                slot_idx,
                                note,
                                is_note_on: false,
                                velocity: 0,
                            });
                        }
                    }
                },
                (),
            )
            .map_err(|e| format!("Failed to connect MIDI input port: {}", e))?;

        self.connections[slot_idx] = Some(conn);
        self.selected_port_ids[slot_idx] = Some(pid);
        self.slots[slot_idx].enabled = true;
        self.slots[slot_idx].port_name = Some(port_name);
        Ok(())
    }

    pub fn disconnect_slot(&mut self, slot_idx: usize) {
        if slot_idx < MAX_GRID_CONTROLLERS {
            self.connections[slot_idx] = None;
            self.selected_port_ids[slot_idx] = None;
            self.slots[slot_idx].enabled = false;
            self.held_pads.retain(|&(s, _), _| s != slot_idx);
        }
    }

    pub fn active_controller_count(&self) -> usize {
        self.selected_port_ids.iter().filter(|p| p.is_some()).count()
    }

    pub fn start_calibration(&mut self, slot_idx: usize) {
        if slot_idx < MAX_GRID_CONTROLLERS {
            let current = self.slots[slot_idx].calibration;
            self.active_calibration = Some((slot_idx, CalibrationStep::UpperLeft, current));
        }
    }

    pub fn cancel_calibration(&mut self) {
        self.active_calibration = None;
    }

    pub fn exit_grid_input_mode(&mut self) {
        self.is_grid_input_mode = false;
        self.held_pads.clear();
        self.release_latch_start = None;
    }

    /// Injects a synthetic or hardware `GridMidiEvent` and returns:
    /// - `chord_changed`: `Some(BTreeMap<u8, i32>)` if the held chord changed and should be rendered on the current duration.
    /// - `calibration_completed`: `Some(usize)` if a 4-corner calibration just finished on that slot.
    pub fn process_event(
        &mut self,
        ev: GridMidiEvent,
        num_strings: u8,
        now: Instant,
    ) -> (Option<BTreeMap<u8, i32>>, Option<usize>) {
        // 1. If calibrating this slot, consume NoteOn events for the 4 corners
        if let Some((cal_slot, step, mut cal)) = self.active_calibration {
            if cal_slot == ev.slot_idx && ev.is_note_on {
                match step {
                    CalibrationStep::UpperLeft => {
                        cal.upper_left = ev.note;
                        self.active_calibration =
                            Some((cal_slot, CalibrationStep::LowerLeft, cal));
                        return (None, None);
                    }
                    CalibrationStep::LowerLeft => {
                        cal.lower_left = ev.note;
                        self.active_calibration =
                            Some((cal_slot, CalibrationStep::UpperRight, cal));
                        return (None, None);
                    }
                    CalibrationStep::UpperRight => {
                        cal.upper_right = ev.note;
                        self.active_calibration =
                            Some((cal_slot, CalibrationStep::LowerRight, cal));
                        return (None, None);
                    }
                    CalibrationStep::LowerRight => {
                        cal.lower_right = ev.note;
                        if let Some((r_stride, c_stride, n_rows, n_cols)) =
                            cal.infer_physical_axes()
                        {
                            cal.row_stride = Some(r_stride);
                            cal.col_stride = Some(c_stride);
                            cal.num_rows = Some(n_rows);
                            cal.num_cols = Some(n_cols);
                        }
                        self.slots[cal_slot].calibration = cal;
                        self.active_calibration = None;
                        return (None, Some(cal_slot));
                    }
                }
            }
            return (None, None);
        }

        // 2. Normal note input processing
        let Some(slot) = self.slots.get(ev.slot_idx) else {
            return (None, None);
        };

        if ev.is_note_on {
            if let Some((string_num, col)) = slot.calibration.map_note(ev.note, num_strings) {
                let fret = (col + slot.fret_offset).max(0);
                self.is_grid_input_mode = true;
                self.release_latch_start = None; // Cancel pending 500ms finalize timer while any pad is held
                self.held_pads
                    .insert((ev.slot_idx, ev.note), (string_num, fret));
                return (Some(self.current_chord()), None);
            }
        } else if self.held_pads.remove(&(ev.slot_idx, ev.note)).is_some() {
            if self.held_pads.is_empty() {
                // All pads released! Start the 500ms release latch without wiping the chord on the score
                if self.is_grid_input_mode {
                    self.release_latch_start = Some(now);
                }
                return (None, None);
            } else {
                // Still holding at least one pad: dynamically update the rendered chord!
                return (Some(self.current_chord()), None);
            }
        }

        (None, None)
    }

    /// Returns the currently held chord mapped by `string (1..=N) -> fret`.
    /// If multiple pads are held on the same string, the highest fret wins (like a real fretboard).
    pub fn current_chord(&self) -> BTreeMap<u8, i32> {
        let mut chord = BTreeMap::new();
        for &(string_num, fret) in self.held_pads.values() {
            let entry = chord.entry(string_num).or_insert(fret);
            if fret > *entry {
                *entry = fret;
            }
        }
        chord
    }

    /// Polls all queued MIDI events and checks whether the 500ms release latch has expired.
    /// Returns `(latest_chord_update, should_finalize_and_advance, finished_calibration_slot)`.
    pub fn poll(
        &mut self,
        num_strings: u8,
        now: Instant,
    ) -> (Option<BTreeMap<u8, i32>>, bool, Option<usize>) {
        let mut latest_chord = None;
        let mut cal_done = None;

        while let Ok(ev) = self.rx.try_recv() {
            let (chord_upd, cal_slot) = self.process_event(ev, num_strings, now);
            if chord_upd.is_some() {
                latest_chord = chord_upd;
            }
            if cal_slot.is_some() {
                cal_done = cal_slot;
            }
        }

        let mut should_finalize = false;
        if self.is_grid_input_mode && self.held_pads.is_empty() {
            if let Some(latch_start) = self.release_latch_start {
                if now.saturating_duration_since(latch_start)
                    >= Duration::from_millis(GRID_RELEASE_LATCH_MS)
                {
                    self.release_latch_start = None;
                    should_finalize = true;
                }
            }
        }

        (latest_chord, should_finalize, cal_done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_4_corner_calibration_standard_launchpad_layout() {
        // 6 strings, 8 columns:
        // Upper-left = 76 (String 1, Fret 0)
        // Lower-left = 36 (String 6, Fret 0 -> row_stride = -8)
        // Upper-right = 83 (String 1, Fret 7 -> col_stride = +1)
        // Lower-right = 43 (String 6, Fret 7)
        let cal = GridCalibration::new_corners(76, 36, 83, 43);
        assert_eq!(cal.map_note(76, 6), Some((1, 0)));
        assert_eq!(cal.map_note(83, 6), Some((1, 7)));
        assert_eq!(cal.map_note(36, 6), Some((6, 0)));
        assert_eq!(cal.map_note(43, 6), Some((6, 7)));
        // Middle pad: String 3 (row 2 -> 76 - 16 = 60), Fret 4 (+4 = 64)
        assert_eq!(cal.map_note(64, 6), Some((3, 4)));
        // Out of bounds note
        assert_eq!(cal.map_note(12, 6), None);
    }

    #[test]
    fn test_8_row_9_col_grid_controller_truncation_for_fewer_strings() {
        // An 8-row x 9-column grid controller:
        // Row stride = +9, Col stride = +1 (9 columns 0..8)
        // Row 0: 0..8   (String 1)
        // Row 1: 9..17  (String 2)
        // Row 2: 18..26 (String 3)
        // Row 3: 27..35 (String 4)
        // Row 4: 36..44 (String 5)
        // Row 5: 45..53 (String 6)
        // Row 6: 54..62 (String 7)
        // Row 7: 63..71 (String 8)
        // Upper-left = 0, Lower-left = 63, Upper-right = 8, Lower-right = 71
        let cal = GridCalibration::new_corners(0, 63, 8, 71);
        assert_eq!(cal.solve_axes(), Some((9, 1, 8, 9)));

        // 1. For an 8-string instrument:
        // Top-left maps to String 1 Fret 0
        assert_eq!(cal.map_note(0, 8), Some((1, 0)));
        // Top-right maps to String 1 Fret 8
        assert_eq!(cal.map_note(8, 8), Some((1, 8)));
        // Row 1 maps to String 2
        assert_eq!(cal.map_note(9, 8), Some((2, 0)));
        // Row 5 maps to String 6
        assert_eq!(cal.map_note(45, 8), Some((6, 0)));
        // Row 6 maps to String 7
        assert_eq!(cal.map_note(54, 8), Some((7, 0)));
        // Row 7 (bottom row) maps to String 8
        assert_eq!(cal.map_note(63, 8), Some((8, 0)));
        assert_eq!(cal.map_note(71, 8), Some((8, 8)));

        // 2. For a 7-string instrument:
        // Strings 1-6 mapping is completely UNTOUCHED
        assert_eq!(cal.map_note(0, 7), Some((1, 0)));
        assert_eq!(cal.map_note(9, 7), Some((2, 0)));
        assert_eq!(cal.map_note(45, 7), Some((6, 0)));
        // String 7 is mapped
        assert_eq!(cal.map_note(54, 7), Some((7, 0)));
        // Bottom row (Row 7 / String 8 pads) is truncated and unmapped!
        assert_eq!(cal.map_note(63, 7), None);
        assert_eq!(cal.map_note(71, 7), None);

        // 3. For a 6-string instrument:
        // Strings 1-6 mapping is STILL 100% UNTOUCHED
        assert_eq!(cal.map_note(0, 6), Some((1, 0)));
        assert_eq!(cal.map_note(8, 6), Some((1, 8)));
        assert_eq!(cal.map_note(9, 6), Some((2, 0)));
        assert_eq!(cal.map_note(45, 6), Some((6, 0)));
        assert_eq!(cal.map_note(53, 6), Some((6, 8)));
        // Bottom rows (Row 6 and Row 7) are truncated and unmapped!
        assert_eq!(cal.map_note(54, 6), None);
        assert_eq!(cal.map_note(62, 6), None);
        assert_eq!(cal.map_note(63, 6), None);
        assert_eq!(cal.map_note(71, 6), None);

        // 4. For a 4-string bass:
        // Strings 1-4 are mapped
        assert_eq!(cal.map_note(0, 4), Some((1, 0)));
        assert_eq!(cal.map_note(27, 4), Some((4, 0)));
        // Rows 4-7 are truncated and unmapped
        assert_eq!(cal.map_note(36, 4), None);
        assert_eq!(cal.map_note(45, 4), None);
        assert_eq!(cal.map_note(54, 4), None);
        assert_eq!(cal.map_note(63, 4), None);
    }

    #[test]
    fn test_launchpad_pro_8_row_9_col_programmer_layout_truncation() {
        // Launchpad Pro programmer layout: row stride = -10, col stride = +1 (9 columns 81..89)
        // Row 0: 81..89 (String 1)
        // Row 1: 71..79 (String 2)
        // Row 5: 31..39 (String 6)
        // Row 6: 21..29 (String 7)
        // Row 7: 11..19 (String 8)
        let cal = GridCalibration::new_corners(81, 11, 89, 19);
        assert_eq!(cal.solve_axes(), Some((-10, 1, 8, 9)));

        // 8 strings:
        assert_eq!(cal.map_note(81, 8), Some((1, 0)));
        assert_eq!(cal.map_note(31, 8), Some((6, 0)));
        assert_eq!(cal.map_note(21, 8), Some((7, 0)));
        assert_eq!(cal.map_note(11, 8), Some((8, 0)));

        // 6 strings: Strings 1-6 unchanged, rows 6 & 7 truncated
        assert_eq!(cal.map_note(81, 6), Some((1, 0)));
        assert_eq!(cal.map_note(31, 6), Some((6, 0)));
        assert_eq!(cal.map_note(21, 6), None);
        assert_eq!(cal.map_note(11, 6), None);
    }

    #[test]
    fn test_grid_input_mode_live_chord_and_500ms_latch() {
        let mut mgr = GridInputManager::new();
        mgr.slots[0].enabled = true;
        mgr.slots[0].fret_offset = 5; // Leftmost column is fret 5

        let t0 = Instant::now();

        // 1. Press pad (String 1, Col 0 -> Fret 5)
        let (chord, _) = mgr.process_event(
            GridMidiEvent {
                slot_idx: 0,
                note: 76,
                is_note_on: true,
                velocity: 100,
            },
            6,
            t0,
        );
        assert!(mgr.is_grid_input_mode);
        let chord = chord.expect("Should emit live chord update");
        assert_eq!(chord.get(&1), Some(&5));

        // 2. Add second finger to chord (String 2, Col 2 -> note 76 - 8 + 2 = 70 -> Fret 7)
        let (chord2, _) = mgr.process_event(
            GridMidiEvent {
                slot_idx: 0,
                note: 70,
                is_note_on: true,
                velocity: 100,
            },
            6,
            t0 + Duration::from_millis(50),
        );
        let chord2 = chord2.expect("Should emit updated 2-note chord");
        assert_eq!(chord2.get(&1), Some(&5));
        assert_eq!(chord2.get(&2), Some(&7));

        // 3. Release first finger while still holding second finger -> re-renders to single note on String 2
        let (chord3, _) = mgr.process_event(
            GridMidiEvent {
                slot_idx: 0,
                note: 76,
                is_note_on: false,
                velocity: 0,
            },
            6,
            t0 + Duration::from_millis(120),
        );
        let chord3 = chord3.expect("Should update chord while second pad is still held");
        assert_eq!(chord3.len(), 1);
        assert_eq!(chord3.get(&2), Some(&7));
        assert!(mgr.release_latch_start.is_none());

        // 4. Release last finger -> starts 500ms timer, does NOT wipe chord
        let (chord4, _) = mgr.process_event(
            GridMidiEvent {
                slot_idx: 0,
                note: 70,
                is_note_on: false,
                velocity: 0,
            },
            6,
            t0 + Duration::from_millis(200),
        );
        assert!(chord4.is_none());
        assert!(mgr.release_latch_start.is_some());

        // 5. Poll at +400ms (only 200ms after release) -> should NOT finalize yet
        let (_, should_finalize_early, _) = mgr.poll(6, t0 + Duration::from_millis(400));
        assert!(!should_finalize_early);

        // 6. Poll at +705ms (505ms after release) -> SHOULD finalize and advance!
        let (_, should_finalize_now, _) = mgr.poll(6, t0 + Duration::from_millis(705));
        assert!(should_finalize_now);
        assert!(mgr.release_latch_start.is_none());
        assert!(mgr.is_grid_input_mode); // Remains in Grid Input Mode until Esc
    }
}
