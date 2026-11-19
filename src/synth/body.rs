//! Direct Form II Transposed Biquad filters, `InstrumentProfile` presets, 12-mode wooden body
//! resonant filter bank, and 4-tap Householder micro soundboard diffusion (`src/synth/body.rs`).
//!
//! ### Architecture
//! - **12-Mode Parallel High-Q Resonant Body Bank (`MODAL_COUNT = 12`):**
//!   Models physical acoustic top-plate, Helmholtz air cavity, back-plate, cross-dipole, bracing,
//!   and speaker-cabinet eigenmodes with profile-specific frequencies, $Q$ factors, alternating
//!   physical polarities, and subtle L/R stereo microphone micro-detuning.
//! - **4-Tap Householder Micro Soundboard Diffuser (`HouseholderDiffuser`):**
//!   Zero-allocation orthogonal $4 \times 4$ Householder feedback delay network with mutually prime
//!   tap lengths strictly `< 120` samples and `< 2.5 ms`, diffusing phase instantaneously to create
//!   authentic wooden body knock and cabinet air without FFT convolution latency.
//! - **Parametric 4-Band EQ & Pickup/Cabinet Cascade:**
//!   Preserves live-editable 4-band EQ, magnetic pickup inductance peaking, and Padé `tanh` drive.
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, and zero `unwrap()`/`panic!()` on the audio thread.
//! - Denormal numbers are flushed to zero inside every biquad delay state (`z1`, `z2`) and
//!   every Householder delay line and damping state.
//! - Filter coefficients and delay lengths scale dynamically from `sample_rate: f32`.

use crate::synth::backend::flush_denormal;
use crate::synth::patch::BodyResonatorParams;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// Number of parallel high-Q physical wood/cavity/cabinet eigenmodes in the modal resonator bank.
pub const MODAL_COUNT: usize = 12;

/// Number of short delay taps in the orthogonal Householder micro soundboard diffuser.
pub const HOUSEHOLDER_TAPS: usize = 4;

/// Fixed power-of-two buffer length per Householder tap (128 samples; all tap lengths `< 120`).
const HOUSEHOLDER_BUF_LEN: usize = 128;
const HOUSEHOLDER_BUF_MASK: usize = HOUSEHOLDER_BUF_LEN - 1;

/// Strict upper bound on Householder tap length in samples (`< 120` samples).
pub const MAX_HOUSEHOLDER_DELAY_SAMPLES: usize = 119;

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

    /// Default Wood Modal Knock / Air intensity for this instrument archetype.
    #[inline(always)]
    pub const fn default_modal_knock_air(self) -> f32 {
        match self {
            InstrumentProfile::AcousticGuitar => 0.68,
            InstrumentProfile::HarpOrLute => 0.62,
            InstrumentProfile::FingeredBass => 0.55,
            InstrumentProfile::PickedBass => 0.45,
            InstrumentProfile::ElectricClean => 0.38,
            InstrumentProfile::ElectricOverdrive => 0.40,
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

/// 12-mode physical body/cavity eigenmode table `(freq_hz, q, signed_gain)` per [`InstrumentProfile`].
///
/// Alternating polarities mimic real physical mode shapes sampled at the bridge/microphone point,
/// avoiding unnatural resonant phase buildup and creating organic wooden body knock.
const fn profile_modal_table(profile: InstrumentProfile) -> [(f32, f32, f32); MODAL_COUNT] {
    match profile {
        InstrumentProfile::AcousticGuitar => [
            (98.0, 11.0, 1.45),   // A0 Helmholtz air cavity breathing mode
            (192.0, 13.0, 1.20),  // T(1,1) main spruce top-plate monopole
            (238.0, 14.0, -0.95), // Back-plate coupled monopole
            (315.0, 15.0, 0.80),  // A1 internal longitudinal air cavity mode
            (408.0, 16.0, -0.88), // T(2,1) cross-dipole top plate mode
            (522.0, 17.0, 0.72),  // T(1,2) longitudinal dipole mode
            (668.0, 18.0, -0.62), // Bridge-wing torsional wood mode
            (865.0, 19.0, 0.54),  // Upper bout spruce plate resonance
            (1140.0, 20.0, -0.48),// X-bracing cluster resonance
            (1520.0, 18.0, 0.36), // Lower wooden knock presence
            (2180.0, 16.0, -0.44),// Soundboard treble sparkle
            (3150.0, 14.0, 0.40), // Bridge-saddle spruce air
        ],
        InstrumentProfile::HarpOrLute => [
            (132.0, 10.5, 1.35),
            (268.0, 12.5, 1.10),
            (345.0, 14.0, -0.85),
            (460.0, 15.0, 0.78),
            (585.0, 16.0, -0.70),
            (740.0, 17.0, 0.62),
            (960.0, 18.0, -0.54),
            (1260.0, 18.0, 0.48),
            (1680.0, 16.0, -0.42),
            (2240.0, 15.0, 0.38),
            (2980.0, 14.0, -0.34),
            (3950.0, 12.0, 0.30),
        ],
        InstrumentProfile::ElectricClean => [
            (112.0, 6.5, 0.85),   // 1x12 open-back cabinet resonance
            (185.0, 7.5, 0.70),   // Alder body + baffle low mode
            (275.0, 8.0, -0.55),  // Internal cab standing wave
            (390.0, 8.5, 0.48),   // Cabinet rear opening dipole
            (540.0, 9.0, -0.44),
            (760.0, 9.5, 0.40),
            (1050.0, 10.0, -0.36),
            (1420.0, 10.0, 0.34),
            (1950.0, 9.5, -0.32),
            (2650.0, 9.0, 0.36),  // Speaker cone breakup sparkle
            (3450.0, 8.0, -0.28),
            (4300.0, 7.0, 0.22),
        ],
        InstrumentProfile::ElectricOverdrive => [
            (92.0, 6.0, 0.90),    // 4x12 closed-back enclosure thump
            (155.0, 7.0, 0.68),   // Birch ply baffle mode
            (235.0, 8.0, -0.52),
            (330.0, 8.5, 0.45),
            (480.0, 9.0, -0.48),
            (690.0, 9.5, 0.40),
            (980.0, 10.0, -0.36),
            (1350.0, 9.5, 0.36),
            (1880.0, 9.0, -0.40),
            (2450.0, 8.5, 0.44),  // Upper-mid speaker cone bark
            (3200.0, 7.5, -0.32),
            (4100.0, 6.5, 0.24),
        ],
        InstrumentProfile::FingeredBass => [
            (58.0, 8.0, 1.15),    // Bass port / upright air cavity A0
            (98.0, 9.5, 1.05),    // Fundamental chest weight
            (148.0, 10.5, -0.78), // Lower bout / baffle mode
            (215.0, 11.5, 0.65),
            (305.0, 12.0, -0.56),
            (430.0, 12.5, 0.48),
            (610.0, 13.0, -0.40),
            (840.0, 12.5, 0.36),
            (1150.0, 12.0, -0.32),
            (1580.0, 11.0, 0.28),
            (2150.0, 10.0, -0.24),
            (2850.0, 9.0, 0.20),
        ],
        InstrumentProfile::PickedBass => [
            (68.0, 7.5, 1.00),    // Tight 4x10 bass cabinet thump
            (112.0, 8.5, 0.95),
            (175.0, 9.5, -0.68),
            (260.0, 10.5, 0.60),
            (370.0, 11.0, -0.52),
            (520.0, 11.5, 0.46),
            (740.0, 12.0, -0.40),
            (1040.0, 11.5, 0.38),
            (1450.0, 10.5, -0.42),// Metallic pick clank / growl
            (1980.0, 10.0, 0.38),
            (2650.0, 9.0, -0.30),
            (3500.0, 8.0, 0.24),
        ],
    }
}

/// Pre-allocated 4-tap orthogonal Householder micro soundboard / cabinet diffusion network.
///
/// All tap lengths are mutually prime / well-spread, strictly `< 120` samples and `< 2.5 ms`
/// across all sample rates, diffusing transient phase into dense wooden body knock and cabinet
/// air with zero FFT latency and zero heap allocations.
#[derive(Clone, Debug)]
pub struct HouseholderDiffuser {
    lines: [[f32; HOUSEHOLDER_BUF_LEN]; HOUSEHOLDER_TAPS],
    write_idx: [usize; HOUSEHOLDER_TAPS],
    delays: [usize; HOUSEHOLDER_TAPS],
    lp_z1: [f32; HOUSEHOLDER_TAPS],
    fb_gain: f32,
    lp_damp: f32,
}

impl Default for HouseholderDiffuser {
    fn default() -> Self {
        Self::new(48_000.0, false, InstrumentProfile::AcousticGuitar)
    }
}

impl HouseholderDiffuser {
    pub fn new(sample_rate: f32, right_channel: bool, profile: InstrumentProfile) -> Self {
        let mut d = Self {
            lines: [[0.0; HOUSEHOLDER_BUF_LEN]; HOUSEHOLDER_TAPS],
            write_idx: [0; HOUSEHOLDER_TAPS],
            delays: [31, 53, 79, 107],
            lp_z1: [0.0; HOUSEHOLDER_TAPS],
            fb_gain: 0.44,
            lp_damp: 0.42,
        };
        d.configure(sample_rate, right_channel, profile);
        d
    }

    /// Configures tap lengths (`< 120` samples and `< 2.5 ms`), feedback gain, and damping
    /// without clearing active delay state (preventing clicks on live profile changes).
    pub fn configure(&mut self, sample_rate: f32, right_channel: bool, profile: InstrumentProfile) {
        let sr = sample_rate.max(8_000.0);
        // Base prime-ratio micro-reflection delays in milliseconds (strictly < 2.5 ms).
        // At 48 kHz: Left = [31, 53, 79, 107] samples, Right = [37, 61, 89, 113] samples.
        let base_ms: [f32; HOUSEHOLDER_TAPS] = if right_channel {
            [0.7708, 1.2708, 1.8542, 2.3542]
        } else {
            [0.6458, 1.1042, 1.6458, 2.2292]
        };

        // Strict dual cap: < 120 samples AND < 2.5 ms at every sample rate.
        let max_allowed = (0.00248 * sr)
            .floor()
            .min(MAX_HOUSEHOLDER_DELAY_SAMPLES as f32)
            .max(12.0);
        let raw_max = base_ms[HOUSEHOLDER_TAPS - 1] * 0.001 * sr;
        let scale = if raw_max > max_allowed {
            max_allowed / raw_max
        } else {
            1.0
        };

        let mut prev = 3usize;
        for i in 0..HOUSEHOLDER_TAPS {
            let mut d = ((base_ms[i] * 0.001 * sr) * scale)
                .round()
                .clamp(5.0, max_allowed) as usize;
            if d <= prev {
                d = (prev + 2).min(MAX_HOUSEHOLDER_DELAY_SAMPLES);
            }
            self.delays[i] = d;
            prev = d;
        }

        // Profile-specific acoustic wood vs. speaker cabinet micro-reflection density
        let (fb, cutoff_hz) = match profile {
            InstrumentProfile::AcousticGuitar => (0.46, 3600.0_f32),
            InstrumentProfile::HarpOrLute => (0.44, 4200.0_f32),
            InstrumentProfile::FingeredBass => (0.40, 2200.0_f32),
            InstrumentProfile::PickedBass => (0.38, 2800.0_f32),
            InstrumentProfile::ElectricClean => (0.36, 3200.0_f32),
            InstrumentProfile::ElectricOverdrive => (0.35, 2900.0_f32),
        };
        self.fb_gain = fb;
        // Sample-rate-normalized 1-pole lowpass wood/air absorption coefficient
        self.lp_damp = (-2.0 * PI * cutoff_hz / sr).exp().clamp(0.15, 0.85);
    }

    /// Returns the configured tap delay lengths in samples (guaranteed `< 120`).
    #[inline(always)]
    pub fn tap_delays(&self) -> [usize; HOUSEHOLDER_TAPS] {
        self.delays
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write_idx = [0; HOUSEHOLDER_TAPS];
        self.lp_z1 = [0.0; HOUSEHOLDER_TAPS];
    }

    /// Processes a single sample through the orthogonal $4 \times 4$ Householder FDN.
    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let mut d = [0.0_f32; HOUSEHOLDER_TAPS];
        for i in 0..HOUSEHOLDER_TAPS {
            let r_idx = self.write_idx[i].wrapping_sub(self.delays[i]) & HOUSEHOLDER_BUF_MASK;
            d[i] = self.lines[i][r_idx];
        }

        // Orthogonal Householder matrix H = I - 0.5 * 1 * 1^T (unitary, norm = 1.0)
        let half_sum = 0.5 * (d[0] + d[1] + d[2] + d[3]);
        const IN_SIGNS: [f32; HOUSEHOLDER_TAPS] = [0.5, -0.5, 0.5, -0.5];
        let one_minus_damp = 1.0 - self.lp_damp;

        for i in 0..HOUSEHOLDER_TAPS {
            let reflected = d[i] - half_sum;
            let damped = flush_denormal(one_minus_damp * reflected + self.lp_damp * self.lp_z1[i]);
            self.lp_z1[i] = damped;
            let write_val = flush_denormal(IN_SIGNS[i] * x + self.fb_gain * damped);
            self.lines[i][self.write_idx[i]] = write_val;
            self.write_idx[i] = (self.write_idx[i] + 1) & HOUSEHOLDER_BUF_MASK;
        }

        // Zero-DC alternating tap sum for crisp phase-diffused wooden body knock & cabinet air
        flush_denormal(0.82 * (d[0] - d[1] + d[2] - d[3]))
    }
}

/// Per-channel biquad, 12-mode wooden body bank, and 4-tap Householder diffuser supporting
/// acoustic modal bodies (`AcousticGuitar`, `HarpOrLute`) and electric pickup + overdrive +
/// 4-stage cabinet + cabinet-air cascades (`ElectricClean`, `ElectricOverdrive`, `FingeredBass`, `PickedBass`).
#[derive(Clone, Debug)]
struct ChannelResonatorBank {
    /// Parallel 12-mode high-Q physical wood/cavity/cabinet eigenmode filter bank.
    modal_bank: [BiquadFilter; MODAL_COUNT],
    /// Signed modal coupling gains for each of the 12 physical modes.
    modal_gains: [f32; MODAL_COUNT],
    /// 4-tap Householder short-delay micro soundboard & cabinet air diffuser (< 120 samples / < 2.5ms).
    diffuser: HouseholderDiffuser,
    /// Right-channel stereo microphone micro-detuning flag.
    right_channel: bool,

    /// User-editable parallel resonant modes (used for AcousticGuitar & HarpOrLute).
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

impl Default for ChannelResonatorBank {
    fn default() -> Self {
        Self::new(false)
    }
}

impl ChannelResonatorBank {
    fn new(right_channel: bool) -> Self {
        Self {
            modal_bank: [BiquadFilter::passthrough(); MODAL_COUNT],
            modal_gains: [0.0; MODAL_COUNT],
            diffuser: HouseholderDiffuser::new(48_000.0, right_channel, InstrumentProfile::AcousticGuitar),
            right_channel,
            mode1: BiquadFilter::passthrough(),
            mode2: BiquadFilter::passthrough(),
            mode3: BiquadFilter::passthrough(),
            pickup_lp: BiquadFilter::passthrough(),
            cab1: BiquadFilter::passthrough(),
            cab2: BiquadFilter::passthrough(),
            cab3: BiquadFilter::passthrough(),
            cab4: BiquadFilter::passthrough(),
        }
    }

    #[inline(always)]
    fn reset(&mut self) {
        for m in &mut self.modal_bank {
            m.reset();
        }
        self.diffuser.reset();
        self.mode1.reset();
        self.mode2.reset();
        self.mode3.reset();
        self.pickup_lp.reset();
        self.cab1.reset();
        self.cab2.reset();
        self.cab3.reset();
        self.cab4.reset();
    }

    fn configure_modal_and_diffuser(&mut self, sample_rate: f32, profile: InstrumentProfile) {
        let sr = sample_rate.max(8_000.0);
        let table = profile_modal_table(profile);
        for (i, &(freq, q, gain)) in table.iter().enumerate() {
            // Subtle ±0.9% stereo mic micro-detuning on upper modes for right channel width
            // (keep lowest Helmholtz / fundamental mode locked in phase across L/R)
            let stereo_detune = if self.right_channel && i >= 2 {
                if i % 2 == 0 { 1.009 } else { 0.991 }
            } else {
                1.0
            };
            let f_clamped = (freq * stereo_detune).clamp(20.0, sr * 0.44);
            let q_clamped = q.clamp(0.5, 24.0);
            self.modal_bank[i]
                .copy_coeffs_from(&BiquadFilter::bandpass_peak(sr, f_clamped, q_clamped));
            self.modal_gains[i] = gain;
        }
        self.diffuser.configure(sr, self.right_channel, profile);
    }

    fn configure(&mut self, sample_rate: f32, profile: InstrumentProfile) {
        self.configure_modal_and_diffuser(sample_rate, profile);

        match profile {
            InstrumentProfile::AcousticGuitar => {
                // Parallel resonant bandpass bank (classic 3 primary acoustic resonances):
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
        self.configure_modal_and_diffuser(sr, params.profile_base);

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

    /// Evaluates the parallel 12-mode wood/cabinet resonant bank + 4-tap Householder diffuser.
    #[inline(always)]
    fn process_modal_and_diffusion(&mut self, x: f32) -> (f32, f32) {
        let mut modal_sum = 0.0_f32;
        for i in 0..MODAL_COUNT {
            modal_sum += self.modal_bank[i].process(x) * self.modal_gains[i];
        }
        // Diffuse direct bridge knock impulse + resonant modal sum through the 4-tap Householder FDN
        let diffused = self.diffuser.process(0.65 * x + 0.35 * modal_sum);
        (flush_denormal(modal_sum), diffused)
    }

    #[inline(always)]
    fn process_sample(&mut self, x: f32, profile: InstrumentProfile, modal_knock_air: f32) -> f32 {
        let knock = modal_knock_air.clamp(0.0, 1.0);
        match profile {
            InstrumentProfile::AcousticGuitar => {
                // Parallel 12-mode wooden body bank + 4-tap Householder micro soundboard diffusion
                // combined with the 3 user-editable acoustic resonance bands
                let (modal_sum, diffused) = self.process_modal_and_diffusion(x);
                let user_modes = self.mode1.process(x) * 1.15
                    + self.mode2.process(x) * 0.95
                    + self.mode3.process(x) * 0.80;
                let wood_body = modal_sum * 0.82 + diffused * 0.68;
                let wet = 0.46 * user_modes + 0.88 * knock * wood_body;
                let dry = self.cab1.process(x);
                0.58 * dry + 0.42 * wet
            }
            InstrumentProfile::ElectricClean => {
                let pu = self.pickup_lp.process(x);
                // Subtle warm tube compression via tanh Padé approximant
                let driven = tanh_pade(pu * 1.35) * 0.85;
                let s1 = self.cab1.process(driven);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                let cab_out = self.cab4.process(s3);
                let (modal_sum, diffused) = self.process_modal_and_diffusion(cab_out);
                cab_out * (1.0 - 0.16 * knock) + (modal_sum * 0.28 + diffused * 0.36) * knock
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
                let cab_out = self.cab4.process(s3);
                let (modal_sum, diffused) = self.process_modal_and_diffusion(cab_out);
                cab_out * (1.0 - 0.16 * knock) + (modal_sum * 0.26 + diffused * 0.34) * knock
            }
            InstrumentProfile::FingeredBass => {
                let pu = self.pickup_lp.process(x);
                let warm = tanh_pade(pu * 1.25) * 0.92;
                let s1 = self.cab1.process(warm);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                let cab_out = self.cab4.process(s3);
                let (modal_sum, diffused) = self.process_modal_and_diffusion(cab_out);
                cab_out * (1.0 - 0.18 * knock) + (modal_sum * 0.30 + diffused * 0.34) * knock
            }
            InstrumentProfile::PickedBass => {
                let pu = self.pickup_lp.process(x);
                let grit = tanh_pade(pu * 1.65) * 0.82;
                let s1 = self.cab1.process(grit);
                let s2 = self.cab2.process(s1);
                let s3 = self.cab3.process(s2);
                let cab_out = self.cab4.process(s3);
                let (modal_sum, diffused) = self.process_modal_and_diffusion(cab_out);
                cab_out * (1.0 - 0.16 * knock) + (modal_sum * 0.28 + diffused * 0.34) * knock
            }
            InstrumentProfile::HarpOrLute => {
                let (modal_sum, diffused) = self.process_modal_and_diffusion(x);
                let user_modes = self.mode1.process(x) * 1.0
                    + self.mode2.process(x) * 0.85
                    + self.mode3.process(x) * 0.70;
                let wood_body = modal_sum * 0.78 + diffused * 0.64;
                let wet = 0.46 * user_modes + 0.86 * knock * wood_body;
                let dry = self.cab1.process(x);
                0.62 * dry + 0.38 * wet
            }
        }
    }
}

/// Stereo post-string body and cabinet resonator (`BodyResonator`) combining a 12-mode physical
/// wood/cavity bank, 4-tap Householder micro soundboard diffusion, and 4-band EQ / cabinet chain.
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
            left: ChannelResonatorBank::new(false),
            right: ChannelResonatorBank::new(true),
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

    /// Exposes the left & right Householder micro-diffusion tap lengths in samples for diagnostics/testing.
    #[inline(always)]
    pub fn diffuser_tap_delays(&self) -> ([usize; HOUSEHOLDER_TAPS], [usize; HOUSEHOLDER_TAPS]) {
        (
            self.left.diffuser.tap_delays(),
            self.right.diffuser.tap_delays(),
        )
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

    /// Updates the active [`InstrumentProfile`] and recalculates biquad/modal/diffuser coefficients
    /// without clearing filter delay states (preventing clicks on live preset changes).
    pub fn set_profile(&mut self, profile: InstrumentProfile) {
        self.profile = profile;
        self.custom_params = None;
        self.post_gain_amp = 1.0;
        self.left.configure(self.sample_rate, profile);
        self.right.configure(self.sample_rate, profile);
    }

    /// Applies custom parameter overrides for the body resonator, modal knock/air, drive, and 4-band filters.
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
            let knock = cp.modal_knock_air.clamp(0.0, 1.0);
            let in_l = left * drive;
            let in_r = right * drive;

            let out_l = self.left.process_sample(in_l, self.profile, knock);
            let out_r = self.right.process_sample(in_r, self.profile, knock);

            let wet_mix = cp.body_wet_mix.clamp(0.0, 1.0);
            let mix_l = (1.0 - wet_mix) * left + wet_mix * out_l;
            let mix_r = (1.0 - wet_mix) * right + wet_mix * out_r;

            let gain = self.post_gain_amp;
            (flush_denormal(mix_l * gain), flush_denormal(mix_r * gain))
        } else {
            let knock = self.profile.default_modal_knock_air();
            let out_l = self.left.process_sample(left, self.profile, knock);
            let out_r = self.right.process_sample(right, self.profile, knock);
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

            // Feed an impulse followed by 4096 zeros and verify finite, decaying response
            let (l0, r0) = body.process_stereo(0.8, 0.8);
            assert!(l0.is_finite() && r0.is_finite());

            let mut total_energy = l0 * l0 + r0 * r0;
            let mut late_energy = 0.0_f32;
            for i in 0..4096 {
                let (l, r) = body.process_stereo(0.0, 0.0);
                assert!(l.is_finite() && r.is_finite());
                let e = l * l + r * r;
                total_energy += e;
                if i >= 3500 {
                    late_energy += e;
                }
            }
            assert!(
                total_energy > 0.01,
                "Profile {:?} produced insufficient impulse energy: {}",
                profile,
                total_energy
            );
            assert!(
                late_energy < total_energy * 0.15,
                "Profile {:?} impulse response did not decay cleanly (total={}, late={})",
                profile,
                total_energy,
                late_energy
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
    fn test_householder_micro_diffuser_tap_bounds_and_modal_knock_scaling() {
        // Verify Householder tap delays stay strictly < 120 samples AND < 2.5 ms at all sample rates
        for sr in [44_100.0_f32, 48_000.0, 88_200.0, 96_000.0, 192_000.0] {
            let body = BodyResonator::new(sr, InstrumentProfile::AcousticGuitar);
            let (left_d, right_d) = body.diffuser_tap_delays();
            for d in left_d.iter().chain(right_d.iter()) {
                assert!(
                    *d > 0 && *d < 120,
                    "Householder tap delay {} at sr={} must be strictly < 120 samples",
                    d,
                    sr
                );
                let ms = (*d as f32) * 1000.0 / sr;
                assert!(
                    ms < 2.5,
                    "Householder tap delay {:.3} ms at sr={} must be strictly < 2.5 ms",
                    ms,
                    sr
                );
            }
        }

        // Verify modal_knock_air smoothly controls 12-mode wood bank + diffuser richness
        let mut body_dry = BodyResonator::new(48_000.0, InstrumentProfile::AcousticGuitar);
        let mut body_wet = BodyResonator::new(48_000.0, InstrumentProfile::AcousticGuitar);
        let mut p_dry = BodyResonatorParams::default();
        p_dry.body_wet_mix = 1.0;
        p_dry.modal_knock_air = 0.0;
        let mut p_wet = p_dry;
        p_wet.modal_knock_air = 1.0;
        body_dry.update_params(&p_dry);
        body_wet.update_params(&p_wet);

        let mut diff_energy = 0.0_f32;
        let (l0d, r0d) = body_dry.process_stereo(0.9, 0.9);
        let (l0w, r0w) = body_wet.process_stereo(0.9, 0.9);
        diff_energy += (l0w - l0d).powi(2) + (r0w - r0d).powi(2);
        for _ in 0..2048 {
            let (ld, rd) = body_dry.process_stereo(0.0, 0.0);
            let (lw, rw) = body_wet.process_stereo(0.0, 0.0);
            assert!(lw.is_finite() && rw.is_finite());
            diff_energy += (lw - ld).powi(2) + (rw - rd).powi(2);
        }
        assert!(
            diff_energy > 0.02,
            "Wood Modal Knock / Air should noticeably shape acoustic body impulse (diff={})",
            diff_energy
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
