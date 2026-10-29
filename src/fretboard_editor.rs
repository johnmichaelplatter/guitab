//! Custom Fretboard / Fretlet & Just Intonation (JI) Editor (`#6`)
//!
//! Provides:
//! - Interactive guitar neck canvas with parallel string lines
//! - Cumulative dotted guide grid supporting Ratios (`5/4`, `25/16`, `125/64`...), Cents (`128.0c`, `256.0c`...),
//!   and EDO steps (`2\41`, `4\41`, `6\41`...)
//! - Click-and-drag fret/fretlet placement across any or all strings with a 10-cent per-string minimum proximity limit
//! - "Anchor Frets + Auto-Interpolated Tenths (`0.1`) with Inline Badge Override" fret numbering UX
//! - Automatic 53-EDO ups/downs note name recommendations relative to an imaginary open C string
//! - Full MNX YAML pitch & `alter` synthesis rules:
//!   1. Cents present -> 12-EDO 3-decimal-place `alter` approximation
//!   2. Pure ratios -> ratio `alter` offsets from `1/1`, with `1/1` frequency in Hz stored in `global._x.tuning.referenceNote`
//!   3. Pure EDO-step frets with $\text{LCM} \le 53$ -> unified $L$-EDO `m\L` `alter`
//!   4. Custom ups/downs names stored in `note.pitch._x.name` throughout the part

use crate::edo::{EdoTuning, generate_edo_scale};
use crate::mnx::{Alteration, Pitch, PitchExt};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke};
use serde::{Deserialize, Serialize};

pub const MIN_FRET_PROXIMITY_CENTS: f64 = 10.0;
pub const DEFAULT_NECK_MAX_CENTS: f64 = 2400.0; // 2 octaves default neck span

/// Greatest common divisor of two positive integers.
pub fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// Least common multiple of two positive integers.
pub fn lcm_u64(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    (a / gcd_u64(a, b)).saturating_mul(b)
}

/// How a fret's pitch location was specified on the neck.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FretLocationSpec {
    /// Pure ratio relative to open string (`num / den`, e.g., `5/4`).
    Ratio { num: u64, den: u64 },
    /// Cents offset above open string (e.g., `128.0`).
    Cents { cents: f64 },
    /// EDO step offset (`steps \ edo`, e.g., `2\41`).
    EdoStep { steps: i32, edo: u32 },
}

impl FretLocationSpec {
    /// Returns the exact cents offset above the open string (`1200 * log2(ratio)`).
    pub fn to_cents(&self) -> f64 {
        match self {
            FretLocationSpec::Ratio { num, den } => {
                if *num == 0 || *den == 0 {
                    0.0
                } else {
                    1200.0 * ((*num as f64) / (*den as f64)).log2()
                }
            }
            FretLocationSpec::Cents { cents } => *cents,
            FretLocationSpec::EdoStep { steps, edo } => {
                if *edo == 0 {
                    0.0
                } else {
                    1200.0 * (*steps as f64) / (*edo as f64)
                }
            }
        }
    }

    /// Formats the label displayed on the neck for this fret or guide line.
    pub fn display_label(&self) -> String {
        match self {
            FretLocationSpec::Ratio { num, den } => format!("{}/{}", num, den),
            FretLocationSpec::Cents { cents } => {
                if (cents - cents.round()).abs() < 1e-3 {
                    format!("{:.1}c", cents)
                } else {
                    format!("{:.2}c", cents)
                }
            }
            FretLocationSpec::EdoStep { steps, edo } => format!("{}\\{}", steps, edo),
        }
    }
}

/// A single custom fret or partial fretlet placed on the neck.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomFret {
    /// Computed or user-overridden fret number (whole number for full-neck anchor frets, or `.1` decimal for fretlets).
    #[serde(rename = "fret", alias = "fretNumber", alias = "fret_number")]
    pub fret_number: f64,
    /// If `Some`, the user explicitly overrode this fret's badge number; subsequent unpinned frets re-index from `floor(override) + 1`.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "manualNumber", alias = "manual_number")]
    pub manual_number: Option<f64>,
    /// Pitch location specification (Ratio, Cents, or EdoStep).
    #[serde(with = "crate::mnx::flow_req")]
    pub spec: FretLocationSpec,
    /// Strings covered by this fret/fretlet (1-indexed, e.g. `[1, 2, 3, 4, 5, 6]`).
    #[serde(with = "crate::mnx::flow_seq_req")]
    pub strings: Vec<u8>,
    /// Ups/downs note name relative to an imaginary open C string (recommended from 53-EDO, editable by user).
    #[serde(rename = "noteName", alias = "note_name")]
    pub note_name: String,
}

impl CustomFret {
    pub fn cents(&self) -> f64 {
        self.spec.to_cents()
    }

    pub fn has_string(&self, s: u8) -> bool {
        self.strings.contains(&s)
    }

    pub fn is_full_neck(&self, num_strings: u8) -> bool {
        if num_strings == 0 {
            return false;
        }
        (1..=num_strings).all(|s| self.strings.contains(&s))
    }

    pub fn formatted_fret_number(&self) -> String {
        format_fret_number(self.fret_number)
    }
}

/// Formats a fret number cleanly: whole integers as `"1"`, `"2"`, or nearest tenth as `"1.5"`.
pub fn format_fret_number(fret: f64) -> String {
    let rounded = (fret * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < 1e-4 {
        format!("{}", rounded.round() as i32)
    } else {
        format!("{:.1}", rounded)
    }
}

/// Recommends a default ups/downs note name in 53-EDO relative to an imaginary open C string.
pub fn recommend_53edo_note_name_from_cents(cents: f64) -> String {
    let steps_53 = ((cents / 1200.0) * 53.0).round() as i32;
    let step_mod = steps_53.rem_euclid(53);
    let scale_53 = generate_edo_scale(53);
    scale_53
        .into_iter()
        .find(|(st, _, _, _)| *st == step_mod)
        .map(|(_, name, _, _)| name)
        .unwrap_or_else(|| "C".to_string())
}

/// Parses the user's dotted guide grid input string (`"5/4"`, `"128.0"`, `"2\\41"`, etc.)
/// and generates cumulative guide positions up to `max_cents`.
///
/// - Ratio `p/q` (must be $> 1/1$): generates $(p/q)^1, (p/q)^2, (p/q)^3 \dots$ as reduced fractions!
/// - EDO step `m\n` (with $m > 0, n > 0$): generates $m\backslash n, 2m\backslash n, 3m\backslash n \dots$!
/// - Cents `c` (with $c > 0$): generates $c, 2c, 3c \dots$!
pub fn generate_cumulative_guides(input: &str, max_cents: f64) -> Result<Vec<FretLocationSpec>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }

    // 1. Check EDO step syntax `m\n`
    if let Some((m_str, n_str)) = s.split_once('\\') {
        let m: i32 = m_str
            .trim()
            .parse()
            .map_err(|_| format!("Invalid EDO step numerator '{}'", m_str))?;
        let n: u32 = n_str
            .trim()
            .parse()
            .map_err(|_| format!("Invalid EDO division '{}'", n_str))?;
        if m <= 0 || n == 0 {
            return Err("EDO step must be positive (e.g. 2\\41)".to_string());
        }
        let step_cents = 1200.0 * (m as f64) / (n as f64);
        if step_cents < 1.0 {
            return Err("Step size is too small (< 1 cent)".to_string());
        }
        let mut guides = Vec::new();
        let mut k = 1;
        loop {
            let cur_steps = m * k;
            let cents = 1200.0 * (cur_steps as f64) / (n as f64);
            if cents > max_cents + 1e-4 || guides.len() >= 256 {
                break;
            }
            guides.push(FretLocationSpec::EdoStep {
                steps: cur_steps,
                edo: n,
            });
            k += 1;
        }
        return Ok(guides);
    }

    // 2. Check Ratio syntax `p/q`
    if let Some((p_str, q_str)) = s.split_once('/') {
        let p: u64 = p_str
            .trim()
            .parse()
            .map_err(|_| format!("Invalid ratio numerator '{}'", p_str))?;
        let q: u64 = q_str
            .trim()
            .parse()
            .map_err(|_| format!("Invalid ratio denominator '{}'", q_str))?;
        if p == 0 || q == 0 || p <= q {
            return Err("Ratio must be greater than 1/1 (e.g. 5/4 or 9/8)".to_string());
        }
        let g = gcd_u64(p, q);
        let base_num = p / g;
        let base_den = q / g;
        let step_cents = 1200.0 * ((base_num as f64) / (base_den as f64)).log2();
        if step_cents < 1.0 {
            return Err("Ratio interval is too small (< 1 cent)".to_string());
        }

        let mut guides = Vec::new();
        let mut cur_num: u64 = 1;
        let mut cur_den: u64 = 1;
        loop {
            cur_num = match cur_num.checked_mul(base_num) {
                Some(v) => v,
                None => break,
            };
            cur_den = match cur_den.checked_mul(base_den) {
                Some(v) => v,
                None => break,
            };
            let g_cur = gcd_u64(cur_num, cur_den);
            cur_num /= g_cur;
            cur_den /= g_cur;

            let cents = 1200.0 * ((cur_num as f64) / (cur_den as f64)).log2();
            if cents > max_cents + 1e-4 || guides.len() >= 256 {
                break;
            }
            guides.push(FretLocationSpec::Ratio {
                num: cur_num,
                den: cur_den,
            });
        }
        return Ok(guides);
    }

    // 3. Otherwise parse as Cents (stripping optional trailing 'c' or '¢')
    let clean_cents = s.trim_end_matches(|c| c == 'c' || c == 'C' || c == '¢').trim();
    let step_cents: f64 = clean_cents
        .parse()
        .map_err(|_| format!("Expected ratio (5/4), EDO step (2\\41), or cents (128.0), got '{}'", s))?;
    if step_cents <= 0.5 {
        return Err("Cents step must be > 0.5c".to_string());
    }

    let mut guides = Vec::new();
    let mut k = 1;
    loop {
        let cents = step_cents * (k as f64);
        if cents > max_cents + 1e-4 || guides.len() >= 256 {
            break;
        }
        guides.push(FretLocationSpec::Cents {
            cents: (cents * 1000.0).round() / 1000.0,
        });
        k += 1;
    }
    Ok(guides)
}

/// Recomputes all fret numbers on `frets` according to the
/// **"Anchor Frets + Auto-Interpolated Tenths (with Inline Badge Override)"** rules:
///
/// 1. Sort frets by ascending cents position.
/// 2. Full-neck frets (covering all `1..=num_strings`) act as whole-number Anchor Frets
///    (unless manually overridden, or if there are no bracketed anchors).
/// 3. Partial-string fretlets bracketed between Anchor $A_L = (C_L, N_L)$ (where nut is $(0.0, 0.0)$)
///    and Anchor $A_R = (C_R, N_R)$ with $N_R = N_L + 1$ interpolate to the nearest tenth (`0.1`):
///    $$\text{fret} = N_L + \text{round}_{0.1}\!\left(\frac{C - C_L}{C_R - C_L}\right)$$
/// 4. If a fret has `manual_number: Some(val)`, it uses `val` (rounded to `0.1`), and subsequent
///    non-interpolated / anchor frets carry on from `floor(val) + 1`.
pub fn recompute_fret_numbers(frets: &mut [CustomFret], num_strings: u8) {
    frets.sort_by(|a, b| {
        a.cents()
            .partial_cmp(&b.cents())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if frets.is_empty() {
        return;
    }

    let has_any_full_fret = frets.iter().any(|f| f.is_full_neck(num_strings));

    if has_any_full_fret {
        // First pass: assign whole numbers to full-neck anchor frets (and respect manual overrides on full frets)
        let mut next_whole: i32 = 1;
        let mut anchor_indices: Vec<(usize, f64, f64)> = Vec::new(); // (idx_in_frets, cents, number)
        for (i, f) in frets.iter_mut().enumerate() {
            if let Some(manual) = f.manual_number {
                let rounded = (manual * 10.0).round() / 10.0;
                f.fret_number = rounded;
                next_whole = rounded.floor() as i32 + 1;
                if f.is_full_neck(num_strings) {
                    anchor_indices.push((i, f.cents(), rounded));
                }
            } else if f.is_full_neck(num_strings) {
                f.fret_number = next_whole as f64;
                anchor_indices.push((i, f.cents(), next_whole as f64));
                next_whole += 1;
            }
        }

        // Second pass: assign partial fretlets by interpolating between bracketing anchors
        // (including the nut at cents = 0.0, fret = 0.0), or falling back to next whole number after the last fret.
        let mut running_floor: i32 = 0;
        for i in 0..frets.len() {
            if frets[i].manual_number.is_some() || frets[i].is_full_neck(num_strings) {
                running_floor = frets[i].fret_number.floor() as i32;
                continue;
            }

            let c = frets[i].cents();
            // Find left anchor (or Nut at 0.0, 0.0)
            let (c_left, n_left) = anchor_indices
                .iter()
                .rev()
                .find(|&&(idx, _, _)| idx < i)
                .map(|&(_, ac, an)| (ac, an))
                .unwrap_or((0.0, 0.0));

            // Find right anchor
            let right_anchor = anchor_indices
                .iter()
                .find(|&&(idx, _, _)| idx > i)
                .map(|&(_, ac, an)| (ac, an));

            if let Some((c_right, n_right)) = right_anchor {
                if c_right > c_left + 1e-6 {
                    let frac = (c - c_left) / (c_right - c_left);
                    let interp = n_left + (n_right - n_left) * frac;
                    let rounded = ((interp * 10.0).round() / 10.0).clamp(n_left + 0.1, n_right - 0.1);
                    frets[i].fret_number = rounded;
                    running_floor = rounded.floor() as i32;
                    continue;
                }
            }

            // Fallback when beyond the rightmost full-neck fret
            let assigned = (running_floor + 1) as f64;
            frets[i].fret_number = assigned;
            running_floor = assigned as i32;
        }
    } else {
        // No full-neck frets exist: sequential whole numbers, respecting manual overrides
        // ("If the user specifies a decimal, carry on from the next whole number on; if whole, carry on from the whole number that follows")
        let mut next_whole: i32 = 1;
        for f in frets.iter_mut() {
            if let Some(manual) = f.manual_number {
                let rounded = (manual * 10.0).round() / 10.0;
                f.fret_number = rounded;
                next_whole = rounded.floor() as i32 + 1;
            } else {
                f.fret_number = next_whole as f64;
                next_whole += 1;
            }
        }
    }
}

/// Checks whether adding a fret at `candidate_cents` on `candidate_strings` violates the
/// 10-cent per-string minimum proximity rule against existing frets.
/// If an existing fret is within `< 0.05` cents, returns `Some(existing_idx)` so they can merge strings;
/// if within `0.05 .. 10.0` cents on any shared string, returns `Err` describing the collision.
pub fn check_fret_proximity(
    existing: &[CustomFret],
    candidate_cents: f64,
    candidate_strings: &[u8],
) -> Result<Option<usize>, String> {
    for (idx, fret) in existing.iter().enumerate() {
        let diff = (fret.cents() - candidate_cents).abs();
        if diff < 0.05 {
            return Ok(Some(idx));
        }
        if diff < MIN_FRET_PROXIMITY_CENTS {
            for &s in candidate_strings {
                if fret.strings.contains(&s) {
                    return Err(format!(
                        "Cannot place '{}' ({:.1}c) on String {}: within {:.1}c of existing fret '{}' ({:.1}c, min limit 10c).",
                        candidate_cents,
                        candidate_cents,
                        s,
                        diff,
                        fret.spec.display_label(),
                        fret.cents()
                    ));
                }
            }
        }
    }
    Ok(None)
}

/// Classification of the custom fretboard's YAML `alter` representation mode.
#[derive(Debug, Clone, PartialEq)]
pub enum CustomFretboardAlterMode {
    /// Any cents locations present (or EDOs with LCM > 53) -> 12-EDO 3-decimal-place `alter` approximation.
    ThreeDecimal12Edo,
    /// Only ratios specified -> `alter` uses ratio string offsets from `1/1`, and `global._x.tuning.referenceNote` stores `1/1` Hz.
    PureRatio { reference_hz: f64 },
    /// Only EDO-step frets where $\text{LCM} \le 53$ -> unified `L`-EDO `m\L` `alter`.
    UnifiedEdo { lcm_edo: u32 },
}

/// Determines the YAML `alter` representation mode for a custom fretboard per user specification:
/// 1. If any fret uses `Cents`, uses `ThreeDecimal12Edo`.
/// 2. If all frets use `Ratio`, uses `PureRatio`.
/// 3. If all frets use `EdoStep` and the LCM of all fret EDOs is $\le 53$, uses `UnifiedEdo { lcm_edo }`.
/// 4. Otherwise falls back to `ThreeDecimal12Edo`.
pub fn classify_fretboard_alter_mode(
    frets: &[CustomFret],
    reference_hz: f64,
) -> CustomFretboardAlterMode {
    if frets.is_empty() {
        return CustomFretboardAlterMode::ThreeDecimal12Edo;
    }

    let all_ratios = frets
        .iter()
        .all(|f| matches!(f.spec, FretLocationSpec::Ratio { .. }));
    if all_ratios {
        return CustomFretboardAlterMode::PureRatio { reference_hz };
    }

    let all_edo = frets
        .iter()
        .all(|f| matches!(f.spec, FretLocationSpec::EdoStep { .. }));
    if all_edo {
        let mut lcm: u64 = 1;
        for f in frets {
            if let FretLocationSpec::EdoStep { edo, .. } = f.spec {
                lcm = lcm_u64(lcm, edo as u64);
                if lcm > 53 {
                    break;
                }
            }
        }
        if lcm >= 1 && lcm <= 53 {
            return CustomFretboardAlterMode::UnifiedEdo {
                lcm_edo: lcm as u32,
            };
        }
    }

    CustomFretboardAlterMode::ThreeDecimal12Edo
}

/// Validates whether `s` is a valid ups/downs pitch name in 53-EDO (e.g. `"G"`, `"vB"`, `"^F#"`).
pub fn is_valid_ups_downs_name(s: &str) -> bool {
    let trimmed = s.trim();
    !trimmed.is_empty() && EdoTuning::new(53).parse_pitch_string(trimmed, 4).is_some()
}

/// Parses a ratio string (`"4/3"`) or cents string (`"498.0"`) for open-string tuning,
/// returning `(normalized_step_string, cents_above_reference)`.
pub fn parse_ratio_or_cents_to_cents(input: &str) -> Option<(String, f64)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((num_s, den_s)) = trimmed.split_once('/') {
        let num = num_s.trim().parse::<u32>().ok()?;
        let den = den_s.trim().parse::<u32>().ok()?;
        if num == 0 || den == 0 {
            return None;
        }
        let cents = 1200.0 * ((num as f64) / (den as f64)).log2();
        if !cents.is_finite() {
            return None;
        }
        Some((format!("{}/{}", num, den), cents))
    } else {
        let clean = trimmed.trim_end_matches('c').trim();
        let val = clean.parse::<f64>().ok()?;
        if !val.is_finite() {
            return None;
        }
        let formatted = if val.fract() == 0.0 {
            format!("{:.1}", val)
        } else {
            format!("{}", val)
        };
        Some((formatted, val))
    }
}

/// Recommends a 53-EDO ups/downs pitch name for a given `offset_cents` above `ref_pitch_name` (e.g. `"D"` + 498.0c -> `"G"`).
pub fn recommend_53edo_name_from_reference(ref_pitch_name: &str, offset_cents: f64) -> String {
    let tuning_53 = EdoTuning::new(53);
    let ref_step = tuning_53
        .parse_pitch_string(ref_pitch_name.trim(), 0)
        .map(|p| tuning_53.pitch_to_steps(&p).rem_euclid(53))
        .unwrap_or(0);
    let delta_steps = ((offset_cents / 1200.0) * 53.0).round() as i32;
    let target_step = (ref_step + delta_steps).rem_euclid(53);
    crate::edo::generate_edo_scale(53)
        .into_iter()
        .find(|(st, _, _, _)| *st == target_step)
        .map(|(_, name, _, _)| name)
        .unwrap_or_else(|| "C".to_string())
}

/// Resolves the 53-EDO ups/downs pitch name for an open string [`Pitch`].
pub fn resolve_open_string_53edo_name(
    open_string_pitch: &Pitch,
    edo: u32,
    ref_pitch_name: Option<&str>,
) -> String {
    if let Some(name) = &open_string_pitch.ext.name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(cents) = open_string_pitch.step_cents_above_reference() {
        let ref_name = ref_pitch_name.unwrap_or("D");
        return recommend_53edo_name_from_reference(ref_name, cents);
    }
    if open_string_pitch.alter.is_none() {
        return open_string_pitch.step.clone();
    }
    let effective_edo = match &open_string_pitch.alter {
        Some(Alteration::Fraction(s)) => s
            .split_once('\\')
            .and_then(|(_, d)| d.trim().parse::<u32>().ok())
            .unwrap_or(edo.max(12)),
        _ => edo.max(12),
    };
    let tuning = EdoTuning::new(effective_edo);
    let step_in_oct = tuning
        .pitch_to_steps(open_string_pitch)
        .rem_euclid(effective_edo as i32);
    crate::edo::generate_edo_scale(effective_edo)
        .into_iter()
        .find(|(st, _, _, _)| *st == step_in_oct)
        .map(|(_, name, _, _)| name)
        .unwrap_or_else(|| open_string_pitch.step.clone())
}

/// Combines an open string's 53-EDO ups/downs name (e.g. `"G"`, step 31) with a fret's C-relative
/// ups/downs name (e.g. `"vE"`, step 17) to produce the sounding note's 53-EDO name (e.g. `"vB"`, step 48).
pub fn combine_53edo_names(
    open_string_name: &str,
    fret_c_relative_name: &str,
    fallback_fret_cents: f64,
) -> String {
    let tuning_53 = EdoTuning::new(53);
    let open_step = tuning_53
        .parse_pitch_string(open_string_name.trim(), 0)
        .map(|p| tuning_53.pitch_to_steps(&p).rem_euclid(53))
        .unwrap_or(0);
    let fret_step = tuning_53
        .parse_pitch_string(fret_c_relative_name.trim(), 0)
        .map(|p| tuning_53.pitch_to_steps(&p).rem_euclid(53))
        .unwrap_or_else(|| ((fallback_fret_cents / 1200.0) * 53.0).round().rem_euclid(53.0) as i32);
    let total_step = (open_step + fret_step).rem_euclid(53);
    crate::edo::generate_edo_scale(53)
        .into_iter()
        .find(|(st, _, _, _)| *st == total_step)
        .map(|(_, name, _, _)| name)
        .unwrap_or_else(|| fret_c_relative_name.to_string())
}

/// Parses the nominal step letter (`C`..`B`) out of an ups/downs note name (e.g. `"vE"` -> `'E'`, `"^F#"` -> `'F'`).
pub fn extract_nominal_from_note_name(name: &str) -> char {
    for c in name.chars() {
        if ('A'..='G').contains(&c) {
            return c;
        }
    }
    'C'
}

/// Returns standard 12-EDO semitones above C for a nominal step character.
pub fn nominal_12edo_semitones(nom: char) -> f64 {
    match nom {
        'C' => 0.0,
        'D' => 2.0,
        'E' => 4.0,
        'F' => 5.0,
        'G' => 7.0,
        'A' => 9.0,
        'B' => 11.0,
        _ => 0.0,
    }
}

/// Synthesizes the MNX [`Pitch`] for a note played on `string` at a given [`CustomFret`] (or open string if `fret_opt` is `None`),
/// applying the 4 YAML rules:
/// - Stores the custom 53-EDO sounding ups/downs name in `pitch._x.name`
/// - Computes `alter` according to [`CustomFretboardAlterMode`] (`3-decimal 12-EDO`, `PureRatio` from `1/1`, or `UnifiedEdo` with $\text{LCM} \le 53$).
pub fn calculate_custom_fret_pitch(
    open_string_pitch: &Pitch,
    fret_opt: Option<&CustomFret>,
    mode: &CustomFretboardAlterMode,
) -> Pitch {
    calculate_custom_fret_pitch_with_ref(open_string_pitch, fret_opt, mode, 12, None)
}

/// Synthesizes the MNX [`Pitch`] for a note played on `string` at a given [`CustomFret`] (or open string if `fret_opt` is `None`),
/// taking into account `edo` and an optional `ref_note` (`1/1` reference note) for Ratio/Cents open strings.
pub fn calculate_custom_fret_pitch_with_ref(
    open_string_pitch: &Pitch,
    fret_opt: Option<&CustomFret>,
    mode: &CustomFretboardAlterMode,
    edo: u32,
    ref_note: Option<&crate::mnx::ReferenceNoteSpec>,
) -> Pitch {
    let ref_pitch_name = ref_note.map(|rn| {
        rn.pitch
            .ext
            .name
            .as_deref()
            .unwrap_or(rn.pitch.step.as_str())
    });
    let open_53_name = resolve_open_string_53edo_name(open_string_pitch, edo, ref_pitch_name);

    let Some(custom_fret) = fret_opt else {
        let mut p = open_string_pitch.clone();
        if p.is_ratio_or_cents_step() && p.ext.name.is_none() {
            p.ext.name = Some(open_53_name);
        }
        return p;
    };

    let sound_53_name =
        combine_53edo_names(&open_53_name, &custom_fret.note_name, custom_fret.cents());
    let open_semitones_from_c0 = open_string_pitch.to_midi_note_with_ref(ref_note) - 12.0;
    let fret_semitones = custom_fret.cents() / 100.0;
    let total_semitones_from_c0 = open_semitones_from_c0 + fret_semitones;

    match mode {
        CustomFretboardAlterMode::UnifiedEdo { lcm_edo } => {
            let tuning = EdoTuning::new(*lcm_edo);
            let open_steps = tuning.pitch_to_steps(open_string_pitch);
            let fret_steps = match custom_fret.spec {
                FretLocationSpec::EdoStep { steps, edo } if edo > 0 => {
                    steps * ((*lcm_edo as i32) / (edo as i32))
                }
                _ => ((custom_fret.cents() / 1200.0) * (*lcm_edo as f64)).round() as i32,
            };
            let mut p = tuning.steps_to_pitch(open_steps + fret_steps);
            p.ext.name = Some(sound_53_name);
            p
        }
        CustomFretboardAlterMode::PureRatio { .. } => {
            let alter = match custom_fret.spec {
                FretLocationSpec::Ratio { num, den } => {
                    Some(Alteration::Fraction(format!("{}/{}", num, den)))
                }
                _ => None,
            };
            Pitch {
                step: open_string_pitch.step.clone(),
                alter,
                octave: open_string_pitch.octave,
                ext: PitchExt {
                    name: Some(sound_53_name),
                },
            }
        }
        CustomFretboardAlterMode::ThreeDecimal12Edo => {
            let nom = extract_nominal_from_note_name(&sound_53_name);
            let nom_semi = nominal_12edo_semitones(nom);
            let octave = ((total_semitones_from_c0 - nom_semi) / 12.0).round() as i32;
            let base_semi = (octave as f64) * 12.0 + nom_semi;
            let raw_alter = total_semitones_from_c0 - base_semi;
            let rounded = (raw_alter * 1000.0).round() / 1000.0;
            let alter = if rounded.abs() < 1e-6 {
                None
            } else {
                Some(Alteration::Value(rounded))
            };
            Pitch {
                step: nom.to_string(),
                alter,
                octave,
                ext: PitchExt {
                    name: Some(sound_53_name),
                },
            }
        }
    }
}

/// Interactive state for the Custom Fretboard Editor dialog (`#6`).
#[derive(Debug, Clone, PartialEq)]
pub struct FretboardEditorState {
    pub num_strings: u8,
    pub guide_input: String,
    pub guides: Vec<FretLocationSpec>,
    pub guide_error: Option<String>,
    pub frets: Vec<CustomFret>,
    pub reference_note_step: String,
    pub reference_note_octave: i32,
    pub reference_frequency_hz: f64,
    pub status_msg: Option<String>,
    /// Active drag operation: `(guide_index, start_string_1_indexed, current_string_1_indexed)`
    pub active_drag: Option<(usize, u8, u8)>,
    /// Index of fret whose top number badge is currently being edited inline, plus text buffer
    pub editing_badge: Option<(usize, String)>,
    /// Selected fret index for editing its ups/downs note name or deleting
    pub selected_fret_idx: Option<usize>,
}

impl FretboardEditorState {
    pub fn from_doc(doc: &crate::mnx::MnxDocument) -> Self {
        let part = doc.parts.first();
        let num_strings = part.map(|p| p.num_strings() as u8).unwrap_or(6);
        let existing_frets = part
            .and_then(|p| p.ext.custom_frets.clone())
            .unwrap_or_default();
        let (ref_step, ref_octave, ref_hz) = doc
            .global
            .ext
            .tuning
            .as_ref()
            .and_then(|t| t.reference_note.as_ref())
            .map(|rn| {
                (
                    rn.pitch.step.clone(),
                    rn.pitch.octave,
                    rn.frequency_hz,
                )
            })
            .unwrap_or_else(|| ("C".to_string(), 4, 261.626));
        Self::new(num_strings, existing_frets, ref_step, ref_octave, ref_hz)
    }

    pub fn new(
        num_strings: u8,
        existing_frets: Vec<CustomFret>,
        ref_step: String,
        ref_octave: i32,
        ref_hz: f64,
    ) -> Self {
        let mut s = Self {
            num_strings: num_strings.clamp(1, 15),
            guide_input: "2\\41".to_string(),
            guides: Vec::new(),
            guide_error: None,
            frets: existing_frets,
            reference_note_step: ref_step,
            reference_note_octave: ref_octave,
            reference_frequency_hz: ref_hz,
            status_msg: None,
            active_drag: None,
            editing_badge: None,
            selected_fret_idx: None,
        };
        s.update_guides();
        recompute_fret_numbers(&mut s.frets, s.num_strings);
        s
    }

    pub fn update_guides(&mut self) {
        match generate_cumulative_guides(&self.guide_input, DEFAULT_NECK_MAX_CENTS) {
            Ok(g) => {
                self.guides = g;
                self.guide_error = None;
            }
            Err(e) => {
                self.guide_error = Some(e);
            }
        }
    }

    /// Synchronizes the fretboard editor's string count with the Instrument Setup's `num_strings`.
    pub fn sync_num_strings(&mut self, new_num_strings: u8) {
        let clamped = new_num_strings.clamp(1, 15);
        if self.num_strings != clamped {
            let old_n = self.num_strings;
            for f in &mut self.frets {
                if f.is_full_neck(old_n) {
                    f.strings = (1..=clamped).collect();
                } else {
                    f.strings.retain(|&s| s >= 1 && s <= clamped);
                    if f.strings.is_empty() {
                        f.strings.push(clamped);
                    }
                }
            }
            self.num_strings = clamped;
            recompute_fret_numbers(&mut self.frets, self.num_strings);
        }
    }

    /// Commits a dragged fret/fretlet from `guide_idx` spanning strings `s_start..=s_end`.
    pub fn commit_dragged_fret(&mut self, guide_idx: usize, s1: u8, s2: u8) {
        let Some(spec) = self.guides.get(guide_idx).cloned() else {
            return;
        };
        let min_s = s1.min(s2).clamp(1, self.num_strings);
        let max_s = s1.max(s2).clamp(1, self.num_strings);
        let strings: Vec<u8> = (min_s..=max_s).collect();
        let cents = spec.to_cents();

        match check_fret_proximity(&self.frets, cents, &strings) {
            Ok(Some(existing_idx)) => {
                // Merge strings into the existing fret at this exact location
                for s in strings {
                    if !self.frets[existing_idx].strings.contains(&s) {
                        self.frets[existing_idx].strings.push(s);
                    }
                }
                self.frets[existing_idx].strings.sort_unstable();
                recompute_fret_numbers(&mut self.frets, self.num_strings);
                self.selected_fret_idx = Some(existing_idx);
                self.status_msg = Some(format!(
                    "Updated fret '{}' strings.",
                    self.frets[existing_idx].spec.display_label()
                ));
            }
            Ok(None) => {
                let default_name = recommend_53edo_note_name_from_cents(cents);
                self.frets.push(CustomFret {
                    fret_number: 1.0,
                    manual_number: None,
                    spec: spec.clone(),
                    strings,
                    note_name: default_name,
                });
                recompute_fret_numbers(&mut self.frets, self.num_strings);
                let new_idx = self
                    .frets
                    .iter()
                    .position(|f| (f.cents() - cents).abs() < 0.05);
                self.selected_fret_idx = new_idx;
                self.status_msg = Some(format!("Placed fret '{}'.", spec.display_label()));
            }
            Err(err) => {
                self.status_msg = Some(err);
            }
        }
    }

    /// Renders the interactive Custom Fretboard Editor UI inside an egui window/panel.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // 1. Top controls: Dotted Guide Grid input + Reference 1/1 Hz
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Dotted Guide Grid:").strong());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.guide_input)
                    .desired_width(95.0)
                    .hint_text("e.g. 5/4, 128.0, 2\\41"),
            );
            if resp.changed() {
                self.update_guides();
            }
            ui.label(
                egui::RichText::new("(Ratio: 5/4 • Cents: 128.0 • EDO Step: 2\\41)")
                    .small()
                    .color(Color32::GRAY),
            );

            ui.separator();
            ui.label("1/1 Reference:");
            egui::ComboBox::from_id_salt("custom_fret_ref_step")
                .selected_text(&self.reference_note_step)
                .width(45.0)
                .show_ui(ui, |ui| {
                    for step in ["C", "D", "E", "F", "G", "A", "B"] {
                        ui.selectable_value(
                            &mut self.reference_note_step,
                            step.to_string(),
                            step,
                        );
                    }
                });
            ui.add(
                egui::DragValue::new(&mut self.reference_note_octave)
                    .range(0..=8)
                    .prefix("Oct "),
            );
            ui.add(
                egui::DragValue::new(&mut self.reference_frequency_hz)
                    .range(20.0..=2000.0)
                    .speed(0.5)
                    .suffix(" Hz"),
            );

            if ui.button("Clear All Frets").clicked() {
                self.frets.clear();
                self.selected_fret_idx = None;
                self.editing_badge = None;
            }
        });

        if let Some(err) = &self.guide_error {
            ui.colored_label(Color32::from_rgb(255, 110, 110), err);
        }

        ui.add_space(4.0);

        // 2. Guitar Neck Canvas inside horizontal ScrollArea
        let string_spacing = 28.0_f32;
        let top_badge_margin = 44.0_f32;
        let bottom_label_margin = 54.0_f32;
        let neck_height = (self.num_strings.saturating_sub(1) as f32) * string_spacing + 24.0;
        let canvas_height = top_badge_margin + neck_height + bottom_label_margin;
        let canvas_width = 1150.0_f32;
        let nut_x_offset = 45.0_f32;
        let usable_neck_w = canvas_width - nut_x_offset - 30.0;

        let cents_to_x = |rect_min_x: f32, cents: f64| -> f32 {
            rect_min_x
                + nut_x_offset
                + ((cents / DEFAULT_NECK_MAX_CENTS) as f32).clamp(0.0, 1.0) * usable_neck_w
        };

        let string_to_y = |rect_min_y: f32, string_num: u8| -> f32 {
            rect_min_y + top_badge_margin + 12.0 + ((string_num.saturating_sub(1)) as f32) * string_spacing
        };

        let num_strings = self.num_strings;
        let y_to_nearest_string = move |rect_min_y: f32, y: f32| -> u8 {
            let rel = (y - (rect_min_y + top_badge_margin + 12.0)) / string_spacing;
            (rel.round() as i32 + 1).clamp(1, num_strings as i32) as u8
        };

        let mut badge_clicked: Option<usize> = None;
        let mut fret_clicked: Option<usize> = None;

        egui::ScrollArea::horizontal()
            .id_salt("fretboard_editor_neck_scroll")
            .show(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(canvas_width, canvas_height),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);

                // Background
                painter.rect_filled(rect, 6.0, Color32::from_rgb(26, 26, 30));

                let neck_top_y = rect.min.y + top_badge_margin;
                let neck_bot_y = neck_top_y + neck_height;
                let nut_x = rect.min.x + nut_x_offset;
                let neck_right_x = rect.max.x - 20.0;

                // Fretboard wood body fill
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(nut_x, neck_top_y),
                        Pos2::new(neck_right_x, neck_bot_y),
                    ),
                    0.0,
                    Color32::from_rgb(42, 36, 32),
                );

                // Parallel top and bottom fretboard edge lines
                painter.line_segment(
                    [Pos2::new(nut_x, neck_top_y), Pos2::new(neck_right_x, neck_top_y)],
                    Stroke::new(2.5, Color32::from_gray(200)),
                );
                painter.line_segment(
                    [Pos2::new(nut_x, neck_bot_y), Pos2::new(neck_right_x, neck_bot_y)],
                    Stroke::new(2.5, Color32::from_gray(200)),
                );

                // Nut line (Fret 0)
                painter.line_segment(
                    [Pos2::new(nut_x, neck_top_y), Pos2::new(nut_x, neck_bot_y)],
                    Stroke::new(5.0, Color32::from_rgb(230, 220, 195)),
                );

                // Faint thick gray parallel string lines
                for s in 1..=self.num_strings {
                    let sy = string_to_y(rect.min.y, s);
                    let thickness = 2.0 + (s as f32) * 0.22;
                    painter.line_segment(
                        [Pos2::new(nut_x, sy), Pos2::new(neck_right_x, sy)],
                        Stroke::new(thickness, Color32::from_rgba_unmultiplied(150, 155, 165, 115)),
                    );
                    painter.text(
                        Pos2::new(nut_x - 10.0, sy),
                        egui::Align2::RIGHT_CENTER,
                        format!("S{}", s),
                        FontId::monospace(11.5),
                        Color32::from_gray(175),
                    );
                }

                // Determine which placed fret's angled label is hovered (if any)
                let hover_pos = ui.input(|i| i.pointer.hover_pos());
                let cos_45 = std::f32::consts::FRAC_1_SQRT_2;
                let sin_45 = std::f32::consts::FRAC_1_SQRT_2;

                let mut hovered_fret_idx: Option<usize> = None;
                let mut min_hover_dist = 9.0_f32;
                if let Some(p) = hover_pos {
                    for (f_idx, fret) in self.frets.iter().enumerate() {
                        let fx = cents_to_x(rect.min.x, fret.cents());
                        let label_text = fret.spec.display_label();
                        let galley = painter.layout_no_wrap(label_text, FontId::monospace(10.0), Color32::WHITE);
                        let tw = galley.size().x;
                        let pos_end = Pos2::new(fx - 2.0, neck_bot_y + 8.0);
                        let pos_start = Pos2::new(pos_end.x - tw * cos_45, pos_end.y + tw * sin_45);
                        let ab = pos_end - pos_start;
                        let ap = p - pos_start;
                        let ab_len_sq = ab.length_sq();
                        let t = if ab_len_sq > 0.0 {
                            (ap.dot(ab) / ab_len_sq).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let closest = pos_start + ab * t;
                        let dist = (p - closest).length();
                        if dist < min_hover_dist {
                            min_hover_dist = dist;
                            hovered_fret_idx = Some(f_idx);
                        }
                    }
                }

                // Dotted guide lines from cumulative grid
                for guide in &self.guides {
                    let gx = cents_to_x(rect.min.x, guide.to_cents());
                    // Draw vertical dotted line
                    let mut y = neck_top_y + 2.0;
                    while y < neck_bot_y - 2.0 {
                        let y_end = (y + 4.0).min(neck_bot_y - 2.0);
                        painter.line_segment(
                            [Pos2::new(gx, y), Pos2::new(gx, y_end)],
                            Stroke::new(1.2, Color32::from_rgba_unmultiplied(130, 195, 255, 130)),
                        );
                        y += 8.0;
                    }
                    // Only draw guide label if no placed fret already occupies this location
                    let has_placed_fret = self.frets.iter().any(|f| (f.cents() - guide.to_cents()).abs() < 0.5);
                    if !has_placed_fret {
                        let g_text = guide.display_label();
                        let g_galley = painter.layout_no_wrap(
                            g_text,
                            FontId::monospace(9.5),
                            Color32::from_rgba_unmultiplied(130, 195, 255, 160),
                        );
                        let g_tw = g_galley.size().x;
                        let g_term = Pos2::new(gx - 2.0, neck_bot_y + 8.0);
                        let g_start = Pos2::new(g_term.x - g_tw * cos_45, g_term.y + g_tw * sin_45);
                        let mut g_shape = egui::epaint::TextShape::new(
                            g_start,
                            g_galley,
                            Color32::from_rgba_unmultiplied(130, 195, 255, 160),
                        );
                        g_shape.angle = -std::f32::consts::FRAC_PI_4;
                        painter.add(g_shape);
                    }
                }

                // Placed solid frets / fretlets + clickable fret number badges
                for (f_idx, fret) in self.frets.iter().enumerate() {
                    let fx = cents_to_x(rect.min.x, fret.cents());
                    let is_sel = self.selected_fret_idx == Some(f_idx);
                    let fret_color = if is_sel {
                        Color32::from_rgb(255, 200, 90)
                    } else {
                        Color32::from_rgb(235, 240, 245)
                    };

                    // Draw thick vertical segment across each fretted string (centered vertically across the string)
                    let half_span = string_spacing * 0.5;
                    for &s in &fret.strings {
                        let sy = string_to_y(rect.min.y, s);
                        let seg_top = (sy - half_span).max(neck_top_y);
                        let seg_bot = (sy + half_span).min(neck_bot_y);
                        painter.line_segment(
                            [Pos2::new(fx, seg_top), Pos2::new(fx, seg_bot)],
                            Stroke::new(4.2, fret_color),
                        );
                    }

                    // Top fret-number badge rotated 90 degrees counterclockwise
                    let badge_text = fret.formatted_fret_number();
                    let galley = painter.layout_no_wrap(badge_text.clone(), FontId::monospace(10.5), Color32::WHITE);
                    let tw = galley.size().x;
                    let th = galley.size().y;
                    let badge_w = 16.0_f32;
                    let badge_h = (tw + 8.0).max(24.0);
                    let badge_bottom_y = neck_top_y - 4.0;
                    let badge_rect = Rect::from_min_max(
                        Pos2::new(fx - badge_w / 2.0, badge_bottom_y - badge_h),
                        Pos2::new(fx + badge_w / 2.0, badge_bottom_y),
                    );
                    let badge_bg = if self.editing_badge.as_ref().map(|(i, _)| *i) == Some(f_idx) {
                        Color32::from_rgb(220, 120, 50)
                    } else if fret.manual_number.is_some() {
                        Color32::from_rgb(75, 110, 155)
                    } else {
                        Color32::from_rgb(55, 60, 70)
                    };
                    painter.rect_filled(badge_rect, 3.5, badge_bg);
                    if is_sel {
                        painter.rect_stroke(
                            badge_rect,
                            3.5,
                            Stroke::new(1.5, Color32::from_rgb(255, 200, 90)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let text_pos = Pos2::new(
                        badge_rect.center().x - th / 2.0,
                        badge_rect.center().y + tw / 2.0,
                    );
                    let mut text_shape = egui::epaint::TextShape::new(text_pos, galley, Color32::WHITE);
                    text_shape.angle = -std::f32::consts::FRAC_PI_2;
                    painter.add(text_shape);

                    // Bottom label at 45 degree angle terminating at the fret (NO note name in text)
                    let label_text = fret.spec.display_label();
                    let is_hovered = hovered_fret_idx == Some(f_idx);
                    let label_color = if is_hovered {
                        Color32::from_rgb(255, 230, 130)
                    } else if is_sel {
                        Color32::from_rgb(255, 200, 90)
                    } else {
                        fret_color
                    };
                    let l_galley = painter.layout_no_wrap(label_text, FontId::monospace(10.0), label_color);
                    let l_tw = l_galley.size().x;
                    let pos_end = Pos2::new(fx - 2.0, neck_bot_y + 8.0);
                    let pos_start = Pos2::new(pos_end.x - l_tw * cos_45, pos_end.y + l_tw * sin_45);
                    let mut l_shape = egui::epaint::TextShape::new(pos_start, l_galley, label_color);
                    l_shape.angle = -std::f32::consts::FRAC_PI_4;
                    painter.add(l_shape);

                    // Subtle tick connecting bottom rail to the label end
                    painter.line_segment(
                        [Pos2::new(fx, neck_bot_y), Pos2::new(fx, neck_bot_y + 5.0)],
                        Stroke::new(1.2, fret_color),
                    );

                    // Note name popup tooltip on hover over the cent/ratio/edo step text
                    if is_hovered {
                        if let Some(p) = hover_pos {
                            egui::Area::new(egui::Id::new("fret_note_tooltip").with(f_idx))
                                .fixed_pos(p + egui::vec2(12.0, 12.0))
                                .order(egui::Order::Tooltip)
                                .interactable(false)
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label("Note:");
                                            ui.label(
                                                egui::RichText::new(&fret.note_name)
                                                    .strong()
                                                    .color(Color32::from_rgb(255, 215, 100)),
                                            );
                                        });
                                    });
                                });
                        }
                    }

                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if badge_rect.expand(2.5).contains(pos) {
                                badge_clicked = Some(f_idx);
                            } else if (pos.x - fx).abs() <= 7.0
                                && pos.y >= neck_top_y
                                && pos.y <= neck_bot_y
                            {
                                fret_clicked = Some(f_idx);
                            } else if is_hovered {
                                fret_clicked = Some(f_idx);
                            }
                        }
                    }
                }

                // Handle click-and-drag along a dotted guide line to place frets/fretlets
                if response.drag_started() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if pos.y >= neck_top_y - 8.0 && pos.y <= neck_bot_y + 8.0 {
                            // Find nearest dotted guide within 14px
                            let mut best_guide = None;
                            let mut best_dist = 14.0_f32;
                            for (g_idx, guide) in self.guides.iter().enumerate() {
                                let gx = cents_to_x(rect.min.x, guide.to_cents());
                                let dist = (pos.x - gx).abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    best_guide = Some(g_idx);
                                }
                            }
                            if let Some(g_idx) = best_guide {
                                let s = y_to_nearest_string(rect.min.y, pos.y);
                                self.active_drag = Some((g_idx, s, s));
                            }
                        }
                    }
                }

                if response.dragged() {
                    if let Some((g_idx, s_start, _)) = self.active_drag {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let s_cur = y_to_nearest_string(rect.min.y, pos.y);
                            self.active_drag = Some((g_idx, s_start, s_cur));
                        }
                    }
                }

                // Draw active drag preview
                if let Some((g_idx, s1, s2)) = self.active_drag {
                    if let Some(guide) = self.guides.get(g_idx) {
                        let gx = cents_to_x(rect.min.x, guide.to_cents());
                        let min_s = s1.min(s2);
                        let max_s = s1.max(s2);
                        let half_span = string_spacing * 0.5;
                        for s in min_s..=max_s {
                            let sy = string_to_y(rect.min.y, s);
                            let seg_top = (sy - half_span).max(neck_top_y);
                            let seg_bot = (sy + half_span).min(neck_bot_y);
                            painter.line_segment(
                                [Pos2::new(gx, seg_top), Pos2::new(gx, seg_bot)],
                                Stroke::new(5.0, Color32::from_rgb(110, 235, 145)),
                            );
                        }
                    }
                }

                if response.drag_stopped() {
                    if let Some((g_idx, s1, s2)) = self.active_drag.take() {
                        self.commit_dragged_fret(g_idx, s1, s2);
                    }
                } else if response.clicked() && badge_clicked.is_none() && fret_clicked.is_none() {
                    // Single click on a dotted guide line places a fretlet on the clicked string
                    if let Some(pos) = response.interact_pointer_pos() {
                        if pos.y >= neck_top_y && pos.y <= neck_bot_y {
                            let mut best_guide = None;
                            let mut best_dist = 12.0_f32;
                            for (g_idx, guide) in self.guides.iter().enumerate() {
                                let gx = cents_to_x(rect.min.x, guide.to_cents());
                                let dist = (pos.x - gx).abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    best_guide = Some(g_idx);
                                }
                            }
                            if let Some(g_idx) = best_guide {
                                let s = y_to_nearest_string(rect.min.y, pos.y);
                                self.commit_dragged_fret(g_idx, s, s);
                            }
                        }
                    }
                }
            });

        if let Some(b_idx) = badge_clicked {
            let cur_num = self.frets[b_idx].formatted_fret_number();
            self.editing_badge = Some((b_idx, cur_num));
            self.selected_fret_idx = Some(b_idx);
        } else if let Some(f_idx) = fret_clicked {
            self.selected_fret_idx = Some(f_idx);
        }

        // 3. Inline Fret Number Badge Override & Ups/Downs Note Name Inspector
        ui.add_space(6.0);
        ui.separator();

        if let Some((b_idx, mut buf)) = self.editing_badge.clone() {
            if b_idx < self.frets.len() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Override Fret Badge for '{}':",
                            self.frets[b_idx].spec.display_label()
                        ))
                        .strong(),
                    );
                    let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(60.0));
                    let apply_clicked = ui.button("Apply Number").clicked()
                        || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    let reset_clicked = ui.button("Auto Number").clicked();
                    let cancel_clicked = ui.button("Cancel").clicked();

                    if apply_clicked {
                        if let Ok(val) = buf.trim().parse::<f64>() {
                            if val > 0.0 {
                                self.frets[b_idx].manual_number =
                                    Some((val * 10.0).round() / 10.0);
                                recompute_fret_numbers(&mut self.frets, self.num_strings);
                            }
                        }
                        self.editing_badge = None;
                    } else if reset_clicked {
                        self.frets[b_idx].manual_number = None;
                        recompute_fret_numbers(&mut self.frets, self.num_strings);
                        self.editing_badge = None;
                    } else if cancel_clicked {
                        self.editing_badge = None;
                    } else {
                        self.editing_badge = Some((b_idx, buf));
                    }
                });
            } else {
                self.editing_badge = None;
            }
        }

        // Table of placed frets with editable note names
        ui.label(
            egui::RichText::new("Placed Frets & Note Names:").strong(),
        );
        let mut delete_idx = None;
        let mut recompute_needed = false;
        egui::ScrollArea::vertical()
            .id_salt("placed_custom_frets_list")
            .max_height(135.0)
            .show(ui, |ui| {
                if self.frets.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "Click or drag vertically across any dotted line on the neck above to place frets or partial fretlets.",
                        )
                        .italics()
                        .color(Color32::GRAY),
                    );
                }
                for (idx, fret) in self.frets.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("Fret {}", fret.formatted_fret_number()))
                                .monospace()
                                .strong(),
                        );
                        ui.label(format!(
                            "({}) — {:.1}¢ — Strings {:?}",
                            fret.spec.display_label(),
                            fret.cents(),
                            fret.strings
                        ));
                        ui.label("Note Name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut fret.note_name).desired_width(60.0),
                        );
                        if ui.small_button("Default").clicked() {
                            fret.note_name = recommend_53edo_note_name_from_cents(fret.cents());
                        }
                        if ui.small_button("🗑").on_hover_text("Remove fret").clicked() {
                            delete_idx = Some(idx);
                        }
                    });
                }
            });

        if let Some(del) = delete_idx {
            self.frets.remove(del);
            recompute_needed = true;
            self.selected_fret_idx = None;
        }
        if recompute_needed {
            recompute_fret_numbers(&mut self.frets, self.num_strings);
        }

        if let Some(msg) = &self.status_msg {
            ui.label(egui::RichText::new(msg).italics().color(Color32::from_rgb(180, 220, 255)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cumulative_guides_ratio_cents_and_edo() {
        // 1. Ratio 5/4 -> 5/4, 25/16, 125/64...
        let ratios = generate_cumulative_guides("5/4", 1200.0).unwrap();
        assert!(ratios.len() >= 3);
        assert_eq!(ratios[0], FretLocationSpec::Ratio { num: 5, den: 4 });
        assert_eq!(ratios[1], FretLocationSpec::Ratio { num: 25, den: 16 });
        assert_eq!(ratios[2], FretLocationSpec::Ratio { num: 125, den: 64 });

        // 2. Cents 128.0 -> 128.0, 256.0, 384.0...
        let cents = generate_cumulative_guides("128.0", 600.0).unwrap();
        assert_eq!(cents.len(), 4);
        assert_eq!(cents[0], FretLocationSpec::Cents { cents: 128.0 });
        assert_eq!(cents[1], FretLocationSpec::Cents { cents: 256.0 });

        // 3. EDO Step 2\41 -> 2\41, 4\41, 6\41...
        let edos = generate_cumulative_guides("2\\41", 300.0).unwrap();
        assert!(!edos.is_empty());
        assert_eq!(edos[0], FretLocationSpec::EdoStep { steps: 2, edo: 41 });
        assert_eq!(edos[1], FretLocationSpec::EdoStep { steps: 4, edo: 41 });
    }

    #[test]
    fn test_anchor_frets_and_tenth_interpolation_numbering() {
        // Frets at 2\41 (full neck = 1), 3\41 (fretlet on string 1 = 1.5), 4\41 (full neck = 2)
        let mut frets = vec![
            CustomFret {
                fret_number: 0.0,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: 2, edo: 41 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "C#".to_string(),
            },
            CustomFret {
                fret_number: 0.0,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: 3, edo: 41 },
                strings: vec![1],
                note_name: "^C#".to_string(),
            },
            CustomFret {
                fret_number: 0.0,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: 4, edo: 41 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "vD".to_string(),
            },
        ];

        recompute_fret_numbers(&mut frets, 6);
        assert_eq!(frets[0].formatted_fret_number(), "1");
        assert_eq!(frets[1].formatted_fret_number(), "1.5");
        assert_eq!(frets[2].formatted_fret_number(), "2");
    }

    #[test]
    fn test_manual_override_carry_on_numbering() {
        // All partial fretlets with a manual override of 2.3 on the first -> next carries on from 3!
        let mut frets = vec![
            CustomFret {
                fret_number: 0.0,
                manual_number: Some(2.3),
                spec: FretLocationSpec::Ratio { num: 9, den: 8 },
                strings: vec![1, 2],
                note_name: "D".to_string(),
            },
            CustomFret {
                fret_number: 0.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 5, den: 4 },
                strings: vec![1, 2],
                note_name: "vE".to_string(),
            },
            CustomFret {
                fret_number: 0.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 4, den: 3 },
                strings: vec![1, 2],
                note_name: "F".to_string(),
            },
        ];

        recompute_fret_numbers(&mut frets, 6);
        assert_eq!(frets[0].formatted_fret_number(), "2.3");
        assert_eq!(frets[1].formatted_fret_number(), "3");
        assert_eq!(frets[2].formatted_fret_number(), "4");
    }

    #[test]
    fn test_10_cent_proximity_limit() {
        // 3/2 is ~701.955 cents; 24\41 is 24/41 * 1200 = 702.439 cents (diff ~0.48c < 10c!)
        let existing = vec![CustomFret {
            fret_number: 1.0,
            manual_number: None,
            spec: FretLocationSpec::Ratio { num: 3, den: 2 },
            strings: vec![1, 2, 3],
            note_name: "G".to_string(),
        }];

        let spec_24_41 = FretLocationSpec::EdoStep { steps: 24, edo: 41 };
        // Collides on string 2:
        let res_same_string = check_fret_proximity(&existing, spec_24_41.to_cents(), &[2]);
        assert!(res_same_string.is_err());

        // Allowed on string 4 (different string!):
        let res_diff_string = check_fret_proximity(&existing, spec_24_41.to_cents(), &[4]);
        assert_eq!(res_diff_string, Ok(None));
    }

    #[test]
    fn test_yaml_alter_classification_and_lcm_35edo() {
        // 2\5 and 6\7 -> LCM(5, 7) = 35 <= 53 -> UnifiedEdo { lcm_edo: 35 }
        let frets = vec![
            CustomFret {
                fret_number: 1.0,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: 2, edo: 5 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: recommend_53edo_note_name_from_cents(480.0),
            },
            CustomFret {
                fret_number: 2.0,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: 6, edo: 7 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: recommend_53edo_note_name_from_cents(1028.57),
            },
        ];

        let mode = classify_fretboard_alter_mode(&frets, 261.626);
        assert_eq!(mode, CustomFretboardAlterMode::UnifiedEdo { lcm_edo: 35 });

        let open_c4 = Pitch {
            step: "C".to_string(),
            alter: None,
            octave: 4,
            ext: PitchExt::default(),
        };
        let p1 = calculate_custom_fret_pitch(&open_c4, Some(&frets[0]), &mode);
        let p2 = calculate_custom_fret_pitch(&open_c4, Some(&frets[1]), &mode);
        // 2\5 = 14\35, 6\7 = 30\35: their alterations in 35-EDO must have denominator \35
        assert!(
            p1.alter
                .as_ref()
                .map(|a| a.to_string().ends_with("\\35"))
                .unwrap_or(true)
        );
        assert!(
            p2.alter
                .as_ref()
                .map(|a| a.to_string().ends_with("\\35"))
                .unwrap_or(true)
        );
        assert_eq!(p1.ext.name, Some(frets[0].note_name.clone()));
    }

    #[test]
    fn test_custom_fretboard_collision_free_labels_and_hover_note_names() {
        let fret_ratio = CustomFret {
            fret_number: 1.0,
            manual_number: None,
            spec: FretLocationSpec::Ratio { num: 5, den: 4 },
            strings: vec![1, 2, 3, 4, 5, 6],
            note_name: recommend_53edo_note_name_from_cents(386.31),
        };
        let fret_cents = CustomFret {
            fret_number: 2.0,
            manual_number: None,
            spec: FretLocationSpec::Cents { cents: 128.0 },
            strings: vec![1, 2],
            note_name: recommend_53edo_note_name_from_cents(128.0),
        };
        let fret_edo = CustomFret {
            fret_number: 3.5,
            manual_number: None,
            spec: FretLocationSpec::EdoStep { steps: 2, edo: 41 },
            strings: vec![1],
            note_name: recommend_53edo_note_name_from_cents(2.0 / 41.0 * 1200.0),
        };

        // 1. Bottom labels should only display the spec without note name guess
        assert_eq!(fret_ratio.spec.display_label(), "5/4");
        assert!(!fret_ratio.spec.display_label().contains(&fret_ratio.note_name));

        assert_eq!(fret_cents.spec.display_label(), "128.0c");
        assert!(!fret_cents.spec.display_label().contains(&fret_cents.note_name));

        assert_eq!(fret_edo.spec.display_label(), "2\\41");
        assert!(!fret_edo.spec.display_label().contains(&fret_edo.note_name));

        // 2. Note names available for hover popup
        assert_eq!(fret_ratio.note_name, "vE");
        assert_eq!(fret_cents.note_name, "^C#");

        // 3. Top badge strings for 90-degree rotated badges
        assert_eq!(fret_ratio.formatted_fret_number(), "1");
        assert_eq!(fret_cents.formatted_fret_number(), "2");
        assert_eq!(fret_edo.formatted_fret_number(), "3.5");
    }
}
