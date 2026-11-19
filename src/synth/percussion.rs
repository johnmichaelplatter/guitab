//! Acoustic physical-modeling drum kit, cast B20 bronze cymbals, and 2-operator FM General MIDI
//! synthesis (`src/synth/percussion.rs`).
//!
//! ### Architecture
//! Implements a fixed-size, heap-backed pool of 16 pre-allocated polyphonic voices
//! ([`PercussionAndGmPool`]) covering:
//! 1. **Circular Membrane Bessel Modal Drums — Kick, Snare & Toms (GM Notes 35, 36, 37, 38, 40, 41–50):**
//!    - 6-mode 2-pole Zero-Delay Feedback (ZDF / TPT) SVF modal resonator bank tuned to air-loaded
//!      circular membrane Bessel ratios `[1.00, 1.59, 2.13, 2.29, 2.65, 2.91] * base_freq`.
//!    - Higher Bessel modes decay progressively faster than the `(0,1)` fundamental (`overtones_ring`),
//!      settling from mylar attack overtones into a warm fundamental body.
//!    - Non-linear membrane tension modulation tracks instantaneous drumhead energy and shifts all
//!      6 modal frequencies upward on hard hits (decaying over 30–80 ms).
//!    - `strike_position` (`0.0` = center punch, `1.0` = rim/edge ring) weights excitation between
//!      axisymmetric modes (`1.00, 2.29`) and non-axisymmetric edge modes (`1.59, 2.13, 2.65, 2.91`).
//!    - Cylindrical wooden shell air-cavity mode + 2-tap micro-reflection (`< 1.5 ms`) adds realistic
//!      wooden shell knock and body resonance.
//! 2. **Physically Coupled Snare Wires & Hand Clap (GM Notes 37, 38, 39, 40):**
//!    - Snare wire rattle is driven directly from the instantaneous amplitude envelope of the snare
//!      membrane's fundamental `(1.00)` and `(1.59)` modes, passed through a non-linear rattle
//!      threshold (`wire_tension`) and a short metallic comb + `1.2 kHz – 8 kHz` bandpass network.
//!    - Ghost notes buzz delicately while hard hits/rimshots snap and sputter naturally as the head decays.
//! 3. **Cast B20 Bronze Dispersive Waveguide Cymbals & Hi-Hats (GM Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59):**
//!    - 4-delay-line orthogonal Hadamard feedback waveguide network with mutually prime delay lengths
//!      scaled by `bronze_size_pitch`, paired with 6 high-Q cast-bronze modal resonators for stick/bell ping.
//!    - Each waveguide loop contains a 1st-order non-linear All-Pass dispersion filter with signal-modulated
//!      coefficient `a_eff = a_base + nonlin_shimmer * sample.clamp(-1.0, 1.0)` creating authentic
//!      4 kHz – 15 kHz cast-bronze shimmer bloom over the first ~25 ms.
//!    - Differentiates Ride Bow (51), Ride Bell (53), Crash (49/57), China (52), Splash (55), and
//!      Hi-Hats (42/44/46) with dynamic loop damping on closed/pedal choke and edge-contact sizzle.
//! 4. **Basic GM Melodic Voices (`TriggerGmVoice`):** Compact 2-operator FM voice with selectable
//!    presets (`EPiano`, `Organ`, `Pad`, `Brass`, `SynthBass`) mapped from GM program numbers,
//!    accepting exact `f64` Hz frequencies for microtonal backing tracks.
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, and zero `unwrap()`/`panic!()` on the audio thread.
//! - All 16 voices, modal banks, shell buffers, wire comb buffers, and cymbal waveguide delay lines
//!   are pre-allocated at stream initialization.
//! - Denormal floats are flushed to zero in all modal, waveguide, envelope, and filter states.

use crate::synth::backend::flush_denormal;
use crate::synth::body::{tanh_pade, BiquadFilter};
use crate::synth::patch::{DrumKitParams, DrumRole, DrumVoiceParams, FmVoiceParams};
use std::f32::consts::{PI, TAU};

/// Fixed polyphonic voice count for percussion and GM melodic backing.
pub const MAX_POLY_VOICES: usize = 16;

/// Air-loaded circular drumhead membrane Bessel modal frequency ratios relative to `(0,1)` fundamental.
pub const MEMBRANE_BESSEL_RATIOS: [f32; 6] = [1.00, 1.59, 2.13, 2.29, 2.65, 2.91];

/// Inharmonic cast B20 bronze plate & bell eigenmode ratios for initial stick ping and bell shimmer.
pub const BRONZE_MODAL_RATIOS: [f32; 6] = [1.00, 1.47, 2.09, 2.83, 3.61, 4.78];

/// Number of parallel modal SVF resonators per drum/cymbal voice.
const NUM_MODES: usize = 6;

/// Number of delay lines in the bronze cymbal Hadamard dispersive waveguide network.
const CYMBAL_LINES: usize = 4;

/// Fixed power-of-two buffer length per cymbal waveguide line (512 samples).
const CYMBAL_DELAY_LEN: usize = 512;
const CYMBAL_DELAY_MASK: usize = CYMBAL_DELAY_LEN - 1;

/// Fixed power-of-two buffer length for wooden shell micro-reflections (< 1.5 ms) and snare wire comb.
const SHORT_COMB_LEN: usize = 128;
const SHORT_COMB_MASK: usize = SHORT_COMB_LEN - 1;

/// Allocates a zeroed `Box<[[f32; CYMBAL_DELAY_LEN]; CYMBAL_LINES]>` directly on the heap
/// without staging an 8 KB array on the stack.
fn alloc_cymbal_lines() -> Box<[[f32; CYMBAL_DELAY_LEN]; CYMBAL_LINES]> {
    let mut v = Vec::with_capacity(CYMBAL_LINES);
    for _ in 0..CYMBAL_LINES {
        v.push([0.0_f32; CYMBAL_DELAY_LEN]);
    }
    match v.into_boxed_slice().try_into() {
        Ok(b) => b,
        Err(_) => unreachable!(),
    }
}

/// 2-pole Zero-Delay Feedback (Cytomic TPT) resonant State Variable Filter modal resonator.
///
/// Unconditionally stable and energy-preserving under fast audio-rate frequency modulation
/// (`Non-Linear Membrane Tension Modulation`), with zero trig calls per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfMode {
    /// Base integrator tuning `tan(pi * f_0 / sr)`.
    g_base: f32,
    /// Damping `k = 1 / Q` computed directly from target modal $T_{60}$.
    k: f32,
    /// Impulse-area normalized modal amplitude gain.
    gain: f32,
    /// Integrator state 1 (bandpass velocity).
    ic1eq: f32,
    /// Integrator state 2 (lowpass displacement).
    ic2eq: f32,
}

impl SvfMode {
    /// Configures the resonant mode frequency, exact $T_{60}$ decay duration, and normalized gain.
    #[inline(always)]
    pub fn configure(&mut self, sample_rate: f32, freq_hz: f32, t60_sec: f32, gain: f32) {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(20.0, sr * 0.42);
        let g = (PI * f / sr).tan().clamp(1.0e-4, 1.65);
        // Exact damping k = 1/Q for -60 dB (10^-3) decay over t60_sec:
        // exp(-pi * f * k * t60) = 10^-3  =>  k = ln(1000) / (pi * f * t60) = 2.1988 / (f * t60)
        let k = (2.1988 / (f * t60_sec.max(0.008))).clamp(0.0008, 2.0);
        self.g_base = g;
        self.k = k;
        // Unit-area impulse produces peak bandpass state v1 ~= 2*g; normalize by 1/(2*g)
        // so modal amplitude is consistent across all frequencies and sample rates.
        self.gain = gain / (2.0 * g).max(0.004);
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Increases modal damping dynamically (used for physical hi-hat clamping / choke).
    #[inline(always)]
    pub fn damp_choke(&mut self, min_k: f32) {
        self.k = self.k.max(min_k).min(2.0);
    }

    /// Steps one sample with audio-rate frequency multiplier `freq_mult` (`>= 1.0` for head stretch).
    #[inline(always)]
    pub fn process(&mut self, x: f32, freq_mult: f32) -> f32 {
        let g = (self.g_base * freq_mult).clamp(1.0e-4, 1.85);
        let a1 = 1.0 / (1.0 + g * (g + self.k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        self.ic1eq = flush_denormal(2.0 * v1 - self.ic1eq);
        self.ic2eq = flush_denormal(2.0 * v2 - self.ic2eq);
        flush_denormal(v1 * self.gain)
    }

    /// Returns instantaneous stored modal energy (proportional to squared amplitude).
    #[inline(always)]
    pub fn energy(&self) -> f32 {
        self.ic1eq * self.ic1eq + self.ic2eq * self.ic2eq
    }
}

/// Cylindrical wooden drum shell air-cavity mode + 2-tap micro-reflection (`< 1.5 ms`).
#[derive(Clone, Debug)]
struct ShellResonator {
    cavity_mode: SvfMode,
    buf: [f32; SHORT_COMB_LEN],
    write_idx: usize,
    tap1: usize,
    tap2: usize,
    lp_z1: f32,
    lp_damp: f32,
}

impl Default for ShellResonator {
    fn default() -> Self {
        Self {
            cavity_mode: SvfMode::default(),
            buf: [0.0; SHORT_COMB_LEN],
            write_idx: 0,
            tap1: 31,
            tap2: 61,
            lp_z1: 0.0,
            lp_damp: 0.45,
        }
    }
}

impl ShellResonator {
    #[inline(always)]
    fn reset(&mut self) {
        self.cavity_mode.reset();
        self.buf.fill(0.0);
        self.write_idx = 0;
        self.lp_z1 = 0.0;
    }

    /// Configures the wooden shell air-cavity resonance and 2-tap micro-reflection (`< 1.5 ms`).
    #[inline(always)]
    fn configure(&mut self, sample_rate: f32, cavity_hz: f32, cavity_t60: f32, wood_cutoff_hz: f32) {
        let sr = sample_rate.max(8_000.0);
        self.reset();
        self.cavity_mode.configure(sr, cavity_hz, cavity_t60, 0.85);

        // Dual bound: strictly < 1.5 ms AND < 120 samples at every sample rate (44.1..192 kHz)
        let max_allowed = (0.00148 * sr).floor().min(119.0).max(10.0);
        let raw_t1 = 0.00068 * sr;
        let raw_t2 = 0.00128 * sr;
        let scale = if raw_t2 > max_allowed {
            max_allowed / raw_t2
        } else {
            1.0
        };
        let t1 = (raw_t1 * scale).round().clamp(4.0, max_allowed * 0.65) as usize;
        let t2 = (raw_t2 * scale)
            .round()
            .clamp((t1 + 3) as f32, max_allowed) as usize;
        self.tap1 = t1;
        self.tap2 = t2;
        self.lp_damp = (-TAU * wood_cutoff_hz.clamp(400.0, sr * 0.4) / sr)
            .exp()
            .clamp(0.15, 0.88);
    }

    #[inline(always)]
    fn process(&mut self, x: f32) -> f32 {
        let d1 = self.buf[self.write_idx.wrapping_sub(self.tap1) & SHORT_COMB_MASK];
        let d2 = self.buf[self.write_idx.wrapping_sub(self.tap2) & SHORT_COMB_MASK];
        let refl = 0.60 * d1 - 0.44 * d2;

        let damped = flush_denormal((1.0 - self.lp_damp) * refl + self.lp_damp * self.lp_z1);
        self.lp_z1 = damped;

        self.buf[self.write_idx] = flush_denormal(x + 0.36 * damped);
        self.write_idx = (self.write_idx + 1) & SHORT_COMB_MASK;

        let cav = self.cavity_mode.process(x * 0.28 + damped * 0.22, 1.0);
        flush_denormal(cav * 0.60 + damped * 0.72)
    }

    #[cfg(test)]
    #[inline(always)]
    fn tap_delays(&self) -> (usize, usize) {
        (self.tap1, self.tap2)
    }
}

/// 2-operator FM melodic preset categories mapped from General MIDI program numbers (0–127).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GmFmPreset {
    #[default]
    EPiano,
    Organ,
    Pad,
    Brass,
    SynthBass,
}

impl GmFmPreset {
    /// Maps a standard 0-indexed General MIDI program number (`0..=127`) to a 2-operator FM archetype.
    #[inline(always)]
    pub const fn from_gm_program(program: u8) -> Self {
        match program {
            // Piano, Chromatic Percussion, Guitar/Plucked -> EPiano
            0..=15 | 24..=31 | 104..=111 => GmFmPreset::EPiano,
            // Organ & Accordion -> Organ
            16..=23 => GmFmPreset::Organ,
            // Bass -> SynthBass
            32..=39 => GmFmPreset::SynthBass,
            // Brass, Reed, Pipe, Synth Lead -> Brass
            56..=87 => GmFmPreset::Brass,
            // Strings, Ensemble, Synth Pad, Synth Effects -> Pad
            _ => GmFmPreset::Pad,
        }
    }

    #[inline(always)]
    pub const fn label(self) -> &'static str {
        match self {
            GmFmPreset::EPiano => "E-Piano",
            GmFmPreset::Organ => "Organ",
            GmFmPreset::Pad => "Pad",
            GmFmPreset::Brass => "Brass",
            GmFmPreset::SynthBass => "Synth Bass",
        }
    }
}

/// Active synthesis mode for a single [`PolyVoice`] slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PolyVoiceMode {
    #[default]
    Inactive,
    /// 6-mode Bessel circular membrane + tension modulation + wooden shell cavity (Kick & Toms).
    KickOrTom,
    /// 6-mode Bessel membrane + membrane-coupled snare wires & metallic comb + wooden shell (or Hand Clap).
    SnareOrClap,
    /// 4-line Hadamard dispersive non-linear bronze plate waveguide + 6 high-Q bronze modes (Hats & Cymbals).
    HatOrCymbal,
    /// 2-operator microtonal FM voice for GM melodic backing.
    GmFmMelodic(GmFmPreset),
}

/// Pre-allocated polyphonic voice capable of rendering any physically modeled drum, bronze cymbal,
/// or 2-operator FM melodic note with zero audio-thread allocations.
#[derive(Clone, Debug)]
pub struct PolyVoice {
    pub mode: PolyVoiceMode,
    /// Identifier for voice stealing and hi-hat choke groups (`gm_drum_note` for drums, `200 + channel` for GM).
    pub voice_tag: u8,
    pub is_open_hat: bool,
    pub age_samples: u32,
    pub pan: f32,
    pub level: f32,
    sample_rate: f32,

    // --- 6-Mode ZDF SVF Resonator Bank (Circular Membrane Bessel Modes / Bronze Plate Modes) ---
    modes: [SvfMode; NUM_MODES],
    /// Excitation weight per mode (shaped by `strike_position` for drums or `bell_level` for cymbals).
    mode_exc_weights: [f32; NUM_MODES],
    /// Excitatory impact pulse generator (unit-area raised-cosine beater/stick contour).
    exc_samples_elapsed: u32,
    exc_total_samples: u32,
    exc_area_norm: f32,
    exc_click_mix: f32,
    exc_lp_z1: f32,
    exc_lp_coeff: f32,

    // --- Non-Linear Membrane Tension Modulation ---
    head_energy_env: f32,
    head_energy_atk: f32,
    head_energy_rel: f32,
    tension_mod_depth: f32,

    // --- Cylindrical Wooden Shell Cavity & 2-Tap Micro-Reflection (< 1.5 ms) ---
    shell: ShellResonator,
    shell_mix: f32,

    // --- Physically Coupled Snare Wire Rattle & Short Metallic Comb Network (1.2 kHz - 8 kHz) ---
    wire_env: f32,
    wire_env_atk: f32,
    wire_env_rel: f32,
    wire_tension: f32,
    wire_comb_buf: [f32; SHORT_COMB_LEN],
    wire_comb_idx: usize,
    wire_comb_tap1: usize,
    wire_comb_tap2: usize,
    wire_lp_z1: f32,
    wire_lp_damp: f32,

    // --- Bronze Cymbal 4-Delay-Line Hadamard Dispersive Non-Linear Waveguide Mesh ---
    cymbal_lines: Box<[[f32; CYMBAL_DELAY_LEN]; CYMBAL_LINES]>,
    cym_write_idx: [usize; CYMBAL_LINES],
    cym_delays: [usize; CYMBAL_LINES],
    cym_ap_x1: [f32; CYMBAL_LINES],
    cym_ap_y1: [f32; CYMBAL_LINES],
    cym_lp_z1: [f32; CYMBAL_LINES],
    cym_dc_x1: [f32; CYMBAL_LINES],
    cym_dc_y1: [f32; CYMBAL_LINES],
    cym_ap_base: [f32; CYMBAL_LINES],
    cym_nonlin_shimmer: f32,
    cym_loop_gain: f32,
    cym_lp_damp: f32,
    cym_modal_mix: f32,
    cym_mesh_mix: f32,
    cym_wash_mix: f32,
    cym_edge_sizzle: f32,
    cym_mesh_env: f32,
    cym_bloom_env: f32,
    cym_bloom_inc: f32,
    cym_shimmer_hp_x1: f32,
    cym_shimmer_hp_y1: f32,
    cym_quiet_samples: u32,

    // --- FM Melodic & Auxiliary Oscillator / Envelope State ---
    pub phase1: f32,
    pub phase2: f32,
    pub base_inc: f32,
    pub sec_inc: f32,
    pub pitch_env: f32,
    pub pitch_decay: f32,
    pub pitch_mod_span_inc: f32,
    pub amp_env: f32,
    pub amp_attack_inc: f32,
    pub in_attack: bool,
    pub fast_env: f32,
    pub fast_decay: f32,
    pub slow_env: f32,
    pub slow_decay: f32,
    pub click_env: f32,
    pub click_decay: f32,
    pub body_mix: f32,
    pub noise_mix: f32,
    pub is_clap: bool,
    pub is_cymbal_bandpass: bool,
    pub fm_index_base: f32,
    pub fm_index_env_scale: f32,
    pub fm_feedback: f32,
    pub fm_last_mod: f32,

    // Per-voice pre-allocated biquad filters & PRNG
    pub filter1: BiquadFilter,
    pub filter2: BiquadFilter,
    pub rng_state: u32,
}

impl Default for PolyVoice {
    fn default() -> Self {
        Self::new(48_000.0, 0x6D2B_79F5)
    }
}

impl PolyVoice {
    pub fn new(sample_rate: f32, rng_seed: u32) -> Self {
        let sr = sample_rate.max(8_000.0);
        Self {
            mode: PolyVoiceMode::Inactive,
            voice_tag: 0,
            is_open_hat: false,
            age_samples: 0,
            pan: 0.5,
            level: 0.0,
            sample_rate: sr,
            modes: [SvfMode::default(); NUM_MODES],
            mode_exc_weights: [1.0, 0.5, 0.3, 0.3, 0.2, 0.15],
            exc_samples_elapsed: 0,
            exc_total_samples: 0,
            exc_area_norm: 1.0,
            exc_click_mix: 0.0,
            exc_lp_z1: 0.0,
            exc_lp_coeff: 0.3,
            head_energy_env: 0.0,
            head_energy_atk: 0.1,
            head_energy_rel: 0.001,
            tension_mod_depth: 0.0,
            shell: ShellResonator::default(),
            shell_mix: 0.0,
            wire_env: 0.0,
            wire_env_atk: 0.1,
            wire_env_rel: 0.001,
            wire_tension: 0.55,
            wire_comb_buf: [0.0; SHORT_COMB_LEN],
            wire_comb_idx: 0,
            wire_comb_tap1: 14,
            wire_comb_tap2: 27,
            wire_lp_z1: 0.0,
            wire_lp_damp: 0.35,
            cymbal_lines: alloc_cymbal_lines(),
            cym_write_idx: [0; CYMBAL_LINES],
            cym_delays: [149, 211, 293, 389],
            cym_ap_x1: [0.0; CYMBAL_LINES],
            cym_ap_y1: [0.0; CYMBAL_LINES],
            cym_lp_z1: [0.0; CYMBAL_LINES],
            cym_dc_x1: [0.0; CYMBAL_LINES],
            cym_dc_y1: [0.0; CYMBAL_LINES],
            cym_ap_base: [-0.48, 0.28, -0.25, 0.44],
            cym_nonlin_shimmer: 0.35,
            cym_loop_gain: 0.96,
            cym_lp_damp: 0.25,
            cym_modal_mix: 0.25,
            cym_mesh_mix: 0.72,
            cym_wash_mix: 0.55,
            cym_edge_sizzle: 0.0,
            cym_mesh_env: 0.0,
            cym_bloom_env: 0.0,
            cym_bloom_inc: 0.001,
            cym_shimmer_hp_x1: 0.0,
            cym_shimmer_hp_y1: 0.0,
            cym_quiet_samples: 0,
            phase1: 0.0,
            phase2: 0.0,
            base_inc: 0.0,
            sec_inc: 0.0,
            pitch_env: 0.0,
            pitch_decay: 0.99,
            pitch_mod_span_inc: 0.0,
            amp_env: 0.0,
            amp_attack_inc: 1.0,
            in_attack: false,
            fast_env: 0.0,
            fast_decay: 0.99,
            slow_env: 0.0,
            slow_decay: 0.999,
            click_env: 0.0,
            click_decay: 0.95,
            body_mix: 0.5,
            noise_mix: 0.5,
            is_clap: false,
            is_cymbal_bandpass: false,
            fm_index_base: 0.5,
            fm_index_env_scale: 1.5,
            fm_feedback: 0.0,
            fm_last_mod: 0.0,
            filter1: BiquadFilter::passthrough(),
            filter2: BiquadFilter::passthrough(),
            rng_state: if rng_seed == 0 { 0x6D2B_79F5 } else { rng_seed },
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.mode != PolyVoiceMode::Inactive
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.mode = PolyVoiceMode::Inactive;
        self.is_open_hat = false;
        self.amp_env = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.click_env = 0.0;
        self.pitch_env = 0.0;
        self.head_energy_env = 0.0;
        self.wire_env = 0.0;
        self.fm_last_mod = 0.0;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = 0;
        self.exc_lp_z1 = 0.0;
        self.cym_quiet_samples = 0;
        for m in &mut self.modes {
            m.reset();
        }
        self.shell.reset();
        self.wire_comb_buf.fill(0.0);
        self.wire_comb_idx = 0;
        self.wire_lp_z1 = 0.0;
        for line in self.cymbal_lines.iter_mut() {
            line.fill(0.0);
        }
        self.cym_write_idx = [0; CYMBAL_LINES];
        self.cym_ap_x1 = [0.0; CYMBAL_LINES];
        self.cym_ap_y1 = [0.0; CYMBAL_LINES];
        self.cym_lp_z1 = [0.0; CYMBAL_LINES];
        self.cym_dc_x1 = [0.0; CYMBAL_LINES];
        self.cym_dc_y1 = [0.0; CYMBAL_LINES];
        self.cym_mesh_env = 0.0;
        self.cym_bloom_env = 0.0;
        self.cym_shimmer_hp_x1 = 0.0;
        self.cym_shimmer_hp_y1 = 0.0;
        self.filter1.reset();
        self.filter2.reset();
    }

    /// Dynamically chokes an active open hi-hat by clamping loop damping inside the bronze
    /// Hadamard waveguide network and increasing modal damping across all 6 bronze modes.
    #[inline(always)]
    pub fn choke(&mut self, sample_rate: f32) {
        if self.is_active() {
            let sr = sample_rate.max(8_000.0);
            let fast = (-1.0 / (0.008 * sr)).exp();
            self.fast_decay = self.fast_decay.min(fast);
            self.slow_decay = self.slow_decay.min(fast);
            self.is_open_hat = false;
            // Physical bronze plate clamping: strongly damp the 4 waveguide loops + 6 bronze modes
            if self.mode == PolyVoiceMode::HatOrCymbal {
                self.cym_loop_gain = self.cym_loop_gain.min(0.68);
                self.cym_lp_damp = self.cym_lp_damp.max(0.62);
                self.cym_edge_sizzle = 0.0;
                self.cym_wash_mix *= 0.25;
                for m in &mut self.modes {
                    m.damp_choke(0.22);
                }
            }
        }
    }

    /// Fast, lock-free xorshift32 PRNG producing white noise in `[-1.0, 1.0]`.
    #[inline(always)]
    fn next_white_noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        if x == 0 {
            x = 0x6D2B_79F5;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        ((x as i32) as f32) * (1.0 / 2_147_483_648.0)
    }

    /// Computes circular membrane excitation weights across the 6 Bessel modes for `strike_pos`
    /// (`0.0` = dead center punch favoring axisymmetric `1.00, 2.29`; `1.0` = edge ring favoring
    /// non-axisymmetric `1.59, 2.13, 2.65, 2.91`).
    #[inline(always)]
    fn compute_membrane_strike_weights(strike_pos: f32) -> [f32; NUM_MODES] {
        let p = strike_pos.clamp(0.0, 1.0);
        [
            1.00 - 0.34 * p,                // (0,1) 1.00: axisymmetric fundamental (strongest at center)
            0.12 + 0.86 * p,                // (1,1) 1.59: primary dipole edge mode
            0.08 + 0.72 * p,                // (2,1) 2.13: quadrupole edge mode
            0.60 * (1.0 - 0.70 * p) + 0.14 * p, // (0,2) 2.29: second axisymmetric center punch mode
            0.05 + 0.56 * p,                // (3,1) 2.65: hexapole rim overtone
            0.10 + 0.44 * p,                // (1,2) 2.91: upper mylar overtone ring
        ]
    }

    /// Configures the 6-mode Bessel circular membrane resonator bank, unit-area beater/stick impulse,
    /// non-linear tension modulation follower, and wooden shell cavity resonator.
    #[inline(always)]
    fn configure_circular_membrane(
        &mut self,
        base_freq_hz: f32,
        t60_fundamental: f32,
        overtones_ring: f32,
        strike_pos: f32,
        tension_depth: f32,
        tension_tau_sec: f32,
        pulse_ms: f32,
        beater_cutoff_hz: f32,
        click_mix: f32,
        shell_cavity_ratio: f32,
        shell_mix: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let vel = velocity.clamp(0.05, 1.0);
        let ring = overtones_ring.clamp(0.0, 1.0);
        let weights = Self::compute_membrane_strike_weights(strike_pos);
        self.mode_exc_weights = weights;

        // Progressive higher-mode decay exponent: higher Bessel modes decay progressively faster
        // than the (0,1) fundamental so metallic/mylar overtones bloom on attack and settle into body.
        let decay_exp = 2.6 - 1.5 * ring;
        let base_f = base_freq_hz.clamp(25.0, sr * 0.14);

        for i in 0..NUM_MODES {
            let ratio = MEMBRANE_BESSEL_RATIOS[i];
            let mode_f = (base_f * ratio).clamp(25.0, sr * 0.42);
            let mode_t60 = (t60_fundamental * ratio.powf(-decay_exp)).clamp(0.012, 2.5);
            // Relative modal balance: fundamental strong, higher modes naturally taper
            let modal_bal = match i {
                0 => 1.00,
                1 => 0.62,
                2 => 0.44,
                3 => 0.48,
                4 => 0.34,
                _ => 0.28,
            };
            self.modes[i].configure(sr, mode_f, mode_t60, modal_bal);
        }

        // Smooth unit-area raised-cosine beater/stick contact impulse
        let pulse_samples = ((pulse_ms * 0.001) * sr).round().clamp(4.0, 256.0) as u32;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = pulse_samples;
        // Integral of 0.5 * (1 - cos(2*pi*u)) over N samples is 0.5 * N => normalize by 2 / N
        self.exc_area_norm = (2.0 / (pulse_samples as f32)) * (0.45 + 0.55 * vel);
        self.exc_click_mix = click_mix.clamp(0.0, 1.0);
        self.exc_lp_z1 = 0.0;
        self.exc_lp_coeff = (-TAU * beater_cutoff_hz.clamp(400.0, sr * 0.42) / sr)
            .exp()
            .clamp(0.05, 0.94);

        // Non-linear membrane tension modulation (tracks instantaneous head energy, decays 30-80 ms)
        let tau = tension_tau_sec.clamp(0.015, 0.12);
        self.head_energy_atk = 1.0 - (-1.0 / (0.0012 * sr)).exp();
        self.head_energy_rel = 1.0 - (-1.0 / (tau * sr)).exp();
        // Seed initial impact tension proportional to velocity^2 for instant physical stretch on hard hits
        self.head_energy_env = (vel * vel * 0.75).clamp(0.0, 1.0);
        self.tension_mod_depth = tension_depth.clamp(0.0, 2.5);

        // Wooden shell cylindrical air-cavity mode + 2-tap micro-reflection (< 1.5 ms)
        let shell_hz = (base_f * shell_cavity_ratio).clamp(40.0, sr * 0.35);
        let shell_t60 = (t60_fundamental * 0.65).clamp(0.03, 0.80);
        self.shell.configure(sr, shell_hz, shell_t60, 2_800.0);
        self.shell_mix = shell_mix.clamp(0.0, 1.0);
    }

    /// Generates the next sample of the unit-area beater/stick contact impulse + transient attack click.
    ///
    /// Keeps the deterministic raised-cosine contact pulse cleanly separated from unnormalized
    /// white noise so consecutive hits have rock-solid modal consistency unless explicit
    /// `hit_variation` humanization is dialed in.
    #[inline(always)]
    fn next_drum_excitation(&mut self) -> (f32, f32) {
        if self.exc_samples_elapsed < self.exc_total_samples {
            let idx = self.exc_samples_elapsed;
            self.exc_samples_elapsed += 1;
            let u = (idx as f32 + 0.5) / (self.exc_total_samples.max(1) as f32);
            // Smooth unit-area raised-cosine contact pulse (felt beater or hickory tip)
            let pulse = 0.5 * (1.0 - (TAU * u).cos()) * self.exc_area_norm;
            let noise = self.next_white_noise();
            let raw_click = noise * (1.0 - u) * self.exc_click_mix * 0.42;
            let lp_click = flush_denormal(
                (1.0 - self.exc_lp_coeff) * raw_click + self.exc_lp_coeff * self.exc_lp_z1,
            );
            self.exc_lp_z1 = lp_click;
            // Deterministic contact pulse drives modal resonators cleanly; tiny area-normalized
            // micro-texture avoids random +/-6 dB modal cancellation between consecutive hits.
            let modal_exc = pulse + (lp_click * 0.12 * self.exc_area_norm);
            (modal_exc, lp_click)
        } else {
            (0.0, 0.0)
        }
    }

    /// Triggers an acoustic physical-modeling Kick or Tom (GM Notes 35, 36, 41–50).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_kick_or_tom(
        &mut self,
        gm_note: u8,
        base_freq_hz: f32,
        start_freq_hz: f32,
        pitch_tau_sec: f32,
        decay_tau_sec: f32,
        head_tension_mod: f32,
        strike_position: f32,
        overtones_ring: f32,
        shell_wood_cavity: f32,
        click_level: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let vel = velocity.clamp(0.05, 1.0);
        let is_kick = gm_note == 35 || gm_note == 36;

        self.clear();
        self.mode = PolyVoiceMode::KickOrTom;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * if is_kick { 0.85 } else { 0.72 };

        // Map user pitch sweep ratio + head_tension_mod slider into modal frequency stretch depth
        let sweep_ratio = (start_freq_hz.max(base_freq_hz) / base_freq_hz.max(25.0) - 1.0).clamp(0.0, 3.0);
        let tension_depth = sweep_ratio * head_tension_mod.clamp(0.0, 1.0);
        // Kick uses a broader felt beater pulse (~2.2 ms); toms use a crisper stick tip (~0.85 ms)
        let pulse_ms = if is_kick {
            2.4 - 1.1 * click_level.clamp(0.0, 1.0)
        } else {
            1.15 - 0.55 * click_level.clamp(0.0, 1.0)
        };
        let beater_cutoff = if is_kick {
            1_400.0 + 3_200.0 * click_level.clamp(0.0, 1.0)
        } else {
            2_200.0 + 4_500.0 * click_level.clamp(0.0, 1.0)
        };
        let shell_ratio = if is_kick { 1.38 } else { 1.48 };
        // Scale T60 cleanly from decay_tau_sec (approx 2.6 * tau_sec = full audible ring)
        let t60_fund = (decay_tau_sec * 2.4).clamp(0.08, 2.2);

        self.configure_circular_membrane(
            base_freq_hz,
            t60_fund,
            overtones_ring,
            strike_position,
            tension_depth,
            pitch_tau_sec.clamp(0.020, 0.085),
            pulse_ms,
            beater_cutoff,
            click_level,
            shell_ratio,
            shell_wood_cavity,
            vel,
            sr,
        );

        // Beater/stick skin slap transient filter
        self.filter1 = BiquadFilter::bandpass_peak(sr, if is_kick { 2_400.0 } else { 3_200.0 }, 1.2);
        self.click_env = 1.0;
        self.click_decay = (-1.0 / (0.005 * sr)).exp();
        self.fast_env = 1.0;
        self.fast_decay = (-1.0 / (decay_tau_sec.max(0.04) * sr)).exp();
    }

    /// Triggers an acoustic physical-modeling Snare, Side Stick, Hand Clap, or Cowbell (GM Notes 37, 38, 39, 40, 56).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_snare_or_clap(
        &mut self,
        gm_note: u8,
        mode1_hz: f32,
        mode2_hz: f32,
        body_tau_sec: f32,
        noise_tau_sec: f32,
        noise_bp_hz: f32,
        noise_bp_q: f32,
        body_mix: f32,
        noise_mix: f32,
        head_tension_mod: f32,
        strike_position: f32,
        overtones_ring: f32,
        shell_wood_cavity: f32,
        wire_tension: f32,
        is_clap: bool,
        clap_spacing_ms: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.clear();
        self.mode = PolyVoiceMode::SnareOrClap;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * 0.78;
        self.body_mix = body_mix;
        self.noise_mix = noise_mix;
        self.is_clap = is_clap;
        self.wire_tension = wire_tension.clamp(0.0, 1.0);

        if is_clap {
            // Multi-burst acoustic hand clap (note 39) with short room comb
            self.pitch_mod_span_inc = (1000.0 / clap_spacing_ms.clamp(4.0, 50.0)) * inv_sr;
            self.fast_env = 1.0;
            self.fast_decay = (-inv_sr / body_tau_sec.max(0.012)).exp();
            self.slow_env = 1.0;
            self.slow_decay = (-inv_sr / noise_tau_sec.max(0.030)).exp();
            self.filter1 = BiquadFilter::bandpass_peak(sr, noise_bp_hz.clamp(800.0, 4000.0), noise_bp_q);
            self.filter2 = BiquadFilter::highpass(sr, 650.0, 0.707);
            self.shell.configure(sr, 420.0, 0.06, 3_600.0);
            self.shell_mix = 0.25;
            return;
        }

        // 6-mode circular membrane Bessel bank for Snare (38/40), Side Stick (37), or tuned Aux (56)
        let t60_fund = (body_tau_sec * 3.2).clamp(0.05, 1.2);
        let tension_depth = if gm_note == 56 {
            0.02
        } else {
            0.55 * head_tension_mod.clamp(0.0, 1.0)
        };
        let pulse_ms = if gm_note == 37 || gm_note == 56 { 0.45 } else { 0.65 };

        self.configure_circular_membrane(
            mode1_hz,
            t60_fund,
            overtones_ring,
            strike_position,
            tension_depth,
            0.035,
            pulse_ms,
            6_200.0,
            0.75,
            1.75,
            shell_wood_cavity,
            vel,
            sr,
        );

        // If user explicitly tuned mode2_freq_hz (e.g. cowbell 56 or snare mode2), honour it on Mode 1
        if gm_note == 56 {
            let f1 = mode1_hz.clamp(150.0, sr * 0.40);
            let f2 = mode2_hz.clamp(180.0, sr * 0.40);
            self.modes[0].configure(sr, f1, t60_fund, 1.0);
            self.modes[1].configure(sr, f2, t60_fund * 0.92, 0.95);
            for i in 2..NUM_MODES {
                let mf = (f2 * (1.35 + 0.35 * (i as f32))).clamp(200.0, sr * 0.42);
                self.modes[i].configure(sr, mf, t60_fund * 0.25, 0.14);
            }
            self.mode_exc_weights = [1.0, 0.95, 0.18, 0.12, 0.08, 0.05];
        } else if (mode2_hz - mode1_hz * 1.59).abs() > 25.0 {
            // Subtle blend between exact 1.59 Bessel ratio and user's custom mode2 slider
            let blended_m2 = 0.65 * (mode1_hz * MEMBRANE_BESSEL_RATIOS[1]) + 0.35 * mode2_hz;
            let decay_exp = 2.6 - 1.5 * overtones_ring.clamp(0.0, 1.0);
            let m2_t60 = (t60_fund * MEMBRANE_BESSEL_RATIOS[1].powf(-decay_exp)).clamp(0.012, 1.5);
            self.modes[1].configure(sr, blended_m2, m2_t60, 0.65);
        }

        // Membrane-coupled snare wire mechanical envelope (tracks head modes 0 & 1)
        let wire_tau = (noise_tau_sec * (1.25 - 0.55 * self.wire_tension)).clamp(0.025, 0.65);
        self.wire_env = 0.0;
        self.wire_env_atk = 1.0 - (-inv_sr / 0.00035).exp();
        self.wire_env_rel = 1.0 - (-inv_sr / wire_tau).exp();

        // Short metallic wire-strand comb taps (~0.29 ms and ~0.53 ms -> 1.8 kHz - 6.5 kHz metallic comb)
        let max_w = (0.0014 * sr).floor().min(119.0).max(8.0);
        self.wire_comb_tap1 = (0.00029 * sr).round().clamp(4.0, max_w * 0.55) as usize;
        self.wire_comb_tap2 = (0.00053 * sr)
            .round()
            .clamp((self.wire_comb_tap1 + 3) as f32, max_w) as usize;
        self.wire_comb_buf.fill(0.0);
        self.wire_comb_idx = 0;

        // Snare wire 1.2 kHz - 8 kHz bandpass / highpass / steel-coil lowpass network
        let wire_bp = noise_bp_hz.clamp(1_200.0, (sr * 0.40).min(8_000.0));
        self.filter1 = BiquadFilter::bandpass_peak(sr, wire_bp, noise_bp_q.clamp(0.6, 4.0));
        self.filter2 = BiquadFilter::highpass(sr, 1_200.0, 0.75);
        self.wire_lp_damp = (-TAU * (8_000.0_f32.min(sr * 0.42)) / sr)
            .exp()
            .clamp(0.08, 0.85);
        self.wire_lp_z1 = 0.0;

        // Track voice stealing energy estimate
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / body_tau_sec.max(0.02)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / noise_tau_sec.max(0.03)).exp();
    }

    /// Triggers a Cast B20 Bronze Cymbal or Hi-Hat (GM Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59)
    /// using the 4-delay-line Hadamard dispersive non-linear waveguide mesh + 6 high-Q bronze modes.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_hat_or_cymbal(
        &mut self,
        gm_note: u8,
        bronze_size_pitch: f32,
        modal_spread: f32,
        plate_dispersion: f32,
        nonlin_shimmer: f32,
        bell_level: f32,
        stick_hardness: f32,
        is_cymbal_bandpass: bool,
        is_open_hat: bool,
        fast_tau_sec: f32,
        slow_tau_sec: f32,
        cutoff_hz: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.clear();
        self.mode = PolyVoiceMode::HatOrCymbal;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = is_open_hat;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * if is_cymbal_bandpass { 0.70 } else { 0.74 };
        self.is_cymbal_bandpass = is_cymbal_bandpass;

        let size_pitch = bronze_size_pitch.clamp(0.5, 2.0);
        let spread = modal_spread.clamp(0.5, 2.0);
        let disp = plate_dispersion.clamp(0.0, 1.0);
        let shimmer = nonlin_shimmer.clamp(0.0, 1.0);
        let bell = bell_level.clamp(0.0, 2.0);
        let hardness = stick_hardness.clamp(0.0, 1.0);

        // 1. Mutually prime incommensurate waveguide delay lengths scaled by cymbal archetype and `bronze_size_pitch`
        let base_primes_48k: [f32; CYMBAL_LINES] = match gm_note {
            42 | 44 | 46 => [107.0, 163.0, 239.0, 337.0], // 14" thin B20 hi-hat pair
            55 => [97.0, 149.0, 223.0, 313.0],            // 10" thin splash
            52 => [139.0, 227.0, 317.0, 443.0],           // 18" thin trashy china
            53 => [113.0, 173.0, 251.0, 349.0],           // Ride bell cup zone
            51 | 59 => [157.0, 233.0, 331.0, 449.0],      // 20"-22" thin-medium ride
            _ => [149.0, 229.0, 337.0, 461.0],            // 18" thin flexible crash
        };

        let sr_scale = (sr / 48_000.0) / size_pitch;
        let max_len = (CYMBAL_DELAY_LEN - 6) as f32;
        let raw_max = base_primes_48k[CYMBAL_LINES - 1] * sr_scale;
        let clamp_scale = if raw_max > max_len {
            max_len / raw_max
        } else {
            1.0
        };

        let mut prev = 13usize;
        let mut delay_sum = 0.0_f32;
        for i in 0..CYMBAL_LINES {
            let mut d = (base_primes_48k[i] * sr_scale * clamp_scale)
                .round()
                .clamp(17.0, max_len) as usize;
            if d <= prev {
                d = (prev + 6).min(CYMBAL_DELAY_LEN - 4);
            }
            self.cym_delays[i] = d;
            prev = d;
            delay_sum += d as f32;
        }
        let avg_delay_sec = (delay_sum * 0.25) * inv_sr;

        // 2. Thin, flexible cast B20 bronze plate voicing differentiated per cymbal/hat role:
        //    - Crashes & Hats favor airy non-tonal shimmer wash (`wash_mix` + `mesh_mix`) with
        //      brief stick-contact modal pings (`modal_mix` kept low so they never sound like an anvil).
        //    - Ride Bow balances crisp stick ping over a smooth thin wash.
        //    - Ride Bell (53) emphasizes the thick singing bell cup modes.
        let (shimmer_mult, lp_damp, modal_mix, mesh_mix, wash_mix, bloom_ms, edge_sizzle) = match gm_note {
            42 => (0.95, 0.16, 0.24, 0.75, 0.95, 2.0, 0.10),   // Closed hat: crisp thin chick (loop_gain handles tight 45ms decay)
            44 => (0.95, 0.18, 0.22, 0.75, 0.95, 3.0, 0.28),   // Pedal hat: clamping thin chick + brief sizzle
            46 => (1.15, 0.12, 0.20, 0.72, 0.92, 12.0, 0.36),  // Open hat: airy thin wash + edge sizzle
            53 => (0.45, 0.15, 0.74, 0.48, 0.30, 8.0, 0.0),    // Ride bell: singing bronze bell ping
            51 | 59 => (0.75, 0.15, 0.34, 0.64, 0.62, 12.0, 0.0), // Ride bow: crisp stick + thin wash
            52 => (1.40, 0.12, 0.20, 0.76, 0.98, 10.0, 0.22),  // China: fast trashy thin flex wash
            55 => (1.25, 0.13, 0.20, 0.74, 0.92, 9.0, 0.0),    // Splash: thin fast bright shimmer
            _ => (1.30, 0.13, 0.16, 0.74, 0.95, 22.0, 0.0),    // Crash 1/2 (49, 57): flexible ~22ms shimmer bloom
        };

        // Stagger per-line base all-pass dispersion polarities across the 4 Hadamard loops to
        // destroy periodic comb tones and simulate a thin flexible plate's chaotic dispersion.
        let disp_scale = 0.35 + 0.65 * disp;
        self.cym_ap_base = [
            -0.58 * disp_scale,
            0.34 * disp_scale,
            -0.31 * disp_scale,
            0.54 * disp_scale,
        ];
        self.cym_nonlin_shimmer =
            (shimmer * 0.62 * shimmer_mult * (0.45 + 0.55 * vel)).clamp(0.0, 0.72);
        self.cym_lp_damp = lp_damp;
        self.cym_modal_mix = modal_mix;
        self.cym_mesh_mix = mesh_mix;
        self.cym_wash_mix = wash_mix * (0.35 + 0.65 * shimmer);
        self.cym_edge_sizzle = edge_sizzle;
        self.cym_mesh_env = 0.0;
        self.cym_bloom_env = if gm_note == 42 || gm_note == 44 { 0.85 } else { 0.20 };
        self.cym_bloom_inc = 1.0 / ((bloom_ms * 0.001) * sr).max(1.0);
        self.cym_shimmer_hp_x1 = 0.0;
        self.cym_shimmer_hp_y1 = 0.0;

        // Per-loop feedback gain calibrated from target decay duration `slow_tau_sec`
        let decay_t60 = (slow_tau_sec * 2.2).clamp(0.03, 5.5);
        let raw_loop_g = 10.0_f32.powf((-3.0 * avg_delay_sec / decay_t60).max(-2.0));
        self.cym_loop_gain = raw_loop_g.clamp(0.68, 0.9945);

        // 3. Configure 6 High-Q Cast B20 Bronze Modal Resonators for stick ping & bell
        //    Thin cymbals sit much higher in pitch with brief stick-ping decay (except Ride Bell 53).
        let base_bronze_hz = match gm_note {
            42 | 44 | 46 => 1_150.0, // Thin airy 14" hi-hat shimmer zone
            53 => 1_050.0,           // Ride bell singing cup zone
            51 | 59 => 840.0,        // Ride bow stick ping
            55 => 980.0,             // Thin 10" splash
            52 => 720.0,             // Thin china
            _ => 790.0,              // Thin 18" crash
        } * size_pitch;

        let bell_boost = if gm_note == 53 {
            1.35 + 1.20 * bell
        } else if gm_note == 51 || gm_note == 59 {
            0.35 + 0.75 * bell
        } else {
            0.18 + 0.55 * bell
        };

        let (low_mode_max_t60, high_mode_max_t60) = match gm_note {
            53 => (0.42, 0.68),      // Ride bell rings clearly
            51 | 59 => (0.14, 0.26), // Ride bow defined stick ting
            42 | 44 => (0.025, 0.045),
            46 => (0.06, 0.11),      // Open hat: thin shimmer, not tonal bell
            _ => (0.055, 0.115),     // Crash / China / Splash: brief stick ping into flexible wash
        };

        for i in 0..NUM_MODES {
            let ratio = 1.0 + (BRONZE_MODAL_RATIOS[i] - 1.0) * (0.65 + 0.35 * spread);
            let mode_f = (base_bronze_hz * ratio).clamp(320.0, sr * 0.43);
            let mode_t60 = if i >= 3 {
                (decay_t60 * (0.20 + 0.15 * disp)).clamp(0.02, high_mode_max_t60)
            } else {
                (decay_t60 * 0.12).clamp(0.015, low_mode_max_t60)
            };
            let mode_gain = if i >= 3 {
                0.30 * bell_boost
            } else {
                0.22
            };
            self.modes[i].configure(sr, mode_f, mode_t60, mode_gain);
            self.mode_exc_weights[i] = if i >= 3 {
                0.65 + 0.35 * hardness
            } else {
                0.70
            };
        }

        // 4. Consistent stick impact impulse filtered by `stick_hardness` (wood tip vs bright nylon tip)
        let stick_ms = if gm_note == 44 {
            2.4
        } else if gm_note == 49 || gm_note == 57 {
            1.20 - 0.55 * hardness
        } else {
            0.75 - 0.35 * hardness
        };
        let pulse_samples = ((stick_ms * 0.001) * sr).round().clamp(4.0, 220.0) as u32;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = pulse_samples;
        self.exc_area_norm = (2.0 / (pulse_samples as f32)) * (0.60 + 0.40 * vel);
        self.exc_click_mix = 0.65 + 0.35 * hardness;
        self.exc_lp_z1 = 0.0;
        let stick_cutoff = (3_800.0 + 13_000.0 * hardness).min(sr * 0.44);
        self.exc_lp_coeff = (-TAU * stick_cutoff / sr).exp().clamp(0.03, 0.88);

        // 5. Thin, airy B20 bronze spectral shaping (removes heavy low-mid brass/anvil mud!)
        if is_cymbal_bandpass {
            let bp_cut = cutoff_hz.clamp(1_400.0, sr * 0.44);
            let hp_cut = match gm_note {
                53 => (bp_cut * 0.32).clamp(950.0, sr * 0.38),   // Ride bell retains cup weight
                51 | 59 => (bp_cut * 0.45).clamp(1_350.0, sr * 0.40), // Ride bow
                _ => (bp_cut * 0.62).clamp(1_850.0, sr * 0.42),  // Thin crash/splash/china
            };
            self.filter1 = BiquadFilter::highpass(sr, hp_cut, 0.72);
            self.filter2 = BiquadFilter::peaking_eq(
                sr,
                (bp_cut * 2.1).clamp(4_500.0, sr * 0.43),
                0.75,
                3.5 + 3.0 * disp,
            );
        } else {
            // Hi-Hats: thin airy bronze highpass removing anvil low-mids + bright top-end shimmer
            let hp_cut = (cutoff_hz * 0.56).clamp(2_600.0, sr * 0.42);
            self.filter1 = BiquadFilter::highpass(sr, hp_cut, 0.76);
            self.filter2 = BiquadFilter::peaking_eq(
                sr,
                (cutoff_hz * 1.15).clamp(5_000.0, sr * 0.43),
                0.80,
                4.0 + 2.5 * hardness,
            );
        }

        // Dual-stage auxiliary envelope trackers (for smooth voice stealing & fast attack click)
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / fast_tau_sec.max(0.008)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / slow_tau_sec.max(0.025)).exp();
        self.cym_quiet_samples = 0;
    }

    /// Configures this voice as a 2-operator microtonal FM voice for General MIDI melodic backing.
    #[inline(always)]
    pub fn trigger_gm_fm(
        &mut self,
        channel: u8,
        freq_hz: f64,
        velocity: f32,
        program: u8,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);
        let preset = GmFmPreset::from_gm_program(program);

        self.mode = PolyVoiceMode::GmFmMelodic(preset);
        self.sample_rate = sr;
        self.voice_tag = 200u8.wrapping_add(channel & 0x0F);
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = (0.32 + ((channel % 7) as f32) * 0.06).clamp(0.20, 0.80);
        self.level = vel * 0.30;

        let carrier_hz = (freq_hz.clamp(15.0, (sr as f64) * 0.45)) as f32;
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.fm_last_mod = 0.0;
        self.base_inc = carrier_hz * inv_sr;

        match preset {
            GmFmPreset::EPiano => {
                self.sec_inc = (carrier_hz * 1.0) * inv_sr;
                self.fm_index_base = 0.35;
                self.fm_index_env_scale = 2.1 * (0.5 + 0.5 * vel);
                self.fm_feedback = 0.12;
                self.in_attack = true;
                self.amp_env = 0.0;
                self.amp_attack_inc = 1.0 / (0.002 * sr).max(1.0);
                self.fast_env = 1.0;
                self.fast_decay = (-inv_sr / 0.18).exp();
                self.slow_env = 1.0;
                self.slow_decay = (-inv_sr / 1.65).exp();
                self.filter1 = BiquadFilter::lowpass(sr, 6_500.0, 0.707);
            }
            GmFmPreset::Organ => {
                self.sec_inc = (carrier_hz * 2.0).min(sr * 0.45) * inv_sr;
                self.fm_index_base = 0.85;
                self.fm_index_env_scale = 0.45;
                self.fm_feedback = 0.25;
                self.in_attack = true;
                self.amp_env = 0.0;
                self.amp_attack_inc = 1.0 / (0.004 * sr).max(1.0);
                self.fast_env = 1.0;
                self.fast_decay = (-inv_sr / 0.035).exp();
                self.slow_env = 1.0;
                self.slow_decay = (-inv_sr / 2.20).exp();
                self.filter1 = BiquadFilter::lowpass(sr, 5_200.0, 0.707);
            }
            GmFmPreset::Pad => {
                self.sec_inc = (carrier_hz * 1.003).min(sr * 0.45) * inv_sr;
                self.fm_index_base = 0.75;
                self.fm_index_env_scale = 0.60;
                self.fm_feedback = 0.30;
                self.in_attack = true;
                self.amp_env = 0.0;
                self.amp_attack_inc = 1.0 / (0.065 * sr).max(1.0);
                self.fast_env = 1.0;
                self.fast_decay = (-inv_sr / 0.90).exp();
                self.slow_env = 1.0;
                self.slow_decay = (-inv_sr / 2.60).exp();
                self.filter1 = BiquadFilter::lowpass(sr, 3_600.0, 0.80);
            }
            GmFmPreset::Brass => {
                self.sec_inc = carrier_hz * inv_sr;
                self.fm_index_base = 0.60;
                self.fm_index_env_scale = 2.6 * (0.4 + 0.6 * vel);
                self.fm_feedback = 0.35;
                self.in_attack = true;
                self.amp_env = 0.0;
                self.amp_attack_inc = 1.0 / (0.022 * sr).max(1.0);
                self.fast_env = 1.0;
                self.fast_decay = (-inv_sr / 0.45).exp();
                self.slow_env = 1.0;
                self.slow_decay = (-inv_sr / 1.50).exp();
                self.filter1 = BiquadFilter::lowpass(sr, 4_400.0, 0.95);
            }
            GmFmPreset::SynthBass => {
                self.sec_inc = (carrier_hz * 2.0).min(sr * 0.45) * inv_sr;
                self.fm_index_base = 0.40;
                self.fm_index_env_scale = 2.8 * (0.5 + 0.5 * vel);
                self.fm_feedback = 0.28;
                self.in_attack = true;
                self.amp_env = 0.0;
                self.amp_attack_inc = 1.0 / (0.0015 * sr).max(1.0);
                self.fast_env = 1.0;
                self.fast_decay = (-inv_sr / 0.095).exp();
                self.slow_env = 1.0;
                self.slow_decay = (-inv_sr / 0.85).exp();
                self.filter1 = BiquadFilter::lowpass(sr, 2_800.0, 1.05);
            }
        }
    }

    /// Triggers a 2-operator FM melodic voice with custom parameters from the Patch Editor.
    #[inline(always)]
    pub fn trigger_gm_fm_custom(
        &mut self,
        channel: u8,
        freq_hz: f64,
        velocity: f32,
        params: &FmVoiceParams,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.mode = PolyVoiceMode::GmFmMelodic(GmFmPreset::EPiano);
        self.sample_rate = sr;
        self.voice_tag = 200u8.wrapping_add(channel & 0x0F);
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = (0.32 + ((channel % 7) as f32) * 0.06).clamp(0.20, 0.80);
        self.level = vel * 0.30;

        let carrier_hz = (freq_hz.clamp(15.0, (sr as f64) * 0.45)) as f32;
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.fm_last_mod = 0.0;
        self.base_inc = carrier_hz * inv_sr;

        let mod_ratio = params.mod_ratio.clamp(0.1, 16.0);
        self.sec_inc = (carrier_hz * mod_ratio).min(sr * 0.45) * inv_sr;
        self.fm_index_base = params.mod_index * params.mod_adsr.sustain;
        self.fm_index_env_scale = params.mod_index * (1.0 - params.mod_adsr.sustain).max(0.0) * vel;
        self.fm_feedback = params.feedback.clamp(0.0, 1.5);
        self.in_attack = true;
        self.amp_env = 0.0;
        let att_sec = (params.carrier_adsr.attack_ms * 0.001).max(0.0005);
        self.amp_attack_inc = 1.0 / (att_sec * sr).max(1.0);
        self.fast_env = 1.0;
        let mod_decay_sec = (params.mod_adsr.decay_ms * 0.001).max(0.005);
        self.fast_decay = (-inv_sr / mod_decay_sec).exp();
        self.slow_env = 1.0;
        let car_decay_sec = (params.carrier_adsr.decay_ms * 0.001).max(0.01);
        self.slow_decay = (-inv_sr / car_decay_sec).exp();
        self.filter1 = BiquadFilter::lowpass(sr, 6_500.0, 0.707);
    }

    /// Steps one audio sample and returns stereo `(left, right)`.
    #[inline(always)]
    pub fn next_frame(&mut self) -> (f32, f32) {
        if self.mode == PolyVoiceMode::Inactive {
            return (0.0, 0.0);
        }

        self.age_samples = self.age_samples.wrapping_add(1);

        let raw = match self.mode {
            PolyVoiceMode::Inactive => return (0.0, 0.0),

            PolyVoiceMode::KickOrTom => {
                // 1. Unit-area felt beater / hickory stick impulse
                let (exc, click_raw) = self.next_drum_excitation();

                // 2. Non-linear membrane tension modulation: shift all 6 Bessel modal frequencies
                // upward proportionally to instantaneous drumhead energy (decaying over 30-80 ms)
                let tension_mult = 1.0 + self.tension_mod_depth * self.head_energy_env;

                // 3. Evaluate 6-mode circular membrane Bessel resonator bank
                let mut membrane_sum = 0.0_f32;
                let mut total_modal_e = 0.0_f32;
                for i in 0..NUM_MODES {
                    let y_i = self.modes[i].process(exc * self.mode_exc_weights[i], tension_mult);
                    membrane_sum += y_i;
                    total_modal_e += self.modes[i].energy();
                }

                // Update instantaneous membrane energy follower for tension stretching
                let inst_e = (membrane_sum * membrane_sum * 0.65).min(1.5);
                let coeff = if inst_e > self.head_energy_env {
                    self.head_energy_atk
                } else {
                    self.head_energy_rel
                };
                self.head_energy_env =
                    flush_denormal(self.head_energy_env + coeff * (inst_e - self.head_energy_env));

                // 4. Cylindrical wooden shell air-cavity mode + 2-tap micro-reflection (< 1.5 ms)
                let shell_out = self.shell.process(membrane_sum * 0.72 + exc * 0.35);

                // 5. Crisp beater/stick skin-slap transient
                let slap = if self.click_env > 1.0e-4 {
                    self.filter1.process(click_raw) * self.click_env * 0.75
                } else {
                    0.0
                };

                self.click_env = flush_denormal(self.click_env * self.click_decay);
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);

                if self.exc_samples_elapsed >= self.exc_total_samples
                    && total_modal_e < 1.0e-10
                    && self.click_env < 1.0e-5
                    && self.age_samples > 256
                {
                    self.clear();
                    return (0.0, 0.0);
                }

                let body = membrane_sum * (1.0 - 0.22 * self.shell_mix)
                    + shell_out * (0.58 * self.shell_mix)
                    + slap;
                tanh_pade(body * 1.25) * self.level
            }

            PolyVoiceMode::SnareOrClap => {
                if self.is_clap {
                    // Acoustic multi-burst hand clap (GM Note 39)
                    let white = self.next_white_noise();
                    let bp_noise = self.filter1.process(white);
                    let shaped = self.filter2.process(bp_noise + white * 0.22);
                    let burst_phase = (self.age_samples as f32) * self.pitch_mod_span_inc;
                    let env = if self.fast_env > 0.22 {
                        0.42 + 0.58 * (burst_phase * TAU).cos().abs()
                    } else {
                        self.slow_env
                    };
                    let room = self.shell.process(shaped * env);
                    self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                    self.slow_env = flush_denormal(self.slow_env * self.slow_decay);
                    if self.fast_env < 1.0e-5 && self.slow_env < 1.0e-5 {
                        self.clear();
                        return (0.0, 0.0);
                    }
                    tanh_pade((shaped * env * 0.85 + room * 0.35) * 1.35) * self.level
                } else {
                    // 1. Hickory drumstick impulse into 6-mode Bessel circular snare membrane
                    let (exc, click_raw) = self.next_drum_excitation();
                    let tension_mult = 1.0 + self.tension_mod_depth * self.head_energy_env;

                    let mut m0 = 0.0_f32;
                    let mut m1 = 0.0_f32;
                    let mut membrane_sum = 0.0_f32;
                    let mut total_modal_e = 0.0_f32;
                    for i in 0..NUM_MODES {
                        let y_i =
                            self.modes[i].process(exc * self.mode_exc_weights[i], tension_mult);
                        if i == 0 {
                            m0 = y_i;
                        } else if i == 1 {
                            m1 = y_i;
                        }
                        membrane_sum += y_i;
                        total_modal_e += self.modes[i].energy();
                    }

                    // Update non-linear head tension energy tracker
                    let inst_e = (membrane_sum * membrane_sum * 0.55).min(1.5);
                    let e_coeff = if inst_e > self.head_energy_env {
                        self.head_energy_atk
                    } else {
                        self.head_energy_rel
                    };
                    self.head_energy_env = flush_denormal(
                        self.head_energy_env + e_coeff * (inst_e - self.head_energy_env),
                    );

                    // 2. Wooden snare shell air-cavity mode + 2-tap micro-reflection (< 1.5 ms)
                    let shell_out = self.shell.process(membrane_sum * 0.70 + exc * 0.35);

                    // 3. Physically coupled snare wires driven directly by fundamental (1.00) & (1.59) modes
                    let rect_drive = m0.abs() + 0.78 * m1.abs();
                    let w_coeff = if rect_drive > self.wire_env {
                        self.wire_env_atk
                    } else {
                        self.wire_env_rel
                    };
                    self.wire_env =
                        flush_denormal(self.wire_env + w_coeff * (rect_drive - self.wire_env));

                    // Non-linear mechanical rattle threshold governed by `wire_tension`:
                    // Soft ghost notes buzz delicately; hard hits & rimshots snap and sputter
                    // synchronously with the decaying drumhead modes.
                    let thresh = 0.010 + 0.11 * self.wire_tension;
                    let white = self.next_white_noise();
                    let sputter = (m0 + 0.65 * m1).abs();
                    let rattle_drive = if self.wire_env > thresh {
                        let excess = self.wire_env - thresh;
                        let bounce = (sputter - thresh * 0.70).max(0.0);
                        (excess * 1.55 + bounce * 0.65 + 0.12 * self.wire_env).min(1.8)
                    } else {
                        // Delicate low-level wire shimmer on soft ghost notes
                        0.12 * self.wire_env
                    };
                    let wire_exc = rattle_drive * (0.72 * white + 0.28 * tanh_pade(white * 3.0));

                    // Short metallic wire-strand comb network + 1.2 kHz - 8 kHz bandpass shaping
                    let wd1 =
                        self.wire_comb_buf[self.wire_comb_idx.wrapping_sub(self.wire_comb_tap1)
                            & SHORT_COMB_MASK];
                    let wd2 =
                        self.wire_comb_buf[self.wire_comb_idx.wrapping_sub(self.wire_comb_tap2)
                            & SHORT_COMB_MASK];
                    let comb_sig = wire_exc + 0.36 * wd1 - 0.28 * wd2;
                    self.wire_comb_buf[self.wire_comb_idx] = flush_denormal(comb_sig);
                    self.wire_comb_idx = (self.wire_comb_idx + 1) & SHORT_COMB_MASK;

                    let wire_hp = self.filter2.process(comb_sig);
                    let wire_bp = self.filter1.process(wire_hp);
                    let wire_mix = wire_bp * 0.72 + wire_hp * 0.38;
                    let wire_out = flush_denormal(
                        (1.0 - self.wire_lp_damp) * wire_mix + self.wire_lp_damp * self.wire_lp_z1,
                    );
                    self.wire_lp_z1 = wire_out;

                    self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                    self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                    if self.exc_samples_elapsed >= self.exc_total_samples
                        && total_modal_e < 1.0e-10
                        && self.wire_env < 1.0e-5
                        && self.age_samples > 256
                    {
                        self.clear();
                        return (0.0, 0.0);
                    }

                    let head_and_shell = membrane_sum * (1.0 - 0.20 * self.shell_mix)
                        + shell_out * (0.56 * self.shell_mix)
                        + click_raw * 0.45;
                    let combined = head_and_shell * self.body_mix + wire_out * (self.noise_mix * 1.45);
                    tanh_pade(combined * 1.25) * self.level
                }
            }

            PolyVoiceMode::HatOrCymbal => {
                // 1. Consistent stick tip / shoulder impact excitation filtered by `stick_hardness`
                let (pulse, stick_noise) = self.next_drum_excitation();
                // Deterministic stick pulse excites the 6 bronze ping modes cleanly; crisp high-passed
                // impact noise seeds dense non-tonal bending waves inside the flexible bronze plate.
                let modal_exc = pulse * 0.90;
                let mesh_seed = pulse * 0.65 + stick_noise * 0.55;

                // 2. 6 High-Q Cast B20 Bronze Modal Resonators (brief stick ping / singing Ride Bell)
                let mut modal_sum = 0.0_f32;
                let mut modal_e = 0.0_f32;
                for i in 0..NUM_MODES {
                    let y_i = self.modes[i].process(modal_exc * self.mode_exc_weights[i], 1.0);
                    modal_sum += y_i;
                    modal_e += self.modes[i].energy();
                }

                // 3. 4-Delay-Line Hadamard Dispersive Non-Linear Thin Bronze Waveguide Network
                let mut w = [0.0_f32; CYMBAL_LINES];
                let one_minus_damp = 1.0 - self.cym_lp_damp;
                for i in 0..CYMBAL_LINES {
                    let r_idx = self.cym_write_idx[i].wrapping_sub(self.cym_delays[i])
                        & CYMBAL_DELAY_MASK;
                    let d_i = self.cymbal_lines[i][r_idx];

                    // Signal-modulated 1st-order all-pass dispersion filter with staggered per-line
                    // base dispersion: breaks periodic comb pitches into flexible thin-plate shimmer.
                    let a_eff = (self.cym_ap_base[i]
                        + self.cym_nonlin_shimmer * tanh_pade(d_i * 4.5))
                    .clamp(-0.86, 0.86);
                    let ap_y = flush_denormal(
                        a_eff * d_i + self.cym_ap_x1[i] - a_eff * self.cym_ap_y1[i],
                    );
                    self.cym_ap_x1[i] = flush_denormal(d_i);
                    self.cym_ap_y1[i] = ap_y;

                    // Visco-elastic plate damping (dynamically increased on hi-hat choke)
                    let lp_y = flush_denormal(
                        one_minus_damp * ap_y + self.cym_lp_damp * self.cym_lp_z1[i],
                    );
                    self.cym_lp_z1[i] = lp_y;

                    // Gentle in-loop DC blocker prevents offset accumulation under non-linear modulation
                    let dc_y = flush_denormal(lp_y - self.cym_dc_x1[i] + 0.992 * self.cym_dc_y1[i]);
                    self.cym_dc_x1[i] = flush_denormal(lp_y);
                    self.cym_dc_y1[i] = dc_y;

                    w[i] = dc_y;
                }

                // Orthogonal 4x4 Hadamard reflection matrix H_4 (unitary, norm = 1.0)
                let h = [
                    0.5 * (w[0] + w[1] + w[2] + w[3]),
                    0.5 * (w[0] - w[1] + w[2] - w[3]),
                    0.5 * (w[0] + w[1] - w[2] - w[3]),
                    0.5 * (w[0] - w[1] - w[2] + w[3]),
                ];

                // Track instantaneous plate energy & 15-25 ms flexible shimmer bloom envelope
                let mesh_inst = 0.25 * (w[0].abs() + w[1].abs() + w[2].abs() + w[3].abs());
                let env_coeff = if mesh_inst > self.cym_mesh_env { 0.08 } else { 0.0025 };
                self.cym_mesh_env =
                    flush_denormal(self.cym_mesh_env + env_coeff * (mesh_inst - self.cym_mesh_env));
                if self.cym_bloom_env < 1.0 {
                    self.cym_bloom_env = (self.cym_bloom_env + self.cym_bloom_inc).min(1.0);
                }

                // High-passed chaotic bronze wave-turbulence shimmer (4 kHz - 15 kHz) driven by
                // the vibrating plate's instantaneous energy (thin flexible cymbal wash, non-tonal!)
                let wn = self.next_white_noise();
                let hp_wn = flush_denormal(0.68 * (self.cym_shimmer_hp_y1 + wn - self.cym_shimmer_hp_x1));
                self.cym_shimmer_hp_x1 = wn;
                self.cym_shimmer_hp_y1 = hp_wn;

                // Subtle energy-coupled chaotic micro-scatter inside the mesh breaks up residual periodicity
                let plate_scatter = hp_wn * self.cym_mesh_env * self.cym_nonlin_shimmer * 0.22;

                // Inject stick impact + brief modal coupling + micro-scatter into Hadamard mesh
                const INJ_SIGNS: [f32; CYMBAL_LINES] = [0.5, -0.5, 0.5, -0.5];
                let chick_seed = if !self.is_cymbal_bandpass {
                    hp_wn * self.fast_env * 0.24
                } else {
                    0.0
                };
                let mesh_in = mesh_seed * 1.35 + modal_sum * 0.14 + plate_scatter + chick_seed;
                let mut mesh_peak = 0.0_f32;
                for i in 0..CYMBAL_LINES {
                    let fb = tanh_pade(h[i] * self.cym_loop_gain * 1.15) * (1.0 / 1.15);
                    let write_val = flush_denormal(fb + INJ_SIGNS[i] * mesh_in);
                    self.cymbal_lines[i][self.cym_write_idx[i]] = write_val;
                    self.cym_write_idx[i] = (self.cym_write_idx[i] + 1) & CYMBAL_DELAY_MASK;
                    mesh_peak = mesh_peak.max(write_val.abs());
                }

                let mesh_out = 0.5 * (w[0] - w[1] + w[2] - w[3]);

                // Thin, airy flexible bronze shimmer wash blooming over the first ~15-25 ms
                let shimmer_wash =
                    hp_wn * self.cym_mesh_env * self.cym_bloom_env * self.cym_wash_mix * 3.1
                        + chick_seed * 1.15;

                // Optional subtle edge-contact sizzle when hi-hat plates are partially open / pedal-clamped
                let edge_sizzle_sig = if self.cym_edge_sizzle > 1.0e-4 && mesh_peak > 0.012 {
                    let contact = (mesh_peak - 0.012).min(0.45);
                    hp_wn * contact * self.cym_edge_sizzle * 1.8
                } else {
                    0.0
                };

                let combined = modal_sum * self.cym_modal_mix
                    + mesh_out * self.cym_mesh_mix
                    + shimmer_wash
                    + edge_sizzle_sig;
                let hp_out = self.filter1.process(combined);
                let shaped = self.filter2.process(hp_out);

                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                // Auto-deactivate once both waveguide mesh and modal bank settle below audible floor
                if self.exc_samples_elapsed >= self.exc_total_samples
                    && mesh_peak < 2.0e-5
                    && self.cym_mesh_env < 2.0e-5
                    && modal_e < 1.0e-9
                {
                    self.cym_quiet_samples += 1;
                    if self.cym_quiet_samples > 512 {
                        self.clear();
                        return (0.0, 0.0);
                    }
                } else {
                    self.cym_quiet_samples = 0;
                }

                tanh_pade(shaped * 1.35) * self.level
            }

            PolyVoiceMode::GmFmMelodic(_preset) => {
                if self.in_attack {
                    self.amp_env += self.amp_attack_inc;
                    if self.amp_env >= 1.0 {
                        self.amp_env = 1.0;
                        self.in_attack = false;
                    }
                }

                // Step modulator (phase2) with self-feedback
                self.phase2 += self.sec_inc;
                if self.phase2 >= 1.0 {
                    self.phase2 -= 1.0;
                }
                let mod_angle = self.phase2 * TAU + self.fm_last_mod * self.fm_feedback;
                let mod_out = mod_angle.sin();
                self.fm_last_mod = flush_denormal(mod_out);

                // Dynamic modulation index shaped by fast envelope
                let mod_index = self.fm_index_base + self.fm_index_env_scale * self.fast_env;

                // Step carrier (phase1) phase-modulated by modulator
                self.phase1 += self.base_inc;
                if self.phase1 >= 1.0 {
                    self.phase1 -= 1.0;
                }
                let carrier_angle = self.phase1 * TAU + mod_out * mod_index;
                let carrier = carrier_angle.sin();

                let env = self.amp_env * (0.25 * self.fast_env + 0.75 * self.slow_env);
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                if !self.in_attack && self.slow_env < 1.0e-5 && self.fast_env < 1.0e-5 {
                    self.clear();
                    return (0.0, 0.0);
                }

                let shaped = self.filter1.process(carrier * env);
                shaped * self.level
            }
        };

        let left = raw * (1.0 - self.pan);
        let right = raw * self.pan;
        (left, right)
    }
}

/// Fixed-size 16-voice polyphonic pool for acoustic physical-modeling percussion and 2-operator FM voices.
pub struct PercussionAndGmPool {
    sample_rate: f32,
    voices: Box<[PolyVoice; MAX_POLY_VOICES]>,
    steal_cursor: usize,
    humanize_rng: u32,
    pub drum_params: DrumKitParams,
    pub fm_params: Option<FmVoiceParams>,
}

impl PercussionAndGmPool {
    /// Pre-allocates all 16 polyphonic voices directly on the heap at stream initialization.
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut voice_vec = Vec::with_capacity(MAX_POLY_VOICES);
        for i in 0..MAX_POLY_VOICES {
            let seed = 0x6D2B_79F5_u32.wrapping_add((i as u32).wrapping_mul(0x9E37_79B9));
            voice_vec.push(PolyVoice::new(sr, seed));
        }
        let voices: Box<[PolyVoice; MAX_POLY_VOICES]> =
            match voice_vec.into_boxed_slice().try_into() {
                Ok(arr) => arr,
                Err(_) => unreachable!(),
            };
        Self {
            sample_rate: sr,
            voices,
            steal_cursor: 0,
            humanize_rng: 0xA5A5_1337,
            drum_params: DrumKitParams::default(),
            fm_params: None,
        }
    }

    /// Draws a zero-mean triangular-distributed random sample in `[-1.0, 1.0]`
    /// (concentrated near 0 like real drummer consistency, avoiding wild uniform swings).
    #[inline(always)]
    fn next_triangular_rand(&mut self) -> f32 {
        let step = |s: &mut u32| -> f32 {
            let mut x = *s;
            if x == 0 {
                x = 0xA5A5_1337;
            }
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *s = x;
            ((x as i32) as f32) * (1.0 / 2_147_483_648.0)
        };
        let u1 = step(&mut self.humanize_rng);
        let u2 = step(&mut self.humanize_rng);
        0.5 * (u1 + u2)
    }

    /// Applies controlled per-hit velocity & strike-position humanization (`hit_variation` in `0.0..=1.0`).
    ///
    /// At `0.0`, returns `(vel, strike_pos)` untouched for exact hit-to-hit consistency.
    #[inline(always)]
    fn apply_hit_variation(&mut self, vel: f32, strike_pos: f32, hit_variation: f32) -> (f32, f32) {
        let v = hit_variation.clamp(0.0, 1.0);
        if v <= 1.0e-5 {
            return (vel, strike_pos);
        }
        let r_vel = self.next_triangular_rand();
        let r_pos = self.next_triangular_rand();
        let eff_vel = (vel * (1.0 + r_vel * 0.28 * v)).clamp(0.05, 1.0);
        let eff_pos = (strike_pos + r_pos * 0.36 * v).clamp(0.0, 1.0);
        (eff_vel, eff_pos)
    }

    /// Updates live physical-modeling drum kit parameters.
    pub fn update_drum_params(&mut self, _role: DrumRole, params: DrumVoiceParams) {
        match params {
            DrumVoiceParams::Kick(k) => self.drum_params.kick = k,
            DrumVoiceParams::Snare(s) => self.drum_params.snare = s,
            DrumVoiceParams::Toms(t) => self.drum_params.toms = t,
            DrumVoiceParams::HiHats(h) => self.drum_params.hi_hats = h,
            DrumVoiceParams::Cymbals(c) => self.drum_params.cymbals = c,
            DrumVoiceParams::PercAux(a) => self.drum_params.aux = a,
        }
    }

    /// Updates live 2-operator FM melodic synthesis parameters.
    pub fn update_fm_params(&mut self, params: FmVoiceParams) {
        self.fm_params = Some(params);
    }

    /// Selects an available voice slot or steals the quietest/oldest active voice.
    #[inline(always)]
    fn allocate_voice(&mut self, voice_tag: u8, retrigger_same_tag: bool) -> usize {
        if retrigger_same_tag {
            for (i, v) in self.voices.iter().enumerate() {
                if v.is_active() && v.voice_tag == voice_tag {
                    return i;
                }
            }
        }
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_active() {
                return i;
            }
        }
        // Steal the voice with the lowest remaining envelope energy
        let mut best_idx = self.steal_cursor % MAX_POLY_VOICES;
        let mut min_energy = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            let energy = v.fast_env + v.slow_env;
            if energy < min_energy {
                min_energy = energy;
                best_idx = i;
            }
        }
        self.steal_cursor = best_idx.wrapping_add(1);
        best_idx
    }

    /// Triggers a General MIDI Channel 10 percussion note (`gm_drum_note`) with zero allocations.
    #[inline(always)]
    pub fn trigger_drum(&mut self, gm_drum_note: u8, velocity: f32) {
        let sr = self.sample_rate;

        // Closed (42) or Pedal (44) Hi-Hat chokes any active Open Hi-Hat (46) via dynamic loop damping
        if gm_drum_note == 42 || gm_drum_note == 44 {
            for v in self.voices.iter_mut() {
                if v.is_active() && v.is_open_hat {
                    v.choke(sr);
                }
            }
        }

        // Allow polyphonic overlapping ring tails on fast 16th-note rolls (hi-hat choke handled above)
        let slot = self.allocate_voice(gm_drum_note, false);

        match gm_drum_note {
            // 1. Kick & Toms (Notes 35, 36, 41–50): 6-mode Bessel circular membrane + wooden shell
            35 | 36 => {
                let k = self.drum_params.kick;
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, k.strike_position, k.hit_variation);
                let body_decay = (k.body_decay_ms * 0.001).clamp(0.04, 0.80);
                let base_hz = if gm_note_is_acoustic_kick(gm_drum_note) {
                    k.pitch_end_hz * 0.96
                } else {
                    k.pitch_end_hz
                };
                self.voices[slot].trigger_kick_or_tom(
                    gm_drum_note,
                    base_hz,
                    k.pitch_start_hz,
                    (k.pitch_decay_ms * 0.001).clamp(0.015, 0.12),
                    body_decay,
                    k.head_tension_mod,
                    eff_pos,
                    k.overtones_ring,
                    k.shell_wood_cavity,
                    k.click_level,
                    0.50,
                    eff_vel,
                    sr,
                );
            }
            // Floor Toms (41, 43), Low & Low-Mid Toms (45, 47), Hi-Mid & High Toms (48, 50)
            41 | 43 | 45 | 47 | 48 | 50 => {
                let t = self.drum_params.toms;
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, t.strike_position, t.hit_variation);
                let std_freq = match gm_drum_note {
                    41 => 78.0,
                    43 => 95.0,
                    45 => 118.0,
                    47 => 145.0,
                    48 => 172.0,
                    50 => 205.0,
                    _ => 120.0,
                };
                let pan = match gm_drum_note {
                    41 => 0.66,
                    43 => 0.60,
                    45 => 0.52,
                    47 => 0.45,
                    48 => 0.38,
                    50 => 0.32,
                    _ => 0.50,
                };
                let base_hz = (std_freq * t.pitch_scale).clamp(35.0, sr * 0.25);
                let start_hz = (base_hz * t.pitch_drop_ratio).clamp(base_hz, sr * 0.42);
                let decay_tau = (t.decay_ms * 0.001).clamp(0.04, 1.20);
                self.voices[slot].trigger_kick_or_tom(
                    gm_drum_note,
                    base_hz,
                    start_hz,
                    0.042,
                    decay_tau,
                    t.head_tension_mod,
                    eff_pos,
                    t.overtones_ring,
                    t.shell_wood_cavity,
                    t.stick_attack,
                    pan,
                    eff_vel,
                    sr,
                );
            }

            // 2. Snare, Rimshot & Claps (Notes 37, 38, 39, 40)
            // Side Stick / Rimshot (37): edge strike + woody shell crack + delicate wire buzz
            37 => {
                let s = self.drum_params.snare;
                let (eff_vel, eff_pos) = self.apply_hit_variation(velocity, 0.92, s.hit_variation);
                self.voices[slot].trigger_snare_or_clap(
                    37,
                    310.0,
                    560.0,
                    0.025,
                    0.035,
                    s.wire_cutoff_hz.max(2_200.0),
                    2.0,
                    0.88,
                    0.18,
                    s.head_tension_mod * 0.5,
                    eff_pos,
                    (s.overtones_ring + 0.25).min(1.0),
                    (s.shell_wood_cavity + 0.25).min(1.0),
                    (s.wire_tension + 0.2).min(1.0),
                    false,
                    s.clap_spacing_ms,
                    0.46,
                    eff_vel,
                    sr,
                );
            }
            // Acoustic Snare (38) & Electric/Rimshot Snare (40):
            38 | 40 => {
                let s = self.drum_params.snare;
                let body_hz = s.tone_freq_hz.clamp(100.0, 450.0);
                let mode2_hz = s.mode2_freq_hz.clamp(150.0, 800.0);
                let tone_decay = (s.tone_decay_ms * 0.001).clamp(0.015, 0.40);
                let wire_decay = (s.noise_decay_ms * 0.001).clamp(0.02, 0.80);
                let noise_m = s.noise_mix.clamp(0.05, 0.95);
                let body_m = (1.0 - noise_m).clamp(0.15, 0.95);
                let base_strike_p = if gm_drum_note == 40 {
                    (s.strike_position + 0.28).min(1.0)
                } else {
                    s.strike_position
                };
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, base_strike_p, s.hit_variation);
                self.voices[slot].trigger_snare_or_clap(
                    gm_drum_note,
                    body_hz,
                    mode2_hz,
                    tone_decay,
                    wire_decay,
                    s.wire_cutoff_hz.clamp(1_200.0, 8_000.0),
                    1.15,
                    body_m,
                    noise_m,
                    s.head_tension_mod,
                    eff_pos,
                    s.overtones_ring,
                    s.shell_wood_cavity,
                    s.wire_tension,
                    false,
                    s.clap_spacing_ms,
                    0.46,
                    eff_vel,
                    sr,
                );
            }
            // Hand Clap (39): multi-burst acoustic hand clap
            39 => {
                let s = self.drum_params.snare;
                let (eff_vel, _) = self.apply_hit_variation(velocity, 0.5, s.hit_variation);
                self.voices[slot].trigger_snare_or_clap(
                    39,
                    185.0,
                    330.0,
                    0.028,
                    0.170,
                    1_350.0,
                    1.5,
                    0.18,
                    0.90,
                    0.0,
                    0.5,
                    0.3,
                    0.3,
                    0.5,
                    true,
                    s.clap_spacing_ms,
                    0.54,
                    eff_vel,
                    sr,
                );
            }

            // 3. Cast B20 Bronze Hi-Hats & Cymbals (Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59)
            // Closed Hi-Hat (42):
            42 => {
                let h = self.drum_params.hi_hats;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, h.stick_hardness, h.hit_variation);
                let decay = (h.closed_decay_ms * 0.001).clamp(0.015, 0.30);
                self.voices[slot].trigger_hat_or_cymbal(
                    42,
                    h.bronze_size_pitch,
                    h.spread,
                    h.plate_dispersion,
                    h.nonlin_shimmer,
                    h.bell_level,
                    eff_hard,
                    false,
                    false,
                    0.015,
                    decay,
                    h.highpass_cutoff_hz,
                    0.34,
                    eff_vel,
                    sr,
                );
            }
            // Pedal Hi-Hat (44):
            44 => {
                let h = self.drum_params.hi_hats;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, h.stick_hardness * 0.75, h.hit_variation);
                let decay = (h.pedal_decay_ms * 0.001).clamp(0.020, 0.35);
                self.voices[slot].trigger_hat_or_cymbal(
                    44,
                    h.bronze_size_pitch * 0.96,
                    h.spread * 0.96,
                    h.plate_dispersion,
                    h.nonlin_shimmer,
                    h.bell_level * 0.8,
                    eff_hard,
                    false,
                    false,
                    0.018,
                    decay,
                    h.highpass_cutoff_hz,
                    0.34,
                    eff_vel,
                    sr,
                );
            }
            // Open Hi-Hat (46):
            46 => {
                let h = self.drum_params.hi_hats;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, h.stick_hardness, h.hit_variation);
                let decay = (h.open_decay_ms * 0.001).clamp(0.08, 2.5);
                self.voices[slot].trigger_hat_or_cymbal(
                    46,
                    h.bronze_size_pitch,
                    h.spread,
                    h.plate_dispersion,
                    h.nonlin_shimmer,
                    h.bell_level,
                    eff_hard,
                    false,
                    true,
                    0.045,
                    decay,
                    h.highpass_cutoff_hz,
                    0.34,
                    eff_vel,
                    sr,
                );
            }
            // Crash Cymbal 1 (49):
            49 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, c.stick_hardness, c.hit_variation);
                let decay = (c.crash_decay_ms * 0.001).clamp(0.2, 4.0);
                self.voices[slot].trigger_hat_or_cymbal(
                    49,
                    c.bronze_size_pitch * 0.95,
                    c.matrix_spread * 0.94,
                    c.plate_dispersion,
                    c.nonlin_shimmer,
                    c.bell_peak_level * 0.65,
                    eff_hard,
                    true,
                    false,
                    0.090,
                    decay,
                    c.cutoff_hz,
                    0.30,
                    eff_vel,
                    sr,
                );
            }
            // Ride Cymbal 1 (51 — Ride Bow):
            51 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) = self.apply_hit_variation(
                    velocity,
                    (c.stick_hardness + 0.12).min(1.0),
                    c.hit_variation,
                );
                let decay = (c.ride_decay_ms * 0.001).clamp(0.2, 4.0);
                self.voices[slot].trigger_hat_or_cymbal(
                    51,
                    c.bronze_size_pitch * 1.04,
                    c.matrix_spread * 1.08,
                    c.plate_dispersion,
                    c.nonlin_shimmer,
                    c.bell_peak_level,
                    eff_hard,
                    true,
                    false,
                    0.055,
                    decay,
                    c.cutoff_hz,
                    0.68,
                    eff_vel,
                    sr,
                );
            }
            // Chinese Cymbal (52):
            52 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, c.stick_hardness, c.hit_variation);
                let decay = (c.crash_decay_ms * 0.001 * 0.85).clamp(0.2, 3.5);
                self.voices[slot].trigger_hat_or_cymbal(
                    52,
                    c.bronze_size_pitch * 0.88,
                    c.matrix_spread * 0.84,
                    c.plate_dispersion,
                    (c.nonlin_shimmer + 0.18).min(1.0),
                    c.bell_peak_level * 0.55,
                    eff_hard,
                    true,
                    false,
                    0.110,
                    decay,
                    c.cutoff_hz * 0.92,
                    0.72,
                    eff_vel,
                    sr,
                );
            }
            // Ride Bell (53): strong cast-bronze bell modal ping & crisp stick hardness
            53 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) = self.apply_hit_variation(
                    velocity,
                    (c.stick_hardness + 0.22).min(1.0),
                    c.hit_variation,
                );
                let decay = (c.ride_decay_ms * 0.001 * 0.80).clamp(0.2, 3.2);
                self.voices[slot].trigger_hat_or_cymbal(
                    53,
                    c.bronze_size_pitch * 1.24,
                    c.matrix_spread * 1.22,
                    (c.plate_dispersion + 0.15).min(1.0),
                    c.nonlin_shimmer * 0.70,
                    (c.bell_peak_level * 1.65).min(2.0),
                    eff_hard,
                    true,
                    false,
                    0.070,
                    decay,
                    c.cutoff_hz * 1.25,
                    0.68,
                    eff_vel,
                    sr,
                );
            }
            // Splash Cymbal (55):
            55 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, c.stick_hardness, c.hit_variation);
                let decay = (c.crash_decay_ms * 0.001 * 0.52).clamp(0.15, 2.0);
                self.voices[slot].trigger_hat_or_cymbal(
                    55,
                    c.bronze_size_pitch * 1.25,
                    c.matrix_spread * 1.15,
                    c.plate_dispersion,
                    c.nonlin_shimmer,
                    c.bell_peak_level * 0.6,
                    eff_hard,
                    true,
                    false,
                    0.050,
                    decay,
                    c.cutoff_hz * 1.15,
                    0.38,
                    eff_vel,
                    sr,
                );
            }
            // Crash Cymbal 2 (57):
            57 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, c.stick_hardness, c.hit_variation);
                let decay = (c.crash_decay_ms * 0.001 * 1.08).clamp(0.2, 4.0);
                self.voices[slot].trigger_hat_or_cymbal(
                    57,
                    c.bronze_size_pitch * 1.03,
                    c.matrix_spread * 0.98,
                    c.plate_dispersion,
                    c.nonlin_shimmer,
                    c.bell_peak_level * 0.65,
                    eff_hard,
                    true,
                    false,
                    0.095,
                    decay,
                    c.cutoff_hz,
                    0.64,
                    eff_vel,
                    sr,
                );
            }
            // Ride Cymbal 2 (59):
            59 => {
                let c = self.drum_params.cymbals;
                let (eff_vel, eff_hard) = self.apply_hit_variation(
                    velocity,
                    (c.stick_hardness + 0.10).min(1.0),
                    c.hit_variation,
                );
                let decay = (c.ride_decay_ms * 0.001 * 0.95).clamp(0.2, 4.0);
                self.voices[slot].trigger_hat_or_cymbal(
                    59,
                    c.bronze_size_pitch * 0.98,
                    c.matrix_spread * 1.04,
                    c.plate_dispersion,
                    c.nonlin_shimmer,
                    c.bell_peak_level,
                    eff_hard,
                    true,
                    false,
                    0.060,
                    decay,
                    c.cutoff_hz,
                    0.66,
                    eff_vel,
                    sr,
                );
            }

            // Cowbell & Aux (56):
            56 => {
                let a = self.drum_params.aux;
                let f1 = (a.freq1_hz + a.detune).clamp(100.0, 3_000.0);
                let f2 = (a.freq2_hz - a.detune).clamp(150.0, 3_500.0);
                let decay = (a.decay_ms * 0.001).clamp(0.015, 0.80);
                self.voices[slot].trigger_snare_or_clap(
                    56,
                    f1,
                    f2,
                    decay,
                    decay * 0.20,
                    2_000.0,
                    a.bandpass_q,
                    0.96,
                    0.04,
                    0.0,
                    0.2,
                    0.5,
                    0.15,
                    0.95,
                    false,
                    12.0,
                    0.58,
                    velocity,
                    sr,
                );
            }
            // Tambourine (54) / Cabasa (69) / Maracas (70)
            54 | 69 | 70 => self.voices[slot].trigger_hat_or_cymbal(
                gm_drum_note,
                1.35,
                1.25,
                0.55,
                0.60,
                0.25,
                0.80,
                false,
                false,
                0.025,
                0.095,
                7_500.0,
                0.60,
                velocity,
                sr,
            ),
            // Fallback for other GM Latin/aux percussion notes (congas, bongos, woodblocks)
            other => {
                let base_hz = (90.0 + ((other % 24) as f32) * 14.0).clamp(65.0, 420.0);
                self.voices[slot].trigger_kick_or_tom(
                    other,
                    base_hz,
                    base_hz * 1.45,
                    0.024,
                    0.14,
                    0.45,
                    0.35,
                    0.40,
                    0.45,
                    0.55,
                    0.50,
                    velocity,
                    sr,
                );
            }
        }
    }

    /// Triggers a 2-operator FM General MIDI melodic voice at an exact `f64` Hz frequency.
    #[inline(always)]
    pub fn trigger_gm_voice(&mut self, channel: u8, freq_hz: f64, velocity: f32, program: u8) {
        let tag = 200u8.wrapping_add(channel & 0x0F);
        let slot = self.allocate_voice(tag, false);
        if let Some(fmp) = &self.fm_params {
            self.voices[slot].trigger_gm_fm_custom(channel, freq_hz, velocity, fmp, self.sample_rate);
        } else {
            self.voices[slot].trigger_gm_fm(channel, freq_hz, velocity, program, self.sample_rate);
        }
    }

    /// Triggers a monophonic-per-string FM voice for a string instrument track (`string_idx`).
    #[inline(always)]
    pub fn trigger_fm_string(&mut self, string_idx: u8, freq_hz: f64, velocity: f32) {
        let tag = 200u8.wrapping_add(string_idx & 0x0F);
        let slot = self.allocate_voice(tag, true);
        if let Some(fmp) = &self.fm_params {
            self.voices[slot].trigger_gm_fm_custom(string_idx, freq_hz, velocity, fmp, self.sample_rate);
        } else {
            self.voices[slot].trigger_gm_fm(string_idx, freq_hz, velocity, 0, self.sample_rate);
        }
    }

    /// Hammer-on / pull-off (`SlurString`) on an FM voice for `string_idx`.
    #[inline(always)]
    pub fn slur_fm_string(&mut self, string_idx: u8, new_freq_hz: f64, hammer_energy: f32) {
        let tag = 200u8.wrapping_add(string_idx & 0x0F);
        let sr = self.sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let carrier_hz = (new_freq_hz.clamp(15.0, (sr as f64) * 0.45)) as f32;
        let energy = hammer_energy.clamp(0.05, 1.0);

        for v in self.voices.iter_mut() {
            if v.is_active() && v.voice_tag == tag {
                let ratio = if let Some(fmp) = &self.fm_params {
                    fmp.mod_ratio.clamp(0.1, 16.0)
                } else if v.base_inc > 1.0e-7 {
                    (v.sec_inc / v.base_inc).clamp(0.1, 16.0)
                } else {
                    1.0
                };
                v.base_inc = carrier_hz * inv_sr;
                v.sec_inc = (carrier_hz * ratio).min(sr * 0.45) * inv_sr;
                v.fast_env = v.fast_env.max(energy * 0.70);
                v.slow_env = v.slow_env.max(energy * 0.85);
                return;
            }
        }

        self.trigger_fm_string(string_idx, new_freq_hz, energy);
    }

    /// Pitch bend / slide (`SlideString`) on an active FM voice for `string_idx`.
    #[inline(always)]
    pub fn slide_fm_string(&mut self, string_idx: u8, target_freq_hz: f64) {
        let tag = 200u8.wrapping_add(string_idx & 0x0F);
        let sr = self.sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let carrier_hz = (target_freq_hz.clamp(15.0, (sr as f64) * 0.45)) as f32;

        for v in self.voices.iter_mut() {
            if v.is_active() && v.voice_tag == tag {
                let ratio = if let Some(fmp) = &self.fm_params {
                    fmp.mod_ratio.clamp(0.1, 16.0)
                } else if v.base_inc > 1.0e-7 {
                    (v.sec_inc / v.base_inc).clamp(0.1, 16.0)
                } else {
                    1.0
                };
                v.base_inc = carrier_hz * inv_sr;
                v.sec_inc = (carrier_hz * ratio).min(sr * 0.45) * inv_sr;
            }
        }
    }

    /// Releases / mutes (`MuteString`) any active FM voice on `string_idx`.
    #[inline(always)]
    pub fn mute_fm_string(&mut self, string_idx: u8, release_rate: f32) {
        let tag = 200u8.wrapping_add(string_idx & 0x0F);
        let rel = release_rate.clamp(0.90, 0.9995);
        for v in self.voices.iter_mut() {
            if v.is_active() && v.voice_tag == tag {
                v.in_attack = false;
                v.fast_decay = v.fast_decay.min(rel);
                v.slow_decay = v.slow_decay.min(rel);
            }
        }
    }

    /// Returns `true` if any voice in this pool is currently active.
    #[inline(always)]
    pub fn has_active_voices(&self) -> bool {
        self.voices.iter().any(|v| v.is_active())
    }

    /// Immediately silences all 16 voices.
    #[inline(always)]
    pub fn all_notes_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.clear();
        }
    }

    /// Renders and sums all active percussion and GM melodic voices for the next stereo frame.
    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;
        for v in self.voices.iter_mut() {
            if v.is_active() {
                let (l, r) = v.next_frame();
                left += l;
                right += r;
            }
        }
        (left, right)
    }
}

#[inline(always)]
const fn gm_note_is_acoustic_kick(note: u8) -> bool {
    note == 35
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase4_kick_snare_hats_and_choke_group() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);

        // 1. Trigger Kick (36) and Snare (38)
        pool.trigger_drum(36, 0.95);
        pool.trigger_drum(38, 0.90);

        let mut energy = 0.0_f32;
        for _ in 0..2048 {
            let (l, r) = pool.next_stereo_frame();
            assert!(l.is_finite() && r.is_finite());
            energy += l * l + r * r;
        }
        assert!(
            energy > 1.0,
            "Kick + Snare must produce strong finite energy, got {}",
            energy
        );

        // 2. Trigger Open Hi-Hat (46), then choke it with Closed Hi-Hat (42)
        pool.all_notes_off();
        pool.trigger_drum(46, 0.90);
        assert!(pool.voices.iter().any(|v| v.is_active() && v.is_open_hat));

        // Capture loop gain before choke
        let open_gain_before = pool
            .voices
            .iter()
            .find(|v| v.is_active() && v.is_open_hat)
            .map(|v| v.cym_loop_gain)
            .unwrap();

        // Triggering closed hi-hat (42) must clear is_open_hat AND dynamically increase waveguide damping
        pool.trigger_drum(42, 0.85);
        assert!(
            !pool.voices.iter().any(|v| v.is_active() && v.is_open_hat),
            "Closed hi-hat (42) must choke open hi-hat (46)"
        );
        let choked_gain = pool
            .voices
            .iter()
            .find(|v| v.voice_tag == 46)
            .map(|v| v.cym_loop_gain)
            .unwrap();
        assert!(
            choked_gain < open_gain_before,
            "Choking open hi-hat must dynamically clamp waveguide loop gain ({} -> {})",
            open_gain_before,
            choked_gain
        );
    }

    #[test]
    fn test_phase4_gm_fm_melodic_presets_microtonal() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);

        let programs = [0_u8, 16, 33, 48, 60];
        let freqs = [261.6256, 311.127, 110.0, 442.5, 523.25];
        for (ch, (&prog, &freq)) in programs.iter().zip(freqs.iter()).enumerate() {
            pool.trigger_gm_voice(ch as u8, freq, 0.85, prog);
        }

        let mut energy = 0.0_f32;
        for _ in 0..2048 {
            let (l, r) = pool.next_stereo_frame();
            assert!(l.is_finite() && r.is_finite());
            energy += l * l + r * r;
        }
        assert!(
            energy > 0.5,
            "2-op FM GM voices must produce audible signal, got {}",
            energy
        );
    }

    #[test]
    fn test_bessel_membrane_strike_position_and_shell_micro_reflection_bounds() {
        // Verify shell 2-tap micro-reflection stays strictly < 1.5 ms across all sample rates
        for sr in [44_100.0_f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut shell = ShellResonator::default();
            shell.configure(sr, 120.0, 0.25, 2_800.0);
            let (t1, t2) = shell.tap_delays();
            assert!(t1 < t2 && t2 < 120);
            assert!((t2 as f32) * 1000.0 / sr < 1.5);
        }

        // Verify center strike vs edge strike shifts excitation from axisymmetric (1.00, 2.29) to edge (1.59, 2.13, 2.65)
        let w_center = PolyVoice::compute_membrane_strike_weights(0.0);
        let w_edge = PolyVoice::compute_membrane_strike_weights(1.0);
        assert!(w_center[0] > w_edge[0] && w_center[3] > w_edge[3]);
        assert!(w_edge[1] > w_center[1] * 4.0 && w_edge[2] > w_center[2] * 4.0 && w_edge[4] > w_center[4] * 4.0);

        // Render Tom at center vs edge and verify distinct finite waveforms & decay
        let mut p_center = PercussionAndGmPool::new(48_000.0);
        let mut p_edge = PercussionAndGmPool::new(48_000.0);
        p_center.drum_params.toms.strike_position = 0.0;
        p_edge.drum_params.toms.strike_position = 1.0;
        p_center.trigger_drum(45, 0.90);
        p_edge.trigger_drum(45, 0.90);

        let mut diff = 0.0_f32;
        for _ in 0..4096 {
            let (lc, rc) = p_center.next_stereo_frame();
            let (le, re) = p_edge.next_stereo_frame();
            assert!(lc.is_finite() && le.is_finite());
            diff += (lc - le).abs() + (rc - re).abs();
        }
        assert!(diff > 5.0, "Center vs edge strike must produce distinct Bessel modal balance (diff={})", diff);
    }

    #[test]
    fn test_snare_membrane_coupled_wires_ghost_vs_hard_hit() {
        let sr = 48_000.0;
        let mut pool_ghost = PercussionAndGmPool::new(sr);
        let mut pool_hard = PercussionAndGmPool::new(sr);

        pool_ghost.trigger_drum(38, 0.15); // Soft ghost note
        pool_hard.trigger_drum(38, 0.98);  // Hard snare hit

        let mut e_ghost = 0.0_f32;
        let mut e_hard = 0.0_f32;
        for _ in 0..4096 {
            let (lg, rg) = pool_ghost.next_stereo_frame();
            let (lh, rh) = pool_hard.next_stereo_frame();
            assert!(lg.is_finite() && lh.is_finite());
            e_ghost += lg * lg + rg * rg;
            e_hard += lh * lh + rh * rh;
        }
        assert!(
            e_hard > e_ghost * 10.0 && e_ghost > 1.0e-3,
            "Snare must respond dynamically from delicate ghost buzz ({}) to snapping hard hit ({})",
            e_ghost,
            e_hard
        );
    }

    #[test]
    fn test_bronze_cymbal_hadamard_nonlin_shimmer_stability_and_role_differentiation() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);
        // Stress-test max non-linear shimmer bloom across Ride Bow (51), Ride Bell (53), Crash (49/57), Hats (42/46)
        pool.drum_params.cymbals.nonlin_shimmer = 1.0;
        pool.drum_params.cymbals.plate_dispersion = 1.0;
        pool.drum_params.hi_hats.nonlin_shimmer = 1.0;

        for &note in &[42_u8, 44, 46, 49, 51, 52, 53, 55, 57, 59] {
            pool.all_notes_off();
            pool.trigger_drum(note, 1.0);
            let mut early_e = 0.0_f32;
            let mut late_e = 0.0_f32;
            for i in 0..24_000 {
                let (l, r) = pool.next_stereo_frame();
                assert!(
                    l.is_finite() && r.is_finite() && l.abs() <= 1.2 && r.abs() <= 1.2,
                    "Bronze cymbal note {} destabilized at sample {} (l={}, r={})",
                    note,
                    i,
                    l,
                    r
                );
                let e = l * l + r * r;
                if i < 4096 {
                    early_e += e;
                } else if i >= 20_000 {
                    late_e += e;
                }
            }
            assert!(
                early_e > 0.2 && early_e > late_e,
                "Bronze cymbal note {} should bloom and decay cleanly (early={}, late={})",
                note,
                early_e,
                late_e
            );
        }

        // Verify Ride Bell (53) differs distinctly from Ride Bow (51) and Crash (49)
        let mut p_bow = PercussionAndGmPool::new(sr);
        let mut p_bell = PercussionAndGmPool::new(sr);
        p_bow.trigger_drum(51, 0.9);
        p_bell.trigger_drum(53, 0.9);
        let mut diff = 0.0_f32;
        for _ in 0..4096 {
            let (l1, _) = p_bow.next_stereo_frame();
            let (l2, _) = p_bell.next_stereo_frame();
            diff += (l1 - l2).abs();
        }
        assert!(
            diff > 5.0,
            "Ride Bell (53) and Ride Bow (51) must sound distinctly differentiated"
        );
    }

    #[test]
    fn test_drum_hit_consistency_at_zero_vs_high_variation() {
        let sr = 48_000.0;

        // Helper to measure RMS energy of 6 consecutive isolated hits on a given note
        let measure_hit_energies = |note: u8, hit_variation: f32| -> Vec<f32> {
            let mut pool = PercussionAndGmPool::new(sr);
            pool.drum_params.kick.hit_variation = hit_variation;
            pool.drum_params.snare.hit_variation = hit_variation;
            pool.drum_params.toms.hit_variation = hit_variation;
            pool.drum_params.hi_hats.hit_variation = hit_variation;
            pool.drum_params.cymbals.hit_variation = hit_variation;

            let mut energies = Vec::new();
            for _ in 0..6 {
                pool.all_notes_off();
                pool.trigger_drum(note, 0.85);
                let mut e = 0.0_f32;
                for _ in 0..2048 {
                    let (l, r) = pool.next_stereo_frame();
                    e += l * l + r * r;
                }
                energies.push(e.sqrt());
            }
            energies
        };

        // At 0.0 variation, consecutive snare & kick hits must be tightly consistent (< 5% spread)
        for &note in &[36_u8, 38] {
            let e_zero = measure_hit_energies(note, 0.0);
            let min_e = e_zero.iter().copied().fold(f32::MAX, f32::min);
            let max_e = e_zero.iter().copied().fold(0.0_f32, f32::max);
            let spread_ratio = (max_e - min_e) / max_e.max(1e-6);
            assert!(
                spread_ratio < 0.05,
                "Drum note {} at hit_variation=0.0 should be tightly consistent (spread={:.4}, min={:.4}, max={:.4})",
                note,
                spread_ratio,
                min_e,
                max_e
            );
        }

        // At 1.0 variation, consecutive snare hits must exhibit wider expressive spread
        let e_high = measure_hit_energies(38, 1.0);
        let min_h = e_high.iter().copied().fold(f32::MAX, f32::min);
        let max_h = e_high.iter().copied().fold(0.0_f32, f32::max);
        let high_spread = (max_h - min_h) / max_h.max(1e-6);
        assert!(
            high_spread > 0.08,
            "Snare at hit_variation=1.0 should produce clear velocity/strike variation (spread={:.4})",
            high_spread
        );
    }

    #[test]
    fn test_thin_flexible_cymbal_shimmer_vs_low_anvil_clang() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);
        pool.trigger_drum(49, 0.90); // Crash cymbal

        // Split crash output through a 1.2 kHz lowpass vs 2.5 kHz highpass to verify thin airy voicing
        let mut lp = BiquadFilter::lowpass(sr, 1_200.0, 0.707);
        let mut hp = BiquadFilter::highpass(sr, 2_500.0, 0.707);
        let mut low_e = 0.0_f32;
        let mut high_e = 0.0_f32;
        for _ in 0..8192 {
            let (l, r) = pool.next_stereo_frame();
            let mono = l + r;
            let lo = lp.process(mono);
            let hi = hp.process(mono);
            low_e += lo * lo;
            high_e += hi * hi;
        }
        assert!(
            high_e > low_e * 3.0,
            "Thin flexible crash cymbal must emphasize airy upper shimmer (>2.5kHz={}) over low anvil clang (<1.2kHz={})",
            high_e,
            low_e
        );
    }
}
