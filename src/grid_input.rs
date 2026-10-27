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
/// 2. `lower_left`  -> Bottom string, leftmost column (column 0)
/// 3. `upper_right` -> Top string (String 1), rightmost column
/// 4. `lower_right` -> Bottom string, rightmost column
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridCalibration {
    pub upper_left: u8,
    pub lower_left: u8,
    pub upper_right: u8,
    pub lower_right: u8,
}

impl Default for GridCalibration {
    fn default() -> Self {
        // Default 8x8 grid starting at note 36 (Launchpad / standard drum/grid layout:
        // upper_left = 84 (row 0), lower_left = 36 (row 5 for 6 strings), col stride = +1, row stride = -8)
        Self {
            upper_left: 76,
            lower_left: 36,
            upper_right: 83,
            lower_right: 43,
        }
    }
}

impl GridCalibration {
    /// Solves the 2D affine grid parameters `(row_stride, col_stride, num_rows, num_cols)`
    /// from the 4 corner MIDI notes.
    ///
    /// Let `d_down = lower_left - upper_left` (total MIDI note delta from top string to bottom string on left edge)
    /// and `d_right = upper_right - upper_left` (total MIDI note delta from left col to right col on top edge).
    pub fn solve_axes(&self, target_num_strings: u8) -> Option<(i32, i32, u8, u8)> {
        let ul = self.upper_left as i32;
        let ll = self.lower_left as i32;
        let ur = self.upper_right as i32;
        let _lr = self.lower_right as i32;

        let d_down = ll - ul;
        let d_right = ur - ul;

        if d_down == 0 && d_right == 0 {
            return None;
        }

        // Determine column stride and number of columns
        // On almost all grid controllers, adjacent columns differ by +/-1 (horizontal chromatic)
        // or by a fixed interval. If |d_right| > 0, check if target_num_strings helps disambiguate,
        // or find gcd / standard stride.
        let (row_stride, num_rows, col_stride, num_cols) =
            Self::infer_strides(d_down, d_right, target_num_strings)?;

        Some((row_stride, col_stride, num_rows, num_cols))
    }

    fn infer_strides(
        d_down: i32,
        d_right: i32,
        target_num_strings: u8,
    ) -> Option<(i32, u8, i32, u8)> {
        // Row stride inference:
        // If d_down == 0, single row.
        let (row_stride, num_rows) = if d_down == 0 {
            (1, 1)
        } else {
            // Try matching `(target_num_strings - 1)` first if it divides `d_down` evenly:
            let pref_play_gaps = (target_num_strings.saturating_sub(1)).max(1) as i32;
            if pref_play_gaps > 0 && d_down % pref_play_gaps == 0 {
                (d_down / pref_play_gaps, (pref_play_gaps + 1) as u8)
            } else {
                // Common row strides on grid controllers (Launchpad: 10 or 8 or 16, LinnStrument: 5, Push: 8)
                let candidates = [8, 10, 16, 12, 5, 6, 7, 4, 3, 2, 1];
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

        // Column stride inference:
        // Usually +/-1 per fret column! If +/-1 gives <= 32 columns and doesn't alias row_stride, use +/-1.
        let (col_stride, num_cols) = if d_right == 0 {
            (1, 1)
        } else if d_right.abs() <= 32 && row_stride.abs() != 1 {
            (d_right.signum(), (d_right.abs() + 1) as u8)
        } else {
            let candidates = [1, 2, 5, 8, 10, 12, 16];
            let mut found = (d_right.signum(), (d_right.abs() + 1).min(32) as u8);
            for &step in &candidates {
                if step != row_stride.abs() && d_right.abs() >= step && d_right % step == 0 {
                    let gaps = d_right.abs() / step;
                    if (1..=32).contains(&gaps) {
                        found = (d_right.signum() * step, (gaps + 1) as u8);
                        break;
                    }
                }
            }
            found
        };

        Some((row_stride, num_rows, col_stride, num_cols))
    }

    /// Maps a raw MIDI note number to `(string_1_indexed, fret_column_0_indexed)`
    /// using this calibration. Returns `None` if the note does not land on the calibrated grid.
    pub fn map_note(&self, note: u8, target_num_strings: u8) -> Option<(u8, i32)> {
        let (row_stride, col_stride, num_rows, num_cols) = self.solve_axes(target_num_strings)?;
        let delta = (note as i32) - (self.upper_left as i32);

        // Search all valid `(row, col)` in `0..num_rows` x `0..num_cols`
        for r in 0..(num_rows as i32) {
            for c in 0..(num_cols as i32) {
                if r * row_stride + c * col_stride == delta {
                    let string_num = (r + 1) as u8;
                    if string_num >= 1 && string_num <= target_num_strings {
                        return Some((string_num, c));
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
            CalibrationStep::UpperLeft => "Step 1/4: Press the UPPER-LEFT pad (String 1, leftmost column)",
            CalibrationStep::LowerLeft => "Step 2/4: Press the LOWER-LEFT pad (Lowest string, leftmost column)",
            CalibrationStep::UpperRight => "Step 3/4: Press the UPPER-RIGHT pad (String 1, rightmost column)",
            CalibrationStep::LowerRight => "Step 4/4: Press the LOWER-RIGHT pad (Lowest string, rightmost column)",
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
        let cal = GridCalibration {
            upper_left: 76,
            lower_left: 36,
            upper_right: 83,
            lower_right: 43,
        };
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
