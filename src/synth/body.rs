//! Direct Form II Transposed Biquad filters, `InstrumentProfile` presets, and zero-latency
//! acoustic/electric/bass body & cabinet resonators (`src/synth/body.rs`).
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, and zero `unwrap()`/`panic!()` on the audio thread.
//! - Denormal numbers are flushed to zero inside every biquad delay state (`z1`, `z2`).
//! - Filter coefficients are computed dynamically from the active stream's `sample_rate: f32`.

use crate::synth::backend::flush_denormal;
use crate::synth::patch::BodyResonatorParams;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// Physical instrument profile configuring both `StringWaveguide` excitation/loop parameters
/// and the post-waveguide `BodyResonator` filter topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InstrumentProfile {
    #[default]
    AcousticGuitar,
    ElectricClean,
    ElectricOverdrive,
    FingeredBass,
    PickedBass,
    HarpOrLute,
}

/// Waveguide tuning multipliers derived from an [`InstrumentProfile`].
#[derive(Clone, Copy, Debug)]
pub struct WaveguideProfileParams {
    /// Default normalized pluck position in `[0.05, 0.50]` when not overridden.
    pub default_pluck_pos: f32,
    /// Multiplier on loop decay time $T_{60}$ (`> 1.0` = longer sustain).
    pub sustain_mult: f32,
    /// Multiplier on loop lowpass stretching factor $S$ (`< 1.0` = brighter, `> 1.0` = warmer/darker).
    pub damping_s_mult: f32,
    /// Additive offset to the initial excitation one-pole lowpass coefficient (`> 0` = softer finger attack).
    pub exc_softness_offset: f32,
    /// Multiplier on wound-string all-pass stiffness dispersion.
    pub dispersion_mult: f32,
}

impl InstrumentProfile {
    pub const ALL: [InstrumentProfile; 6] = [
        InstrumentProfile::AcousticGuitar,
        InstrumentProfile::ElectricClean,
        InstrumentProfile::ElectricOverdrive,
        InstrumentProfile::FingeredBass,
        InstrumentProfile::PickedBass,
        InstrumentProfile::HarpOrLute,
    ];

    #[inline]
    pub const fn label(self) -> &'static str {
        match self {
            InstrumentProfile::AcousticGuitar => "Acoustic Guitar",
            InstrumentProfile::ElectricClean => "Electric Clean",
            InstrumentProfile::ElectricOverdrive => "Electric Overdrive",
            InstrumentProfile::FingeredBass => "Fingered Bass",
            InstrumentProfile::PickedBass => "Picked Bass",
            InstrumentProfile::HarpOrLute => "Harp / Lute",
        }
    }

    /// Returns waveguide parameter adjustments for this instrument profile.
    #[inline(always)]
    pub const fn waveguide_params(self) -> WaveguideProfileParams {
        match self {
            InstrumentProfile::AcousticGuitar => WaveguideProfileParams {
                default_pluck_pos: 0.18,
                sustain_mult: 1.0,
                damping_s_mult: 1.0,
                exc_softness_offset: 0.0,
                dispersion_mult: 1.0,
            },
            InstrumentProfile::ElectricClean => WaveguideProfileParams {
                default_pluck_pos: 0.15,
                sustain_mult: 1.30,
                damping_s_mult: 0.80,
                exc_softness_offset: -0.05,
                dispersion_mult: 0.85,
            },
            InstrumentProfile::ElectricOverdrive => WaveguideProfileParams {
                default_pluck_pos: 0.12,
                sustain_mult: 1.45,
                damping_s_mult: 0.75,
                exc_softness_offset: -0.08,
                dispersion_mult: 0.80,
            },
            InstrumentProfile::FingeredBass => WaveguideProfileParams {
                default_pluck_pos: 0.28,
                sustain_mult: 1.50,
                damping_s_mult: 1.15,
                exc_softness_offset: 0.18,
                dispersion_mult: 1.25,
            },
            InstrumentProfile::PickedBass => WaveguideProfileParams {
                default_pluck_pos: 0.14,
                sustain_mult: 1.40,
                damping_s_mult: 0.85,
                exc_softness_offset: -0.06,
                dispersion_mult: 1.25,
            },
            InstrumentProfile::HarpOrLute => WaveguideProfileParams {
                default_pluck_pos: 0.35,
                sustain_mult: 1.25,
                damping_s_mult: 0.70,
                exc_softness_offset: 0.10,
                dispersion_mult: 0.50,
            },
        }
    }
}

/// Fast rational Padé approximant of $\tanh(x)$:
/// $$\tanh(x) \approx \frac{x (27 + x^2)}{27 + 9 x^2}$$
/// Exact at $x = 0$ with unit slope and smooth saturation clamped to $[-1.0, 1.0]$ for $|x| \ge 3$.
#[inline(always)]
pub fn tanh_pade(x: f32) -> f32 {
    let xc = x.clamp(-3.0, 3.0);
    let x2 = xc * xc;
    (xc * (27.0 + x2)) / (27.0 + 9.0 * x2)
}

/// Direct Form II Transposed 2nd-order Biquad filter with denormal protection.
#[derive(Clone, Copy, Debug)]
pub struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for BiquadFilter {
    fn default() -> Self {
        Self::passthrough()
    }
}

impl BiquadFilter {
    #[inline(always)]
    pub const fn passthrough() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Processes a single sample using Direct Form II Transposed recurrence.
    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush_denormal(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush_denormal(self.b2 * x - self.a2 * y);
        flush_denormal(y)
    }

    /// 2nd-order resonant bandpass filter (0 dB peak gain at center frequency).
    pub fn bandpass_peak(sample_rate: f32, freq_hz: f32, q: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(10.0, sr * 0.45);
        let q_val = q.max(0.1);
        let w0 = 2.0 * PI * f / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q_val);

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self::from_normalized(b0, b1, b2, a0, a1, a2)
    }

    /// 2nd-order lowpass filter (RBJ Audio EQ Cookbook).
    pub fn lowpass(sample_rate: f32, freq_hz: f32, q: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(20.0, sr * 0.47);
        let q_val = q.max(0.1);
        let w0 = 2.0 * PI * f / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q_val);

        let b1 = 1.0 - cos_w0;
        let b0 = b1 * 0.5;
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self::from_normalized(b0, b1, b2, a0, a1, a2)
    }

    /// 2nd-order highpass filter (RBJ Audio EQ Cookbook).
    pub fn highpass(sample_rate: f32, freq_hz: f32, q: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(10.0, sr * 0.45);
        let q_val = q.max(0.1);
        let w0 = 2.0 * PI * f / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q_val);

        let b0 = (1.0 + cos_w0) * 0.5;
        let b1 = -(1.0 + cos_w0);
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self::from_normalized(b0, b1, b2, a0, a1, a2)
    }

    /// 2nd-order peaking EQ filter with `gain_db` at `freq_hz`.
    pub fn peaking_eq(sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(15.0, sr * 0.45);
        let q_val = q.max(0.1);
        let a_amp = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q_val);

        let b0 = 1.0 + alpha * a_amp;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a_amp;
        let a0 = 1.0 + alpha / a_amp;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a_amp;

        Self::from_normalized(b0, b1, b2, a0, a1, a2)
    }

    #[inline(always)]
    fn from_normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv_a0 = 1.0 / a0.max(1.0e-9);
        Self {
            b0: b0 * inv_a0,
            b1: b1 * inv_a0,
            b2: b2 * inv_a0,
            a1: a1 * inv_a0,
            a2: a2 * inv_a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Evaluates the steady-state complex magnitude response $|H(e^{j\omega})|$ at `freq_hz`.
    pub fn magnitude_response(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let sr = sample_rate.max(8_000.0);
        let f = freq_hz.clamp(1.0, sr * 0.499);
        let w = 2.0 * PI * f / sr;
        let cos_w = w.cos();
        let cos_2w = (2.0 * w).cos();
        let sin_w = w.sin();
        let sin_2w = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -self.b1 * sin_w - self.b2 * sin_2w;
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -self.a1 * sin_w - self.a2 * sin_2w;

        let num_sq = num_re * num_re + num_im * num_im;
        let den_sq = (den_re * den_re + den_im * den_im).max(1.0e-12);
        (num_sq / den_sq).sqrt()
    }

    /// Evaluates the magnitude response in decibels (dB) at `freq_hz`.
    pub fn magnitude_db(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let mag = self.magnitude_response(freq_hz, sample_rate).max(1.0e-6);
        20.0 * mag.log10()
    }

    #[inline(always)]
    fn copy_coeffs_from(&mut self, other: &Self) {
        self.b0 = other.b0;
        self.b1 = other.b1;
        self.b2 = other.b2;
        self.a1 = other.a1;
        self.a2 = other.a2;
    }
}

/// Per-channel biquad filter bank supporting both parallel modal resonance (`AcousticGuitar`,
/// `HarpOrLute`) and series pickup + overdrive + 4-stage cabinet cascades (`ElectricClean`,
/// `ElectricOverdrive`, `FingeredBass`, `PickedBass`).
#[derive(Clone, Copy, Debug, Default)]
struct ChannelResonatorBank {
    /// Parallel resonant modes (used for AcousticGuitar & HarpOrLute).
    mode1: BiquadFilter,
    mode2: BiquadFilter,
    mode3: BiquadFilter,
    /// Pre-drive magnetic pickup inductance resonant lowpass.
    pickup_lp: BiquadFilter,
    /// 4-stage series cabinet / tone-shaping biquad cascade.
    cab1: BiquadFilter,
    cab2: BiquadFilter,
    cab3: BiquadFilter,
    cab4: BiquadFilter,
}

impl ChannelResonatorBank {
    #[inline(always)]
    fn reset(&mut self) {
        self.mode1.reset();
        self.mode2.reset();
        self.mode3.reset();
        self.pickup_lp.reset();
        self.cab1.reset();
        self.cab2.reset();
        self.cab3.reset();
        self.cab4.reset();
    }

    fn configure(&mut self, sample_rate: f32, profile: InstrumentProfile) {
        match profile {
            InstrumentProfile::AcousticGuitar => {
                // Parallel resonant bandpass bank:
                // 1. Helmholtz air cavity (~98 Hz)
                // 2. Top-plate dipole (~205 Hz)
                // 3. Upper body wood mode (~410 Hz)
                self.mode1
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 98.0, 6.0));
                self.mode2
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 205.0, 5.2));
                self.mode3
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 410.0, 4.5));
                // Gentle high-frequency piezo/air warmth filter on the dry path
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 2800.0, 1.0, 2.0));
            }
            InstrumentProfile::ElectricClean => {
                // Single-coil / humbucker inductance resonant lowpass peak @ 3.3 kHz (Q = 1.85)
                self.pickup_lp
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 3300.0, 1.85));
                // 4-stage cabinet filter:
                // 1. Highpass @ 85 Hz
                // 2. Boxiness dip @ 400 Hz (-4.0 dB)
                // 3. Presence peak @ 2.2 kHz (+4.0 dB)
                // 4. Steep 2nd-order speaker roll-off @ 5.0 kHz
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::highpass(sample_rate, 85.0, 0.707));
                self.cab2
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 400.0, 1.2, -4.0));
                self.cab3
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 2200.0, 1.35, 4.0));
                self.cab4
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 5000.0, 0.85));
            }
            InstrumentProfile::ElectricOverdrive => {
                // Hot humbucker inductance resonant lowpass peak @ 2.85 kHz (Q = 2.2)
                self.pickup_lp
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 2850.0, 2.2));
                // 4-stage guitar speaker cabinet filter:
                // 1. Tight highpass @ 85 Hz
                // 2. Mud dip @ 400 Hz (-4.5 dB)
                // 3. Presence bark @ 2.2 kHz (+5.5 dB)
                // 4. Steep 2nd-order speaker cone roll-off @ 5.0 kHz (Q = 0.9)
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::highpass(sample_rate, 85.0, 0.75));
                self.cab2
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 400.0, 1.25, -4.5));
                self.cab3
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 2200.0, 1.4, 5.5));
                self.cab4
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 5000.0, 0.90));
            }
            InstrumentProfile::FingeredBass => {
                // Warm split-coil bass pickup lowpass @ 2.1 kHz (Q = 1.3)
                self.pickup_lp
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 2100.0, 1.3));
                // Bass cab: sub highpass @ 38 Hz, 100 Hz fundamental weight bump (+6.0 dB),
                // 320 Hz scoop (-2.5 dB), smooth cab roll-off @ 3.2 kHz
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::highpass(sample_rate, 38.0, 0.707));
                self.cab2
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 100.0, 1.1, 6.0));
                self.cab3
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 320.0, 1.2, -2.5));
                self.cab4
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 3200.0, 0.75));
            }
            InstrumentProfile::PickedBass => {
                // Aggressive bass pickup resonant peak @ 2.6 kHz (Q = 1.75)
                self.pickup_lp
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 2600.0, 1.75));
                // Bass cab: 42 Hz highpass, 100 Hz fundamental bump (+5.0 dB),
                // 1.6 kHz pick attack clank (+4.0 dB), cab roll-off @ 4.2 kHz
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::highpass(sample_rate, 42.0, 0.707));
                self.cab2
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 100.0, 1.1, 5.0));
                self.cab3
                    .copy_coeffs_from(&BiquadFilter::peaking_eq(sample_rate, 1600.0, 1.4, 4.0));
                self.cab4
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 4200.0, 0.80));
            }
            InstrumentProfile::HarpOrLute => {
                // Resonant wooden soundboard modes (~135 Hz, ~290 Hz, ~580 Hz)
                self.mode1
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 135.0, 5.5));
                self.mode2
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 290.0, 4.8));
                self.mode3
                    .copy_coeffs_from(&BiquadFilter::bandpass_peak(sample_rate, 580.0, 4.2));
                self.cab1
                    .copy_coeffs_from(&BiquadFilter::lowpass(sample_rate, 6500.0, 0.707));
            }
        }
    }

    fn configure_custom(&mut self, sample_rate: f32, params: &BodyResonatorParams) {
        let sr = sample_rate.max(8_000.0);
        let b0 = params.bands[0];
        let b1 = params.bands[1];
        let b2 = params.bands[2];
        let b3 = params.bands[3];

        let f0 = b0.freq_hz.clamp(20.0, sr * 0.45);
        let q0 = b0.q.clamp(0.1, 15.0);
        let f1 = b1.freq_hz.clamp(20.0, sr * 0.45);
        let q1 = b1.q.clamp(0.1, 15.0);
        let f2 = b2.freq_hz.clamp(20.0, sr * 0.45);
        let q2 = b2.q.clamp(0.1, 15.0);
        let f3 = b3.freq_hz.clamp(20.0, sr * 0.45);
        let q3 = b3.q.clamp(0.1, 15.0);

        match params.profile_base {
            InstrumentProfile::AcousticGuitar | InstrumentProfile::HarpOrLute => {
                if b0.enabled {
                    self.mode1
                        .copy_coeffs_from(&BiquadFilter::bandpass_peak(sr, f0, q0));
                } else {
                    self.mode1.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b1.enabled {
                    self.mode2
                        .copy_coeffs_from(&BiquadFilter::bandpass_peak(sr, f1, q1));
                } else {
                    self.mode2.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b2.enabled {
                    self.mode3
                        .copy_coeffs_from(&BiquadFilter::bandpass_peak(sr, f2, q2));
                } else {
                    self.mode3.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b3.enabled {
                    self.cab1.copy_coeffs_from(&BiquadFilter::peaking_eq(
                        sr,
                        f3,
                        q3,
                        b3.gain_db.clamp(-24.0, 24.0),
                    ));
                } else {
                    self.cab1.copy_coeffs_from(&BiquadFilter::passthrough());
                }
            }
            InstrumentProfile::ElectricClean
            | InstrumentProfile::ElectricOverdrive
            | InstrumentProfile::FingeredBass
            | InstrumentProfile::PickedBass => {
                if b0.enabled {
                    self.cab1
                        .copy_coeffs_from(&BiquadFilter::highpass(sr, f0, q0));
                } else {
                    self.cab1.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b1.enabled {
                    self.cab2.copy_coeffs_from(&BiquadFilter::peaking_eq(
                        sr,
                        f1,
                        q1,
                        b1.gain_db.clamp(-24.0, 24.0),
                    ));
                } else {
                    self.cab2.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b2.enabled {
                    self.pickup_lp
                        .copy_coeffs_from(&BiquadFilter::lowpass(sr, f2, q2));
                    self.cab3.copy_coeffs_from(&BiquadFilter::peaking_eq(
                        sr,
                        f2,
                        q2,
                        b2.gain_db.clamp(-24.0, 24.0),
                    ));
                } else {
                    self.pickup_lp.copy_coeffs_from(&BiquadFilter::passthrough());
                    self.cab3.copy_coeffs_from(&BiquadFilter::passthrough());
                }
                if b3.enabled {
                    self.cab4
                        .copy_coeffs_from(&BiquadFilter::lowpass(sr, f3, q3));
                } else {
                    self.cab4.copy_coeffs_from(&BiquadFilter::passthrough());
                }
            }
        }
    }

    #[inline(always)]
    fn process_sample(&mut self, x: f32, profile: InstrumentProfile) -> f32 {
        match profile {
            InstrumentProfile::AcousticGuitar => {
                // Parallel bank of 3 resonant bandpass biquads mixed at 35% wet with 65% dry
                let m1 = self.mode1.process(x) * 1.15;
                let m2 = self.mode2.process(x) * 0.95;
                let m3 = self.mode3.process(x) * 0.80;
                let wet = m1 + m2 + m3;
                let dry = self.cab1.process(x);
                0.65 * dry + 0.35 * wet
            }
            InstrumentProfile::ElectricClean => {
                let pu = self.pickup_lp.process(x);
                // Subtle warm tube compression via tanh Padé approximant
                let driven = tanh_pade(pu * 1.35) * 0.85;
                let s1 = self.cab1.process(driven);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                self.cab4.process(s3)
            }
            InstrumentProfile::ElectricOverdrive => {
                let pu = self.pickup_lp.process(x);
                // Multi-stage asymmetrical overdrive with tanh Padé approximant
                let pre = pu * 4.2;
                let stage1 = tanh_pade(pre + 0.08) - 0.08;
                let stage2 = tanh_pade(stage1 * 2.1) * 0.62;
                let s1 = self.cab1.process(stage2);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                self.cab4.process(s3)
            }
            InstrumentProfile::FingeredBass => {
                let pu = self.pickup_lp.process(x);
                let warm = tanh_pade(pu * 1.25) * 0.92;
                let s1 = self.cab1.process(warm);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                self.cab4.process(s3)
            }
            InstrumentProfile::PickedBass => {
                let pu = self.pickup_lp.process(x);
                let grit = tanh_pade(pu * 1.65) * 0.82;
                let s1 = self.cab1.process(grit);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                self.cab4.process(s3)
            }
            InstrumentProfile::HarpOrLute => {
                let m1 = self.mode1.process(x) * 1.0;
                let m2 = self.mode2.process(x) * 0.85;
                let m3 = self.mode3.process(x) * 0.70;
                let wet = m1 + m2 + m3;
                let dry = self.cab1.process(x);
                0.70 * dry + 0.30 * wet
            }
        }
    }
}

/// Stereo post-string body and cabinet resonator (`BodyResonator`).
#[derive(Clone, Debug)]
pub struct BodyResonator {
    sample_rate: f32,
    profile: InstrumentProfile,
    left: ChannelResonatorBank,
    right: ChannelResonatorBank,
    pub custom_params: Option<BodyResonatorParams>,
    pub post_gain_amp: f32,
}

impl Default for BodyResonator {
    fn default() -> Self {
        Self::new(48_000.0, InstrumentProfile::AcousticGuitar)
    }
}

impl BodyResonator {
    pub fn new(sample_rate: f32, profile: InstrumentProfile) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut res = Self {
            sample_rate: sr,
            profile,
            left: ChannelResonatorBank::default(),
            right: ChannelResonatorBank::default(),
            custom_params: None,
            post_gain_amp: 1.0,
        };
        res.set_profile(profile);
        res
    }

    #[inline(always)]
    pub fn profile(&self) -> InstrumentProfile {
        self.profile
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        if let Some(cp) = &self.custom_params {
            let cp_copy = *cp;
            self.update_params(&cp_copy);
        } else {
            self.left.configure(self.sample_rate, self.profile);
            self.right.configure(self.sample_rate, self.profile);
        }
    }

    /// Updates the active [`InstrumentProfile`] and recalculates biquad coefficients without
    /// clearing filter delay states (preventing clicks on live preset changes).
    pub fn set_profile(&mut self, profile: InstrumentProfile) {
        self.profile = profile;
        self.custom_params = None;
        self.post_gain_amp = 1.0;
        self.left.configure(self.sample_rate, profile);
        self.right.configure(self.sample_rate, profile);
    }

    /// Applies custom parameter overrides for the body resonator, saturation drive, and 4-band filters.
    pub fn update_params(&mut self, params: &BodyResonatorParams) {
        self.profile = params.profile_base;
        self.custom_params = Some(*params);
        self.post_gain_amp = 10.0_f32.powf(params.post_gain_db.clamp(-24.0, 24.0) / 20.0);
        self.left.configure_custom(self.sample_rate, params);
        self.right.configure_custom(self.sample_rate, params);
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }

    /// Processes a stereo string waveguide frame `(left, right)` through the active body/cabinet resonator.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if let Some(cp) = &self.custom_params {
            let drive = cp.pre_drive.clamp(0.0, 10.0);
            let in_l = left * drive;
            let in_r = right * drive;

            let out_l = self.left.process_sample(in_l, self.profile);
            let out_r = self.right.process_sample(in_r, self.profile);

            let wet_mix = cp.body_wet_mix.clamp(0.0, 1.0);
            let mix_l = (1.0 - wet_mix) * left + wet_mix * out_l;
            let mix_r = (1.0 - wet_mix) * right + wet_mix * out_r;

            let gain = self.post_gain_amp;
            (flush_denormal(mix_l * gain), flush_denormal(mix_r * gain))
        } else {
            let out_l = self.left.process_sample(left, self.profile);
            let out_r = self.right.process_sample(right, self.profile);
            (flush_denormal(out_l), flush_denormal(out_r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biquad_and_body_resonator_profiles_stability_and_frequency_shaping() {
        let sr = 48_000.0;

        for profile in InstrumentProfile::ALL {
            let mut body = BodyResonator::new(sr, profile);
            assert_eq!(body.profile(), profile);

            // Feed an impulse followed by 2048 zeros and verify finite, decaying response
            let (l0, r0) = body.process_stereo(0.8, 0.8);
            assert!(l0.is_finite() && r0.is_finite());

            let mut total_energy = l0 * l0 + r0 * r0;
            for _ in 0..2048 {
                let (l, r) = body.process_stereo(0.0, 0.0);
                assert!(l.is_finite() && r.is_finite());
                total_energy += l * l + r * r;
            }
            assert!(
                total_energy > 0.01,
                "Profile {:?} produced insufficient impulse energy: {}",
                profile,
                total_energy
            );
        }
    }

    #[test]
    fn test_acoustic_resonator_boosts_helmholtz_and_top_plate_modes() {
        let sr = 48_000.0;
        let mut body = BodyResonator::new(sr, InstrumentProfile::AcousticGuitar);

        // Measure steady-state gain at Helmholtz resonance (98 Hz) vs non-resonant 1500 Hz
        let mut energy_98 = 0.0_f32;
        for n in 0..4096 {
            let x = (2.0 * PI * 98.0 * (n as f32) / sr).sin() * 0.2;
            let (y, _) = body.process_stereo(x, x);
            if n >= 1024 {
                energy_98 += y * y;
            }
        }

        body.reset();
        let mut energy_1500 = 0.0_f32;
        for n in 0..4096 {
            let x = (2.0 * PI * 1500.0 * (n as f32) / sr).sin() * 0.2;
            let (y, _) = body.process_stereo(x, x);
            if n >= 1024 {
                energy_1500 += y * y;
            }
        }

        assert!(
            energy_98 > energy_1500 * 1.35,
            "Expected Helmholtz 98Hz resonance ({}) to exceed 1500Hz response ({})",
            energy_98,
            energy_1500
        );
    }

    #[test]
    fn test_tanh_pade_bounds_and_linearity() {
        assert!((tanh_pade(0.0)).abs() < 1.0e-7);
        assert!((tanh_pade(0.1) - 0.1_f32.tanh()).abs() < 1.0e-4);
        assert!((tanh_pade(3.0) - 1.0).abs() < 1.0e-6);
        assert!((tanh_pade(-5.0) + 1.0).abs() < 1.0e-6);
    }
}
