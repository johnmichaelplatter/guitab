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
//! 3. **Two-Stage Dispersive Feedback + Parallel Inharmonic Comb Cymbals & Hi-Hats (GM Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59):**
//!    - **Stage 1 (`DispersionLoop` — Rides & Crashes/Splash/China):** Self-feeding single-channel
//!      dispersive loop (feeding Stage 2 comb banks, never the master bus directly) containing a
//!      `1.0..=25.0 ms` fractional base delay (`0.80..=0.985` feedback), 3-stage high-coefficient
//!      all-pass cascade (`0.65, 0.78, 0.84`), `0..=4 ms` smoothed random-walk hammering vibrato,
//!      and a 128-sample Self-Phase-Modulation delay driven by a 1-pole `tone_hz` low-passed copy
//!      of the loop signal with a `tanh` soft-limiter.
//!    - **Stage 2 (12 Parallel Wet-Only Combs + Hilbert Shifter):** 4 tightly clustered whole-plate
//!      fundamental combs (`Bank A`) + 8 non-uniform upper shimmer combs (`Bank B`, with optional
//!      4-comb `serial_mode` cascade for ringy hi-hat sizzle), using internal 1-pole low-pass
//!      material damping ONLY and zero dry bleed, followed by a 4-stage quadrature Hilbert frequency
//!      shifter (`shift_hz`) turning integer comb harmonics into authentic inharmonic bronze partials.
//!    - **Hi-Hat Fast Path & Choke (42, 44, 46):** Bypasses `DispersionLoop`, exciting Bank A + B
//!      directly with a pitch-enveloped metallic oscillator + shaped noise burst (with slight attack
//!      + fast noise decay on Pedal Hat 44 "chick") and rapidly ramping comb feedback down on choke.
//!    - **Post-Sum Polish & EQ:** Static low-feedback polish comb (`polish_comb_hz`, `polish_comb_mix`)
//!      followed by post-comb high-pass (`post_hp_hz`) and mid-cut peaking EQ (`mid_cut_hz`, `mid_cut_db`).
//! 4. **Basic GM Melodic Voices (`TriggerGmVoice`):** Compact 2-operator FM voice with selectable
//!    presets (`EPiano`, `Organ`, `Pad`, `Brass`, `SynthBass`) mapped from GM program numbers,
//!    accepting exact `f64` Hz frequencies for microtonal backing tracks.
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, and zero `unwrap()`/`panic!()` on the audio thread.
//! - All 16 voices, modal banks, shell buffers, wire comb buffers, dispersion loops, and parallel
//!   wet-only comb banks are pre-allocated on the heap at stream initialization.
//! - Denormal floats are flushed to zero in all modal, dispersion, comb, Hilbert, and filter states.

use crate::synth::backend::flush_denormal;
use crate::synth::body::{tanh_pade, BiquadFilter};
use crate::synth::patch::{
    DrumKitParams, DrumRole, DrumVoiceParams, FmVoiceParams, HiHatParams, RideCymbalParams,
    ShakerAndEffectParams, SingleCymbalParams, TomParams,
};
use std::f32::consts::{PI, TAU};

/// Fixed polyphonic voice count for percussion and GM melodic backing.
pub const MAX_POLY_VOICES: usize = 16;

/// Air-loaded circular drumhead membrane Bessel modal frequency ratios relative to `(0,1)` fundamental.
pub const MEMBRANE_BESSEL_RATIOS: [f32; 6] = [1.00, 1.59, 2.13, 2.29, 2.65, 2.91];

/// Number of parallel modal SVF resonators per drum/modal percussion voice.
const NUM_MODES: usize = 6;

/// Fixed power-of-two buffer length for wooden shell micro-reflections (< 1.5 ms) and snare wire comb.
const SHORT_COMB_LEN: usize = 128;
const SHORT_COMB_MASK: usize = SHORT_COMB_LEN - 1;

// --- Two-Stage Cymbal & Hi-Hat Pre-Allocated Buffer Sizes ---
/// Stage 1 Base Dispersion Delay buffer length (`8192` samples covers `25 ms` up to `327 kHz` SR).
const DISP_BASE_LEN: usize = 8192;
const DISP_BASE_MASK: usize = DISP_BASE_LEN - 1;

/// Stage 1 Hammering Vibrato fractional delay buffer length (`1024` samples covers `4 ms` up to `256 kHz` SR).
const DISP_VIB_LEN: usize = 1024;
const DISP_VIB_MASK: usize = DISP_VIB_LEN - 1;

/// Stage 1 Self-Phase Modulation (Phase Distortion) circular buffer length (`256` samples).
const DISP_PM_LEN: usize = 256;
const DISP_PM_MASK: usize = DISP_PM_LEN - 1;

/// Number of staggered 1st-order all-pass dispersion stages inside `DispersionLoop`.
const DISP_AP_STAGES: usize = 12;

/// Stage 2 Bank A: 4 Lower-Order Fundamental Wet-Only Combs (`2048` samples each).
const BANK_A_COMBS: usize = 4;
const COMB_A_LEN: usize = 2048;
const COMB_A_MASK: usize = COMB_A_LEN - 1;

/// Stage 2 Bank B: 8 Higher-Order Shimmer Wet-Only Combs (`1024` samples each).
const BANK_B_COMBS: usize = 8;
const COMB_B_LEN: usize = 1024;
const COMB_B_MASK: usize = COMB_B_LEN - 1;

/// Post-Sum Static Polish Comb buffer length (`512` samples).
const POLISH_COMB_LEN: usize = 512;
const POLISH_COMB_MASK: usize = POLISH_COMB_LEN - 1;

/// Allocates a zeroed `Box<[f32; N]>` directly on the heap without staging on the stack.
fn alloc_box_array<const N: usize>() -> Box<[f32; N]> {
    match vec![0.0_f32; N].into_boxed_slice().try_into() {
        Ok(b) => b,
        Err(_) => unreachable!(),
    }
}

/// Allocates a zeroed `Box<[[f32; N]; M]>` directly on the heap without staging on the stack.
fn alloc_box_2d<const M: usize, const N: usize>() -> Box<[[f32; N]; M]> {
    let mut v = Vec::with_capacity(M);
    for _ in 0..M {
        v.push([0.0_f32; N]);
    }
    match v.into_boxed_slice().try_into() {
        Ok(b) => b,
        Err(_) => unreachable!(),
    }
}

/// 4-point Catmull-Rom / Hermite cubic fractional interpolation read from a power-of-two circular
/// buffer where `write_idx` points to the NEXT slot to be written (`delay_samples >= 2.0`).
///
/// Preserves flat high-frequency magnitude (`6–18 kHz`) across modulated feedback passes without
/// the severe `-6 dB/pass` Nyquist rolloff of 2-point linear interpolation.
#[inline(always)]
fn read_hermite_ring<const N: usize>(
    buf: &[f32; N],
    write_idx: usize,
    delay_samples: f32,
    mask: usize,
) -> f32 {
    let d = delay_samples.clamp(2.0, (N - 4) as f32);
    let d_int = d.floor() as usize;
    let t = d - (d_int as f32);
    let ym1 = buf[write_idx.wrapping_sub(d_int - 1) & mask];
    let y0 = buf[write_idx.wrapping_sub(d_int) & mask];
    let y1 = buf[write_idx.wrapping_sub(d_int + 1) & mask];
    let y2 = buf[write_idx.wrapping_sub(d_int + 2) & mask];
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + y0
}

/// Stage 1: Self-Feeding Dispersion Loop for Rides and Crashes/Splash/China.
///
/// Feeds Stage 2 parallel wet-only comb banks ONLY (never mixed directly into the master bus).
#[derive(Clone, Debug)]
pub struct DispersionLoop {
    base_buf: Box<[f32; DISP_BASE_LEN]>,
    vib_buf: Box<[f32; DISP_VIB_LEN]>,
    pm_buf: [f32; DISP_PM_LEN],
    base_write: usize,
    vib_write: usize,
    pm_write: usize,
    base_delay_samples: f32,
    fb_gain: f32,
    /// 12 staggered 1st-order all-pass filters smearing sharp strikes into broadband wash.
    ap_coeffs: [f32; DISP_AP_STAGES],
    ap_x1: [f32; DISP_AP_STAGES],
    ap_y1: [f32; DISP_AP_STAGES],
    /// Smoothed random-walk + stepped hammering vibrato LFO (`0..=4 ms`).
    max_vib_samples: f32,
    vibrato_depth: f32,
    vibrato_chaos: f32,
    lfo_walk: f32,
    lfo_step: f32,
    lfo_val: f32,
    lfo_smooth_alpha: f32,
    lfo_counter: u32,
    lfo_period: u32,
    /// Multi-radian Self-Phase-Modulation state (`256`-sample Hermite delay + 4-stage Gray-Markel Orthogonal Self-PM All-Pass).
    pm_center_samples: f32,
    pm_max_excursion: f32,
    pm_drive: f32,
    pm_fold_beta: f32,
    pm_lp_alpha: f32,
    pm_lp_z1: f32,
    pm_ap_state: [f32; 4],
    /// In-loop 1-pole bass-trim filter (~190 Hz) shedding low mud after upward harmonic folding.
    bass_trim_alpha: f32,
    bass_trim_amount: f32,
    bass_lp_z1: f32,
    /// Gentle DC blocker inside feedback loop.
    dc_x1: f32,
    dc_y1: f32,
    /// Instantaneous loop energy tracker for auto-deactivation.
    energy_env: f32,
}

impl DispersionLoop {
    pub fn new() -> Self {
        Self {
            base_buf: alloc_box_array::<DISP_BASE_LEN>(),
            vib_buf: alloc_box_array::<DISP_VIB_LEN>(),
            pm_buf: [0.0; DISP_PM_LEN],
            base_write: 0,
            vib_write: 0,
            pm_write: 0,
            base_delay_samples: 480.0,
            fb_gain: 0.93,
            ap_coeffs: [
                0.56, 0.63, -0.68, 0.72, 0.76, -0.79, 0.81, 0.83, -0.85, 0.86, 0.88, 0.89,
            ],
            ap_x1: [0.0; DISP_AP_STAGES],
            ap_y1: [0.0; DISP_AP_STAGES],
            max_vib_samples: 192.0,
            vibrato_depth: 0.04,
            vibrato_chaos: 0.30,
            lfo_walk: 0.0,
            lfo_step: 0.0,
            lfo_val: 0.0,
            lfo_smooth_alpha: 0.005,
            lfo_counter: 0,
            lfo_period: 256,
            pm_center_samples: 34.0,
            pm_max_excursion: 28.0,
            pm_drive: 0.50,
            pm_fold_beta: 4.5,
            pm_lp_alpha: 0.30,
            pm_lp_z1: 0.0,
            pm_ap_state: [0.0; 4],
            bass_trim_alpha: 0.03,
            bass_trim_amount: 0.18,
            bass_lp_z1: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            energy_env: 0.0,
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.base_buf.fill(0.0);
        self.vib_buf.fill(0.0);
        self.pm_buf.fill(0.0);
        self.base_write = 0;
        self.vib_write = 0;
        self.pm_write = 0;
        self.ap_x1 = [0.0; DISP_AP_STAGES];
        self.ap_y1 = [0.0; DISP_AP_STAGES];
        self.lfo_walk = 0.0;
        self.lfo_step = 0.0;
        self.lfo_val = 0.0;
        self.lfo_counter = 0;
        self.pm_lp_z1 = 0.0;
        self.pm_ap_state = [0.0; 4];
        self.bass_lp_z1 = 0.0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.energy_env = 0.0;
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn configure(
        &mut self,
        sample_rate: f32,
        dispersion_size_ms: f32,
        bronze_size_pitch: f32,
        decay_t60_sec: f32,
        allpass_smear: f32,
        vibrato_depth: f32,
        vibrato_chaos: f32,
        pm_drive: f32,
        pm_tone_hz: f32,
        velocity: f32,
        is_ride: bool,
    ) {
        let sr = sample_rate.max(8_000.0);
        let vel = velocity.clamp(0.05, 1.0);

        // 1. Base Dispersion Delay (`1.0..=25.0 ms` scaled inversely by `bronze_size_pitch`)
        let eff_ms = (dispersion_size_ms / bronze_size_pitch.clamp(0.35, 3.0)).clamp(1.0, 25.0);
        let max_base = (DISP_BASE_LEN - 6) as f32;
        self.base_delay_samples = (eff_ms * 0.001 * sr).max(3.0).min(max_base);

        // Sub-unity feedback gain (`0.70..=0.968`) auto-derived from Ring/Wash Decay:
        // Crashes bloom fiercely over ~15–90 ms across 6–14 lossless Self-PM rim reflections before handing off cleanly to Stage 2 combs.
        let loop_t60 = if is_ride {
            (decay_t60_sec * 0.12).clamp(0.10, 0.36)
        } else {
            (decay_t60_sec * 0.18).clamp(0.12, 0.52)
        };
        let raw_fb = 10.0_f32.powf((-3.0 * (eff_ms * 0.001) / loop_t60).max(-1.2));
        self.fb_gain = raw_fb.clamp(0.70, 0.968);

        // 2. 12-stage Staggered 1st-Order All-Pass Disperser
        const BASE_AP_12: [f32; DISP_AP_STAGES] = [
            0.56, 0.63, -0.68, 0.72, 0.76, -0.79, 0.81, 0.83, -0.85, 0.86, 0.88, 0.89,
        ];
        let smear = allpass_smear.clamp(0.0, 1.25);
        let smear_scale = (0.14 + 0.86 * smear).clamp(0.0, 1.05);
        for k in 0..DISP_AP_STAGES {
            self.ap_coeffs[k] = (BASE_AP_12[k] * smear_scale).clamp(-0.93, 0.93);
        }

        // 3. Smoothed Random Vibrato (`0..=4 ms`)
        self.max_vib_samples = (0.004 * sr).min((DISP_VIB_LEN - 8) as f32).max(5.0);
        self.vibrato_depth = vibrato_depth.clamp(0.0, 0.50);
        self.vibrato_chaos = vibrato_chaos.clamp(0.0, 1.0);
        let period_ms = 12.5 - 9.2 * self.vibrato_chaos;
        self.lfo_period = ((period_ms * 0.001 * sr).round() as u32).max(16);
        self.lfo_counter = 0;
        let lfo_bw_hz = 10.0 + 56.0 * self.vibrato_chaos;
        self.lfo_smooth_alpha = (1.0 - (-TAU * lfo_bw_hz / sr).exp()).clamp(0.0002, 0.28);

        // 4. Multi-Radian Lossless Self-Phase Modulation (Hermite Delay PM + 4-Stage Orthogonal All-Pass Self-PM)
        self.pm_center_samples = (34.0 * (sr / 48_000.0)).clamp(14.0, 96.0);
        self.pm_max_excursion =
            (self.pm_center_samples - 3.0).min((DISP_PM_LEN as f32 - 8.0) - self.pm_center_samples);
        let vel_scale = 0.42 + 0.78 * vel;
        self.pm_drive = (pm_drive.clamp(0.0, 1.35) * vel_scale).clamp(0.0, 1.45);
        self.pm_fold_beta = if is_ride {
            2.8 * self.pm_drive
        } else {
            6.6 * self.pm_drive
        };
        let tone = pm_tone_hz.clamp(200.0, sr * 0.45);
        self.pm_lp_alpha = (1.0 - (-TAU * tone / sr).exp()).clamp(0.01, 0.96);

        // In-loop 1-pole bass-trim (~190 Hz) sheds low strike mud after folding harmonics into 6–18 kHz
        let trim_hz = (190.0 * bronze_size_pitch.clamp(0.5, 2.2)).min(sr * 0.15);
        self.bass_trim_alpha = (1.0 - (-TAU * trim_hz / sr).exp()).clamp(0.003, 0.16);
        self.bass_trim_amount = if is_ride { 0.10 } else { 0.18 };
    }

    #[inline(always)]
    pub fn process(&mut self, exc_in: f32, choke_fb_scale: f32, rng: &mut u32) -> f32 {
        // 1. Base Dispersion Delay read (`1.0..=25.0 ms`) via 4-point Hermite cubic interpolation
        let delayed = read_hermite_ring(
            &self.base_buf,
            self.base_write,
            self.base_delay_samples,
            DISP_BASE_MASK,
        );
        let eff_fb = (self.fb_gain * choke_fb_scale).clamp(0.0, 0.968);
        // Apply 12-stage disperser + vibrato + multi-radian Self-PM on every rim reflection (`delayed * eff_fb`)
        // while `exc_in` seeds Pass 0 as a warm physical bronze strike — creating genuine upward 6–18 kHz bloom!
        let mut sig = delayed * eff_fb;

        // 2. 12-stage staggered 1st-order All-Pass Disperser
        for k in 0..DISP_AP_STAGES {
            let a = self.ap_coeffs[k];
            let y = flush_denormal(a * sig + self.ap_x1[k] - a * self.ap_y1[k]);
            self.ap_x1[k] = flush_denormal(sig);
            self.ap_y1[k] = y;
            sig = y;
        }

        // 3. Smoothed Random Vibrato (`0..=4 ms`, Hermite cubic read)
        if self.lfo_counter == 0 {
            let mut x = *rng;
            if x == 0 {
                x = 0x6D2B_79F5;
            }
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *rng = x;
            let r = ((x as i32) as f32) * (1.0 / 2_147_483_648.0);
            self.lfo_walk = (self.lfo_walk * 0.88 + r * 0.44).clamp(-1.0, 1.0);
            self.lfo_step = r;
            self.lfo_counter = self.lfo_period;
        } else {
            self.lfo_counter -= 1;
        }
        let target_lfo =
            (1.0 - self.vibrato_chaos) * self.lfo_walk + self.vibrato_chaos * self.lfo_step;
        self.lfo_val =
            flush_denormal(self.lfo_val + self.lfo_smooth_alpha * (target_lfo - self.lfo_val));

        self.vib_buf[self.vib_write] = flush_denormal(sig);
        self.vib_write = (self.vib_write + 1) & DISP_VIB_MASK;
        let depth_norm = (self.vibrato_depth * 2.0).clamp(0.0, 1.0);
        let vib_extra =
            0.35 * (self.max_vib_samples - 2.0) * depth_norm * (1.0 + 0.96 * self.lfo_val);
        let vib_out = read_hermite_ring(
            &self.vib_buf,
            self.vib_write,
            2.0 + vib_extra,
            DISP_VIB_MASK,
        );

        // 4. Multi-Radian Lossless Self-Phase Modulation:
        //    (a) 256-sample Hermite fractional delay self-PM (`kHs Phase Distortion` delay warp)
        //    (b) 4-stage Gray-Markel Orthogonal Self-PM All-Pass Cascade (`sin^2 + cos^2 == 1` ->
        //        exact 0.00 dB energy conservation + 100% passive L2 norm <= 1.0 with zero limit cycles,
        //        folding <2 kHz strike energy rapidly into dense 6–18 kHz bronze sizzle!)
        self.pm_buf[self.pm_write] = flush_denormal(vib_out);
        self.pm_write = (self.pm_write + 1) & DISP_PM_MASK;
        let prev_lp = self.pm_lp_z1;
        self.pm_lp_z1 =
            flush_denormal(prev_lp + self.pm_lp_alpha * (vib_out - prev_lp));
        let pm_ctrl = self.pm_lp_z1 + 0.85 * (self.pm_lp_z1 - prev_lp);
        let pm_mod = (pm_ctrl * self.pm_drive * 2.2).clamp(-1.0, 1.0) * self.pm_max_excursion;
        let pm_delay = (self.pm_center_samples + pm_mod).clamp(2.0, (DISP_PM_LEN - 5) as f32);
        let mut warped = read_hermite_ring(&self.pm_buf, self.pm_write, pm_delay, DISP_PM_MASK);

        if self.pm_fold_beta > 1.0e-4 {
            const BASE_PHI: [f32; 4] = [0.38, -0.52, 0.67, -0.79];
            const STAGE_SCALE: [f32; 4] = [0.85, 1.08, 1.34, 1.62];
            for m in 0..4 {
                let mod_sig = 0.56 * pm_ctrl + 0.44 * warped;
                let phi = BASE_PHI[m] + self.pm_fold_beta * STAGE_SCALE[m] * mod_sig;
                let (s_p, c_p) = phi.sin_cos();
                let st = self.pm_ap_state[m];
                let y = flush_denormal(s_p * warped + c_p * st);
                self.pm_ap_state[m] = flush_denormal(c_p * warped - s_p * st);
                warped = y;
            }
        }

        // In-loop bass-trim (~240 Hz) sheds sub-bass mud so 6–18 kHz harmonics own the loop headroom
        self.bass_lp_z1 = flush_denormal(
            self.bass_lp_z1 + self.bass_trim_alpha * (warped - self.bass_lp_z1),
        );
        let tilted = warped - self.bass_trim_amount * self.bass_lp_z1;

        // Unity-Nyquist DC blocker on feedback return + `tanh` soft-limiter summing `exc_in + feedback`
        let dc_y = flush_denormal(0.9975 * (tilted - self.dc_x1) + 0.995 * self.dc_y1);
        self.dc_x1 = flush_denormal(tilted);
        self.dc_y1 = dc_y;
        let loop_out = flush_denormal(tanh_pade((exc_in + dc_y) * 1.12) * (1.0 / 1.12));

        self.base_buf[self.base_write] = loop_out;
        self.base_write = (self.base_write + 1) & DISP_BASE_MASK;

        let abs_o = loop_out.abs();
        let e_c = if abs_o > self.energy_env { 0.06 } else { 0.002 };
        self.energy_env = flush_denormal(self.energy_env + e_c * (abs_o - self.energy_env));

        loop_out
    }
}

/// Per-comb scalar state for an independent wet-only comb filter using 1st-order Thiran all-pass
/// fractional delay interpolation (0 dB HF loss up to Nyquist!) + internal 1-pole LP damping ONLY.
#[derive(Clone, Copy, Debug, Default)]
struct WetCombMeta {
    write_idx: usize,
    delay_int: usize,
    thiran_a: f32,
    thiran_x1: f32,
    thiran_y1: f32,
    fb_gain: f32,
    in_norm: f32,
    lp_alpha: f32,
    lp_z1: f32,
}

impl WetCombMeta {
    #[inline(always)]
    fn from_delay(delay_samples: f32, fb_gain: f32, in_norm: f32, lp_alpha: f32) -> Self {
        let d = delay_samples.max(3.0);
        // Split total delay `d = delay_int + delta` with `delta in [0.35, 1.35)` so `thiran_a` stays bounded in `(-0.15, +0.48]`
        let delay_int = (d - 0.35).floor() as usize;
        let delta = (d - (delay_int as f32)).clamp(0.35, 1.35);
        let thiran_a = (1.0 - delta) / (1.0 + delta);
        Self {
            write_idx: 0,
            delay_int,
            thiran_a,
            thiran_x1: 0.0,
            thiran_y1: 0.0,
            fb_gain,
            in_norm,
            lp_alpha: lp_alpha.clamp(0.03, 0.9985),
            lp_z1: 0.0,
        }
    }

    #[inline(always)]
    fn step<const N: usize>(
        &mut self,
        buf: &mut [f32; N],
        mask: usize,
        input: f32,
        choke_lp_scale: f32,
        choke_fb_scale: f32,
    ) -> f32 {
        // Pure delayed integer read -> unity-gain Thiran all-pass fractional delay -> delay-compensated 1-pole LP damping ONLY
        let x_int = buf[self.write_idx.wrapping_sub(self.delay_int) & mask];
        let delayed = flush_denormal(
            self.thiran_a * x_int + self.thiran_x1 - self.thiran_a * self.thiran_y1,
        );
        self.thiran_x1 = flush_denormal(x_int);
        self.thiran_y1 = delayed;

        let eff_lp = (self.lp_alpha * choke_lp_scale).clamp(0.03, 0.9985);
        let damped = flush_denormal(self.lp_z1 + eff_lp * (delayed - self.lp_z1));
        self.lp_z1 = damped;

        let eff_fb = (self.fb_gain * choke_fb_scale).clamp(0.0, 0.9990);
        let write_val = flush_denormal(input * self.in_norm + damped * eff_fb);
        buf[self.write_idx] = write_val;
        self.write_idx = (self.write_idx + 1) & mask;

        damped
    }
}

/// Post-Comb 4-Stage All-Pass Quadrature Hilbert Network + Quadrature Oscillator Frequency Shifter.
///
/// Shifts positive frequencies by `shift_hz` (`-200.0..=+400.0 Hz`) to turn integer comb harmonics
/// into authentic non-integer inharmonic cast-bronze partials.
#[derive(Clone, Copy, Debug)]
pub struct HilbertFreqShifter {
    i_z1: [f32; 4],
    i_z2: [f32; 4],
    i_delay1: f32,
    q_z1: [f32; 4],
    q_z2: [f32; 4],
    osc_phase: f32,
    osc_inc: f32,
}

impl Default for HilbertFreqShifter {
    fn default() -> Self {
        Self {
            i_z1: [0.0; 4],
            i_z2: [0.0; 4],
            i_delay1: 0.0,
            q_z1: [0.0; 4],
            q_z2: [0.0; 4],
            osc_phase: 0.0,
            osc_inc: 0.0,
        }
    }
}

impl HilbertFreqShifter {
    /// Niemitalo wideband 90-degree phase-difference `z^-2` all-pass coefficients (`a_k^2`).
    const C_I: [f32; 4] = [0.22982519, 0.76775885, 0.95374285, 0.99500477];
    const C_Q: [f32; 4] = [0.02616581, 0.53733142, 0.89368606, 0.98128669];

    #[inline(always)]
    pub fn reset(&mut self) {
        self.i_z1 = [0.0; 4];
        self.i_z2 = [0.0; 4];
        self.i_delay1 = 0.0;
        self.q_z1 = [0.0; 4];
        self.q_z2 = [0.0; 4];
        self.osc_phase = 0.0;
    }

    #[inline(always)]
    pub fn configure(&mut self, sample_rate: f32, shift_hz: f32) {
        let sr = sample_rate.max(8_000.0);
        self.osc_inc = shift_hz.clamp(-600.0, 1_200.0) / sr;
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        // Real-axis-pole wideband Hilbert all-pass sections: H(z) = (c - z^-2) / (1 - c z^-2)
        let mut xi = x;
        for k in 0..4 {
            let c = Self::C_I[k];
            let w = flush_denormal(xi + c * self.i_z2[k]);
            let y = flush_denormal(c * w - self.i_z2[k]);
            self.i_z2[k] = self.i_z1[k];
            self.i_z1[k] = w;
            xi = y;
        }
        // 1-sample delay aligns I/Q quadrature pair (`q_out` leads `i_out` by +90 deg across 20 Hz..20 kHz)
        let i_out = self.i_delay1;
        self.i_delay1 = flush_denormal(xi);

        let mut xq = x;
        for k in 0..4 {
            let c = Self::C_Q[k];
            let w = flush_denormal(xq + c * self.q_z2[k]);
            let y = flush_denormal(c * w - self.q_z2[k]);
            self.q_z2[k] = self.q_z1[k];
            self.q_z1[k] = w;
            xq = y;
        }
        let q_out = xq;

        if self.osc_inc.abs() < 1.0e-7 {
            return q_out;
        }

        self.osc_phase += self.osc_inc;
        if self.osc_phase >= 1.0 {
            self.osc_phase -= 1.0;
        } else if self.osc_phase < 0.0 {
            self.osc_phase += 1.0;
        }
        let angle = self.osc_phase * TAU;
        let (osc_sin, osc_cos) = angle.sin_cos();
        // Upper-sideband shift for +shift_hz (and lower-sideband for -shift_hz):
        // cos(A)*cos(B) - sin(A)*sin(B) = cos(A + B)
        flush_denormal(q_out * osc_cos - i_out * osc_sin)
    }
}

/// Post-sum low-feedback static Polish Comb Filter using Thiran all-pass fractional interpolation for crisp metallic sheen.
#[derive(Clone, Debug)]
pub struct PolishComb {
    buf: [f32; POLISH_COMB_LEN],
    write_idx: usize,
    delay_int: usize,
    thiran_a: f32,
    thiran_x1: f32,
    thiran_y1: f32,
    fb: f32,
    mix: f32,
}

impl Default for PolishComb {
    fn default() -> Self {
        Self {
            buf: [0.0; POLISH_COMB_LEN],
            write_idx: 0,
            delay_int: 12,
            thiran_a: 0.0,
            thiran_x1: 0.0,
            thiran_y1: 0.0,
            fb: 0.28,
            mix: 0.22,
        }
    }
}

impl PolishComb {
    #[inline(always)]
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write_idx = 0;
        self.thiran_x1 = 0.0;
        self.thiran_y1 = 0.0;
    }

    #[inline(always)]
    pub fn configure(&mut self, sample_rate: f32, freq_hz: f32, mix: f32) {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(220.0, (sr * 0.45).min(15_000.0));
        let d = (sr / f).clamp(2.5, (POLISH_COMB_LEN - 4) as f32);
        let delay_int = (d - 0.35).floor() as usize;
        let delta = (d - (delay_int as f32)).clamp(0.35, 1.35);
        self.delay_int = delay_int.max(2);
        self.thiran_a = (1.0 - delta) / (1.0 + delta);
        self.mix = mix.clamp(0.0, 0.85);
        self.fb = (0.18 + 0.24 * self.mix).clamp(0.15, 0.42);
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.mix <= 1.0e-4 {
            return x;
        }
        let x_int = self.buf[self.write_idx.wrapping_sub(self.delay_int) & POLISH_COMB_MASK];
        let delayed = flush_denormal(
            self.thiran_a * x_int + self.thiran_x1 - self.thiran_a * self.thiran_y1,
        );
        self.thiran_x1 = flush_denormal(x_int);
        self.thiran_y1 = delayed;
        let w = flush_denormal(x + delayed * self.fb);
        self.buf[self.write_idx] = w;
        self.write_idx = (self.write_idx + 1) & POLISH_COMB_MASK;
        x * (1.0 - 0.28 * self.mix) + delayed * self.mix
    }
}

/// Excitation archetype for the Two-Stage Cymbal & Hi-Hat engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum CymbalExcitationKind {
    /// Hi-Hat fast path (42, 44, 46): bypasses `DispersionLoop`, excites Bank A + Bank B directly.
    #[default]
    HiHatFastPath,
    /// Ride Bow / Bell / Ride 2 (51, 53, 59): pitch-enveloped sine/FM stick click -> `DispersionLoop` + light direct mix.
    RideStickAndBell,
    /// Crash / Splash / China (49, 57, 55, 52): low-freq saw/tri energy impulse -> `DispersionLoop` ONLY (muted from master).
    CrashMutedImpulse,
}

/// Complete pre-allocated Two-Stage Cymbal & Hi-Hat DSP state owned by a [`PolyVoice`].
#[derive(Clone, Debug)]
pub struct CymbalEngineState {
    pub dirty: bool,
    exc_kind: CymbalExcitationKind,
    use_dispersion: bool,
    serial_mode: bool,
    /// Excitation oscillator / noise envelope state
    exc_phase1: f32,
    exc_phase2: f32,
    exc_base_inc: f32,
    exc_ratio2: f32,
    exc_pitch_env: f32,
    exc_pitch_decay: f32,
    exc_pitch_span: f32,
    exc_amp_env: f32,
    exc_amp_decay: f32,
    exc_atk_env: f32,
    exc_atk_inc: f32,
    exc_bell_env: f32,
    exc_bell_decay: f32,
    exc_bell_gain: f32,
    exc_noise_env: f32,
    exc_noise_decay: f32,
    exc_noise_gain: f32,
    exc_osc_gain: f32,
    exc_fm_index: f32,
    exc_hp_x1: f32,
    exc_hp_y1: f32,
    exc_lp_z1: f32,
    exc_lp_alpha: f32,
    direct_stick_mix: f32,
    /// Optional plate-energy-coupled 6–18 kHz Air Sizzle + Open Hat (46) plate-contact bloom state
    air_sizzle_amount: f32,
    open_hat_contact_sizzle: f32,
    bloom_ramp_env: f32,
    bloom_ramp_inc: f32,
    air_hp_x1: f32,
    air_hp_y1: f32,
    /// Stage 1: Self-Feeding Dispersion Loop
    pub dispersion: DispersionLoop,
    /// Stage 2: Bank A (4 Fundamental Combs) + Bank B (8 Shimmer Combs)
    bank_a_bufs: Box<[[f32; COMB_A_LEN]; BANK_A_COMBS]>,
    bank_b_bufs: Box<[[f32; COMB_B_LEN]; BANK_B_COMBS]>,
    bank_a_meta: [WetCombMeta; BANK_A_COMBS],
    bank_b_meta: [WetCombMeta; BANK_B_COMBS],
    comb_lp_alpha: f32,
    /// Dynamic choke feedback multiplier (`1.0` normal; rapidly ramps down on Closed/Pedal choke).
    pub comb_fb_scale: f32,
    comb_choke_ramp: f32,
    comb_energy_env: f32,
    /// Post-comb Hilbert Frequency Shifter + Static Polish Comb
    pub hilbert: HilbertFreqShifter,
    pub polish: PolishComb,
    quiet_samples: u32,
}

impl CymbalEngineState {
    pub fn new() -> Self {
        Self {
            dirty: false,
            exc_kind: CymbalExcitationKind::HiHatFastPath,
            use_dispersion: false,
            serial_mode: false,
            exc_phase1: 0.0,
            exc_phase2: 0.0,
            exc_base_inc: 0.01,
            exc_ratio2: 2.41,
            exc_pitch_env: 0.0,
            exc_pitch_decay: 0.99,
            exc_pitch_span: 1.5,
            exc_amp_env: 0.0,
            exc_amp_decay: 0.98,
            exc_atk_env: 1.0,
            exc_atk_inc: 1.0,
            exc_bell_env: 0.0,
            exc_bell_decay: 0.998,
            exc_bell_gain: 0.0,
            exc_noise_env: 0.0,
            exc_noise_decay: 0.99,
            exc_noise_gain: 0.0,
            exc_osc_gain: 0.0,
            exc_fm_index: 1.0,
            exc_hp_x1: 0.0,
            exc_hp_y1: 0.0,
            exc_lp_z1: 0.0,
            exc_lp_alpha: 0.4,
            direct_stick_mix: 0.0,
            air_sizzle_amount: 0.0,
            open_hat_contact_sizzle: 0.0,
            bloom_ramp_env: 0.0,
            bloom_ramp_inc: 0.001,
            air_hp_x1: 0.0,
            air_hp_y1: 0.0,
            dispersion: DispersionLoop::new(),
            bank_a_bufs: alloc_box_2d::<BANK_A_COMBS, COMB_A_LEN>(),
            bank_b_bufs: alloc_box_2d::<BANK_B_COMBS, COMB_B_LEN>(),
            bank_a_meta: [WetCombMeta::default(); BANK_A_COMBS],
            bank_b_meta: [WetCombMeta::default(); BANK_B_COMBS],
            comb_lp_alpha: 0.5,
            comb_fb_scale: 1.0,
            comb_choke_ramp: 1.0,
            comb_energy_env: 0.0,
            hilbert: HilbertFreqShifter::default(),
            polish: PolishComb::default(),
            quiet_samples: 0,
        }
    }

    #[inline(always)]
    pub fn reset_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.dispersion.reset();
        for b in self.bank_a_bufs.iter_mut() {
            b.fill(0.0);
        }
        for b in self.bank_b_bufs.iter_mut() {
            b.fill(0.0);
        }
        self.bank_a_meta = [WetCombMeta::default(); BANK_A_COMBS];
        self.bank_b_meta = [WetCombMeta::default(); BANK_B_COMBS];
        self.hilbert.reset();
        self.polish.reset();
        self.exc_phase1 = 0.0;
        self.exc_phase2 = 0.0;
        self.exc_pitch_env = 0.0;
        self.exc_amp_env = 0.0;
        self.exc_atk_env = 1.0;
        self.exc_bell_env = 0.0;
        self.exc_noise_env = 0.0;
        self.exc_hp_x1 = 0.0;
        self.exc_hp_y1 = 0.0;
        self.exc_lp_z1 = 0.0;
        self.air_sizzle_amount = 0.0;
        self.open_hat_contact_sizzle = 0.0;
        self.bloom_ramp_env = 0.0;
        self.air_hp_x1 = 0.0;
        self.air_hp_y1 = 0.0;
        self.comb_fb_scale = 1.0;
        self.comb_choke_ramp = 1.0;
        self.comb_energy_env = 0.0;
        self.quiet_samples = 0;
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn configure_comb_banks(
        &mut self,
        sample_rate: f32,
        fundamental_cluster_hz: f32,
        bronze_size_pitch: f32,
        shimmer_spread: f32,
        decay_t60_sec: f32,
        comb_damping_hz: f32,
        exc_window_sec: f32,
        serial_mode: bool,
    ) {
        let sr = sample_rate.max(8_000.0);
        let size_pitch = bronze_size_pitch.clamp(0.30, 3.0);
        let spread = shimmer_spread.clamp(0.25, 3.0);
        let t60 = decay_t60_sec.clamp(0.020, 9.0);
        let exc_samples = (exc_window_sec.clamp(0.002, 0.12) * sr).max(8.0);
        self.serial_mode = serial_mode;

        // Delay-compensated internal 1-pole low-pass material damping ONLY inside all 12 comb loops:
        // Scales `(1 - lp_alpha)` proportionally to `(d / d_ref)^0.82` so high-frequency Bank B shimmer
        // combs (2–6.8 kHz) sustain 6–18 kHz bronze partials at the same physical dB/sec rate as Bank A
        // instead of dying 12x faster from 5,000 round-trips/sec!
        let damp_hz = comb_damping_hz.clamp(900.0, sr * 0.47);
        let hf_loss_ref = ((-TAU * damp_hz / sr).exp()).powf(1.65);
        let d_ref = (0.0025 * sr).max(16.0);
        // `comb_lp_alpha` acts as dynamic choke damping multiplier (`1.0` normal, `0.55` on choke)
        self.comb_lp_alpha = 1.0;
        self.comb_fb_scale = 1.0;
        self.comb_choke_ramp = 1.0;

        // Bank A: 4 tightly clustered whole-plate fundamental combs (Thiran all-pass fractional delay)
        const CLUSTER_RATIOS: [f32; BANK_A_COMBS] = [0.931, 0.979, 1.027, 1.083];
        let f_cluster = (fundamental_cluster_hz * size_pitch).clamp(75.0, sr * 0.24);
        let max_a = (COMB_A_LEN - 6) as f32;
        for i in 0..BANK_A_COMBS {
            let fi = (f_cluster * CLUSTER_RATIOS[i]).clamp(60.0, sr * 0.30);
            let d = (sr / fi).clamp(6.0, max_a);
            let fb = 10.0_f32
                .powf((-3.0 * (d / sr) / t60).max(-2.5))
                .clamp(0.18, 0.9985);
            let n_ov = (exc_samples / d).clamp(1.0, 20.0);
            let in_norm = 0.44 / n_ov.sqrt();
            let p_i = (hf_loss_ref * (d / d_ref).powf(0.82)).clamp(0.002, 0.96);
            self.bank_a_meta[i] = WetCombMeta::from_delay(d, fb, in_norm, 1.0 - p_i);
        }

        // Bank B: 8 non-uniform higher-order shimmer combs spanning up to ~6.8 kHz fundamentals
        // so Thiran-interpolated upper harmonics + Hilbert shift densely populate 6–18 kHz!
        const SHIMMER_BASE: [f32; BANK_B_COMBS] =
            [1.000, 1.272, 1.618, 2.058, 2.618, 3.330, 4.236, 5.388];
        let f_shimmer_anchor = (f_cluster * 2.55).clamp(580.0, sr * 0.30);
        let spread_exp = 0.52 + 0.42 * spread;
        let max_b = (COMB_B_LEN - 6) as f32;
        for j in 0..BANK_B_COMBS {
            let fj = (f_shimmer_anchor * SHIMMER_BASE[j].powf(spread_exp)).clamp(420.0, sr * 0.43);
            let d = (sr / fj).clamp(3.2, max_b);
            let mode_t60 = if serial_mode && j < 4 {
                (t60 * 0.22).clamp(0.015, 0.24)
            } else {
                (t60 * (0.94 - 0.025 * (j as f32))).max(0.018)
            };
            let fb = 10.0_f32
                .powf((-3.0 * (d / sr) / mode_t60).max(-2.5))
                .clamp(0.15, 0.9988);
            let n_ov = (exc_samples / d).clamp(1.0, 20.0);
            let in_norm = if serial_mode && j < 4 {
                0.68 / n_ov.powf(0.42)
            } else {
                0.52 / n_ov.sqrt()
            };
            let p_j = (hf_loss_ref * (d / d_ref).powf(0.82)).clamp(0.0015, 0.95);
            self.bank_b_meta[j] = WetCombMeta::from_delay(d, fb, in_norm, 1.0 - p_j);
        }
    }

    #[inline(always)]
    pub fn trigger_choke(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(8_000.0);
        // Immediate step reduction + rapid ~3.5 ms feedback ramp down on Closed/Pedal choke
        self.comb_fb_scale = self.comb_fb_scale.min(0.68);
        self.comb_choke_ramp = (-1.0 / (0.0035 * sr)).exp();
        self.comb_lp_alpha = (self.comb_lp_alpha * 0.55).max(0.08);
        self.open_hat_contact_sizzle = 0.0;
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

    /// Configures the resonant mode as a steady-state unity-gain bandpass filter (`0 dB` peak at `freq_hz`).
    ///
    /// Unlike [`Self::configure`] (which normalizes for a 1-sample unit-area impulse and has high
    /// steady-state gain on continuous signals), Cytomic TPT SVF bandpass output `v1` has steady-state
    /// peak amplitude `1 / k = Q`, so setting `self.gain = gain * k` yields exact unity peak gain
    /// across all frequencies and sample rates.
    #[inline(always)]
    pub fn configure_bandpass_unity(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain: f32) {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(20.0, sr * 0.42);
        let g = (PI * f / sr).tan().clamp(1.0e-4, 1.65);
        let k = (1.0 / q.clamp(0.5, 25.0)).clamp(0.04, 2.0);
        self.g_base = g;
        self.k = k;
        self.gain = gain * k;
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

    /// Smoothly absorbs stored modal energy (`ic1eq`, `ic2eq`) by `retain` (`0.0..1.0`) per sample
    /// during physical hand/stick damping without jumping filter coefficient `k` (zero clicks/pops).
    #[inline(always)]
    pub fn absorb_energy(&mut self, retain: f32) {
        self.ic1eq = flush_denormal(self.ic1eq * retain);
        self.ic2eq = flush_denormal(self.ic2eq * retain);
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
        // Use steady-state unity-gain bandpass normalization because the shell cavity is driven
        // continuously by the ringing membrane waveform rather than a 1-sample impulse.
        let cavity_q = (cavity_hz.max(25.0) * cavity_t60.max(0.02) / 2.1988).clamp(2.2, 4.8);
        self.cavity_mode.configure_bandpass_unity(sr, cavity_hz, cavity_q, 0.90);

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

        let cav = self.cavity_mode.process(x * 0.80 + damped * 0.25, 1.0);
        flush_denormal(cav * 0.72 + damped * 0.58)
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
    /// Tuned metallic / hardwood modal percussion (Cowbell, Agogo, Triangle, Claves, Wood Blocks).
    ModalPerc,
    /// Acoustic shaker, ratchet, whistle, or friction-drum voice keyed by GM percussion note.
    ShakerOrEffect(u8),
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
    /// Post-saturation linear output volume multiplier derived from the drum instrument's `volume_db` (`0.0` = muted).
    pub volume_scale: f32,
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

    // --- Two-Stage Dispersive Feedback + 12 Parallel Wet-Only Inharmonic Comb Cymbal Engine ---
    pub cymbal: Box<CymbalEngineState>,

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
    pub fm_index_base: f32,
    pub fm_index_env_scale: f32,
    pub fm_feedback: f32,
    pub fm_last_mod: f32,

    // --- 4-Stage Hand-Gesture ADSR & Auxiliary Shaker State (Cabasa, Guiro, Maracas, Whistle) ---
    gesture_atk_end: u32,
    gesture_dec_end: u32,
    gesture_sus_end: u32,
    gesture_rel_end: u32,
    gesture_sustain_lvl: f32,
    shaker_aux_amount: f32,

    // --- Smooth Click-Free Physical Choke / Hand-Damping State ---
    pub is_choking: bool,
    pub choke_env: f32,
    pub choke_decay: f32,

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
            volume_scale: 1.0,
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
            cymbal: Box::new(CymbalEngineState::new()),
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
            fm_index_base: 0.5,
            fm_index_env_scale: 1.5,
            fm_feedback: 0.0,
            fm_last_mod: 0.0,
            gesture_atk_end: 0,
            gesture_dec_end: 0,
            gesture_sus_end: 0,
            gesture_rel_end: 0,
            gesture_sustain_lvl: 0.8,
            shaker_aux_amount: 0.0,
            is_choking: false,
            choke_env: 1.0,
            choke_decay: 0.996,
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
        self.volume_scale = 1.0;
        self.amp_env = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.click_env = 0.0;
        self.pitch_env = 0.0;
        self.head_energy_env = 0.0;
        self.wire_env = 0.0;
        self.fm_last_mod = 0.0;
        self.gesture_atk_end = 0;
        self.gesture_dec_end = 0;
        self.gesture_sus_end = 0;
        self.gesture_rel_end = 0;
        self.gesture_sustain_lvl = 0.8;
        self.shaker_aux_amount = 0.0;
        self.is_choking = false;
        self.choke_env = 1.0;
        self.choke_decay = 0.996;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = 0;
        self.exc_lp_z1 = 0.0;
        for m in &mut self.modes {
            m.reset();
        }
        self.shell.reset();
        self.wire_comb_buf.fill(0.0);
        self.wire_comb_idx = 0;
        self.wire_lp_z1 = 0.0;
        self.cymbal.reset_if_dirty();
        self.filter1.reset();
        self.filter2.reset();
    }

    /// Smoothly damps an active physical model (for choke groups and mono/one-shot re-triggers)
    /// without hard-cutting resonator states or jumping SVF coefficients in a single sample (zero clicks/pops).
    #[inline(always)]
    pub fn choke(&mut self, sample_rate: f32) {
        if self.is_active() {
            let sr = sample_rate.max(8_000.0);
            // Smooth ~5.5 ms physical damping time constant (-60 dB in ~38 ms, 100% click-free)
            let damp_per_sample = (-1.0 / (0.0055 * sr)).exp();
            self.is_choking = true;
            self.choke_decay = damp_per_sample;
            self.fast_decay = self.fast_decay.min(damp_per_sample);
            self.slow_decay = self.slow_decay.min(damp_per_sample);
            self.is_open_hat = false;
            if self.mode == PolyVoiceMode::HatOrCymbal {
                self.cymbal.trigger_choke(sr);
            }
        }
    }

    /// Configures a 4-stage hand-moved ADSR contour (smooth attack -> fast decay -> high sustain -> fast release)
    /// over `total_dur_sec` seconds (used for Cabasa 69 and Guiro 73/74).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn configure_hand_gesture_adsr(
        &mut self,
        sample_rate: f32,
        total_dur_sec: f32,
        atk_frac: f32,
        min_atk_sec: f32,
        max_atk_sec: f32,
        dec_frac: f32,
        sustain_level: f32,
        rel_frac: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let total_samples = (total_dur_sec.max(0.012) * sr).round().max(24.0) as u32;
        let min_atk_samples = (min_atk_sec * sr).round() as u32;
        let max_atk_samples = ((max_atk_sec * sr).round() as u32)
            .max(min_atk_samples)
            .min(total_samples / 2);
        let atk_samples = ((total_samples as f32 * atk_frac).round() as u32)
            .max(min_atk_samples)
            .min(max_atk_samples);
        let dec_samples = ((total_samples as f32 * dec_frac).round() as u32)
            .max(6)
            .min(total_samples / 5);
        let rel_samples = ((total_samples as f32 * rel_frac).round() as u32)
            .max(6)
            .min(total_samples / 5);
        let sus_samples = total_samples
            .saturating_sub(atk_samples + dec_samples + rel_samples)
            .max(6);

        self.gesture_atk_end = atk_samples;
        self.gesture_dec_end = atk_samples + dec_samples;
        self.gesture_sus_end = atk_samples + dec_samples + sus_samples;
        self.gesture_rel_end = atk_samples + dec_samples + sus_samples + rel_samples;
        self.gesture_sustain_lvl = sustain_level.clamp(0.2, 1.0);
    }

    /// Evaluates the 4-stage hand-gesture ADSR envelope at `self.age_samples`, returning `(env, finished)`.
    #[inline(always)]
    fn eval_hand_gesture_adsr(&self) -> (f32, bool) {
        let age = self.age_samples;
        if age <= self.gesture_atk_end {
            let u = (age as f32) / (self.gesture_atk_end.max(1) as f32);
            let env = 0.5 * (1.0 - (PI * u).cos());
            (env, false)
        } else if age <= self.gesture_dec_end {
            let span = (self.gesture_dec_end - self.gesture_atk_end).max(1) as f32;
            let u = ((age - self.gesture_atk_end) as f32) / span;
            let env = 1.0 - (1.0 - self.gesture_sustain_lvl) * u;
            (env, false)
        } else if age <= self.gesture_sus_end {
            (self.gesture_sustain_lvl, false)
        } else if age <= self.gesture_rel_end {
            let span = (self.gesture_rel_end - self.gesture_sus_end).max(1) as f32;
            let u = ((age - self.gesture_sus_end) as f32) / span;
            let env = self.gesture_sustain_lvl * (1.0 - u);
            (env, false)
        } else {
            (0.0, true)
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
        self.level = vel * if is_kick { 0.90 } else { 0.78 };

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
        self.level = vel * match gm_note {
            37 => 0.70,
            39 => 0.74,
            56 => 0.58,
            _ => 0.84,
        };
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
            self.filter1 = BiquadFilter::bandpass_peak(sr, noise_bp_hz.clamp(600.0, 4000.0), noise_bp_q);
            self.filter2 = BiquadFilter::highpass(sr, 650.0, 0.707);
            self.shell.configure(sr, 420.0, 0.06, 3_600.0);
            self.shell_mix = shell_wood_cavity.clamp(0.0, 1.0);
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

    /// Triggers a Cast B20 Bronze Hi-Hat (GM Notes 42 Closed, 44 Pedal, 46 Open) via the
    /// Fast-Path Parallel Wet-Only Inharmonic Comb + Hilbert Frequency Shifter architecture
    /// (bypassing `DispersionLoop`).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_hi_hat(
        &mut self,
        gm_note: u8,
        h: &HiHatParams,
        eff_hardness: f32,
        decay_sec: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);
        let hardness = eff_hardness.clamp(0.0, 1.5);
        let size_pitch = h.bronze_size_pitch.clamp(0.30, 3.0);

        self.clear();
        self.mode = PolyVoiceMode::HatOrCymbal;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = gm_note == 46;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * match gm_note {
            42 => 0.76,
            44 => 0.78,
            _ => 0.72,
        };

        let cym = &mut *self.cymbal;
        cym.dirty = true;
        cym.use_dispersion = false;
        cym.exc_kind = CymbalExcitationKind::HiHatFastPath;
        cym.direct_stick_mix = 0.0;
        cym.air_sizzle_amount = 0.0;
        // Open Hi-Hat (46): dynamic plate-contact sizzle/PM bloom as the two bronze plates vibrate against each other
        cym.open_hat_contact_sizzle = if gm_note == 46 {
            0.35 + 0.65 * h.nonlin_shimmer.clamp(0.0, 1.5)
        } else {
            0.0
        };
        cym.bloom_ramp_env = 0.0;
        cym.bloom_ramp_inc = 1.0 / (0.022 * sr).max(1.0);

        // Short pitch-enveloped metallic chick oscillator + shaped noise burst
        let osc_hz = (1_420.0 * size_pitch).clamp(350.0, sr * 0.30);
        cym.exc_phase1 = 0.0;
        cym.exc_phase2 = 0.0;
        cym.exc_base_inc = osc_hz * inv_sr;
        cym.exc_ratio2 = 2.63;
        cym.exc_pitch_env = 1.0;
        cym.exc_pitch_decay = (-inv_sr / 0.0022).exp();
        cym.exc_pitch_span = 1.9 + 0.8 * hardness;
        cym.exc_amp_env = 1.0;
        cym.exc_amp_decay = (-inv_sr / (0.0028 + 0.0020 * h.plate_dispersion.clamp(0.0, 1.5))).exp();
        cym.exc_fm_index = 1.35 + 1.65 * hardness;
        cym.exc_osc_gain = (0.32 + 0.85 * h.bell_level.clamp(0.0, 2.0)) * (0.55 + 0.45 * vel);
        cym.exc_bell_env = 0.0;
        cym.exc_bell_gain = 0.0;

        // Pedal Hat 44 "chick": slight ~3.5 ms clamping attack ramp + faster ~4.2 ms noise decay
        let (atk_sec, noise_tau) = match gm_note {
            44 => (0.0035, 0.0042),
            42 => (0.0004, 0.0058),
            _ => (0.0009, 0.0135),
        };
        cym.exc_atk_env = if gm_note == 44 { 0.0 } else { 0.35 };
        cym.exc_atk_inc = 1.0 / (atk_sec * sr).max(1.0);
        cym.exc_noise_env = 1.0;
        cym.exc_noise_decay = (-inv_sr / noise_tau).exp();
        cym.exc_noise_gain = (0.95 + 0.70 * h.nonlin_shimmer.clamp(0.0, 1.5)) * (0.55 + 0.45 * vel);
        let lp_cut = (3_800.0 + 13_000.0 * hardness).min(sr * 0.45);
        cym.exc_lp_alpha = (1.0 - (-TAU * lp_cut / sr).exp()).clamp(0.08, 0.96);

        let decay_t60 = (decay_sec * 2.15).clamp(0.020, 6.5);
        let exc_win = if gm_note == 46 { 0.014 } else { 0.0065 };
        cym.configure_comb_banks(
            sr,
            h.fundamental_cluster_hz,
            size_pitch,
            h.spread,
            decay_t60,
            h.comb_damping_hz,
            exc_win,
            h.serial_sizzle,
        );
        cym.hilbert.configure(sr, h.inharmonic_shift_hz);
        cym.polish.configure(sr, h.polish_comb_hz, h.polish_comb_mix);

        // Post-comb High-Pass + Mid-Cut Peaking EQ
        let hp_hz = h.highpass_cutoff_hz.clamp(400.0, sr * 0.43);
        let mid_hz = h.mid_cut_hz.clamp(250.0, sr * 0.40);
        self.filter1 = BiquadFilter::highpass(sr, hp_hz, 0.74);
        self.filter2 = BiquadFilter::peaking_eq(sr, mid_hz, 1.15, h.mid_cut_db.clamp(-24.0, 12.0));

        let clash_tau = if gm_note == 46 {
            (decay_sec * 0.26).clamp(0.035, 0.17)
        } else {
            0.018
        };
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / clash_tau).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / decay_sec.max(0.015)).exp();
    }

    /// Triggers a Cast B20 Bronze Cymbal or Ride (GM Notes 49, 51, 52, 53, 55, 57, 59)
    /// using the Two-Stage `DispersionLoop` + 12 Parallel Wet-Only Inharmonic Comb architecture.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_cymbal_two_stage(
        &mut self,
        gm_note: u8,
        bronze_size_pitch: f32,
        dispersion_size_ms: f32,
        allpass_smear: f32,
        vibrato_depth: f32,
        vibrato_chaos: f32,
        pm_drive: f32,
        pm_tone_hz: f32,
        fundamental_cluster_hz: f32,
        matrix_spread: f32,
        inharmonic_shift_hz: f32,
        comb_damping_hz: f32,
        polish_comb_hz: f32,
        polish_comb_mix: f32,
        post_hp_hz: f32,
        mid_cut_hz: f32,
        mid_cut_db: f32,
        air_sizzle: f32,
        bell_level: f32,
        eff_hardness: f32,
        decay_sec: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);
        let size_pitch = bronze_size_pitch.clamp(0.30, 3.0);
        let hardness = eff_hardness.clamp(0.0, 1.5);
        let bell = bell_level.clamp(0.0, 2.5);

        self.clear();
        self.mode = PolyVoiceMode::HatOrCymbal;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * match gm_note {
            53 => 0.72,
            51 | 59 => 0.74,
            _ => 0.78,
        };

        let is_ride = matches!(gm_note, 51 | 53 | 59);
        let decay_t60 = (decay_sec * 2.15).clamp(0.06, 9.0);

        let cym = &mut *self.cymbal;
        cym.dirty = true;
        cym.use_dispersion = true;
        cym.exc_phase1 = 0.0;
        cym.exc_phase2 = 0.0;
        cym.exc_atk_env = 1.0;
        cym.exc_atk_inc = 1.0;
        cym.air_sizzle_amount = air_sizzle.clamp(0.0, 1.25) * (0.45 + 0.55 * vel);
        cym.open_hat_contact_sizzle = 0.0;
        cym.bloom_ramp_env = 0.0;
        let bloom_ms = if is_ride { 20.0 } else { 42.0 };
        cym.bloom_ramp_inc = 1.0 / ((bloom_ms * 0.001) * sr).max(1.0);

        if is_ride {
            // Ride (51 Bow, 53 Bell, 59 Ride 2):
            // Short, bright pitch-enveloped sine/FM stick click -> feeds DispersionLoop AND
            // mixes lightly into master output for stick attack (plus singing bell ping on 53).
            cym.exc_kind = CymbalExcitationKind::RideStickAndBell;
            let base_hz = if gm_note == 53 {
                1_180.0 * size_pitch
            } else if gm_note == 59 {
                820.0 * size_pitch
            } else {
                910.0 * size_pitch
            }
            .clamp(240.0, sr * 0.34);
            cym.exc_base_inc = base_hz * inv_sr;
            cym.exc_ratio2 = if gm_note == 53 { 2.72 } else { 2.41 };
            cym.exc_pitch_env = 1.0;
            cym.exc_pitch_decay = (-inv_sr / 0.0016).exp();
            cym.exc_pitch_span = if gm_note == 53 { 1.75 } else { 2.25 } + 0.65 * hardness;
            cym.exc_amp_env = 1.0;
            let click_tau = if gm_note == 53 { 0.0042 } else { 0.0028 };
            cym.exc_amp_decay = (-inv_sr / click_tau).exp();
            cym.exc_fm_index = (if gm_note == 53 { 1.35 } else { 0.85 }) + 1.75 * hardness;
            cym.exc_osc_gain = 1.20 * (0.55 + 0.45 * vel);

            // Singing bronze bell cup component (strongest on Ride Bell 53, subtle on Bow 51/59)
            cym.exc_bell_env = 1.0;
            let bell_tau = if gm_note == 53 { 0.038 } else { 0.014 };
            cym.exc_bell_decay = (-inv_sr / bell_tau).exp();
            cym.exc_bell_gain = if gm_note == 53 {
                0.48 + 0.68 * bell
            } else {
                0.14 * bell
            };
            cym.exc_noise_env = 1.0;
            cym.exc_noise_decay = (-inv_sr / 0.0015).exp();
            cym.exc_noise_gain = 0.24 * (0.4 + 0.6 * hardness);
            let lp_cut = (4_200.0 + 13_500.0 * hardness).min(sr * 0.45);
            cym.exc_lp_alpha = (1.0 - (-TAU * lp_cut / sr).exp()).clamp(0.08, 0.96);

            // Lightly mixed into master output for crisp ride stick definition
            cym.direct_stick_mix = if gm_note == 53 {
                0.30 * (0.45 + 0.55 * hardness)
            } else {
                0.18 * (0.40 + 0.60 * hardness)
            };
        } else {
            // Crash / Splash / China (49, 57, 55, 52):
            // Energetic low-to-mid saw/triangle flex impulse -> feeds DispersionLoop ONLY
            // (muted from master output; multi-radian Self-PM + 12-stage disperser folds it into 6–18 kHz sizzle!).
            cym.exc_kind = CymbalExcitationKind::CrashMutedImpulse;
            let base_low_hz = match gm_note {
                55 => 215.0, // 10" Splash
                52 => 160.0, // 18" China
                _ => 170.0,  // 16"-18" Crash
            } * size_pitch * (0.80 + 0.48 * hardness);
            cym.exc_base_inc = base_low_hz.clamp(55.0, sr * 0.22) * inv_sr;
            cym.exc_ratio2 = 1.618;
            cym.exc_pitch_env = 1.0;
            cym.exc_pitch_decay = (-inv_sr / 0.0042).exp();
            cym.exc_pitch_span = 1.85 + 0.85 * hardness;
            cym.exc_amp_env = 1.0;
            let imp_tau = match gm_note {
                55 => 0.0048,
                52 => 0.0072,
                _ => 0.0066,
            };
            cym.exc_amp_decay = (-inv_sr / imp_tau).exp();
            cym.exc_osc_gain = 1.55 * (0.60 + 0.40 * vel) * (0.85 + 0.30 * bell);
            cym.exc_bell_env = 0.0;
            cym.exc_bell_gain = 0.0;
            cym.exc_noise_env = 1.0;
            cym.exc_noise_decay = (-inv_sr / 0.0028).exp();
            cym.exc_noise_gain = 0.05 * hardness;
            // Warm <2 kHz initial bronze bow flex impulse so 6–18 kHz sizzle blooms genuinely via Stage 1 Self-PM!
            let lp_cut = (680.0 + 1_350.0 * hardness).min(sr * 0.35);
            cym.exc_lp_alpha = (1.0 - (-TAU * lp_cut / sr).exp()).clamp(0.04, 0.85);
            cym.direct_stick_mix = 0.0;
        }

        // Stage 1: Self-Feeding Dispersion Loop (12-stage AP + Hermite Vibrato + Multi-Radian Self-PM)
        cym.dispersion.configure(
            sr,
            dispersion_size_ms,
            size_pitch,
            decay_t60,
            allpass_smear,
            vibrato_depth,
            vibrato_chaos,
            pm_drive,
            pm_tone_hz,
            vel,
            is_ride,
        );

        // Stage 2: 12 Independent Parallel Wet-Only Combs (Thiran all-pass) + Hilbert Frequency Shifter
        cym.configure_comb_banks(
            sr,
            fundamental_cluster_hz,
            size_pitch,
            matrix_spread,
            decay_t60,
            comb_damping_hz,
            0.036,
            false,
        );
        cym.hilbert.configure(sr, inharmonic_shift_hz);
        cym.polish.configure(sr, polish_comb_hz, polish_comb_mix);

        // Post-comb High-Pass + Mid-Cut Peaking EQ
        let hp_hz = post_hp_hz.clamp(250.0, sr * 0.43);
        let mid_hz = mid_cut_hz.clamp(200.0, sr * 0.40);
        self.filter1 = BiquadFilter::highpass(sr, hp_hz, 0.72);
        self.filter2 = BiquadFilter::peaking_eq(sr, mid_hz, 1.15, mid_cut_db.clamp(-24.0, 12.0));

        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / 0.065).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / decay_sec.max(0.040)).exp();
    }

    /// Triggers a tuned metallic or hardwood modal percussion voice
    /// (Cowbell 56, Agogo 67/68, Triangle 80/81, Claves 75, Wood Blocks 76/77).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn trigger_modal_perc(
        &mut self,
        gm_note: u8,
        freq1_hz: f32,
        freq2_hz: f32,
        bandpass_q: f32,
        decay_sec: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.clear();
        self.mode = PolyVoiceMode::ModalPerc;
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * match gm_note {
            80 | 81 => 0.48, // Steel triangle: high-Q modes carry strongly
            75 => 0.62,      // Claves
            76 | 77 => 0.68, // Wood blocks
            67 | 68 => 0.56, // Agogo bells
            _ => 0.58,       // Cowbell
        };

        let f1 = freq1_hz.clamp(60.0, sr * 0.43);
        let f2 = freq2_hz.clamp(80.0, sr * 0.44);
        let t60 = (decay_sec * 2.8).clamp(0.012, 8.0);

        // Configure 6 resonant modes matched to instrument acoustics (steel bell/bar vs. hollow wood)
        let (ratios, gains, decay_scales): ([f32; NUM_MODES], [f32; NUM_MODES], [f32; NUM_MODES]) =
            match gm_note {
                80 | 81 => (
                    // Steel triangle: bright inharmonic bar partials
                    [1.0, f2 / f1, 2.26, 3.18, 4.24, 5.42],
                    [0.75, 0.85, 0.62, 0.52, 0.36, 0.24],
                    [1.0, 0.94, 0.78, 0.64, 0.48, 0.34],
                ),
                75 => (
                    // Rosewood claves: tight resonant primary wood cavity + brief crisp click overtones
                    [1.0, f2 / f1, 1.94, 2.75, 3.40, 4.10],
                    [1.00, 0.46, 0.22, 0.14, 0.08, 0.05],
                    [1.0, 0.55, 0.32, 0.22, 0.16, 0.12],
                ),
                76 | 77 => (
                    // Hollow hardwood wood blocks: warm cavity fundamental + woody upper resonance
                    [1.0, f2 / f1, 2.42, 3.18, 4.05, 4.90],
                    [1.00, 0.58, 0.28, 0.16, 0.10, 0.06],
                    [1.0, 0.68, 0.38, 0.24, 0.18, 0.12],
                ),
                67 | 68 => (
                    // Agogo bells: singing open cone-bell harmonics
                    [1.0, f2 / f1, 2.15, 2.96, 3.85, 4.72],
                    [1.00, 0.82, 0.44, 0.28, 0.18, 0.12],
                    [1.0, 0.88, 0.58, 0.42, 0.30, 0.22],
                ),
                _ => (
                    // Cowbell: classic two-mode damped steel bell + upper metallic strike overtones
                    [1.0, f2 / f1, 1.85, 2.46, 3.45, 4.65],
                    [1.00, 0.94, 0.28, 0.20, 0.16, 0.12],
                    [1.0, 0.92, 0.35, 0.24, 0.14, 0.08],
                ),
            };

        for i in 0..NUM_MODES {
            let mf = (f1 * ratios[i]).clamp(60.0, sr * 0.43);
            let mt60 = (t60 * decay_scales[i]).clamp(0.008, 8.0);
            self.modes[i].configure(sr, mf, mt60, gains[i]);
            self.mode_exc_weights[i] = 1.0;
        }

        // Crisp hard-stick / steel-beater contact pulse
        let pulse_ms = match gm_note {
            80 | 81 => 0.22,
            56 => 0.32,
            75 | 76 | 77 => 0.35,
            _ => 0.42,
        };
        let pulse_samples = ((pulse_ms * 0.001) * sr).round().clamp(3.0, 96.0) as u32;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = pulse_samples;
        self.exc_area_norm = (2.0 / (pulse_samples as f32)) * (0.55 + 0.45 * vel);
        self.exc_click_mix = if gm_note == 56 { 0.90 } else { 0.55 };
        self.exc_lp_z1 = 0.0;
        let stick_cut = if matches!(gm_note, 56 | 80 | 81) {
            14_000.0_f32
        } else {
            7_500.0_f32
        }
        .min(sr * 0.44);
        self.exc_lp_coeff = (-TAU * stick_cut / sr).exp().clamp(0.04, 0.90);

        let bp_center = (0.5 * (f1 + f2)).clamp(100.0, sr * 0.42);
        self.filter1 = BiquadFilter::bandpass_peak(sr, bp_center, bandpass_q.clamp(0.25, 25.0));
        // High-end metallic strike transient filter (especially for Cowbell 56 right on strike, dying off immediately)
        let hp_transient_hz = if gm_note == 56 {
            3_600.0_f32
        } else {
            2_400.0_f32
        }
        .min(sr * 0.40);
        self.filter2 = BiquadFilter::highpass(sr, hp_transient_hz, 0.90);
        self.click_env = 1.0;
        // Immediate ~1.4 ms time constant so the high-end metallic strike snap dies off in ~3 ms
        let click_tau = if gm_note == 56 { 0.0014 } else { 0.0010 };
        self.click_decay = (-inv_sr / click_tau).exp();

        self.shell.configure(
            sr,
            (f1 * 0.92).clamp(90.0, sr * 0.38),
            (decay_sec * 0.6).clamp(0.015, 0.6),
            4_500.0,
        );
        self.shell_mix = if matches!(gm_note, 75 | 76 | 77) { 0.45 } else { 0.18 };

        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / decay_sec.max(0.008)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / (decay_sec * 1.4).max(0.012)).exp();
    }

    /// Triggers an acoustic shaker, ratchet, whistle, or friction-drum voice
    /// (Tambourine 54, Vibraslap 58, Cabasa 69, Maracas 70, Whistle 71/72, Guiro 73/74, Cuica 78/79).
    #[inline(always)]
    pub fn trigger_shaker_or_effect(
        &mut self,
        gm_note: u8,
        params: &ShakerAndEffectParams,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);
        let pitch = params.pitch_scale.clamp(0.20, 4.0);
        let bright = params.brightness_hz.clamp(120.0, sr * 0.43);
        let tex = params.texture_rate.clamp(0.0, 2.0);
        let q = params.resonance_q.clamp(0.25, 25.0);
        let aux_decay_sec = (params.aux_decay_ms * 0.001).clamp(0.005, 4.0);
        let aux_amt = params.aux_amount.clamp(0.0, 2.0);

        self.clear();
        self.mode = PolyVoiceMode::ShakerOrEffect(gm_note);
        self.sample_rate = sr;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.shaker_aux_amount = aux_amt;

        // Note-specific duration scaling:
        // - Guiro Short (73) uses `aux_decay_ms` independently from Guiro Long (74) (`decay_ms`)!
        // - Whistle Short/High (71) uses `aux_decay_ms` independently from Whistle Long/Low (72) (`decay_ms`)!
        // - Cuica Mute (78) scales from `decay_ms`.
        let decay_sec = match gm_note {
            73 => aux_decay_sec.clamp(0.008, 4.0),
            74 => (params.decay_ms * 0.001).clamp(0.015, 4.0),
            71 => aux_decay_sec.clamp(0.008, 4.0),
            72 => (params.decay_ms * 0.001).clamp(0.012, 4.0),
            78 => ((params.decay_ms * 0.001) * 0.52).clamp(0.010, 4.0),
            _ => (params.decay_ms * 0.001).clamp(0.008, 4.0),
        };

        self.level = vel * match gm_note {
            71 | 72 => 0.44, // Samba whistle cuts loudly
            78 | 79 => 0.66, // Cuica
            58 => 0.72,      // Vibraslap
            73 | 74 => 0.72, // Guiro
            54 => 0.68,      // Tambourine
            70 => 2.15,      // Maracas (linear post-tanh level; pre-tanh drive kept strictly < 0.55 so zero clipping)
            _ => 0.66,       // Cabasa
        };

        self.in_attack = true;
        self.amp_env = 0.0;
        let atk_sec = match gm_note {
            70 => 0.012,     // Maracas: seed throw ramp-in
            71 | 72 => 0.009,
            78 | 79 => 0.012,
            _ => 0.0025,
        };
        self.amp_attack_inc = 1.0 / (atk_sec * sr).max(1.0);
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / (decay_sec * 0.35).max(0.008)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / decay_sec).exp();
        self.pitch_env = 1.0;
        self.pitch_decay = (-inv_sr / (decay_sec * 0.55).max(0.012)).exp();
        self.body_mix = tex;

        match gm_note {
            54 => {
                // Tambourine:
                // 1. Slower jingle flutter rate range (3.5 Hz at tex=0 up to ~40+ Hz)
                // 2. Tunable drumhead decay length via `aux_decay_ms` on modes[0]
                // 3. 5 high-Q inharmonic stamped-metal zil resonators on modes[1..6] so jingles
                //    sound genuinely metallic rather than bandpassed white noise.
                let jingle_hz = (3.5 + 36.0 * tex) * pitch.clamp(0.25, 3.0);
                self.base_inc = jingle_hz * inv_sr;
                let zil_base = (bright * pitch).clamp(1_600.0, sr * 0.38);
                self.filter1 = BiquadFilter::bandpass_peak(sr, zil_base, q.clamp(0.8, 16.0));
                self.filter2 = BiquadFilter::highpass(sr, (zil_base * 0.72).clamp(2_400.0, sr * 0.40), 0.85);

                // modes[0]: Tambourine frame drumhead with dedicated user decay `aux_decay_ms`
                let head_t60 = aux_decay_sec.clamp(0.005, 2.5);
                self.modes[0].configure(sr, (230.0_f32 * pitch).clamp(80.0, 900.0), head_t60, 0.75);
                // Track drumhead decay in fast_decay so long drumhead settings never cut off early
                self.fast_decay = (-inv_sr / (head_t60 * 0.55).max(0.005)).exp();

                // modes[1..6]: 5 high-Q inharmonic stamped steel/brass zil pair modes
                const ZIL_RATIOS: [f32; 5] = [0.74, 1.00, 1.31, 1.69, 2.14];
                const ZIL_GAINS: [f32; 5] = [0.42, 0.54, 0.48, 0.38, 0.30];
                let zil_t60 = (decay_sec * (0.48 + 0.10 * q.clamp(0.5, 12.0))).clamp(0.025, 0.65);
                for idx in 0..5 {
                    let zf = (zil_base * ZIL_RATIOS[idx]).clamp(1_400.0, sr * 0.43);
                    let zt = (zil_t60 * (1.0 - 0.11 * (idx as f32))).clamp(0.018, 0.65);
                    self.modes[idx + 1].configure(sr, zf, zt, ZIL_GAINS[idx]);
                }

                self.exc_samples_elapsed = 0;
                self.exc_total_samples = ((0.0009 * sr).round().clamp(4.0, 64.0)) as u32;
                self.exc_area_norm = 2.0 / (self.exc_total_samples as f32);
                self.exc_click_mix = 0.45;
                self.exc_lp_coeff = 0.35;
            }
            69 => {
                // Cabasa (69):
                // - Moved by hand, not struck: 4-stage ADSR contour (longer smooth attack ->
                //   fast decay -> high sustain plateau -> very fast release).
                // - Slower bead grain rate (6 Hz .. 42 Hz * pitch) with metallic cylinder ridge clicks.
                self.in_attack = false;
                self.amp_env = 1.0;
                self.slow_env = 1.0;
                // Keep slow_decay = 1.0 during normal gesture; choke() clamps slow_decay if choked
                self.slow_decay = 1.0;
                self.configure_hand_gesture_adsr(
                    sr,
                    decay_sec.max(0.020),
                    0.22,  // ~22% smooth hand-twist attack
                    0.010, // min 10 ms attack
                    0.055, // max 55 ms attack
                    0.13,  // fast ~13% decay after initial hand grip peak
                    0.78,  // high sustain plateau (0.78)
                    0.11,  // very fast ~11% release when hand stops
                );

                let grain_hz = (6.0 + 34.0 * tex) * pitch.clamp(0.25, 3.0);
                self.base_inc = grain_hz * inv_sr;
                let center_hz = (bright * pitch).clamp(900.0, sr * 0.40);
                self.filter1 = BiquadFilter::bandpass_peak(sr, center_hz, q.clamp(0.5, 16.0));
                self.filter2 = BiquadFilter::highpass(sr, (center_hz * 0.52).clamp(1_000.0, sr * 0.38), 0.80);
                // Subtle corrugated steel cylinder & wooden core modes (unity-normalized so continuous drive never clips)
                self.modes[0].configure_bandpass_unity(sr, (center_hz * 0.78).clamp(800.0, sr * 0.38), 5.5, 0.32);
                self.modes[1].configure_bandpass_unity(sr, (center_hz * 1.24).clamp(1_200.0, sr * 0.40), 6.5, 0.24);
            }
            70 => {
                // Maracas (70):
                // - Strong hollow small wooden shell / gourd resonance (ShellResonator + 3 unity-normalized wood cavity modes)
                // - Random loose beads flying around inside the body controlled by `aux_amount` fader
                let grain_hz = (14.0 + 34.0 * tex) * pitch.clamp(0.25, 3.0);
                self.base_inc = grain_hz * inv_sr;
                let seed_hz = (bright * pitch.clamp(0.25, 2.2)).clamp(1_200.0, (sr * 0.35).min(13_500.0));
                self.filter1 = BiquadFilter::bandpass_peak(sr, seed_hz, q.clamp(0.5, 14.0));
                self.filter2 = BiquadFilter::highpass(sr, (seed_hz * 0.42).clamp(900.0, sr * 0.34), 0.72);

                // Small hollow wooden maraca shell cavity & hardwood wall modes:
                // Use `configure_bandpass_unity` so continuous seed/bead excitation never overloads or clips!
                let eff_wood_pitch = pitch.clamp(0.25, 2.4);
                let wood_f1 = (680.0 * eff_wood_pitch).clamp(260.0, 2_200.0);
                let wood_f2 = (1_180.0 * eff_wood_pitch).clamp(440.0, 3_400.0);
                let wood_f3 = (1_880.0 * eff_wood_pitch).clamp(700.0, 5_200.0);
                let wood_q = (2.0 + 1.5 * q.clamp(0.25, 12.0)).clamp(2.0, 14.0);
                self.modes[0].configure_bandpass_unity(sr, wood_f1, wood_q, 0.58);
                self.modes[1].configure_bandpass_unity(sr, wood_f2, wood_q * 0.9, 0.46);
                self.modes[2].configure_bandpass_unity(sr, wood_f3, wood_q * 0.8, 0.35);
                self.shell.configure(sr, (wood_f1 * 0.92).clamp(240.0, 2_000.0), 0.055, 4_200.0);

                // Extend slow_decay slightly when loose beads are active so stray beads can bounce in the wake
                let tail_mult = 1.0 + 0.85 * aux_amt;
                self.slow_decay = (-inv_sr / (decay_sec * tail_mult).max(0.012)).exp();
                self.click_env = 0.0;
                self.click_decay = (-inv_sr / 0.0008).exp();
            }
            73 | 74 => {
                // Guiro (73 Short, 74 Long):
                // - Scraped by hand: 4-stage ADSR contour with a responsive smooth attack,
                //   fast decay, high sustain plateau (0.82), and fast release.
                // - Separate scrape length for Short Guiro (`aux_decay_ms`) vs Long Guiro (`decay_ms`).
                self.in_attack = false;
                self.amp_env = 1.0;
                self.slow_env = 1.0;
                self.slow_decay = 1.0;
                self.configure_hand_gesture_adsr(
                    sr,
                    decay_sec.max(0.012),
                    0.16,  // ~16% smooth hand-scrape attack (shorter, more responsive)
                    0.004, // min 4 ms attack for short guiro strikes
                    0.042, // max 42 ms attack cap so long guiro scrapes don't lag
                    0.10,  // fast ~10% decay into high sustain
                    0.82,  // high sustain plateau (0.82)
                    0.10,  // fast ~10% release at end of scrape
                );

                let ratchet_hz = (12.0 + 46.0 * tex) * pitch.clamp(0.25, 3.0);
                self.base_inc = ratchet_hz * inv_sr;
                self.filter1 = BiquadFilter::bandpass_peak(sr, (bright * pitch).clamp(350.0, sr * 0.40), q.clamp(0.8, 20.0));
                self.filter2 = BiquadFilter::highpass(sr, 420.0, 0.72);
                self.shell.configure(sr, (bright * 0.42 * pitch).clamp(180.0, 2_200.0), 0.085, 4_200.0);
            }
            58 => {
                // Vibraslap: spring strike + decaying wood-box rattle
                let rattle_hz = (16.0 + 36.0 * tex) * pitch;
                self.base_inc = rattle_hz * inv_sr;
                self.filter1 = BiquadFilter::bandpass_peak(sr, (bright * pitch).clamp(350.0, sr * 0.40), q.clamp(0.8, 16.0));
                self.filter2 = BiquadFilter::highpass(sr, 340.0, 0.75);
                self.modes[0].configure(sr, (480.0_f32 * pitch).clamp(140.0, 2_200.0), 0.14, 0.75);
                self.shell.configure(sr, (bright * 0.55 * pitch).clamp(220.0, 2_600.0), 0.12, 3_600.0);
                self.exc_samples_elapsed = 0;
                self.exc_total_samples = ((0.0012 * sr).round().clamp(4.0, 80.0)) as u32;
                self.exc_area_norm = 2.0 / (self.exc_total_samples as f32);
                self.exc_click_mix = 0.50;
                self.exc_lp_coeff = 0.35;
            }
            71 | 72 => {
                // Samba Whistle: pure high pea-trill whistle + breath air
                // + Velocity/accent pitch bend (`aux_amount` slider: higher pitch when louder!)
                let base_f = (bright * pitch).clamp(600.0, (sr * 0.42).min(9_000.0));
                // Accent pitch lift: louder blows shift base pitch upward proportionally to `aux_amount`
                let accent_lift = 1.0 + (vel - 0.35).max(0.0) * aux_amt * 0.18;
                let accented_f = (base_f * accent_lift).min(sr * 0.43);
                let trill_hz = 14.0 + 32.0 * tex;
                self.base_inc = accented_f * inv_sr;
                self.sec_inc = trill_hz * inv_sr;
                // Store initial overblow breath-attack pitch burst in fm_index_env_scale
                self.fm_index_env_scale = vel * aux_amt * 0.045;
                self.filter1 = BiquadFilter::bandpass_peak(sr, accented_f, q.clamp(0.8, 20.0));
            }
            _ => {
                // Cuica (78 Mute, 79 Open): pitched friction-stick drumhead resonance sweep
                let target_f = (bright * pitch).clamp(110.0, 2_200.0);
                self.base_inc = target_f * inv_sr;
                self.sec_inc = if gm_note == 78 { -0.28 } else { 0.48 } * (0.35 + 0.65 * tex);
                self.filter1 = BiquadFilter::bandpass_peak(sr, (target_f * 1.35).clamp(160.0, sr * 0.40), q.clamp(0.8, 18.0));
                self.shell.configure(sr, (target_f * 0.85).clamp(90.0, 1_400.0), 0.14, 2_800.0);
            }
        }
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
        self.volume_scale = 1.0;

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
        self.volume_scale = 1.0;

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

        // If this voice is being physically choked/damped, smoothly absorb resonator energy
        // each sample without jumping filter coefficients (100% click-free).
        if self.is_choking {
            self.choke_env = flush_denormal(self.choke_env * self.choke_decay);
            for m in &mut self.modes {
                m.absorb_energy(self.choke_decay);
            }
            if self.choke_env < 1.0e-4 {
                self.clear();
                return (0.0, 0.0);
            }
        }

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

                let body = membrane_sum * (1.0 - 0.32 * self.shell_mix)
                    + shell_out * (0.72 * self.shell_mix)
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
                    tanh_pade((shaped * env * (1.0 - 0.25 * self.shell_mix) + room * (0.65 * self.shell_mix)) * 1.35) * self.level
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

                    let head_and_shell = membrane_sum * (1.0 - 0.30 * self.shell_mix)
                        + shell_out * (0.68 * self.shell_mix)
                        + click_raw * 0.45;
                    let combined = head_and_shell * self.body_mix + wire_out * (self.noise_mix * 1.45);
                    tanh_pade(combined * 1.25) * self.level
                }
            }

            PolyVoiceMode::HatOrCymbal => {
                // Draw noise sample upfront before taking `&mut *self.cymbal`
                let wn = self.next_white_noise();
                let cym = &mut *self.cymbal;

                // Rapidly ramp comb feedback down if voice was choked (e.g. Open Hat -> Closed/Pedal Hat)
                if cym.comb_choke_ramp < 0.99999 {
                    cym.comb_fb_scale = (cym.comb_fb_scale * cym.comb_choke_ramp).max(0.18);
                }

                // 1. Excitation Generator (Hi-Hat Fast Path vs Ride Stick/Bell vs Crash Muted Impulse)
                let (exc_sig, direct_stick_out) = match cym.exc_kind {
                    CymbalExcitationKind::HiHatFastPath => {
                        if cym.exc_atk_env < 1.0 {
                            cym.exc_atk_env = (cym.exc_atk_env + cym.exc_atk_inc).min(1.0);
                        }
                        let osc = if cym.exc_amp_env > 1.0e-5 {
                            let inst_inc =
                                cym.exc_base_inc * (1.0 + cym.exc_pitch_span * cym.exc_pitch_env);
                            cym.exc_phase1 = (cym.exc_phase1 + inst_inc).fract();
                            cym.exc_phase2 =
                                (cym.exc_phase2 + inst_inc * cym.exc_ratio2).fract();
                            let mod_s = (cym.exc_phase2 * TAU).sin();
                            let fm_s =
                                (cym.exc_phase1 * TAU + mod_s * cym.exc_fm_index * cym.exc_amp_env)
                                    .sin();
                            fm_s * cym.exc_amp_env * cym.exc_osc_gain
                        } else {
                            0.0
                        };

                        let noise_burst = if cym.exc_noise_env > 1.0e-5 {
                            // 1-pole highpass + stick-hardness 1-pole lowpass shaped metallic noise burst
                            let hp_w =
                                flush_denormal(0.74 * (cym.exc_hp_y1 + wn - cym.exc_hp_x1));
                            cym.exc_hp_x1 = wn;
                            cym.exc_hp_y1 = hp_w;
                            cym.exc_lp_z1 = flush_denormal(
                                cym.exc_lp_z1 + cym.exc_lp_alpha * (hp_w - cym.exc_lp_z1),
                            );
                            cym.exc_lp_z1 * cym.exc_noise_env * cym.exc_atk_env * cym.exc_noise_gain
                        } else {
                            0.0
                        };

                        cym.exc_pitch_env = flush_denormal(cym.exc_pitch_env * cym.exc_pitch_decay);
                        cym.exc_amp_env = flush_denormal(cym.exc_amp_env * cym.exc_amp_decay);
                        cym.exc_noise_env = flush_denormal(cym.exc_noise_env * cym.exc_noise_decay);

                        (osc + noise_burst, 0.0)
                    }

                    CymbalExcitationKind::RideStickAndBell => {
                        let active_exc =
                            cym.exc_amp_env > 1.0e-5 || cym.exc_bell_env > 1.0e-5;
                        let stick_and_bell = if active_exc {
                            let inst_inc =
                                cym.exc_base_inc * (1.0 + cym.exc_pitch_span * cym.exc_pitch_env);
                            cym.exc_phase1 = (cym.exc_phase1 + inst_inc).fract();
                            cym.exc_phase2 =
                                (cym.exc_phase2 + inst_inc * cym.exc_ratio2).fract();
                            let mod_s = (cym.exc_phase2 * TAU).sin();
                            let stick_fm = (cym.exc_phase1 * TAU
                                + mod_s * cym.exc_fm_index * cym.exc_amp_env)
                                .sin()
                                * cym.exc_amp_env
                                * cym.exc_osc_gain;
                            // Singing cast-bronze bell cup ping
                            let bell_ping = (cym.exc_phase1 * TAU + mod_s * 0.85 * cym.exc_bell_env)
                                .sin()
                                * cym.exc_bell_env
                                * cym.exc_bell_gain;
                            let tip_snap = wn * cym.exc_noise_env * cym.exc_noise_gain;
                            let raw_exc = stick_fm + bell_ping + tip_snap;
                            cym.exc_lp_z1 = flush_denormal(
                                cym.exc_lp_z1 + cym.exc_lp_alpha * (raw_exc - cym.exc_lp_z1),
                            );
                            cym.exc_lp_z1
                        } else {
                            0.0
                        };

                        cym.exc_pitch_env = flush_denormal(cym.exc_pitch_env * cym.exc_pitch_decay);
                        cym.exc_amp_env = flush_denormal(cym.exc_amp_env * cym.exc_amp_decay);
                        cym.exc_bell_env = flush_denormal(cym.exc_bell_env * cym.exc_bell_decay);
                        cym.exc_noise_env = flush_denormal(cym.exc_noise_env * cym.exc_noise_decay);

                        // Feeds Stage 1 DispersionLoop AND mixes lightly into master output for stick attack
                        (stick_and_bell, stick_and_bell * cym.direct_stick_mix)
                    }

                    CymbalExcitationKind::CrashMutedImpulse => {
                        let imp = if cym.exc_amp_env > 1.0e-5 {
                            let inst_inc =
                                cym.exc_base_inc * (1.0 + cym.exc_pitch_span * cym.exc_pitch_env);
                            cym.exc_phase1 = (cym.exc_phase1 + inst_inc).fract();
                            cym.exc_phase2 =
                                (cym.exc_phase2 + inst_inc * cym.exc_ratio2).fract();
                            // Low-frequency triangle + sawtooth energy impulse
                            let p1 = cym.exc_phase1;
                            let p2 = cym.exc_phase2;
                            let tri = 4.0 * (p1 - 0.5).abs() - 1.0;
                            let saw = (2.0 * p1 - 1.0) + 0.45 * (2.0 * p2 - 1.0);
                            let raw_imp = (0.55 * tri + 0.45 * saw) * cym.exc_amp_env * cym.exc_osc_gain
                                + wn * cym.exc_noise_env * cym.exc_noise_gain;
                            cym.exc_lp_z1 = flush_denormal(
                                cym.exc_lp_z1 + cym.exc_lp_alpha * (raw_imp - cym.exc_lp_z1),
                            );
                            cym.exc_lp_z1
                        } else {
                            0.0
                        };

                        cym.exc_pitch_env = flush_denormal(cym.exc_pitch_env * cym.exc_pitch_decay);
                        cym.exc_amp_env = flush_denormal(cym.exc_amp_env * cym.exc_amp_decay);
                        cym.exc_noise_env = flush_denormal(cym.exc_noise_env * cym.exc_noise_decay);

                        // Strictly muted from master output (`0.0` direct bleed) — feeds DispersionLoop ONLY!
                        (imp, 0.0)
                    }
                };

                // 2. Stage 1: Self-Feeding Dispersion Loop (Rides & Crashes; bypassed for Hi-Hats)
                if cym.bloom_ramp_env < 1.0 {
                    cym.bloom_ramp_env = (cym.bloom_ramp_env + cym.bloom_ramp_inc).min(1.0);
                }

                // Steep >5.5 kHz high-passed bronze air/contact excitation (used ONLY when Open Hat 46
                // plate-contact sizzle or optional `Air Sizzle` slider is active; 0.0 at sample 0!)
                let hp_air = flush_denormal(0.62 * (cym.air_hp_y1 + wn - cym.air_hp_x1));
                cym.air_hp_x1 = wn;
                cym.air_hp_y1 = hp_air;

                let stage2_in = if cym.use_dispersion {
                    let disp_out =
                        cym.dispersion
                            .process(exc_sig, cym.comb_fb_scale, &mut self.rng_state);
                    // Feed optional plate-energy-coupled air sizzle gently into Stage 2 wet combs
                    let air_comb_seed = if cym.air_sizzle_amount > 1.0e-4 {
                        hp_air
                            * cym.dispersion.energy_env
                            * cym.bloom_ramp_env
                            * cym.air_sizzle_amount
                            * 0.65
                    } else {
                        0.0
                    };
                    disp_out + air_comb_seed
                } else {
                    exc_sig
                };

                // 3. Stage 2: 12 Independent Parallel Wet-Only Combs (pure delayed output, zero dry bleed)
                let lp_a = cym.comb_lp_alpha;
                let fb_s = cym.comb_fb_scale;

                // Bank A: 4 Lower-Order Fundamental Whole-Plate Combs
                const POL_A: [f32; BANK_A_COMBS] = [1.0, -1.0, 1.0, -1.0];
                let mut bank_a_sum = 0.0_f32;
                let mut comb_peak = 0.0_f32;
                for i in 0..BANK_A_COMBS {
                    let yi = cym.bank_a_meta[i].step(
                        &mut cym.bank_a_bufs[i],
                        COMB_A_MASK,
                        stage2_in,
                        lp_a,
                        fb_s,
                    );
                    bank_a_sum += POL_A[i] * yi;
                    comb_peak = comb_peak.max(yi.abs());
                }

                // Open Hi-Hat (46) dynamic two-plate contact sizzle/PM bloom:
                // Strictly feedforward from Bank A whole-plate vibration (`bank_a_sum`) * decaying clash
                // envelope (`self.fast_env`) into Bank B shimmer combs (zero feedback loop, 0.0 at sample 0!).
                let open_contact_inj = if self.is_open_hat && cym.open_hat_contact_sizzle > 1.0e-4 {
                    let plate_slosh = (bank_a_sum * 0.85).clamp(-1.0, 1.0);
                    let clash_env = self.fast_env * cym.bloom_ramp_env * cym.open_hat_contact_sizzle;
                    let contact_pm = plate_slosh * (3.8 * hp_air).sin();
                    (0.55 * contact_pm + 0.45 * hp_air * plate_slosh.abs()) * clash_env * 0.48
                } else {
                    0.0
                };
                let bank_b_in = stage2_in + open_contact_inj;

                // Bank B: 8 Higher-Order Shimmer Combs (with optional 4-comb `serial_mode` cascade)
                let mut bank_b_sum = 0.0_f32;
                if cym.serial_mode {
                    let y0 = cym.bank_b_meta[0].step(
                        &mut cym.bank_b_bufs[0],
                        COMB_B_MASK,
                        bank_b_in,
                        lp_a,
                        fb_s,
                    );
                    let y1 = cym.bank_b_meta[1].step(
                        &mut cym.bank_b_bufs[1],
                        COMB_B_MASK,
                        y0 * 0.85,
                        lp_a,
                        fb_s,
                    );
                    let y2 = cym.bank_b_meta[2].step(
                        &mut cym.bank_b_bufs[2],
                        COMB_B_MASK,
                        y1 * 0.85,
                        lp_a,
                        fb_s,
                    );
                    let y3 = cym.bank_b_meta[3].step(
                        &mut cym.bank_b_bufs[3],
                        COMB_B_MASK,
                        y2 * 0.85,
                        lp_a,
                        fb_s,
                    );
                    let cascade = 0.42 * y0 - 0.40 * y1 + 0.45 * y2 + 0.58 * y3;
                    comb_peak = comb_peak
                        .max(y0.abs())
                        .max(y1.abs())
                        .max(y2.abs())
                        .max(y3.abs());
                    bank_b_sum += cascade;
                    const POL_B_REM: [f32; 4] = [1.0, -1.0, 1.0, -1.0];
                    for j in 4..BANK_B_COMBS {
                        let yj = cym.bank_b_meta[j].step(
                            &mut cym.bank_b_bufs[j],
                            COMB_B_MASK,
                            bank_b_in,
                            lp_a,
                            fb_s,
                        );
                        bank_b_sum += POL_B_REM[j - 4] * yj * 0.82;
                        comb_peak = comb_peak.max(yj.abs());
                    }
                } else {
                    const POL_B: [f32; BANK_B_COMBS] =
                        [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
                    for j in 0..BANK_B_COMBS {
                        let yj = cym.bank_b_meta[j].step(
                            &mut cym.bank_b_bufs[j],
                            COMB_B_MASK,
                            bank_b_in,
                            lp_a,
                            fb_s,
                        );
                        bank_b_sum += POL_B[j] * yj * 0.84;
                        comb_peak = comb_peak.max(yj.abs());
                    }
                }

                let e_c = if comb_peak > cym.comb_energy_env {
                    0.06
                } else {
                    0.0025
                };
                cym.comb_energy_env =
                    flush_denormal(cym.comb_energy_env + e_c * (comb_peak - cym.comb_energy_env));

                // Optional post-comb plate-energy-coupled 6–18 kHz Air Sizzle (strictly 0.0 on sample 0 since comb_energy_env == 0 on sample 0 before reflection!)
                let post_air_sizzle = if cym.air_sizzle_amount > 1.0e-4 && cym.comb_energy_env > 1.0e-4 {
                    hp_air
                        * cym.comb_energy_env
                        * cym.bloom_ramp_env
                        * cym.air_sizzle_amount
                        * 1.45
                } else {
                    0.0
                };

                // Post-Comb Hilbert Frequency Shifter (`shift_hz` turns integer comb harmonics into inharmonic bronze partials)
                let shifted = cym
                    .hilbert
                    .process(bank_a_sum * 0.56 + bank_b_sum * 0.78)
                    + post_air_sizzle;

                // 4. Post-Sum Spectral Polish Comb + High-Pass + Mid-Cut Peaking EQ
                let polished = cym.polish.process(shifted);
                let disp_env = cym.dispersion.energy_env;
                let comb_env = cym.comb_energy_env;
                let exc_done = cym.exc_amp_env < 1.0e-5
                    && cym.exc_noise_env < 1.0e-5
                    && cym.exc_bell_env < 1.0e-5;

                let hp_out = self.filter1.process(polished);
                let eq_out = self.filter2.process(hp_out);
                let combined = eq_out * 1.65 + direct_stick_out;

                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                if exc_done && disp_env < 1.5e-5 && comb_env < 1.5e-5 && self.age_samples > 1024 {
                    self.cymbal.quiet_samples += 1;
                    if self.cymbal.quiet_samples > 512 {
                        self.clear();
                        return (0.0, 0.0);
                    }
                } else {
                    self.cymbal.quiet_samples = 0;
                }

                tanh_pade(combined * 1.25) * self.level
            }

            PolyVoiceMode::ModalPerc => {
                let (exc, click_raw) = self.next_drum_excitation();
                let mut modal_sum = 0.0_f32;
                let mut total_modal_e = 0.0_f32;
                for i in 0..NUM_MODES {
                    let y_i = self.modes[i].process(exc * self.mode_exc_weights[i], 1.0);
                    modal_sum += y_i;
                    total_modal_e += self.modes[i].energy();
                }

                // High-end metallic strike transient right as the instrument is struck that dies off immediately (~2-3 ms)
                let strike_transient = if self.click_env > 1.0e-4 {
                    let wn = self.next_white_noise();
                    let snap_in = click_raw * 1.85 + wn * self.click_env * 0.65;
                    let hp_snap = self.filter2.process(snap_in);
                    let snap_gain = if self.voice_tag == 56 { 1.45 } else { 0.45 };
                    hp_snap * self.click_env * snap_gain
                } else {
                    0.0
                };
                self.click_env = flush_denormal(self.click_env * self.click_decay);

                let bp = self.filter1.process(modal_sum);
                let shell_out = self.shell.process(modal_sum * 0.65 + exc * 0.30);
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                if self.exc_samples_elapsed >= self.exc_total_samples
                    && total_modal_e < 1.0e-10
                    && self.click_env < 1.0e-5
                    && self.age_samples > 192
                {
                    self.clear();
                    return (0.0, 0.0);
                }

                let body = modal_sum * 0.62
                    + bp * 0.48
                    + shell_out * (0.55 * self.shell_mix)
                    + click_raw * 0.35
                    + strike_transient;
                tanh_pade(body * 1.15) * self.level
            }

            PolyVoiceMode::ShakerOrEffect(gm_note) => {
                if self.in_attack {
                    self.amp_env += self.amp_attack_inc;
                    if self.amp_env >= 1.0 {
                        self.amp_env = 1.0;
                        self.in_attack = false;
                    }
                }

                let mut gesture_done = false;
                let sig = match gm_note {
                    54 => {
                        // Tambourine:
                        // - Frame drumhead on modes[0] with dedicated decay `aux_decay_ms`
                        // - 5 high-Q inharmonic stamped-metal zil modes on modes[1..6] excited on
                        //   initial strike + each flutter clash peak so jingles sound metallic, not noisy
                        let (exc, click_raw) = self.next_drum_excitation();
                        let frame = self.modes[0].process(exc, 1.0);
                        self.phase1 += self.base_inc;
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        let sin_w = (self.phase1 * TAU).sin().abs();
                        // Crisp clash peak each time the jingle pair impacts on the shake cycle
                        let clash_peak = sin_w.powf(5.0);
                        let flutter = 0.35 + 0.65 * sin_w;
                        let wn = self.next_white_noise();
                        let env = self.amp_env * self.slow_env;

                        // Drive the 5 high-Q metallic zil resonators with strike impulse + cyclic clash impulse
                        let zil_drive = exc * 0.95 + (clash_peak * (0.35 + 0.65 * wn) + wn * 0.08) * env * 0.14;
                        let mut zil_modal_sum = 0.0_f32;
                        for idx in 1..NUM_MODES {
                            let sign = if (idx & 1) == 1 { 1.0 } else { -0.85 };
                            zil_modal_sum += self.modes[idx].process(zil_drive * sign, 1.0);
                        }
                        // Crisp high-frequency metallic air edge (> 4 kHz), kept subtle so tone is metallic, not white noise
                        let edge_air = self.filter2.process(self.filter1.process(wn * (0.20 + 0.80 * clash_peak)));
                        let metallic_jingles = (zil_modal_sum * 1.40 + edge_air * 0.32 * env) * (0.55 + 0.45 * flutter);
                        metallic_jingles + frame * 0.68 + click_raw * 0.25
                    }
                    69 => {
                        // Cabasa (69): hand-moved 4-stage ADSR envelope + slower corrugated steel bead grain
                        let (adsr_env, done) = self.eval_hand_gesture_adsr();
                        gesture_done = done;
                        let env = adsr_env * self.slow_env;
                        self.fast_env = env;

                        self.phase1 += self.base_inc * (0.70 + 0.30 * adsr_env);
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        // Sharp ridge-crossing bead click + granular chain scrape
                        let ridge = (self.phase1 * TAU).cos().abs().powf(3.5);
                        let grain = 0.24 + 0.76 * ridge;
                        let wn = self.next_white_noise();
                        let shaped = self.filter2.process(self.filter1.process(wn));
                        let cyl_ring = self.modes[0].process(shaped * ridge * 0.45, 1.0)
                            + self.modes[1].process(shaped * ridge * 0.35, 1.0);
                        (shaped * grain * 0.85 + cyl_ring * 0.65) * env
                    }
                    70 => {
                        // Maracas (70):
                        // - Strong small wooden shell / hollow gourd impact (unity-normalized modes[0..3] + ShellResonator)
                        // - Random loose beads flying around inside the body controlled by `shaker_aux_amount`
                        // - Strictly gain-staged below tanh_pade clipping threshold!
                        self.phase1 += self.base_inc;
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        let grain = 0.45 + 0.55 * (self.phase1 * TAU).cos().abs();
                        let wn = self.next_white_noise();
                        let cluster_env = self.amp_env * (0.45 * self.fast_env + 0.55 * self.slow_env);

                        // Stochastic flying loose beads knocking against the wooden maraca walls here and there
                        if self.shaker_aux_amount > 1.0e-4 && self.slow_env > 0.015 {
                            let rand_u = self.next_white_noise().abs();
                            // ~20 to 110 random individual bead wall hits per second scaled by fader
                            let hit_prob = (0.0005 + 0.0022 * self.shaker_aux_amount)
                                * (0.35 + 0.65 * self.slow_env);
                            if rand_u < hit_prob {
                                let bead_vel = 0.35 + 0.45 * self.next_white_noise().abs();
                                self.click_env = (self.click_env + bead_vel * self.shaker_aux_amount)
                                    .min(0.65);
                            }
                        }
                        let loose_bead_exc = self.click_env * (0.45 + 0.55 * wn);
                        self.click_env = flush_denormal(self.click_env * self.click_decay);

                        let seed_exc = (wn * grain * cluster_env + loose_bead_exc * 0.65) * 0.42;
                        let seeds_hi = self.filter2.process(self.filter1.process(seed_exc));

                        // Drive the 3 unity-normalized wooden maraca shell modes + 2-tap wooden gourd cavity
                        let wood_drive = seed_exc * 0.55 + loose_bead_exc * 0.32;
                        let wood_modes = self.modes[0].process(wood_drive, 1.0)
                            + self.modes[1].process(wood_drive * 0.85, 1.0)
                            + self.modes[2].process(wood_drive * 0.70, 1.0);
                        let wood_shell = self.shell.process(seeds_hi * 0.35 + wood_modes * 0.55);

                        seeds_hi * 0.50 + wood_modes * 0.55 + wood_shell * 0.45
                    }
                    73 | 74 => {
                        // Guiro (Short 73 / Long 74):
                        // - Hand-scraped 4-stage ADSR envelope (longer attack than Cabasa, fast decay,
                        //   high sustain plateau, fast release)
                        // - Separate scrape length for Short 73 (`aux_decay_ms`) and Long 74 (`decay_ms`)
                        let (adsr_env, done) = self.eval_hand_gesture_adsr();
                        gesture_done = done;
                        let env = adsr_env * self.slow_env;
                        self.fast_env = env;

                        self.phase1 += self.base_inc * (0.72 + 0.34 * adsr_env);
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        // Sharp tooth impact per carved gourd notch crossing
                        let tooth = (1.0 - self.phase1).powf(4.0);
                        let wn = self.next_white_noise();
                        let scrape = self.filter2.process(self.filter1.process(wn * (0.16 + 1.50 * tooth)));
                        let gourd = self.shell.process(scrape * 0.90);
                        (scrape * 0.72 + gourd * 0.58) * env * 1.45
                    }
                    58 => {
                        // Vibraslap: spring strike + decaying wood-box rattle
                        let (exc, _) = self.next_drum_excitation();
                        let spring = self.modes[0].process(exc, 1.0);
                        self.phase1 += self.base_inc * (0.55 + 0.55 * self.pitch_env);
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        let clack = (1.0 - self.phase1).powf(5.0);
                        let wn = self.next_white_noise();
                        let box_rattle = self.filter2.process(self.filter1.process(wn * (0.12 + 1.55 * clack)));
                        let wood = self.shell.process(box_rattle);
                        let env = self.amp_env * self.slow_env;
                        (box_rattle * 0.68 + wood * 0.58) * env * 1.45 + spring * 0.38
                    }
                    71 | 72 => {
                        // Samba Whistle (Short 71 / Long 72): pea-trill sine + breath air
                        // + dynamic accent pitch crest on initial breath attack (`fm_index_env_scale`)
                        self.phase2 += self.sec_inc;
                        if self.phase2 >= 1.0 {
                            self.phase2 -= 1.0;
                        }
                        let trill = (self.phase2 * TAU).sin();
                        let accent_attack_bend = self.fm_index_env_scale * self.pitch_env;
                        let freq_mod = 1.0 + accent_attack_bend + 0.032 * self.body_mix * trill;
                        self.phase1 += self.base_inc * freq_mod;
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        let tone = (self.phase1 * TAU).sin() * (0.72 + 0.28 * self.body_mix * trill);
                        let wn = self.next_white_noise();
                        let breath = self.filter1.process(wn) * 0.14;
                        let env = self.amp_env * self.slow_env;
                        (tone + breath) * env * 0.95
                    }
                    _ => {
                        // Cuica (Mute 78 / Open 79): friction-drum pitched sweep + skin formant
                        let sweep_mult = (1.0 + self.sec_inc * (1.0 - self.pitch_env)).clamp(0.45, 2.2);
                        self.phase1 += self.base_inc * sweep_mult;
                        if self.phase1 >= 1.0 {
                            self.phase1 -= 1.0;
                        }
                        let raw_osc = (self.phase1 * TAU).sin() + 0.32 * (self.phase1 * 2.0 * TAU).sin();
                        let friction = tanh_pade(raw_osc * 1.45) + self.next_white_noise() * 0.05;
                        let skin = self.filter1.process(friction);
                        let body = self.shell.process(skin * 0.75);
                        let env = self.amp_env * self.slow_env;
                        (skin * 0.72 + body * 0.45) * env * 1.25
                    }
                };

                if !matches!(gm_note, 69 | 73 | 74) {
                    self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                }
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);
                self.pitch_env = flush_denormal(self.pitch_env * self.pitch_decay);

                if gesture_done
                    || (!self.in_attack && self.slow_env < 1.0e-5 && self.fast_env < 1.0e-5)
                    || (matches!(gm_note, 69 | 73 | 74) && self.slow_env < 1.0e-5)
                {
                    self.clear();
                    return (0.0, 0.0);
                }

                tanh_pade(sig * 1.20) * self.level
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

        let scaled = raw * self.choke_env * self.volume_scale;
        let left = scaled * (1.0 - self.pan);
        let right = scaled * self.pan;
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

    /// Converts a drum instrument volume in dB (`-48.0..=+24.0 dB`) to a linear amplitude scale.
    /// Values `<= -47.9 dB` map to exact `0.0` (true mute).
    #[inline(always)]
    pub fn drum_volume_db_to_amp(db: f32) -> f32 {
        if db <= -47.9 {
            0.0
        } else {
            10.0_f32.powf(db.clamp(-48.0, 24.0) / 20.0)
        }
    }

    /// Resolves the configured absolute volume (in dB) for any General MIDI drum note (`35..=81`).
    #[inline(always)]
    fn drum_note_volume_db_from(dp: &DrumKitParams, gm_drum_note: u8) -> f32 {
        match gm_drum_note {
            35 => dp.acoustic_kick.volume_db,
            36 => dp.kick.volume_db,
            37 => dp.snare.rim_volume_db,
            38 => dp.snare.volume_db,
            39 => dp.hand_clap.volume_db,
            40 => dp.electric_snare.volume_db,
            41 | 43 | 45 | 47 | 48 | 50 => dp.toms.volume_db,
            42 => dp.hi_hats.closed_volume_db,
            44 => dp.hi_hats.pedal_volume_db,
            46 => dp.hi_hats.open_volume_db,
            49 => dp.crash1.volume_db,
            57 => dp.crash2.volume_db,
            51 => dp.ride1_and_bell.bow_volume_db,
            53 => dp.ride1_and_bell.bell_volume_db,
            59 => dp.ride2.volume_db,
            55 => dp.splash.volume_db,
            52 => dp.china.volume_db,
            60 | 61 => dp.bongos.volume_db,
            62 | 63 | 64 => dp.congas.volume_db,
            65 | 66 => dp.timbales.volume_db,
            78 | 79 => dp.cuica.volume_db,
            56 => dp.cowbell.volume_db,
            67 | 68 => dp.agogo.volume_db,
            80 | 81 => dp.triangle.volume_db,
            75 => dp.claves.volume_db,
            76 | 77 => dp.wood_blocks.volume_db,
            54 => dp.tambourine.volume_db,
            69 => dp.cabasa.volume_db,
            70 => dp.maracas.volume_db,
            73 => dp.guiro.aux_volume_db,
            74 => dp.guiro.volume_db,
            71 | 72 => dp.whistle.volume_db,
            58 => dp.vibraslap.volume_db,
            _ => dp.cowbell.volume_db,
        }
    }

    /// Updates live physical-modeling drum kit parameters and immediately adjusts active ringing
    /// voices' absolute output volume scales.
    pub fn update_drum_params(&mut self, role: DrumRole, params: DrumVoiceParams) {
        self.drum_params.set_params_for_role(role, params);
        let dp = &self.drum_params;
        for v in self.voices.iter_mut() {
            if v.is_active()
                && !matches!(v.mode, PolyVoiceMode::GmFmMelodic(_))
                && DrumRole::from_gm_note(v.voice_tag) == role
            {
                v.volume_scale =
                    Self::drum_volume_db_to_amp(Self::drum_note_volume_db_from(dp, v.voice_tag));
            }
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

    /// Allocates a voice slot for `gm_drum_note`.
    /// Prefers an inactive voice slot so any physically damping (`is_choking`) voice from a
    /// mono/one-shot re-trigger or choke group can finish its smooth ~5-15 ms damping curve
    /// without being hard-cut (eliminating all clicks and pops).
    #[inline(always)]
    fn allocate_drum_voice(&mut self, gm_drum_note: u8) -> usize {
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_active() {
                return i;
            }
        }
        // If all 16 voices are active, prefer stealing an already-choking voice with lowest energy
        let mut best_choking: Option<(usize, f32)> = None;
        for (i, v) in self.voices.iter().enumerate() {
            if v.is_choking {
                let e = (v.fast_env + v.slow_env) * v.choke_env;
                if best_choking.map_or(true, |(_, min_e)| e < min_e) {
                    best_choking = Some((i, e));
                }
            }
        }
        if let Some((idx, _)) = best_choking {
            return idx;
        }
        self.allocate_voice(gm_drum_note, false)
    }

    /// Triggers a General MIDI Channel 10 percussion note (`gm_drum_note`) with zero allocations.
    #[inline(always)]
    pub fn trigger_drum(&mut self, gm_drum_note: u8, velocity: f32) {
        let sr = self.sample_rate;

        // 1. Mono / One-Shot instruments (Vibraslap 58, Cabasa 69, Whistle 71/72, Guiro 73/74, Cuica 78/79)
        //    and Choke Groups (Open Hat 46 -> 42/44, Open High Conga 63 -> 62, Open Triangle 81 -> 80,
        //    Long Whistle 72 -> 71, Long Guiro 74 -> 73, Open Cuica 79 -> 78):
        //    Physically damp any currently ringing voice of that mono instrument or choked target via `v.choke(sr)`
        //    and trigger the new note on a fresh voice slot so the old physical model damps smoothly without clicks!
        let should_damp_voice = |active_tag: u8, is_open_hat: bool, trig_note: u8| -> bool {
            match trig_note {
                // Mono / One-Shot instruments damp any active voice of the same physical instrument
                58 => active_tag == 58,
                69 => active_tag == 69,
                71 | 72 => active_tag == 71 || active_tag == 72,
                73 | 74 => active_tag == 73 || active_tag == 74,
                78 | 79 => active_tag == 78 || active_tag == 79,
                // Polyphonic Choke Groups (2nd note damps 1st note)
                42 | 44 => active_tag == 46 || is_open_hat,
                62 => active_tag == 63,
                80 => active_tag == 81,
                _ => false,
            }
        };
        for v in self.voices.iter_mut() {
            if v.is_active() && should_damp_voice(v.voice_tag, v.is_open_hat, gm_drum_note) {
                v.choke(sr);
            }
        }

        // Allocate a fresh voice slot for the new excitation so the damping voice decays click-free
        let slot = self.allocate_drum_voice(gm_drum_note);

        match gm_drum_note {
            // 1. Separated Bass Drums: Acoustic Bass Drum (35) & Bass Drum 1 (36)
            35 | 36 => {
                let k = if gm_drum_note == 35 {
                    self.drum_params.acoustic_kick
                } else {
                    self.drum_params.kick
                };
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, k.strike_position, k.hit_variation);
                let body_decay = (k.body_decay_ms * 0.001).clamp(0.015, 3.0);
                self.voices[slot].trigger_kick_or_tom(
                    gm_drum_note,
                    k.pitch_end_hz,
                    k.pitch_start_hz,
                    (k.pitch_decay_ms * 0.001).clamp(0.003, 0.50),
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
            // Grouped with individual per-tom semitone tuning offsets!
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
                let tune_st = TomParams::tuning_slot_for_note(gm_drum_note)
                    .map(|idx| t.tom_tuning_semitones[idx].clamp(-24.0, 24.0))
                    .unwrap_or(0.0);
                let tune_mult = 2.0_f32.powf(tune_st / 12.0);
                let base_hz = (std_freq * t.pitch_scale.clamp(0.20, 4.5) * tune_mult).clamp(25.0, sr * 0.35);
                let start_hz = (base_hz * t.pitch_drop_ratio.clamp(0.5, 5.0)).clamp(base_hz, sr * 0.43);
                let decay_tau = (t.decay_ms * 0.001).clamp(0.015, 3.5);
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

            // 2. Acoustic Snare (38) + Side Stick (37) grouped on same resonating snare shell;
            //    Electric Snare (40) and Hand Clap (39) independently shaped!
            37 => {
                // Side Stick / Cross-Stick shares Acoustic Snare's shell size & cavity
                let s = self.drum_params.snare;
                let (eff_vel, eff_pos) = self.apply_hit_variation(velocity, 0.92, s.hit_variation);
                let shell_m1 = (s.tone_freq_hz * 1.68).clamp(80.0, 1_400.0);
                let shell_m2 = (s.mode2_freq_hz * 1.70).clamp(120.0, 2_400.0);
                self.voices[slot].trigger_snare_or_clap(
                    37,
                    shell_m1,
                    shell_m2,
                    (s.tone_decay_ms * 0.001 * 0.45).clamp(0.008, 0.40),
                    (s.noise_decay_ms * 0.001 * 0.22).clamp(0.008, 0.40),
                    s.wire_cutoff_hz.max(1_200.0),
                    2.0,
                    0.88,
                    0.18,
                    s.head_tension_mod * 0.5,
                    eff_pos,
                    (s.overtones_ring + 0.25).min(1.5),
                    (s.shell_wood_cavity + 0.22).min(1.5),
                    (s.wire_tension + 0.2).min(1.0),
                    false,
                    12.0,
                    0.46,
                    eff_vel,
                    sr,
                );
            }
            38 | 40 => {
                let s = if gm_drum_note == 40 {
                    self.drum_params.electric_snare
                } else {
                    self.drum_params.snare
                };
                let body_hz = s.tone_freq_hz.clamp(45.0, 1_200.0);
                let mode2_hz = s.mode2_freq_hz.clamp(70.0, 2_400.0);
                let tone_decay = (s.tone_decay_ms * 0.001).clamp(0.008, 2.0);
                let wire_decay = (s.noise_decay_ms * 0.001).clamp(0.008, 2.5);
                let noise_m = s.noise_mix.clamp(0.0, 1.0);
                let body_m = (1.0 - noise_m * 0.85).clamp(0.05, 1.2);
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, s.strike_position, s.hit_variation);
                self.voices[slot].trigger_snare_or_clap(
                    gm_drum_note,
                    body_hz,
                    mode2_hz,
                    tone_decay,
                    wire_decay,
                    s.wire_cutoff_hz.clamp(400.0, (sr * 0.42).min(14_000.0)),
                    1.15,
                    body_m,
                    noise_m,
                    s.head_tension_mod,
                    eff_pos,
                    s.overtones_ring,
                    s.shell_wood_cavity,
                    s.wire_tension,
                    false,
                    12.0,
                    0.46,
                    eff_vel,
                    sr,
                );
            }
            // Dedicated Hand Clap (39)
            39 => {
                let c = self.drum_params.hand_clap;
                let (eff_vel, _) = self.apply_hit_variation(velocity, 0.5, c.hit_variation);
                self.voices[slot].trigger_snare_or_clap(
                    39,
                    185.0,
                    330.0,
                    0.028,
                    (c.tail_decay_ms * 0.001).clamp(0.010, 2.5),
                    c.bandpass_hz.clamp(200.0, (sr * 0.42).min(10_000.0)),
                    c.bandpass_q.clamp(0.25, 18.0),
                    0.18,
                    0.90,
                    0.0,
                    0.5,
                    0.3,
                    c.room_mix.clamp(0.0, 1.5),
                    0.5,
                    true,
                    c.clap_spacing_ms.clamp(1.5, 120.0),
                    0.54,
                    eff_vel,
                    sr,
                );
            }

            // 3. Cast B20 Bronze Hi-Hats (42, 44, 46 — Fast-Path Parallel Wet Combs, grouped & choke-coupled)
            42 | 44 | 46 => {
                let h = self.drum_params.hi_hats;
                let base_hard = if gm_drum_note == 44 {
                    h.stick_hardness * 0.78
                } else {
                    h.stick_hardness
                };
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, base_hard, h.hit_variation);
                let decay = match gm_drum_note {
                    42 => (h.closed_decay_ms * 0.001).clamp(0.006, 2.0),
                    44 => (h.pedal_decay_ms * 0.001).clamp(0.008, 2.0),
                    _ => (h.open_decay_ms * 0.001).clamp(0.025, 6.5),
                };
                self.voices[slot].trigger_hi_hat(
                    gm_drum_note,
                    &h,
                    eff_hard,
                    decay,
                    0.34,
                    eff_vel,
                    sr,
                );
            }

            // 4. Two-Stage Dispersive + Parallel Inharmonic Comb Cymbals
            //    (Crash 1, Crash 2, Ride 2, Splash, China) + Grouped Ride 1 & Ride Bell (51, 53)
            49 | 52 | 55 | 57 | 59 => {
                let (c, pan): (SingleCymbalParams, f32) = match gm_drum_note {
                    49 => (self.drum_params.crash1, 0.30),
                    57 => (self.drum_params.crash2, 0.64),
                    59 => (self.drum_params.ride2, 0.66),
                    55 => (self.drum_params.splash, 0.38),
                    _ => (self.drum_params.china, 0.72),
                };
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, c.stick_hardness, c.hit_variation);
                let decay = (c.decay_ms * 0.001).clamp(0.04, 8.0);
                self.voices[slot].trigger_cymbal_two_stage(
                    gm_drum_note,
                    c.bronze_size_pitch,
                    c.dispersion_size_ms,
                    c.plate_dispersion,
                    c.vibrato_depth,
                    c.vibrato_chaos,
                    c.nonlin_shimmer,
                    c.pm_tone_hz,
                    c.fundamental_cluster_hz,
                    c.matrix_spread,
                    c.inharmonic_shift_hz,
                    c.comb_damping_hz,
                    c.polish_comb_hz,
                    c.polish_comb_mix,
                    c.cutoff_hz,
                    c.mid_cut_hz,
                    c.mid_cut_db,
                    c.air_sizzle,
                    c.bell_level,
                    eff_hard,
                    decay,
                    pan,
                    eff_vel,
                    sr,
                );
            }
            51 => {
                let r: RideCymbalParams = self.drum_params.ride1_and_bell;
                let (eff_vel, eff_hard) =
                    self.apply_hit_variation(velocity, r.stick_hardness, r.hit_variation);
                let decay = (r.bow_decay_ms * 0.001).clamp(0.05, 8.0);
                self.voices[slot].trigger_cymbal_two_stage(
                    51,
                    r.bronze_size_pitch,
                    r.dispersion_size_ms,
                    r.plate_dispersion,
                    r.vibrato_depth,
                    r.vibrato_chaos,
                    r.nonlin_shimmer,
                    r.pm_tone_hz as f32,
                    r.fundamental_cluster_hz,
                    r.matrix_spread,
                    r.inharmonic_shift_hz,
                    r.comb_damping_hz as f32,
                    r.polish_comb_hz as f32,
                    r.polish_comb_mix,
                    r.cutoff_hz,
                    r.mid_cut_hz as f32,
                    r.mid_cut_db,
                    r.air_sizzle,
                    r.bell_peak_level,
                    eff_hard,
                    decay,
                    0.68,
                    eff_vel,
                    sr,
                );
            }
            53 => {
                let r: RideCymbalParams = self.drum_params.ride1_and_bell;
                let (eff_vel, eff_hard) = self.apply_hit_variation(
                    velocity,
                    (r.stick_hardness + 0.16).min(1.5),
                    r.hit_variation,
                );
                let decay = (r.bell_decay_ms * 0.001).clamp(0.05, 8.0);
                self.voices[slot].trigger_cymbal_two_stage(
                    53,
                    r.bronze_size_pitch * 1.15,
                    (r.dispersion_size_ms * 0.82).max(1.0),
                    (r.plate_dispersion * 0.88).clamp(0.0, 1.25),
                    r.vibrato_depth * 0.75,
                    r.vibrato_chaos * 0.75,
                    r.nonlin_shimmer * 0.70,
                    (r.pm_tone_hz as f32) * 1.15,
                    r.fundamental_cluster_hz * 1.22,
                    r.matrix_spread * 1.10,
                    r.inharmonic_shift_hz + 38.0,
                    ((r.comb_damping_hz as f32) * 1.15).min(17_500.0),
                    r.polish_comb_hz as f32,
                    r.polish_comb_mix,
                    (r.cutoff_hz * 0.90).max(600.0),
                    r.mid_cut_hz as f32,
                    r.mid_cut_db * 0.75,
                    r.air_sizzle * 0.5,
                    (r.bell_peak_level * 1.45).min(2.5),
                    eff_hard,
                    decay,
                    0.68,
                    eff_vel,
                    sr,
                );
            }

            // 5. Latin & Hand Drum Membrane Groups: Bongos (60, 61), Congas (62, 63, 64), Timbales (65, 66)
            60 | 61 | 62 | 63 | 64 | 65 | 66 => {
                let (t, std_freq, decay_scale, pan) = match gm_drum_note {
                    60 => (self.drum_params.bongos, 315.0, 0.90, 0.42),
                    61 => (self.drum_params.bongos, 220.0, 1.05, 0.56),
                    62 => (self.drum_params.congas, 245.0, 0.34, 0.44), // Mute High Conga (damped!)
                    63 => (self.drum_params.congas, 235.0, 1.00, 0.46), // Open High Conga
                    64 => (self.drum_params.congas, 165.0, 1.15, 0.58), // Low Conga
                    65 => (self.drum_params.timbales, 290.0, 0.95, 0.40),
                    _ => (self.drum_params.timbales, 212.0, 1.10, 0.60),
                };
                let (eff_vel, eff_pos) =
                    self.apply_hit_variation(velocity, t.strike_position, t.hit_variation);
                let tune_st = TomParams::tuning_slot_for_note(gm_drum_note)
                    .map(|idx| t.tom_tuning_semitones[idx].clamp(-24.0, 24.0))
                    .unwrap_or(0.0);
                let tune_mult = 2.0_f32.powf(tune_st / 12.0);
                let base_hz = (std_freq * t.pitch_scale.clamp(0.20, 4.5) * tune_mult).clamp(35.0, sr * 0.35);
                let start_hz = (base_hz * t.pitch_drop_ratio.clamp(0.5, 5.0)).clamp(base_hz, sr * 0.43);
                let decay_tau = ((t.decay_ms * 0.001) * decay_scale).clamp(0.012, 3.5);
                self.voices[slot].trigger_kick_or_tom(
                    gm_drum_note,
                    base_hz,
                    start_hz,
                    0.026,
                    decay_tau,
                    t.head_tension_mod,
                    eff_pos,
                    if gm_drum_note == 62 { t.overtones_ring * 0.45 } else { t.overtones_ring },
                    t.shell_wood_cavity,
                    t.stick_attack,
                    pan,
                    eff_vel,
                    sr,
                );
            }

            // 6. Metallic & Hardwood Modal Percussion: Cowbell (56), Agogo (67, 68),
            //    Claves (75), Wood Blocks (76, 77), Triangle (80, 81)
            56 | 67 | 68 | 75 | 76 | 77 | 80 | 81 => {
                let (a, pitch_mult, decay_mult, pan) = match gm_drum_note {
                    56 => (self.drum_params.cowbell, 1.0, 1.0, 0.58),
                    67 => (self.drum_params.agogo, 1.0, 1.0, 0.42),
                    68 => (self.drum_params.agogo, 0.76, 1.12, 0.58),
                    75 => (self.drum_params.claves, 1.0, 1.0, 0.54),
                    76 => (self.drum_params.wood_blocks, 1.0, 0.92, 0.44),
                    77 => (self.drum_params.wood_blocks, 0.74, 1.08, 0.56),
                    80 => (self.drum_params.triangle, 1.0, 0.06, 0.62), // Mute Triangle
                    _ => (self.drum_params.triangle, 1.0, 1.0, 0.62),   // Open Triangle
                };
                let (eff_vel, _) = self.apply_hit_variation(velocity, 0.5, a.hit_variation);
                let f1 = ((a.freq1_hz + a.detune) * pitch_mult).clamp(60.0, sr * 0.43);
                let f2 = ((a.freq2_hz - a.detune) * pitch_mult).clamp(80.0, sr * 0.44);
                let decay = ((a.decay_ms * 0.001) * decay_mult).clamp(0.005, 5.0);
                self.voices[slot].trigger_modal_perc(
                    gm_drum_note,
                    f1,
                    f2,
                    a.bandpass_q,
                    decay,
                    pan,
                    eff_vel,
                    sr,
                );
            }

            // 7. Shakers, Ratchets, Whistle & Friction Drum:
            //    Tambourine (54), Vibraslap (58), Cabasa (69), Maracas (70),
            //    Whistle (71, 72), Guiro (73, 74), Cuica (78, 79)
            54 | 58 | 69 | 70 | 71 | 72 | 73 | 74 | 78 | 79 => {
                let (p, pan) = match gm_drum_note {
                    54 => (self.drum_params.tambourine, 0.62),
                    58 => (self.drum_params.vibraslap, 0.56),
                    69 => (self.drum_params.cabasa, 0.38),
                    70 => (self.drum_params.maracas, 0.64),
                    71 | 72 => (self.drum_params.whistle, 0.50),
                    73 | 74 => (self.drum_params.guiro, 0.42),
                    _ => (self.drum_params.cuica, 0.54),
                };
                let (eff_vel, _) = self.apply_hit_variation(velocity, 0.5, p.hit_variation);
                self.voices[slot].trigger_shaker_or_effect(gm_drum_note, &p, pan, eff_vel, sr);
            }

            // Fallback for any custom MIDI notes outside GM 35..=81
            other => {
                let a = self.drum_params.cowbell;
                let base_hz = (90.0 + ((other % 24) as f32) * 14.0).clamp(65.0, 420.0);
                self.voices[slot].trigger_kick_or_tom(
                    other,
                    base_hz,
                    base_hz * 1.45,
                    0.024,
                    (a.decay_ms * 0.001).clamp(0.04, 0.5),
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

        // Apply per-instrument absolute volume scaling post-saturation so timbre is preserved
        self.voices[slot].volume_scale = Self::drum_volume_db_to_amp(
            Self::drum_note_volume_db_from(&self.drum_params, gm_drum_note),
        );
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

        // Capture comb feedback scale before choke
        let open_gain_before = pool
            .voices
            .iter()
            .find(|v| v.is_active() && v.is_open_hat)
            .map(|v| v.cymbal.comb_fb_scale)
            .unwrap();

        // Triggering closed hi-hat (42) must clear is_open_hat AND rapidly ramp comb feedback down
        pool.trigger_drum(42, 0.85);
        assert!(
            !pool.voices.iter().any(|v| v.is_active() && v.is_open_hat),
            "Closed hi-hat (42) must choke open hi-hat (46)"
        );
        for _ in 0..64 {
            let _ = pool.next_stereo_frame();
        }
        let choked_gain = pool
            .voices
            .iter()
            .find(|v| v.voice_tag == 46)
            .map(|v| v.cymbal.comb_fb_scale)
            .unwrap();
        assert!(
            choked_gain < open_gain_before * 0.65,
            "Choking open hi-hat must rapidly ramp comb feedback down ({} -> {})",
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
    fn test_two_stage_cymbal_dispersion_wet_combs_hilbert_and_hats() {
        let sr = 48_000.0;

        // 1. Verify WetCombMeta (Thiran all-pass fractional delay) is strictly wet-only (zero dry bleed on sample 0)
        let mut comb_buf = [0.0_f32; 64];
        let mut meta = WetCombMeta::from_delay(12.0, 0.85, 1.0, 0.6);
        let y0 = meta.step(&mut comb_buf, 63, 1.0, 1.0, 1.0);
        assert_eq!(y0, 0.0, "Wet-only comb must have zero dry bleed on sample 0");
        let mut pre_arrival_max = 0.0_f32;
        for _ in 1..12 {
            pre_arrival_max = pre_arrival_max.max(meta.step(&mut comb_buf, 63, 0.0, 1.0, 1.0).abs());
        }
        assert_eq!(pre_arrival_max, 0.0, "Wet-only comb must remain silent prior to delay_samples");
        let y_arrive = meta.step(&mut comb_buf, 63, 0.0, 1.0, 1.0);
        assert!(y_arrive > 0.4, "Wet-only comb must output delayed reflection at delay_samples (got {})", y_arrive);

        // 2. Verify Crash (49) mutes excitation from master output (sample 0 == 0.0), while Ride (51) mixes stick click
        let mut p_crash = PercussionAndGmPool::new(sr);
        p_crash.trigger_drum(49, 0.95);
        let (lc0, rc0) = p_crash.next_stereo_frame();
        assert_eq!(lc0 + rc0, 0.0, "Crash (49) impulse must be muted from master output (feeding DispersionLoop + wet combs only)");

        let mut p_ride = PercussionAndGmPool::new(sr);
        p_ride.trigger_drum(51, 0.95);
        let mut ride_stick_early = 0.0_f32;
        for _ in 0..8 {
            let (lr, rr) = p_ride.next_stereo_frame();
            ride_stick_early += (lr + rr).abs();
        }
        assert!(ride_stick_early > 1.0e-3, "Ride (51) must mix bright stick click attack lightly into master output");

        // 3. Verify HilbertFreqShifter shifts a 1000 Hz sinusoid upward cleanly when shift_hz = +300 Hz
        let mut hilbert_up = HilbertFreqShifter::default();
        hilbert_up.configure(sr, 300.0);
        let mut zc_in = 0usize;
        let mut zc_out = 0usize;
        let mut prev_x = 0.0_f32;
        let mut prev_y = 0.0_f32;
        for n in 0..4800 {
            let x = (TAU * 1_000.0 * (n as f32) / sr).sin();
            let y = hilbert_up.process(x);
            assert!(y.is_finite());
            if n > 480 {
                if prev_x <= 0.0 && x > 0.0 { zc_in += 1; }
                if prev_y <= 0.0 && y > 0.0 { zc_out += 1; }
            }
            prev_x = x;
            prev_y = y;
        }
        assert!(
            zc_out > zc_in + 18,
            "Hilbert +300 Hz shift must increase zero-crossing frequency (in={}, out={})",
            zc_in,
            zc_out
        );

        // 4. Verify Hi-Hat Bank B `serial_sizzle` toggle alters Open Hat (46) metallic sizzle response
        let mut p_par = PercussionAndGmPool::new(sr);
        let mut p_ser = PercussionAndGmPool::new(sr);
        p_par.drum_params.hi_hats.hit_variation = 0.0;
        p_ser.drum_params.hi_hats.hit_variation = 0.0;
        p_par.drum_params.hi_hats.serial_sizzle = false;
        p_ser.drum_params.hi_hats.serial_sizzle = true;
        p_par.trigger_drum(46, 0.90);
        p_ser.trigger_drum(46, 0.90);
        let mut sizzle_diff = 0.0_f32;
        for _ in 0..4096 {
            let (l1, _) = p_par.next_stereo_frame();
            let (l2, _) = p_ser.next_stereo_frame();
            sizzle_diff += (l1 - l2).abs();
        }
        assert!(sizzle_diff > 1.0, "Bank B serial_sizzle toggle must distinctly alter Hi-Hat ring (diff={})", sizzle_diff);

        // 5. Stress-test stability across all cymbal/hat GM notes and sample rates under max Self-PM & Vibrato
        for &test_sr in &[44_100.0_f32, 48_000.0, 96_000.0] {
            let mut pool = PercussionAndGmPool::new(test_sr);
            pool.drum_params.crash1.nonlin_shimmer = 1.25;
            pool.drum_params.crash1.plate_dispersion = 1.0;
            pool.drum_params.crash1.vibrato_depth = 0.45;
            pool.drum_params.crash1.vibrato_chaos = 1.0;
            pool.drum_params.crash1.dispersion_size_ms = 25.0;
            pool.drum_params.ride1_and_bell.nonlin_shimmer = 1.0;

            for &note in &[42_u8, 44, 46, 49, 51, 52, 53, 55, 57, 59] {
                pool.all_notes_off();
                pool.trigger_drum(note, 1.0);
                let mut early_e = 0.0_f32;
                let mut late_e = 0.0_f32;
                let total_frames = (1.15 * test_sr) as usize;
                let early_end = (0.25 * test_sr) as usize;
                let late_start = (0.90 * test_sr) as usize;
                for i in 0..total_frames {
                    let (l, r) = pool.next_stereo_frame();
                    assert!(
                        l.is_finite() && r.is_finite() && l.abs() <= 2.2 && r.abs() <= 2.2,
                        "Two-stage cymbal note {} destabilized at sr={}, sample {} (l={}, r={})",
                        note,
                        test_sr,
                        i,
                        l,
                        r
                    );
                    let e = l * l + r * r;
                    if i < early_end {
                        early_e += e;
                    } else if i >= late_start {
                        late_e += e;
                    }
                }
                assert!(
                    early_e > 0.05 && early_e > late_e,
                    "Two-stage cymbal note {} at sr={} should bloom and decay cleanly (early={}, late={})",
                    note,
                    test_sr,
                    early_e,
                    late_e
                );
            }
        }

        // 6. Verify Ride Bell (53) differs distinctly from Ride Bow (51)
        let mut p_bow = PercussionAndGmPool::new(sr);
        let mut p_bell = PercussionAndGmPool::new(sr);
        p_bow.drum_params.ride1_and_bell.hit_variation = 0.0;
        p_bell.drum_params.ride1_and_bell.hit_variation = 0.0;
        p_bow.trigger_drum(51, 0.9);
        p_bell.trigger_drum(53, 0.9);
        let mut diff = 0.0_f32;
        for _ in 0..4096 {
            let (l1, _) = p_bow.next_stereo_frame();
            let (l2, _) = p_bell.next_stereo_frame();
            diff += (l1 - l2).abs();
        }
        assert!(
            diff > 3.0,
            "Ride Bell (53) and Ride Bow (51) must sound distinctly differentiated (diff={})",
            diff
        );

        // 7. Verify pure deterministic Self-PM + Thiran Comb upward >6 kHz spectral bloom on Crash (49)
        //    even with `air_sizzle = 0.0`: high-frequency (>6 kHz) energy must swell dramatically after
        //    the initial 0..6 ms flex transient into the 20..80 ms bloom window!
        let mut p_bloom = PercussionAndGmPool::new(sr);
        p_bloom.drum_params.crash1.hit_variation = 0.0;
        p_bloom.drum_params.crash1.air_sizzle = 0.0;
        p_bloom.trigger_drum(49, 0.95);
        let mut hp_6k = BiquadFilter::highpass(sr, 6_000.0, 0.707);
        let mut hf_onset_rms_sq = 0.0_f32;
        let mut hf_bloom_rms_sq = 0.0_f32;
        let onset_n = (0.006 * sr) as usize; // 0..6 ms
        let bloom_start = (0.020 * sr) as usize;
        let bloom_end = (0.080 * sr) as usize; // 20..80 ms
        for i in 0..bloom_end {
            let (l, r) = p_bloom.next_stereo_frame();
            let hf = hp_6k.process(l + r);
            if i < onset_n {
                hf_onset_rms_sq += hf * hf;
            } else if i >= bloom_start {
                hf_bloom_rms_sq += hf * hf;
            }
        }
        let onset_rms = (hf_onset_rms_sq / (onset_n as f32)).sqrt();
        let bloom_rms = (hf_bloom_rms_sq / ((bloom_end - bloom_start) as f32)).sqrt();
        assert!(
            bloom_rms > 0.02 && bloom_rms > onset_rms * 1.5,
            "Crash (49) pure Self-PM + Thiran combs must produce strong upward >6 kHz bloom (onset_rms={:.4}, bloom_rms={:.4})",
            onset_rms,
            bloom_rms
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
            pool.drum_params.crash1.hit_variation = hit_variation;

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

        // At 0.0 variation, consecutive snare & kick hits must be tightly consistent (< 7.5% / < 0.65 dB spread)
        for &note in &[36_u8, 38] {
            let e_zero = measure_hit_energies(note, 0.0);
            let min_e = e_zero.iter().copied().fold(f32::MAX, f32::min);
            let max_e = e_zero.iter().copied().fold(0.0_f32, f32::max);
            let spread_ratio = (max_e - min_e) / max_e.max(1e-6);
            assert!(
                spread_ratio < 0.075,
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

    #[test]
    fn test_shell_wood_cavity_full_range_clean_no_clipping() {
        let sr = 48_000.0;

        // 1. Direct ShellResonator unit test: steady-state resonant sine at cavity_hz must stay bounded near unity
        //    (previously exploded by 100x+ due to impulse-area 1/(2*g) normalization on continuous signals)
        for &(cavity_hz, cavity_t60) in &[(66.0_f32, 0.37_f32), (175.0, 0.37), (324.0, 0.11)] {
            let mut shell = ShellResonator::default();
            shell.configure(sr, cavity_hz, cavity_t60, 2_800.0);
            let mut peak = 0.0_f32;
            for i in 0..8192 {
                let x = (TAU * cavity_hz * (i as f32) / sr).sin() * 0.7;
                let y = shell.process(x);
                assert!(y.is_finite());
                peak = peak.max(y.abs());
            }
            assert!(
                peak > 0.15 && peak < 1.8,
                "ShellResonator at {} Hz must stay clean & bounded (got peak={:.4})",
                cavity_hz,
                peak
            );
        }

        // 2. Full Kick (36), Snare (38), and Tom (45) test across shell_wood_cavity = 0.0, 0.5, 1.0:
        //    output must stay clean, well-behaved, and non-blown-out across the entire 0..1 range!
        for &note in &[36_u8, 38, 45] {
            let mut rms_vals = [0.0_f32; 3];
            let mut peak_vals = [0.0_f32; 3];
            for (idx, &cavity) in [0.0_f32, 0.5, 1.0].iter().enumerate() {
                let mut pool = PercussionAndGmPool::new(sr);
                pool.drum_params.kick.hit_variation = 0.0;
                pool.drum_params.snare.hit_variation = 0.0;
                pool.drum_params.toms.hit_variation = 0.0;
                pool.drum_params.kick.volume_db = 0.0;
                pool.drum_params.snare.volume_db = 0.0;
                pool.drum_params.toms.volume_db = 0.0;
                pool.drum_params.kick.shell_wood_cavity = cavity;
                pool.drum_params.snare.shell_wood_cavity = cavity;
                pool.drum_params.toms.shell_wood_cavity = cavity;

                pool.trigger_drum(note, 0.90);
                let mut sum_sq = 0.0_f32;
                let mut peak = 0.0_f32;
                for _ in 0..4096 {
                    let (l, r) = pool.next_stereo_frame();
                    assert!(l.is_finite() && r.is_finite());
                    let m = l + r;
                    sum_sq += m * m;
                    peak = peak.max(m.abs());
                }
                rms_vals[idx] = (sum_sq / 4096.0).sqrt();
                peak_vals[idx] = peak;
            }
            // No blown-out level explosion at shell_wood_cavity = 1.0 vs 0.0
            let rms_ratio = rms_vals[2] / rms_vals[0].max(1e-6);
            assert!(
                rms_ratio > 0.55 && rms_ratio < 1.85,
                "Drum note {} shell_wood_cavity=1.0 must stay clean & balanced vs 0.0 (rms0={:.4}, rms0.5={:.4}, rms1.0={:.4}, ratio={:.3})",
                note,
                rms_vals[0],
                rms_vals[1],
                rms_vals[2],
                rms_ratio
            );
            assert!(
                peak_vals[2] < 1.05,
                "Drum note {} at shell_wood_cavity=1.0 must not blow out (peak={:.4})",
                note,
                peak_vals[2]
            );
        }
    }

    #[test]
    fn test_per_drum_absolute_volume_mute_and_boost() {
        let sr = 48_000.0;

        let render_rms = |setup: &dyn Fn(&mut PercussionAndGmPool), note: u8| -> f32 {
            let mut pool = PercussionAndGmPool::new(sr);
            pool.drum_params.kick.hit_variation = 0.0;
            pool.drum_params.snare.hit_variation = 0.0;
            pool.drum_params.toms.hit_variation = 0.0;
            pool.drum_params.hi_hats.hit_variation = 0.0;
            pool.drum_params.crash1.hit_variation = 0.0;
            pool.drum_params.crash2.hit_variation = 0.0;
            pool.drum_params.splash.hit_variation = 0.0;
            pool.drum_params.china.hit_variation = 0.0;
            pool.drum_params.ride1_and_bell.hit_variation = 0.0;
            pool.drum_params.cowbell.hit_variation = 0.0;
            pool.drum_params.hand_clap.hit_variation = 0.0;
            setup(&mut pool);
            pool.trigger_drum(note, 0.85);
            let mut sum_sq = 0.0_f32;
            for _ in 0..2048 {
                let (l, r) = pool.next_stereo_frame();
                assert!(l.is_finite() && r.is_finite());
                sum_sq += l * l + r * r;
            }
            (sum_sq / 2048.0).sqrt()
        };

        // 1. -48.0 dB produces exact 0.0 silence across distinct instruments
        assert_eq!(render_rms(&|p| p.drum_params.kick.volume_db = -48.0, 36), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.acoustic_kick.volume_db = -48.0, 35), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.snare.rim_volume_db = -48.0, 37), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.hand_clap.volume_db = -48.0, 39), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.electric_snare.volume_db = -48.0, 40), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.hi_hats.open_volume_db = -48.0, 46), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.ride1_and_bell.bell_volume_db = -48.0, 53), 0.0);
        assert_eq!(render_rms(&|p| p.drum_params.cowbell.volume_db = -48.0, 56), 0.0);

        // 2. +6.02 dB doubles RMS cleanly vs 0.0 dB (post-saturation linear scaling)
        let rms_0 = render_rms(&|p| p.drum_params.snare.volume_db = 0.0, 38);
        let rms_6 = render_rms(&|p| p.drum_params.snare.volume_db = 6.0206, 38);
        let ratio = rms_6 / rms_0.max(1e-6);
        assert!(
            (ratio - 2.0).abs() < 0.05,
            "+6.02 dB snare volume should ~2x RMS (got ratio={:.4})",
            ratio
        );

        // 3. Independent instrument isolation:
        //    - Muting Side Stick (37) does NOT mute Acoustic Snare (38)
        //    - Muting Acoustic Snare (38) does NOT mute Electric Snare (40)
        //    - Muting Kick 1 (36) does NOT mute Acoustic Kick (35)
        //    - Muting Crash 1 (49) does NOT mute Crash 2 (57), Splash (55), or China (52)
        let snare_with_muted_rim = render_rms(
            &|p| {
                p.drum_params.snare.volume_db = 0.0;
                p.drum_params.snare.rim_volume_db = -48.0;
            },
            38,
        );
        assert!(
            (snare_with_muted_rim - rms_0).abs() < 1e-4,
            "Muting Side Stick must not affect Acoustic Snare level"
        );
        assert!(
            render_rms(&|p| p.drum_params.snare.volume_db = -48.0, 40) > 0.05,
            "Muting Acoustic Snare (38) must not mute Electric Snare (40)"
        );
        assert!(
            render_rms(&|p| p.drum_params.kick.volume_db = -48.0, 35) > 0.05,
            "Muting Bass Drum 1 (36) must not mute Acoustic Bass Drum (35)"
        );
        let crash2_base = render_rms(&|_| {}, 57);
        let crash2_after = render_rms(&|p| p.drum_params.crash1.volume_db = -48.0, 57);
        assert!(
            crash2_base > 0.005 && (crash2_after - crash2_base).abs() < 1e-4,
            "Muting Crash 1 (49) must not affect Crash 2 (57) (base={}, after={})",
            crash2_base,
            crash2_after
        );
        let splash_base = render_rms(&|_| {}, 55);
        let splash_after = render_rms(&|p| p.drum_params.crash1.volume_db = -48.0, 55);
        assert!(
            splash_base > 0.005 && (splash_after - splash_base).abs() < 1e-4,
            "Muting Crash 1 (49) must not affect Splash (55) (base={}, after={})",
            splash_base,
            splash_after
        );
        let china_base = render_rms(&|_| {}, 52);
        let china_after = render_rms(&|p| p.drum_params.crash1.volume_db = -48.0, 52);
        assert!(
            china_base > 0.005 && (china_after - china_base).abs() < 1e-4,
            "Muting Crash 1 (49) must not affect China (52) (base={}, after={})",
            china_base,
            china_after
        );

        // 4. Live update_drum_params immediately updates volume of currently ringing crash cymbal (49)
        let mut pool = PercussionAndGmPool::new(sr);
        pool.trigger_drum(49, 0.90);
        for _ in 0..512 {
            let _ = pool.next_stereo_frame();
        }
        let mut c = pool.drum_params.crash1;
        c.volume_db = -48.0;
        pool.update_drum_params(DrumRole::Crash1, DrumVoiceParams::SingleCymbal(c));
        let (l, r) = pool.next_stereo_frame();
        assert_eq!(l, 0.0, "Live -48 dB update must immediately mute ringing crash cymbal");
        assert_eq!(r, 0.0, "Live -48 dB update must immediately mute ringing crash cymbal");
    }

    #[test]
    fn test_all_47_gm_percussion_notes_and_individual_tom_tuning() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);

        // 1. Every standard GM percussion note (35..=81) must map to a valid role and render clean non-silent audio
        for note in 35..=81_u8 {
            let role = DrumRole::from_gm_note(note);
            assert!(
                role.gm_notes().contains(&note),
                "GM note {} must belong to role {:?}'s gm_notes()",
                note,
                role
            );
            pool.all_notes_off();
            pool.trigger_drum(note, 0.85);
            let mut energy = 0.0_f32;
            for _ in 0..2048 {
                let (l, r) = pool.next_stereo_frame();
                assert!(
                    l.is_finite() && r.is_finite() && l.abs() <= 2.5 && r.abs() <= 2.5,
                    "GM percussion note {} produced invalid sample ({}, {})",
                    note,
                    l,
                    r
                );
                energy += l * l + r * r;
            }
            assert!(
                energy > 1.0e-3,
                "GM percussion note {} ({:?}) must produce audible audio (energy={})",
                note,
                role,
                energy
            );
        }

        // 2. Individual Tom tuning affects ONLY the targeted tom (e.g., High Floor Tom 43) without altering Hi-Mid Tom (48)
        let mut p_base = PercussionAndGmPool::new(sr);
        let mut p_tuned = PercussionAndGmPool::new(sr);
        p_base.drum_params.toms.hit_variation = 0.0;
        p_tuned.drum_params.toms.hit_variation = 0.0;
        // Shift slot 1 (GM 43 High Floor Tom) up +7 semitones
        p_tuned.drum_params.toms.tom_tuning_semitones[1] = 7.0;

        p_base.trigger_drum(43, 0.9);
        p_tuned.trigger_drum(43, 0.9);
        let mut diff_43 = 0.0_f32;
        for _ in 0..2048 {
            let (l1, _) = p_base.next_stereo_frame();
            let (l2, _) = p_tuned.next_stereo_frame();
            diff_43 += (l1 - l2).abs();
        }
        assert!(
            diff_43 > 5.0,
            "Tuning Tom 43 by +7 st must visibly shift its waveform (diff={})",
            diff_43
        );

        p_base.all_notes_off();
        p_tuned.all_notes_off();
        p_base.trigger_drum(48, 0.9);
        p_tuned.trigger_drum(48, 0.9);
        let mut diff_48 = 0.0_f32;
        for _ in 0..2048 {
            let (l1, _) = p_base.next_stereo_frame();
            let (l2, _) = p_tuned.next_stereo_frame();
            diff_48 += (l1 - l2).abs();
        }
        assert!(
            diff_48 < 1.0e-3,
            "Tuning Tom 43 must NOT alter untuned Tom 48 (diff={})",
            diff_48
        );

        // 3. Congas (62 Mute High vs 63 Open High) decay distinctly while sharing Congas group
        let mut p_mute = PercussionAndGmPool::new(sr);
        let mut p_open = PercussionAndGmPool::new(sr);
        p_mute.trigger_drum(62, 0.9);
        p_open.trigger_drum(63, 0.9);
        let mut tail_mute = 0.0_f32;
        let mut tail_open = 0.0_f32;
        for i in 0..8192 {
            let (lm, _) = p_mute.next_stereo_frame();
            let (lo, _) = p_open.next_stereo_frame();
            if i >= 4096 {
                tail_mute += lm * lm;
                tail_open += lo * lo;
            }
        }
        assert!(
            tail_open > tail_mute * 2.5,
            "Open High Conga (63) should sustain longer than Mute High Conga (62) (open={}, mute={})",
            tail_open,
            tail_mute
        );
    }

    #[test]
    fn test_mono_one_shot_choke_groups_and_shaker_physical_models() {
        let sr = 48_000.0;

        // 1. Mono / One-Shot instruments (Vibraslap 58, Cabasa 69, Whistle 71/72, Guiro 73/74, Cuica 78/79)
        //    must physically damp the previous voice (`is_choking = true`) and start the new strike
        //    on a fresh voice slot with zero hard-cut waveform discontinuity across the trigger boundary.
        for &(note_a, note_b) in &[
            (58_u8, 58_u8),
            (69, 69),
            (72, 72),
            (72, 71),
            (74, 74),
            (74, 73),
            (79, 79),
            (79, 78),
        ] {
            let mut pool = PercussionAndGmPool::new(sr);
            pool.trigger_drum(note_a, 0.90);
            let mut prev_sample = 0.0_f32;
            for _ in 0..512 {
                let (l, r) = pool.next_stereo_frame();
                prev_sample = l + r;
            }
            assert_eq!(
                pool.voices.iter().filter(|v| v.is_active() && !v.is_choking).count(),
                1,
                "Initial trigger of {} should have 1 non-choking active voice",
                note_a
            );

            // Re-trigger: old voice must enter smooth physical damping (`is_choking == true`)
            // while new voice starts on a separate slot (`!v.is_choking`) with no hard-cut pop!
            pool.trigger_drum(note_b, 0.90);
            let non_choking: Vec<&PolyVoice> = pool
                .voices
                .iter()
                .filter(|v| v.is_active() && !v.is_choking)
                .collect();
            let choking_count = pool
                .voices
                .iter()
                .filter(|v| v.is_active() && v.is_choking)
                .count();
            assert_eq!(
                non_choking.len(),
                1,
                "Mono instrument ({} -> {}) must have exactly 1 non-choking active voice",
                note_a,
                note_b
            );
            assert_eq!(
                choking_count, 1,
                "Previous voice of mono instrument ({} -> {}) must smoothly damp via is_choking rather than hard-cutting",
                note_a, note_b
            );
            assert_eq!(non_choking[0].voice_tag, note_b);
            assert_eq!(non_choking[0].age_samples, 0);

            let (l_next, r_next) = pool.next_stereo_frame();
            let boundary_jump = ((l_next + r_next) - prev_sample).abs();
            assert!(
                boundary_jump < 0.45,
                "Re-triggering mono instrument ({} -> {}) must not produce a hard-cut click/pop (jump={})",
                note_a,
                note_b,
                boundary_jump
            );
        }

        // 2. Polyphonic choke groups:
        //    - Open Triangle (81) -> Mute Triangle (80)
        //    - Open High Conga (63) -> Mute High Conga (62)
        //    Playing the mute note must rapidly damp the open note's ringing modes without clicks.
        for &(open_note, mute_note) in &[(81_u8, 80_u8), (63, 62)] {
            let mut pool_unchoked = PercussionAndGmPool::new(sr);
            let mut pool_choked = PercussionAndGmPool::new(sr);
            pool_unchoked.drum_params.triangle.hit_variation = 0.0;
            pool_choked.drum_params.triangle.hit_variation = 0.0;
            pool_unchoked.drum_params.congas.hit_variation = 0.0;
            pool_choked.drum_params.congas.hit_variation = 0.0;

            pool_unchoked.trigger_drum(open_note, 0.95);
            pool_choked.trigger_drum(open_note, 0.95);
            for _ in 0..512 {
                let _ = pool_unchoked.next_stereo_frame();
                let _ = pool_choked.next_stereo_frame();
            }
            // Trigger mute note (with muted volume so we measure ONLY the remaining tail of open_note!)
            if mute_note == 80 {
                pool_choked.drum_params.triangle.volume_db = -48.0;
            } else {
                pool_choked.drum_params.congas.volume_db = -48.0;
            }
            pool_choked.trigger_drum(mute_note, 0.90);
            for v in pool_choked.voices.iter_mut() {
                if v.is_active() && v.voice_tag == open_note {
                    v.volume_scale = 1.0;
                }
            }
            for v in pool_unchoked.voices.iter_mut() {
                if v.is_active() && v.voice_tag == open_note {
                    v.volume_scale = 1.0;
                }
            }

            // Advance 25 ms (~1200 samples) for the ~5.5 ms smooth physical choke damping to take effect
            for _ in 0..1200 {
                let _ = pool_unchoked.next_stereo_frame();
                let _ = pool_choked.next_stereo_frame();
            }
            let mut unchoked_tail = 0.0_f32;
            let mut choked_tail = 0.0_f32;
            for _ in 0..4096 {
                let (lu, ru) = pool_unchoked.next_stereo_frame();
                let (lc, rc) = pool_choked.next_stereo_frame();
                unchoked_tail += lu * lu + ru * ru;
                choked_tail += lc * lc + rc * rc;
            }
            assert!(
                choked_tail < unchoked_tail * 0.05,
                "Choke group ({} -> {}) must damp open voice tail by >95% (unchoked={}, choked={})",
                open_note,
                mute_note,
                unchoked_tail,
                choked_tail
            );
        }

        // 3. Separate Short Guiro (73) and Long Guiro (74) scrape durations AND independent volumes
        let mut pool = PercussionAndGmPool::new(sr);
        pool.drum_params.guiro.hit_variation = 0.0;
        pool.drum_params.guiro.aux_decay_ms = 40.0; // Short Guiro = 40 ms
        pool.drum_params.guiro.decay_ms = 180.0;    // Long Guiro = 180 ms

        pool.trigger_drum(73, 0.90);
        let short_end = pool
            .voices
            .iter()
            .find(|v| v.is_active() && !v.is_choking)
            .unwrap()
            .gesture_rel_end;
        pool.trigger_drum(74, 0.90);
        let long_end = pool
            .voices
            .iter()
            .find(|v| v.is_active() && !v.is_choking)
            .unwrap()
            .gesture_rel_end;
        assert!(
            long_end > short_end * 3,
            "Long Guiro (74) scrape length ({}) must be separately controllable from Short Guiro (73) ({})",
            long_end,
            short_end
        );

        // Verify independent Short Guiro (`aux_volume_db`) vs Long Guiro (`volume_db`) volume controls
        pool.all_notes_off();
        pool.drum_params.guiro.aux_volume_db = -48.0; // Mute Short Guiro (73)
        pool.drum_params.guiro.volume_db = 6.5;       // Keep Long Guiro (74) audible
        pool.trigger_drum(73, 0.90);
        let (l73, r73) = pool.next_stereo_frame();
        assert_eq!(l73 + r73, 0.0, "Muting Short Guiro (73) via aux_volume_db must silence note 73");
        pool.all_notes_off();
        pool.trigger_drum(74, 0.90);
        let mut e74 = 0.0_f32;
        for _ in 0..512 {
            let (l, r) = pool.next_stereo_frame();
            e74 += l * l + r * r;
        }
        assert!(e74 > 0.01, "Muting Short Guiro (73) must NOT silence Long Guiro (74)");

        // 4. Separate Short Whistle (71, `aux_decay_ms`) and Long Whistle (72, `decay_ms`) durations
        //    + velocity/accent pitch bend (`aux_amount` slider: louder = higher pitch)
        let mut pool_w = PercussionAndGmPool::new(sr);
        pool_w.drum_params.whistle.hit_variation = 0.0;
        pool_w.drum_params.whistle.aux_decay_ms = 30.0; // Short Whistle 71 = 30 ms
        pool_w.drum_params.whistle.decay_ms = 240.0;    // Long Whistle 72 = 240 ms
        pool_w.trigger_drum(71, 0.90);
        let decay_71 = pool_w
            .voices
            .iter()
            .find(|v| v.is_active() && !v.is_choking)
            .unwrap()
            .slow_decay;
        pool_w.trigger_drum(72, 0.90);
        let decay_72 = pool_w
            .voices
            .iter()
            .find(|v| v.is_active() && !v.is_choking)
            .unwrap()
            .slow_decay;
        assert!(
            decay_72 > decay_71,
            "Long Whistle (72) decay coeff ({}) must be longer than Short Whistle (71) ({})",
            decay_72,
            decay_71
        );

        let mut pool_soft = PercussionAndGmPool::new(sr);
        let mut pool_loud = PercussionAndGmPool::new(sr);
        pool_soft.drum_params.whistle.hit_variation = 0.0;
        pool_loud.drum_params.whistle.hit_variation = 0.0;
        pool_soft.drum_params.whistle.aux_amount = 0.80;
        pool_loud.drum_params.whistle.aux_amount = 0.80;

        pool_soft.trigger_drum(72, 0.25);
        pool_loud.trigger_drum(72, 1.00);
        let inc_soft = pool_soft.voices.iter().find(|v| v.is_active()).unwrap().base_inc;
        let inc_loud = pool_loud.voices.iter().find(|v| v.is_active()).unwrap().base_inc;
        assert!(
            inc_loud > inc_soft * 1.08,
            "Accented whistle must have higher pitch increment than soft whistle when aux_amount > 0 (soft={}, loud={})",
            inc_soft,
            inc_loud
        );

        // 5. Maracas (70) must remain clean and unclipped (natural crest factor > 2.4, no flat-topping)
        let mut pool_m = PercussionAndGmPool::new(sr);
        pool_m.trigger_drum(70, 1.0);
        let mut sum_sq = 0.0_f32;
        let mut peak = 0.0_f32;
        for _ in 0..2048 {
            let (l, r) = pool_m.next_stereo_frame();
            let m = l + r;
            sum_sq += m * m;
            peak = peak.max(m.abs());
        }
        let rms = (sum_sq / 2048.0).sqrt();
        let crest = peak / rms.max(1.0e-6);
        assert!(
            crest > 2.4 && peak > 0.005,
            "Maracas (70) must have natural unclipped crest factor > 2.4 (got crest={:.2}, peak={:.4}, rms={:.4})",
            crest,
            peak,
            rms
        );
    }
}
