//! Extended Karplus-Strong (EKS) physical string waveguide model with Modartt-style physical
//! modeling realism (`src/synth/waveguide.rs`).
//!
//! ### DSP Pipeline per String (`StringWaveguide`)
//! 1. **Composite Physical Pluck Excitation & Pre-Pluck Damping:**
//!    - Smooth raised-cosine displacement pulse (width controlled by `pick_hardness`) combined with
//!      a 4 ms filtered winding-scrape transient (`noise_mix`), shaped by a velocity-sensitive
//!      one-pole lowpass and a pick-position feed-forward comb filter.
//!    - Re-plucking an already vibrating string triggers a 4 ms rapid pre-pluck damping ramp
//!      (modeling the pick/finger muting the string upon contact) before releasing the new impulse.
//! 2. **Dual-Polarization Coupled Fractional Delay Lines:**
//!    - Two pre-allocated, heap-boxed circular buffers (`[f32; 8192]` each):
//!      - **Horizontal polarization ($H$):** Long $T_{60}$ sustain parallel to the soundboard.
//!      - **Vertical polarization ($V$):** Shorter $T_{60}$ ($\approx 35\%$ of $H$) for initial
//!        attack punch, detuned by `polarization_detune_cents` (default $\sim 0.7$ cents).
//!    - Coupled at the bridge termination via a reciprocal passive $2 \times 2$ mixing matrix
//!      (`polarization_coupling`, default `0.012`) for natural two-stage decay and harmonic bloom.
//! 3. **Tension Modulation & Asymmetric Fret Buzz Non-Linearity:**
//!    - Fast energy envelope follower of squared waveguide signal + slope energy inversely modulates
//!      effective fractional delay (`tension_mod_amount`), yielding a realistic pitch-drop envelope
//!      over the first 50–100 ms on hard plucks while settling cleanly on exact pitch.
//!    - Passive unilateral soft-clipping barrier on the vertical (fretboard-facing) excursion when
//!      instantaneous amplitude exceeds `fret_clearance` (`fret_buzz_sizzle`), adding authentic
//!      acoustic fret sizzle only on hard plucks.
//! 4. **Sympathetic Bridge Coupling:**
//!    - Unmuted strings exchange energy through a shared scalar `bridge_bus` (`sympathetic_coupling`,
//!      default `0.0025`) using a provably passive doubly-stochastic bridge scattering matrix so
//!      chords and open strings resonate sympathetically without feedback instability.
//! 5. **Loop Damping, Stiffness Dispersion & Articulations:**
//!    - One-zero loop lowpass $H(z) = g \cdot ((1 - S) + S \cdot z^{-1})$, 1st-order stiffness
//!      all-pass filter, DC blocker ($R = 0.995$), and strict denormal flushing everywhere.
//!    - Smooth delay slewing for `SlideString` and `SlurString`, palm mute damping, natural/artificial
//!      harmonic node coupling, and zero pitch-drift note-off release (`mute()`).

use crate::synth::backend::flush_denormal;
use crate::synth::body::{tanh_pade, InstrumentProfile};
use crate::synth::patch::StringSynthParams;
use std::f32::consts::PI;

/// Maximum number of simultaneous physical strings supported per waveguide bank.
pub const MAX_STRINGS: usize = 12;

/// Pre-allocated circular buffer length per polarization (power of two: 8192 samples).
/// Supports down to $96000 / 8192 \approx 11.7\text{ Hz}$ at $96\text{ kHz}$.
pub const DELAY_BUFFER_SIZE: usize = 8192;
const DELAY_BUFFER_MASK: usize = DELAY_BUFFER_SIZE - 1;

/// Pre-allocated excitation scratch buffer size for pick-position comb filtering.
const EXCITATION_BUFFER_SIZE: usize = 2048;
const EXCITATION_BUFFER_MASK: usize = EXCITATION_BUFFER_SIZE - 1;

/// Default vertical-to-horizontal T60 ratio (~35% of horizontal sustain for realistic initial punch).
const VERTICAL_T60_RATIO: f32 = 0.35;

/// Deferred pluck payload held during the 4 ms pre-pluck pick-contact damping ramp.
#[derive(Clone, Copy, Debug, Default)]
struct PendingPluck {
    active: bool,
    string_idx: u8,
    freq_hz: f64,
    velocity: f32,
    pluck_pos: f32,
    palm_mute: f32,
    harmonic_node: Option<u8>,
}

/// Allocates a zeroed fixed-size `Box<[f32; N]>` directly on the heap without staging a large
/// array on the call stack.
fn alloc_zeroed_box<const N: usize>() -> Box<[f32; N]> {
    match vec![0.0_f32; N].into_boxed_slice().try_into() {
        Ok(b) => b,
        Err(_) => unreachable!(),
    }
}

/// Maps normalized `fret_buzz_sizzle` in `[0.0, 1.0]` to the unilateral vertical `fret_clearance`
/// excursion threshold inside the waveguide loop.
#[inline(always)]
fn fret_clearance_from_sizzle(sizzle: f32) -> f32 {
    let s = sizzle.clamp(0.0, 1.0);
    (0.32 - 0.24 * s).clamp(0.06, 1.0)
}

/// Single physical string waveguide state using dual-polarization Extended Karplus-Strong (EKS)
/// with tension modulation, fret-buzz non-linearity, physical pluck contour, pre-pluck damping,
/// and sympathetic bridge coupling.
#[derive(Clone)]
pub struct StringWaveguide {
    /// Horizontal polarization circular waveguide delay line (long T60 sustain).
    delay_line_h: Box<[f32; DELAY_BUFFER_SIZE]>,
    /// Vertical polarization circular waveguide delay line (short T60 punch, detuned).
    delay_line_v: Box<[f32; DELAY_BUFFER_SIZE]>,
    /// Circular buffer for pick-position feed-forward comb filter on the excitation signal.
    exc_comb_buf: Box<[f32; EXCITATION_BUFFER_SIZE]>,
    write_idx: usize,
    exc_write_idx: usize,

    pub active: bool,
    pub string_idx: u8,
    pub profile: InstrumentProfile,
    sample_rate: f32,

    /// Current effective horizontal delay length in samples (slewed sample-by-sample during slides/slurs).
    current_delay: f32,
    target_delay: f32,
    delay_slew_step: f32,
    glide_samples_remaining: u32,

    /// Loop filter parameters: $H(z) = g \cdot ((1 - S) + S \cdot z^{-1})$.
    loop_gain_g: f32,
    loop_gain_g_v: f32,
    loop_stretch_s: f32,
    palm_mute: f32,
    lp_z1_h: f32,
    lp_z1_v: f32,

    /// 1st-order Thiran / stiffness dispersion all-pass filter states (H & V).
    ap_coeff: f32,
    ap_x1_h: f32,
    ap_y1_h: f32,
    ap_x1_v: f32,
    ap_y1_v: f32,

    /// Sub-sonic in-loop DC blocker pole (`f_c = 1.5 Hz`, phase-compensated in `compute_loop_parameters`)
    /// for H & V polarizations, preserving full low-string fundamental sustain and exact microtonal pitch.
    dc_r: f32,
    dc_x1_h: f32,
    dc_y1_h: f32,
    dc_x1_v: f32,
    dc_y1_v: f32,

    /// Pre-loop excitation DC blocker state ($R = 0.992$) removing raised-cosine pulse DC before loop entry.
    exc_dc_x1: f32,
    exc_dc_y1: f32,

    /// Dual-polarization detune and reciprocal $2 \times 2$ bridge mixing coefficient.
    polarization_detune_cents: f32,
    detune_ratio_v: f32,
    polarization_coupling: f32,

    /// Tension modulation envelope follower state (fast + slow trackers of squared waveguide energy).
    tension_mod_amount: f32,
    energy_env: f32,
    energy_slow: f32,
    energy_atk_coeff: f32,
    energy_rel_coeff: f32,
    energy_slow_coeff: f32,

    /// Asymmetric fret-buzz barrier state.
    fret_buzz_sizzle: f32,
    fret_clearance: f32,

    /// Sympathetic bridge coupling parameter & previous bridge output.
    pub sympathetic_coupling: f32,
    last_bridge_out: f32,
    /// Whether this string has been tuned via at least one trigger (enables sympathetic wake-up).
    pub was_tuned: bool,
    /// Whether the player explicitly muted this string (blocks sympathetic excitation until re-triggered).
    pub muted_by_player: bool,

    /// Composite physical excitation generator state (raised-cosine displacement + 4ms winding scrape).
    exc_samples_elapsed: u32,
    exc_total_samples: u32,
    exc_pulse_samples: u32,
    exc_scrape_samples: u32,
    exc_amplitude: f32,
    exc_lp_coeff: f32,
    exc_lp_z1: f32,
    scrape_hp_x1: f32,
    scrape_hp_y1: f32,
    scrape_lp_coeff: f32,
    scrape_lp_z1: f32,
    exc_comb_delay: usize,

    /// 4 ms pre-pluck rapid damping ramp when re-plucking an already vibrating string.
    pre_pluck_samples_remaining: u32,
    pre_pluck_total_samples: u32,
    pre_pluck_damp_per_sample: f32,
    pending_pluck: PendingPluck,

    /// Harmonic node coupling state (active for the first 30ms when `harmonic_node: Some(n)`).
    harmonic_node: u8,
    harmonic_samples_remaining: u32,
    harmonic_total_samples: u32,

    /// Deterministic real-time PRNG state (xorshift32).
    rng_state: u32,
    /// Stereo pan position in `[0.0, 1.0]`.
    pub pan: f32,
    /// Silence counter to deactivate the waveguide once energy decays below the noise floor.
    quiet_samples: u32,
    /// Live custom patch parameters for this string.
    pub custom_params: Option<StringSynthParams>,
    pub exc_noise_mix: f32,
    /// Whether the string is actively undergoing exponential release / muting.
    pub is_muting: bool,
    /// Release envelope amplitude multiplier (decays smoothly per-sample during note-off).
    pub mute_env: f32,
    /// Per-sample multiplier for the release envelope.
    pub mute_rate: f32,
}

impl Default for StringWaveguide {
    fn default() -> Self {
        Self::new(48_000.0, 1)
    }
}

impl StringWaveguide {
    pub fn new(sample_rate: f32, string_idx: u8) -> Self {
        let sr = sample_rate.max(8_000.0);
        let seed = 0x9E37_79B9u32.wrapping_mul((string_idx as u32).wrapping_add(1));
        let default_detune = 0.7_f32;
        let default_coupling = 0.012_f32;
        let default_sizzle = 0.20_f32;
        let energy_atk_coeff = 1.0 - (-1.0 / (0.0015 * sr)).exp();
        let energy_rel_coeff = 1.0 - (-1.0 / (0.035 * sr)).exp();
        let energy_slow_coeff = 1.0 - (-1.0 / (0.016 * sr)).exp();
        let dc_r = (1.0 - (2.0 * PI * 1.5 / sr)).clamp(0.995, 0.99995);

        Self {
            delay_line_h: alloc_zeroed_box::<DELAY_BUFFER_SIZE>(),
            delay_line_v: alloc_zeroed_box::<DELAY_BUFFER_SIZE>(),
            exc_comb_buf: alloc_zeroed_box::<EXCITATION_BUFFER_SIZE>(),
            write_idx: 0,
            exc_write_idx: 0,
            active: false,
            string_idx,
            profile: InstrumentProfile::AcousticGuitar,
            sample_rate: sr,
            current_delay: 100.0,
            target_delay: 100.0,
            delay_slew_step: 0.0,
            glide_samples_remaining: 0,
            loop_gain_g: 0.995,
            loop_gain_g_v: 0.986,
            loop_stretch_s: 0.15,
            palm_mute: 0.0,
            lp_z1_h: 0.0,
            lp_z1_v: 0.0,
            ap_coeff: -0.1,
            ap_x1_h: 0.0,
            ap_y1_h: 0.0,
            ap_x1_v: 0.0,
            ap_y1_v: 0.0,
            dc_r,
            dc_x1_h: 0.0,
            dc_y1_h: 0.0,
            dc_x1_v: 0.0,
            dc_y1_v: 0.0,
            exc_dc_x1: 0.0,
            exc_dc_y1: 0.0,
            polarization_detune_cents: default_detune,
            detune_ratio_v: 2.0_f32.powf(-default_detune / 1200.0),
            polarization_coupling: default_coupling,
            tension_mod_amount: 0.30,
            energy_env: 0.0,
            energy_slow: 0.0,
            energy_atk_coeff,
            energy_rel_coeff,
            energy_slow_coeff,
            fret_buzz_sizzle: default_sizzle,
            fret_clearance: fret_clearance_from_sizzle(default_sizzle),
            sympathetic_coupling: 0.0025,
            last_bridge_out: 0.0,
            was_tuned: false,
            muted_by_player: false,
            exc_samples_elapsed: 0,
            exc_total_samples: 0,
            exc_pulse_samples: 0,
            exc_scrape_samples: 0,
            exc_amplitude: 0.0,
            exc_lp_coeff: 0.3,
            exc_lp_z1: 0.0,
            scrape_hp_x1: 0.0,
            scrape_hp_y1: 0.0,
            scrape_lp_coeff: 0.35,
            scrape_lp_z1: 0.0,
            exc_comb_delay: 8,
            pre_pluck_samples_remaining: 0,
            pre_pluck_total_samples: 0,
            pre_pluck_damp_per_sample: 1.0,
            pending_pluck: PendingPluck::default(),
            harmonic_node: 1,
            harmonic_samples_remaining: 0,
            harmonic_total_samples: 0,
            rng_state: if seed == 0 { 0x1234_5678 } else { seed },
            pan: (0.30 + ((string_idx % MAX_STRINGS as u8) as f32) * 0.035).clamp(0.15, 0.85),
            quiet_samples: 0,
            custom_params: None,
            exc_noise_mix: 0.65,
            is_muting: false,
            mute_env: 1.0,
            mute_rate: 1.0,
        }
    }

    #[inline(always)]
    pub fn update_params(&mut self, params: &StringSynthParams) {
        self.custom_params = Some(*params);
        self.exc_noise_mix = params.noise_mix.clamp(0.0, 1.0);

        let detune = params.polarization_detune_cents.clamp(0.0, 3.0);
        self.polarization_detune_cents = detune;
        self.detune_ratio_v = 2.0_f32.powf(-detune / 1200.0);

        let coupling = params.polarization_coupling.clamp(0.0, 0.06);
        self.polarization_coupling = coupling;

        self.tension_mod_amount = params.tension_mod_amount.clamp(0.0, 1.0);
        self.fret_buzz_sizzle = params.fret_buzz_sizzle.clamp(0.0, 1.0);
        self.fret_clearance = fret_clearance_from_sizzle(self.fret_buzz_sizzle);
        self.sympathetic_coupling = params.sympathetic_coupling.clamp(0.0, 0.015);

        if self.active {
            let clamped_freq = (self.sample_rate / self.current_delay.max(2.0))
                .clamp(15.0, self.sample_rate * 0.42);
            let t60_h = if self.palm_mute > 0.001 {
                params.t60_decay_sec
                    * (1.0 - 0.92 * self.palm_mute * (params.palm_mute_damping / 0.22)).max(0.04)
            } else {
                params.t60_decay_sec
            };
            let t60_v = (t60_h * VERTICAL_T60_RATIO).max(0.03);

            let periods_h = (t60_h * clamped_freq).max(1.0);
            let periods_v = (t60_v * clamped_freq).max(1.0);
            let mut gain_h = 10.0_f32.powf(-3.0 / periods_h);
            let mut gain_v = 10.0_f32.powf(-3.0 / periods_v);
            if self.palm_mute > 0.001 {
                let pm_factor = 1.0 - 0.045 * self.palm_mute;
                gain_h *= pm_factor;
                gain_v *= pm_factor;
            }
            self.loop_gain_g = gain_h.clamp(0.85, 0.9999);
            self.loop_gain_g_v = gain_v.clamp(0.70, 0.9999);
            self.loop_stretch_s = (params.brightness_s + self.palm_mute * 0.52).clamp(0.01, 0.95);
            self.ap_coeff = (-params.inharmonicity_dispersion).clamp(-0.80, -0.005);
        }
    }

    #[inline(always)]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(8_000.0);
        self.sample_rate = sr;
        self.energy_atk_coeff = 1.0 - (-1.0 / (0.0015 * sr)).exp();
        self.energy_rel_coeff = 1.0 - (-1.0 / (0.035 * sr)).exp();
        self.energy_slow_coeff = 1.0 - (-1.0 / (0.016 * sr)).exp();
        self.dc_r = (1.0 - (2.0 * PI * 1.5 / sr)).clamp(0.995, 0.99995);
    }

    #[inline(always)]
    pub fn set_profile(&mut self, profile: InstrumentProfile) {
        self.profile = profile;
    }

    /// Returns `true` if this string is eligible to participate in sympathetic bridge coupling.
    #[inline(always)]
    pub fn is_sympathetically_coupled(&self) -> bool {
        self.active
            && !self.is_muting
            && !self.muted_by_player
            && self.pre_pluck_samples_remaining == 0
            && self.palm_mute < 0.35
    }

    /// Wakes an already-tuned, unmuted silent string when the shared `bridge_bus` carries
    /// noticeable vibration energy so open/unmuted strings resonate sympathetically.
    #[inline(always)]
    pub fn try_sympathetic_wake(&mut self, bridge_bus_in: f32) -> bool {
        if !self.active
            && self.was_tuned
            && !self.muted_by_player
            && self.palm_mute < 0.35
            && self.sympathetic_coupling > 1.0e-5
            && bridge_bus_in.abs() > 2.0e-4
        {
            self.active = true;
            self.is_muting = false;
            self.mute_env = 1.0;
            self.mute_rate = 1.0;
            self.quiet_samples = 0;
            true
        } else {
            false
        }
    }

    /// Fast, real-time xorshift32 PRNG returning uniform white noise in `[-1.0, 1.0]`.
    #[inline(always)]
    fn next_white_noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = if x == 0 { 0x6D2B_79F5 } else { x };
        ((x as i32) as f32) * (1.0 / 2_147_483_648.0)
    }

    /// Passive asymmetric fret-buzz barrier triggered only when positive vertical (fretboard-side)
    /// excursion exceeds `clearance`. Strictly passive (`|out| <= |x|`), ensuring unconditional
    /// waveguide loop stability.
    #[inline(always)]
    fn apply_fret_barrier(x: f32, clearance: f32, buzz_amount: f32) -> f32 {
        if buzz_amount <= 1.0e-4 || x <= clearance {
            return x;
        }
        let excess = x - clearance;
        let knee = (0.14 * (1.0 - 0.65 * buzz_amount)).max(0.03);
        let clipped = clearance + knee * tanh_pade(excess / knee);
        // Subtle elastic fret-crown reflection adds metallic upper-harmonic sizzle while remaining passive
        let rattle = 0.22 * buzz_amount * (excess - (clipped - clearance));
        (clipped - rattle).max(-x.abs())
    }

    /// Computes loop filter coefficients (`effective_delay_h`, `gain_g_h`, `gain_g_v`,
    /// `loop_stretch_s`, `ap_coeff`) and phase-compensated fractional delay length for `freq_hz`.
    #[inline(always)]
    fn compute_loop_parameters(&self, freq_hz: f64, palm_mute: f32) -> (f32, f32, f32, f32, f32) {
        let sr = self.sample_rate as f64;
        let clamped_freq = freq_hz.clamp(15.0, sr * 0.42);
        let raw_period = (sr / clamped_freq) as f32;
        let prof = self.profile.waveguide_params();

        let mute = palm_mute.clamp(0.0, 1.0);

        let (base_s, base_t60, base_ap) = if let Some(cp) = self.custom_params {
            (
                cp.brightness_s,
                cp.t60_decay_sec,
                (-cp.inharmonicity_dispersion).clamp(-0.80, -0.005),
            )
        } else {
            let s = ((0.06 + (180.0 / (clamped_freq as f32 + 120.0)) * 0.18) * prof.damping_s_mult)
                .clamp(0.03, 0.42);
            let t = ((4.2 - (clamped_freq as f32) * 0.0018) * prof.sustain_mult).clamp(1.2, 6.8);
            let ap = if clamped_freq < 260.0 {
                let bass_factor = ((260.0 - clamped_freq as f32) / 245.0).clamp(0.0, 1.0);
                ((-0.04 - 0.28 * bass_factor) * prof.dispersion_mult).clamp(-0.42, -0.01)
            } else {
                (-0.02 * prof.dispersion_mult).clamp(-0.15, -0.005)
            };
            (s, t, ap)
        };

        // Brightness / stretching factor S in H(z) = g * ((1 - S) + S * z^-1):
        let stretch_s = (base_s + mute * 0.52).clamp(0.01, 0.95);

        // Horizontal T60 (long sustain) and Vertical T60 (~35% of horizontal for initial punch)
        let t60_h = if mute > 0.001 {
            let mute_scale = self
                .custom_params
                .map(|cp| cp.palm_mute_damping / 0.22)
                .unwrap_or(1.0);
            base_t60 * (1.0 - 0.92 * mute * mute_scale).max(0.04)
        } else {
            base_t60
        };
        let t60_v = (t60_h * VERTICAL_T60_RATIO).max(0.03);

        // Per-period attenuation for -60 dB (10^-3) over T60 seconds:
        let periods_h = (t60_h * (clamped_freq as f32)).max(1.0);
        let periods_v = (t60_v * (clamped_freq as f32)).max(1.0);
        let mut gain_g_h = 10.0_f32.powf(-3.0 / periods_h);
        let mut gain_g_v = 10.0_f32.powf(-3.0 / periods_v);
        if mute > 0.001 {
            let pm_factor = 1.0 - 0.045 * mute;
            gain_g_h *= pm_factor;
            gain_g_v *= pm_factor;
        }
        let gain_g_h = gain_g_h.clamp(0.85, 0.9999);
        let gain_g_v = gain_g_v.clamp(0.70, 0.9999);
        let ap_coeff = base_ap;

        // Phase delay compensation at fundamental omega_0 = 2 * pi * f0 / sr:
        let omega = (2.0 * std::f64::consts::PI * clamped_freq / sr) as f32;
        let (sin_w, cos_w) = omega.sin_cos();

        // 1) One-zero lowpass phase delay
        let lp_num = stretch_s * sin_w;
        let lp_den = (1.0 - stretch_s) + stretch_s * cos_w;
        let lp_phase_delay = if omega > 1.0e-5 {
            lp_num.atan2(lp_den) / omega
        } else {
            stretch_s
        };

        // 2) Exact 1st-order all-pass phase delay at omega_0: H_ap(z) = (a + z^-1) / (1 + a*z^-1)
        let ap_phase_delay = if omega > 1.0e-5 {
            let num_ang = (-sin_w).atan2(ap_coeff + cos_w);
            let den_ang = (-ap_coeff * sin_w).atan2(1.0 + ap_coeff * cos_w);
            let mut phi = -(num_ang - den_ang);
            if phi < 0.0 {
                phi += 2.0 * PI;
            }
            (phi / omega).clamp(0.0, 4.0)
        } else {
            ((1.0 - ap_coeff) / (1.0 + ap_coeff).max(0.1)).clamp(0.0, 4.0)
        };

        // 3) Exact sub-sonic loop DC blocker phase delay at omega_0 (slight phase lead -> negative delay)
        let r_dc = self.dc_r;
        let dc_phase_delay = if omega > 1.0e-5 {
            let num_ang = sin_w.atan2(1.0 - cos_w);
            let den_ang = (r_dc * sin_w).atan2(1.0 - r_dc * cos_w);
            (-(num_ang - den_ang) / omega).clamp(-2.0, 0.0)
        } else {
            0.0
        };

        let effective_delay = (raw_period - lp_phase_delay - ap_phase_delay - dc_phase_delay)
            .clamp(4.0, (DELAY_BUFFER_SIZE - 8) as f32);

        (effective_delay, gain_g_h, gain_g_v, stretch_s, ap_coeff)
    }

    /// Plucks the string with the given `freq_hz`, `velocity`, `pluck_pos`, `palm_mute`, and `harmonic_node`.
    ///
    /// When the string is currently silent, excitation fires immediately with zero latency.
    /// When re-plucking an already vibrating string, a 4 ms rapid pre-pluck damping ramp models
    /// the pick/finger muting the string on contact before releasing the new displacement impulse.
    #[inline(always)]
    pub fn trigger(
        &mut self,
        string_idx: u8,
        freq_hz: f64,
        velocity: f32,
        pluck_pos: f32,
        palm_mute: f32,
        harmonic_node: Option<u8>,
    ) {
        let pluck = PendingPluck {
            active: true,
            string_idx,
            freq_hz,
            velocity,
            pluck_pos,
            palm_mute,
            harmonic_node,
        };

        if self.active {
            // Re-plucking an already vibrating string: initiate (or update) a 4 ms rapid pre-pluck
            // damping ramp modeling pick/finger contact before launching the release impulse.
            self.pending_pluck = pluck;
            if self.pre_pluck_samples_remaining == 0 {
                let damp_samples = (0.004 * self.sample_rate).round().max(4.0) as u32;
                self.pre_pluck_total_samples = damp_samples;
                self.pre_pluck_samples_remaining = damp_samples;
                // Exponential decay by ~40 dB over 4 ms (per-sample factor ~0.977 at 48 kHz)
                self.pre_pluck_damp_per_sample = (-4.6 / (damp_samples as f32)).exp();
            }
            self.is_muting = false;
            self.muted_by_player = false;
            self.quiet_samples = 0;
        } else {
            self.pre_pluck_samples_remaining = 0;
            self.pending_pluck.active = false;
            self.launch_pluck(pluck);
        }
    }

    /// Commits a pluck's loop parameters and composite physical excitation (raised-cosine
    /// displacement pulse + 4 ms filtered winding-scrape transient).
    #[inline(always)]
    fn launch_pluck(&mut self, pluck: PendingPluck) {
        self.string_idx = pluck.string_idx;
        self.palm_mute = pluck.palm_mute.clamp(0.0, 1.0);
        self.pan = (0.30 + ((pluck.string_idx % MAX_STRINGS as u8) as f32) * 0.035).clamp(0.15, 0.85);
        let prof = self.profile.waveguide_params();

        let (delay, g_h, g_v, s, ap) = self.compute_loop_parameters(pluck.freq_hz, self.palm_mute);
        self.current_delay = delay;
        self.target_delay = delay;
        self.delay_slew_step = 0.0;
        self.glide_samples_remaining = 0;
        self.loop_gain_g = g_h;
        self.loop_gain_g_v = g_v;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;

        // If coming out of pre-pluck damping, ensure any residual energy in both delay lines is
        // cleanly attenuated without abrupt discontinuity
        for sample in self.delay_line_h.iter_mut() {
            *sample = flush_denormal(*sample * 0.08);
        }
        for sample in self.delay_line_v.iter_mut() {
            *sample = flush_denormal(*sample * 0.08);
        }
        self.exc_comb_buf.fill(0.0);
        self.exc_write_idx = 0;

        let vel = pluck.velocity.clamp(0.02, 1.0);

        // Determine normalized pick hardness in [0, 1] (logarithmic over 500..16000 Hz)
        let norm_hardness = if let Some(cp) = self.custom_params {
            self.exc_noise_mix = cp.noise_mix.clamp(0.0, 1.0);
            let h_hz = cp.pick_hardness.clamp(500.0, 16_000.0);
            ((h_hz.ln() - 500.0_f32.ln()) / (16_000.0_f32.ln() - 500.0_f32.ln())).clamp(0.0, 1.0)
        } else {
            (0.58 - prof.exc_softness_offset * 1.4).clamp(0.12, 0.92)
        };

        // Composite physical excitation durations:
        // 1) Raised-cosine displacement pulse: 0.35 ms (hard pick) to 3.4 ms (soft finger)
        let pulse_sec = 0.00035 + (1.0 - norm_hardness) * 0.00305;
        let pulse_samples = (pulse_sec * self.sample_rate)
            .round()
            .clamp(4.0, (EXCITATION_BUFFER_SIZE / 2) as f32) as u32;
        // 2) 4 ms filtered winding-scrape transient
        let scrape_samples = (0.004 * self.sample_rate)
            .round()
            .clamp(4.0, (EXCITATION_BUFFER_SIZE / 2) as f32) as u32;
        let burst_len = pulse_samples.max(scrape_samples);

        self.exc_samples_elapsed = 0;
        self.exc_pulse_samples = pulse_samples;
        self.exc_scrape_samples = scrape_samples;
        self.exc_total_samples = burst_len;
        self.exc_amplitude = vel * 0.44;

        // Pick-direction / velocity dynamics one-pole lowpass
        self.exc_lp_coeff = if self.custom_params.is_some() {
            (0.88 - 0.68 * norm_hardness * vel).clamp(0.06, 0.92)
        } else {
            (0.78 - 0.62 * vel + prof.exc_softness_offset).clamp(0.06, 0.92)
        };
        self.exc_lp_z1 = 0.0;
        self.exc_dc_x1 = 0.0;
        self.exc_dc_y1 = 0.0;

        // Winding-scrape bandpass filter state (LP cutoff follows pick hardness & velocity)
        self.scrape_lp_coeff = (0.72 - 0.54 * norm_hardness * vel).clamp(0.12, 0.85);
        self.scrape_hp_x1 = 0.0;
        self.scrape_hp_y1 = 0.0;
        self.scrape_lp_z1 = 0.0;

        // Pick-position comb filter delay: pluck_pos in [0.05, 0.5] of fundamental period
        let base_pluck = self
            .custom_params
            .map(|cp| cp.pluck_pos)
            .unwrap_or(prof.default_pluck_pos);
        let effective_pos = (0.75 * pluck.pluck_pos + 0.25 * base_pluck).clamp(0.05, 0.50);
        self.exc_comb_delay =
            ((effective_pos * delay).round() as usize).clamp(1, EXCITATION_BUFFER_SIZE - 2);

        // Harmonic node coupling during the first 30ms of excitation
        match pluck.harmonic_node {
            Some(n) if n >= 2 => {
                self.harmonic_node = n.min(8);
                let harm_ms = self
                    .custom_params
                    .map(|cp| cp.harmonic_damping_ms)
                    .unwrap_or(30.0);
                let h_samples = ((harm_ms * 0.001) * self.sample_rate).round().max(1.0) as u32;
                self.harmonic_total_samples = h_samples;
                self.harmonic_samples_remaining = h_samples;
            }
            _ => {
                self.harmonic_node = 1;
                self.harmonic_samples_remaining = 0;
                self.harmonic_total_samples = 0;
            }
        }

        // Reset tension energy trackers so hard plucks exhibit a crisp initial pitch-drop envelope
        // over the first 50-100 ms before settling accurately onto the target pitch.
        self.energy_env = 0.0;
        self.energy_slow = 0.0;
        self.last_bridge_out = 0.0;
        self.quiet_samples = 0;
        self.is_muting = false;
        self.muted_by_player = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;
        self.was_tuned = true;
        self.active = true;
    }

    /// Smoothly glides the string's fractional read pointer to `target_freq_hz` over `glide_samples`
    /// without clearing the waveguide buffers.
    #[inline(always)]
    pub fn slide(&mut self, target_freq_hz: f64, glide_samples: u32) {
        if self.pending_pluck.active {
            self.pending_pluck.freq_hz = target_freq_hz;
            return;
        }
        if !self.active {
            return;
        }
        let (target_delay, g_h, g_v, s, ap) =
            self.compute_loop_parameters(target_freq_hz, self.palm_mute);
        self.target_delay = target_delay;
        self.loop_gain_g = g_h;
        self.loop_gain_g_v = g_v;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;
        self.is_muting = false;
        self.muted_by_player = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;

        if glide_samples <= 1 {
            self.current_delay = target_delay;
            self.delay_slew_step = 0.0;
            self.glide_samples_remaining = 0;
        } else {
            self.delay_slew_step = (target_delay - self.current_delay) / (glide_samples as f32);
            self.glide_samples_remaining = glide_samples;
        }
    }

    /// Hammer-on / pull-off (`SlurString`): smoothly slews the read pointer to `new_freq_hz`
    /// WITHOUT clearing the waveguide buffers, injecting only a warm raised-cosine finger impulse.
    #[inline(always)]
    pub fn slur(&mut self, new_freq_hz: f64, hammer_energy: f32) {
        if self.pending_pluck.active {
            self.pending_pluck.freq_hz = new_freq_hz;
            return;
        }
        if !self.active {
            self.trigger(self.string_idx, new_freq_hz, hammer_energy, 0.25, 0.0, None);
            return;
        }
        let (target_delay, g_h, g_v, s, ap) =
            self.compute_loop_parameters(new_freq_hz, self.palm_mute);
        self.target_delay = target_delay;
        self.loop_gain_g = g_h;
        self.loop_gain_g_v = g_v;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;
        self.is_muting = false;
        self.muted_by_player = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;

        // Short ~2.5ms slew to prevent step discontinuity while keeping fretting crisp
        let slew_samples = (0.0025 * self.sample_rate).round().max(8.0) as u32;
        self.delay_slew_step = (target_delay - self.current_delay) / (slew_samples as f32);
        self.glide_samples_remaining = slew_samples;

        // Inject a small, warm raised-cosine finger impulse representing hammering onto the fret
        let energy = hammer_energy.clamp(0.02, 1.0);
        let imp_samples = (target_delay * 0.45).round().clamp(8.0, 200.0) as u32;
        self.exc_samples_elapsed = 0;
        self.exc_pulse_samples = imp_samples;
        self.exc_scrape_samples = 0; // No pick winding scrape on legato slur
        self.exc_total_samples = imp_samples;
        self.exc_amplitude = energy * 0.16;
        self.exc_lp_coeff = 0.78;
        self.quiet_samples = 0;
    }

    /// Dampens the string with the specified exponential release rate without altering
    /// loop filter phase delay, guaranteeing zero pitch drift between note on and note off.
    #[inline(always)]
    pub fn mute(&mut self, release_rate: f32) {
        self.pending_pluck.active = false;
        self.pre_pluck_samples_remaining = 0;
        self.muted_by_player = true;
        if self.active {
            self.is_muting = true;
            // Split the target per-sample release rate evenly between the output envelope and
            // per-period loop attenuation (identical for H and V) so total amplitude decays at
            // the exact requested `release_rate` while both delay lines empty smoothly.
            let target_rate = release_rate.clamp(0.90, 0.9995);
            let half_rate = target_rate.sqrt();
            self.mute_rate = half_rate;

            let per_period = if release_rate > 0.90 {
                half_rate.powf(self.current_delay)
            } else {
                release_rate.clamp(0.01, 0.90)
            };
            self.loop_gain_g = self.loop_gain_g.min(per_period).clamp(0.01, 0.9999);
            self.loop_gain_g_v = self.loop_gain_g_v.min(per_period).clamp(0.01, 0.9999);

            // CRITICAL INVARIANT: We strictly PRESERVE `loop_stretch_s` (and freeze `energy_env`)
            // here so loop phase delay never increases on note-off (0 cents pitch drift).
        }
    }

    /// Immediately silences the string and clears all filter/delay state.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.active = false;
        self.was_tuned = false;
        self.is_muting = false;
        self.muted_by_player = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;
        self.delay_line_h.fill(0.0);
        self.delay_line_v.fill(0.0);
        self.exc_comb_buf.fill(0.0);
        self.lp_z1_h = 0.0;
        self.lp_z1_v = 0.0;
        self.ap_x1_h = 0.0;
        self.ap_y1_h = 0.0;
        self.ap_x1_v = 0.0;
        self.ap_y1_v = 0.0;
        self.dc_x1_h = 0.0;
        self.dc_y1_h = 0.0;
        self.dc_x1_v = 0.0;
        self.dc_y1_v = 0.0;
        self.exc_dc_x1 = 0.0;
        self.exc_dc_y1 = 0.0;
        self.energy_env = 0.0;
        self.energy_slow = 0.0;
        self.last_bridge_out = 0.0;
        self.exc_samples_elapsed = 0;
        self.exc_total_samples = 0;
        self.exc_pulse_samples = 0;
        self.exc_scrape_samples = 0;
        self.exc_lp_z1 = 0.0;
        self.scrape_hp_x1 = 0.0;
        self.scrape_hp_y1 = 0.0;
        self.scrape_lp_z1 = 0.0;
        self.pre_pluck_samples_remaining = 0;
        self.pending_pluck.active = false;
        self.harmonic_samples_remaining = 0;
        self.glide_samples_remaining = 0;
        self.quiet_samples = 0;
    }

    /// Reads from a circular delay buffer at `delay_samples` behind `write_idx` using
    /// 3rd-order (4-point) Lagrange fractional delay interpolation.
    #[inline(always)]
    fn read_fractional_lagrange(
        buf: &[f32; DELAY_BUFFER_SIZE],
        write_idx: usize,
        delay_samples: f32,
    ) -> f32 {
        let d_clamped = delay_samples.clamp(2.0, (DELAY_BUFFER_SIZE - 4) as f32);
        let d_int = d_clamped.floor() as usize;
        let frac = d_clamped - (d_int as f32);

        let idx0 = write_idx.wrapping_sub(d_int.saturating_sub(1)) & DELAY_BUFFER_MASK;
        let idx1 = write_idx.wrapping_sub(d_int) & DELAY_BUFFER_MASK;
        let idx2 = write_idx.wrapping_sub(d_int + 1) & DELAY_BUFFER_MASK;
        let idx3 = write_idx.wrapping_sub(d_int + 2) & DELAY_BUFFER_MASK;

        let y0 = buf[idx0];
        let y1 = buf[idx1];
        let y2 = buf[idx2];
        let y3 = buf[idx3];

        let c0 = -frac * (frac - 1.0) * (frac - 2.0) * (1.0 / 6.0);
        let c1 = (frac + 1.0) * (frac - 1.0) * (frac - 2.0) * 0.5;
        let c2 = -(frac + 1.0) * frac * (frac - 2.0) * 0.5;
        let c3 = (frac + 1.0) * frac * (frac - 1.0) * (1.0 / 6.0);

        c0 * y0 + c1 * y1 + c2 * y2 + c3 * y3
    }

    /// Advances the waveguide by one sample with sympathetic bridge bus coupling.
    ///
    /// Returns `(left, right, bridge_force)` where `bridge_force` is summed into the part's
    /// shared scalar `bridge_bus` for the next sample frame.
    #[inline(always)]
    pub fn next_frame_with_bridge(
        &mut self,
        bridge_bus_in: f32,
        coupled_count: usize,
    ) -> (f32, f32, f32) {
        if !self.active {
            self.last_bridge_out = 0.0;
            return (0.0, 0.0, 0.0);
        }

        // 0. Advance 4 ms pre-pluck contact damping ramp if a re-pluck is queued
        let mut pre_damp = 1.0_f32;
        let mut pre_out_env = 1.0_f32;
        if self.pre_pluck_samples_remaining > 0 {
            self.pre_pluck_samples_remaining -= 1;
            pre_damp = self.pre_pluck_damp_per_sample;
            pre_out_env = (self.pre_pluck_samples_remaining as f32)
                / (self.pre_pluck_total_samples.max(1) as f32);

            if self.pre_pluck_samples_remaining == 0 && self.pending_pluck.active {
                let pending = self.pending_pluck;
                self.pending_pluck.active = false;
                self.launch_pluck(pending);
                pre_damp = 1.0;
                pre_out_env = 1.0;
            }
        }

        // 1. Slew fractional delay pointer if sliding or slurring
        if self.glide_samples_remaining > 0 {
            self.current_delay += self.delay_slew_step;
            self.glide_samples_remaining -= 1;
            if self.glide_samples_remaining == 0 {
                self.current_delay = self.target_delay;
            }
        }

        // 2. Tension modulation: modulate target fractional delay inversely with string energy.
        // Excess transient energy above the slow sustain floor yields a realistic 50-100 ms
        // pitch-drop envelope on hard plucks while steady-state sustain remains dead-on in tune.
        let excess_e = (self.energy_env - self.energy_slow).max(0.0);
        let tension_signal = (excess_e * 10.5 + 0.03 * self.energy_env).clamp(0.0, 0.85);
        let tension_factor = 1.0 + self.tension_mod_amount * 0.045 * tension_signal;
        let eff_delay_h =
            (self.current_delay / tension_factor).clamp(4.0, (DELAY_BUFFER_SIZE - 8) as f32);
        let eff_delay_v =
            (eff_delay_h * self.detune_ratio_v).clamp(4.0, (DELAY_BUFFER_SIZE - 8) as f32);

        // 3. Composite physical excitation: smooth raised-cosine pulse + 4 ms filtered winding scrape
        let excitation = if self.exc_samples_elapsed < self.exc_total_samples {
            let idx = self.exc_samples_elapsed;
            self.exc_samples_elapsed += 1;

            // 3a. Raised-cosine displacement contour (width controlled by pick_hardness)
            let pulse = if idx < self.exc_pulse_samples {
                let u = (idx as f32 + 0.5) / (self.exc_pulse_samples.max(1) as f32);
                0.5 * (1.0 - (2.0 * PI * u).cos())
            } else {
                0.0
            };

            // 3b. 4 ms filtered winding-scrape transient (highpass + hardness-controlled lowpass noise)
            let scrape = if idx < self.exc_scrape_samples {
                let s_prog = (idx as f32) / (self.exc_scrape_samples.max(1) as f32);
                let win = if s_prog < 0.12 {
                    s_prog / 0.12
                } else {
                    (-(s_prog - 0.12) * 4.2).exp()
                };
                let wn = self.next_white_noise();
                let hp = flush_denormal(0.78 * (self.scrape_hp_y1 + wn - self.scrape_hp_x1));
                self.scrape_hp_x1 = wn;
                self.scrape_hp_y1 = hp;
                let bp = flush_denormal(
                    (1.0 - self.scrape_lp_coeff) * hp + self.scrape_lp_coeff * self.scrape_lp_z1,
                );
                self.scrape_lp_z1 = bp;
                bp * win
            } else {
                0.0
            };

            let raw_exc = ((1.0 - 0.45 * self.exc_noise_mix) * pulse * 1.15
                + self.exc_noise_mix * scrape * 0.95)
                * self.exc_amplitude;

            // Pick-direction / velocity dynamics one-pole lowpass filter
            let lp_exc = flush_denormal(
                (1.0 - self.exc_lp_coeff) * raw_exc + self.exc_lp_coeff * self.exc_lp_z1,
            );
            self.exc_lp_z1 = lp_exc;

            // Feed-forward pick-position comb filter: e_comb[n] = e[n] - 0.85 * e[n - M]
            self.exc_comb_buf[self.exc_write_idx] = lp_exc;
            let delayed_idx =
                self.exc_write_idx.wrapping_sub(self.exc_comb_delay) & EXCITATION_BUFFER_MASK;
            let comb_out = lp_exc - 0.85 * self.exc_comb_buf[delayed_idx];
            self.exc_write_idx = (self.exc_write_idx + 1) & EXCITATION_BUFFER_MASK;

            // Pre-loop DC blocker removes raised-cosine pulse DC offset cleanly before loop entry
            let exc_dc = flush_denormal(comb_out - self.exc_dc_x1 + 0.992 * self.exc_dc_y1);
            self.exc_dc_x1 = flush_denormal(comb_out);
            self.exc_dc_y1 = exc_dc;
            exc_dc
        } else {
            0.0
        };

        // 4. Read fractional delay taps for Horizontal & Vertical polarizations (+ optional harmonic node)
        let mut delayed_h =
            Self::read_fractional_lagrange(&self.delay_line_h, self.write_idx, eff_delay_h);
        let mut delayed_v =
            Self::read_fractional_lagrange(&self.delay_line_v, self.write_idx, eff_delay_v);

        if self.harmonic_samples_remaining > 0 && self.harmonic_node >= 2 {
            self.harmonic_samples_remaining -= 1;
            let n = self.harmonic_node as f32;
            let node_delay_h = (eff_delay_h / n).max(2.0);
            let node_delay_v = (eff_delay_v / n).max(2.0);
            let node_tap_h =
                Self::read_fractional_lagrange(&self.delay_line_h, self.write_idx, node_delay_h);
            let node_tap_v =
                Self::read_fractional_lagrange(&self.delay_line_v, self.write_idx, node_delay_v);
            let blend = (self.harmonic_samples_remaining as f32)
                / (self.harmonic_total_samples.max(1) as f32);
            let coupling = 0.55 * blend;
            delayed_h = (1.0 - coupling) * delayed_h + coupling * node_tap_h;
            delayed_v = (1.0 - coupling) * delayed_v + coupling * node_tap_v;
        }

        // 5. One-zero loop lowpass filters (H: long T60, V: ~35% T60 for initial punch)
        let s = self.loop_stretch_s;
        let lp_out_h = (self.loop_gain_g * pre_damp) * ((1.0 - s) * delayed_h + s * self.lp_z1_h);
        self.lp_z1_h = flush_denormal(delayed_h);

        let lp_out_v =
            (self.loop_gain_g_v * pre_damp) * ((1.0 - s) * delayed_v + s * self.lp_z1_v);
        self.lp_z1_v = flush_denormal(delayed_v);

        // 6. 1st-order stiffness dispersion all-pass filters (H & V)
        let a = self.ap_coeff;
        let ap_out_h = flush_denormal(a * lp_out_h + self.ap_x1_h - a * self.ap_y1_h);
        self.ap_x1_h = flush_denormal(lp_out_h);
        self.ap_y1_h = ap_out_h;

        let ap_out_v = flush_denormal(a * lp_out_v + self.ap_x1_v - a * self.ap_y1_v);
        self.ap_x1_v = flush_denormal(lp_out_v);
        self.ap_y1_v = ap_out_v;

        // 7. Reciprocal passive 2x2 bridge mixing between H & V polarizations:
        //    [h'] = [1 - c,  c  ] [h]
        //    [v'] = [  c,  1 - c] [v]
        // Real eigenvalues (1 and 1 - 2c): unconditionally passive with ZERO gyroscopic phase/pitch shift.
        let c = self.polarization_coupling;
        let one_minus_c = 1.0 - c;
        let coupled_h = one_minus_c * ap_out_h + c * ap_out_v;
        let coupled_v = c * ap_out_h + one_minus_c * ap_out_v;

        // 8. Passive sympathetic bridge bus coupling into unmuted vertical waveguide
        let bridge_in = if !self.is_muting
            && !self.muted_by_player
            && self.pre_pluck_samples_remaining == 0
            && self.palm_mute < 0.35
            && coupled_count > 1
            && self.sympathetic_coupling > 1.0e-6
        {
            let beta = self.sympathetic_coupling.clamp(0.0, 0.015);
            let m = (coupled_count as f32).min(MAX_STRINGS as f32);
            (beta * (bridge_bus_in - m * self.last_bridge_out)).clamp(-0.25, 0.25)
        } else {
            0.0
        };

        // 9. Inject excitation (natural ~47 deg pick angle) + asymmetric fret-buzz barrier + DC blockers
        let loop_in_h = coupled_h + excitation * 0.68;
        let raw_v_in = coupled_v + excitation * 0.73 + bridge_in;
        let loop_in_v =
            Self::apply_fret_barrier(raw_v_in, self.fret_clearance, self.fret_buzz_sizzle);

        let r_dc = self.dc_r;
        let dc_out_h = flush_denormal(loop_in_h - self.dc_x1_h + r_dc * self.dc_y1_h);
        self.dc_x1_h = flush_denormal(loop_in_h);
        self.dc_y1_h = dc_out_h;

        let dc_out_v = flush_denormal(loop_in_v - self.dc_x1_v + r_dc * self.dc_y1_v);
        self.dc_x1_v = flush_denormal(loop_in_v);
        self.dc_y1_v = dc_out_v;

        // 10. Update fast & slow envelope followers of squared waveguide signal energy.
        // Frozen during note-off `is_muting` so release decay never introduces flat pitch drift.
        if !self.is_muting {
            let inst_energy = dc_out_h * dc_out_h + dc_out_v * dc_out_v;
            let coeff = if inst_energy > self.energy_env {
                self.energy_atk_coeff
            } else {
                self.energy_rel_coeff
            };
            self.energy_env =
                flush_denormal(self.energy_env + coeff * (inst_energy - self.energy_env));
            self.energy_slow = flush_denormal(
                self.energy_slow + self.energy_slow_coeff * (self.energy_env - self.energy_slow),
            );
        }

        // Apply smooth per-sample release envelope when muting
        if self.is_muting {
            self.mute_env = flush_denormal(self.mute_env * self.mute_rate);
            if self.mute_env < 1.0e-3 {
                self.active = false;
                self.is_muting = false;
                self.mute_env = 0.0;
                self.last_bridge_out = 0.0;
                return (0.0, 0.0, 0.0);
            }
        }

        // Soft guard clamp inside both waveguides to prevent runaway peaks on rapid re-plucks
        let clamped_h = flush_denormal(dc_out_h.clamp(-2.0, 2.0));
        let clamped_v = flush_denormal(dc_out_v.clamp(-2.0, 2.0));
        self.delay_line_h[self.write_idx] = clamped_h;
        self.delay_line_v[self.write_idx] = clamped_v;
        self.write_idx = (self.write_idx + 1) & DELAY_BUFFER_MASK;

        // 11. Auto-deactivate once both polarizations decay below audible threshold
        let peak_amp = clamped_h.abs().max(clamped_v.abs());
        if peak_amp < 1.0e-5
            && self.exc_samples_elapsed >= self.exc_total_samples
            && self.pre_pluck_samples_remaining == 0
            && !self.pending_pluck.active
            && bridge_bus_in.abs() < 1.0e-4
        {
            self.quiet_samples += 1;
            if self.quiet_samples > (self.current_delay as u32).saturating_mul(2).max(256) {
                self.active = false;
                self.is_muting = false;
                self.last_bridge_out = 0.0;
                return (0.0, 0.0, 0.0);
            }
        } else {
            self.quiet_samples = 0;
        }

        let bridge_force = if !self.is_muting
            && !self.muted_by_player
            && self.palm_mute < 0.35
            && self.pre_pluck_samples_remaining == 0
        {
            clamped_v
        } else {
            0.0
        };
        self.last_bridge_out = bridge_force;

        // Sum vertical bridge force (initial punch) + horizontal sustain torque
        let out = (0.64 * clamped_v + 0.56 * clamped_h) * self.mute_env * pre_out_env;
        let left = out * (1.0 - self.pan);
        let right = out * self.pan;
        (left, right, bridge_force)
    }

    /// Advances the waveguide by one sample with zero external bridge coupling and returns `(left, right)`.
    #[inline(always)]
    pub fn next_frame(&mut self) -> (f32, f32) {
        let (l, r, _) = self.next_frame_with_bridge(0.0, 1);
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to estimate fundamental period via energy-normalized autocorrelation (Pearson
    /// correlation) with parabolic interpolation, eliminating amplitude-decay lag bias.
    fn estimate_period(samples: &[f32], min_lag: usize, max_lag: usize) -> f32 {
        let eval_corr = |l: usize| -> f32 {
            let mut dot = 0.0_f32;
            let mut e0 = 0.0_f32;
            let mut e1 = 0.0_f32;
            let count = samples.len() - l;
            for i in 0..count {
                let a = samples[i];
                let b = samples[i + l];
                dot += a * b;
                e0 += a * a;
                e1 += b * b;
            }
            let denom = (e0 * e1).sqrt();
            if denom > 1.0e-12 {
                dot / denom
            } else {
                0.0
            }
        };

        let mut best_lag = min_lag;
        let mut max_corr = f32::MIN;
        for lag in min_lag..=max_lag {
            let corr = eval_corr(lag);
            if corr > max_corr {
                max_corr = corr;
                best_lag = lag;
            }
        }
        if best_lag > min_lag && best_lag < max_lag {
            let y1 = eval_corr(best_lag - 1);
            let y2 = eval_corr(best_lag);
            let y3 = eval_corr(best_lag + 1);
            let denom = y1 - 2.0 * y2 + y3;
            let delta = if denom.abs() > 1e-6 {
                0.5 * (y1 - y3) / denom
            } else {
                0.0
            };
            (best_lag as f32) + delta.clamp(-0.5, 0.5)
        } else {
            best_lag as f32
        }
    }

    #[test]
    fn test_waveguide_microtonal_pluck_slide_slur_and_harmonics() {
        let mut wg = StringWaveguide::new(48_000.0, 1);

        // 1. Pluck a microtonal note (e.g. 385.0 Hz, JI 7/4)
        wg.trigger(1, 385.0, 0.9, 0.2, 0.0, None);
        assert!(wg.active);

        let mut energy = 0.0_f32;
        for _ in 0..1024 {
            let (l, r) = wg.next_frame();
            assert!(l.is_finite() && r.is_finite());
            energy += l * l + r * r;
        }
        assert!(energy > 0.05, "Expected strong pluck energy, got {}", energy);

        // 2. Slide to 440.0 Hz over 512 samples without clearing waveguide
        wg.slide(440.0, 512);
        let mut slide_energy = 0.0_f32;
        for _ in 0..512 {
            let (l, r) = wg.next_frame();
            slide_energy += l * l + r * r;
        }
        assert!(slide_energy > 0.01, "Expected continuous energy during slide");

        // 3. Hammer-on slur to 495.0 Hz
        wg.slur(495.0, 0.8);
        let mut slur_energy = 0.0_f32;
        for _ in 0..512 {
            let (l, r) = wg.next_frame();
            slur_energy += l * l + r * r;
        }
        assert!(slur_energy > 0.01, "Expected sustained energy after slur");

        // 4. Palm mute vs Open decay comparison
        let mut wg_open = StringWaveguide::new(48_000.0, 2);
        let mut wg_muted = StringWaveguide::new(48_000.0, 2);
        wg_open.trigger(2, 110.0, 0.85, 0.2, 0.0, None);
        wg_muted.trigger(2, 110.0, 0.85, 0.2, 0.9, None);

        // Advance 0.25 seconds (12,000 samples)
        for _ in 0..12_000 {
            wg_open.next_frame();
            wg_muted.next_frame();
        }
        let mut tail_open = 0.0_f32;
        let mut tail_muted = 0.0_f32;
        for _ in 0..2048 {
            let (lo, ro) = wg_open.next_frame();
            let (lm, rm) = wg_muted.next_frame();
            tail_open += lo * lo + ro * ro;
            tail_muted += lm * lm + rm * rm;
        }
        assert!(
            tail_open > tail_muted * 4.0,
            "Palm-muted tail ({}) should decay much faster than open string tail ({})",
            tail_muted,
            tail_open
        );

        // 5. Natural harmonic node 2 excitation
        let mut wg_harm = StringWaveguide::new(48_000.0, 1);
        wg_harm.trigger(1, 110.0, 0.9, 0.15, 0.0, Some(2));
        let mut harm_energy = 0.0_f32;
        for _ in 0..2048 {
            let (l, r) = wg_harm.next_frame();
            harm_energy += l * l + r * r;
        }
        assert!(harm_energy > 0.01);
    }

    #[test]
    fn test_waveguide_mute_pitch_stability() {
        let mut wg = StringWaveguide::new(48_000.0, 1);
        let target_freq = 440.0; // A4 (period = 109.0909 samples at 48kHz)
        wg.trigger(1, target_freq, 0.9, 0.25, 0.0, None);

        // 1. Advance through excitation burst into steady-state resonance
        for _ in 0..1500 {
            wg.next_frame();
        }

        // 2. Capture steady-state sustain frames
        let mut sustain_samples = Vec::with_capacity(1500);
        for _ in 0..1500 {
            let (l, _) = wg.next_frame();
            sustain_samples.push(l);
        }

        let period_sustain = estimate_period(&sustain_samples, 90, 130);

        // Verify sustain fundamental is in tune (< 2 cents from 48000 / 440 = 109.09)
        let expected_period = 48_000.0 / target_freq as f32;
        let sustain_cents = 1200.0 * (expected_period / period_sustain).log2();
        assert!(
            sustain_cents.abs() < 2.0,
            "Sustain pitch out of tune by {:.2} cents (expected: {:.4}, got: {:.4})",
            sustain_cents,
            expected_period,
            period_sustain
        );

        // 3. Send note-off mute command
        wg.mute(0.9965);

        // 4. Capture decaying release frames while still audible
        let mut mute_samples = Vec::with_capacity(800);
        for _ in 0..800 {
            let (l, _) = wg.next_frame();
            mute_samples.push(l);
        }

        let period_mute = estimate_period(&mute_samples, 90, 130);

        // 5. Pitch drift in cents between sustain and mute (< 0.5 cents)
        let drift_cents = 1200.0 * (period_sustain / period_mute).log2();
        assert!(
            drift_cents.abs() < 0.50,
            "Note-off pitch went flat by {:.2} cents! (sustain period: {:.4}, mute period: {:.4})",
            drift_cents,
            period_sustain,
            period_mute
        );
    }

    #[test]
    fn test_dual_polarization_two_stage_decay_and_coupling_stability() {
        let mut wg = StringWaveguide::new(48_000.0, 1);
        let mut params = StringSynthParams::default();
        params.t60_decay_sec = 4.0;
        params.polarization_detune_cents = 0.7;
        params.polarization_coupling = 0.012;
        wg.update_params(&params);

        wg.trigger(1, 220.0, 0.9, 0.18, 0.0, None);
        assert!(
            wg.loop_gain_g_v < wg.loop_gain_g,
            "Vertical polarization must decay faster than horizontal polarization"
        );

        // Measure initial attack RMS vs mid-sustain RMS vs late-sustain RMS (2 seconds)
        let mut e_early = 0.0_f32;
        let mut e_mid = 0.0_f32;
        let mut e_late = 0.0_f32;
        for i in 0..96_000 {
            let (l, r) = wg.next_frame();
            assert!(l.is_finite() && r.is_finite() && l.abs() <= 2.0 && r.abs() <= 2.0);
            let e = l * l + r * r;
            if i >= 500 && i < 4_500 {
                e_early += e;
            } else if i >= 24_000 && i < 28_000 {
                e_mid += e;
            } else if i >= 88_000 && i < 92_000 {
                e_late += e;
            }
        }
        assert!(
            e_early > e_mid && e_mid > e_late && e_late > 1.0e-5,
            "Expected smooth two-stage decay (early={}, mid={}, late={})",
            e_early,
            e_mid,
            e_late
        );
    }

    #[test]
    fn test_tension_modulation_pitch_drop_on_hard_pluck() {
        let mut wg = StringWaveguide::new(48_000.0, 1);
        let mut params = StringSynthParams::default();
        params.tension_mod_amount = 0.9;
        params.polarization_detune_cents = 0.0;
        params.polarization_coupling = 0.0;
        params.fret_buzz_sizzle = 0.0;
        wg.update_params(&params);

        let freq = 220.0; // period = 218.1818 samples
        wg.trigger(1, freq, 1.0, 0.2, 0.0, None);

        // Capture early transient (4ms..25ms) vs settled sustain (120ms..160ms)
        let mut early = Vec::with_capacity(1000);
        let mut settled = Vec::with_capacity(1800);
        for i in 0..8000 {
            let (l, _) = wg.next_frame();
            if i >= 200 && i < 1200 {
                early.push(l);
            } else if i >= 5800 && i < 7600 {
                settled.push(l);
            }
        }
        let period_early = estimate_period(&early, 195, 240);
        let period_settled = estimate_period(&settled, 195, 240);
        // Hard pluck should start slightly sharper (shorter period) and drop into settled pitch
        let drop_cents = 1200.0 * (period_settled / period_early).log2();
        assert!(
            drop_cents > 2.0 && drop_cents < 45.0,
            "Expected natural initial pitch-drop envelope on hard pluck (got {:.2} cents: early={:.3}, settled={:.3})",
            drop_cents,
            period_early,
            period_settled
        );
    }

    #[test]
    fn test_fret_buzz_triggers_on_hard_pluck_only() {
        let mut wg_clean = StringWaveguide::new(48_000.0, 1);
        let mut wg_buzz = StringWaveguide::new(48_000.0, 1);

        let mut p_clean = StringSynthParams::default();
        p_clean.fret_buzz_sizzle = 0.0;
        let mut p_buzz = StringSynthParams::default();
        p_buzz.fret_buzz_sizzle = 0.95;

        wg_clean.update_params(&p_clean);
        wg_buzz.update_params(&p_buzz);

        // Soft pluck: both should remain virtually identical (below fret_clearance)
        wg_clean.trigger(1, 110.0, 0.12, 0.22, 0.0, None);
        wg_buzz.trigger(1, 110.0, 0.12, 0.22, 0.0, None);
        let mut soft_diff = 0.0_f32;
        for _ in 0..2048 {
            let (lc, _) = wg_clean.next_frame();
            let (lb, _) = wg_buzz.next_frame();
            soft_diff += (lc - lb).abs();
        }
        assert!(
            soft_diff < 1.0e-5,
            "Soft pluck should stay below fret_clearance with zero buzz distortion (diff={})",
            soft_diff
        );

        // Hard pluck: asymmetric fret barrier engages and injects audible difference without blowup
        wg_clean.clear();
        wg_buzz.clear();
        wg_clean.trigger(1, 110.0, 1.0, 0.22, 0.0, None);
        wg_buzz.trigger(1, 110.0, 1.0, 0.22, 0.0, None);
        let mut hard_diff = 0.0_f32;
        for _ in 0..2048 {
            let (lc, _) = wg_clean.next_frame();
            let (lb, _) = wg_buzz.next_frame();
            assert!(lb.is_finite() && lb.abs() <= 2.0);
            hard_diff += (lc - lb).abs();
        }
        assert!(
            hard_diff > 0.5,
            "Hard pluck with high fret buzz sizzle should engage fret barrier (diff={})",
            hard_diff
        );
    }

    #[test]
    fn test_pre_pluck_damping_ramp_on_repluck() {
        let mut wg = StringWaveguide::new(48_000.0, 1);
        wg.trigger(1, 196.0, 0.9, 0.2, 0.0, None);
        for _ in 0..2000 {
            wg.next_frame();
        }
        // Re-pluck already vibrating string -> should initiate 4ms (~192 samples at 48kHz) pre-pluck ramp
        wg.trigger(1, 293.66, 0.85, 0.18, 0.0, None);
        assert!(
            wg.pre_pluck_samples_remaining >= 180 && wg.pre_pluck_samples_remaining <= 200,
            "Expected ~4ms (192 samples) pre-pluck damping ramp, got {}",
            wg.pre_pluck_samples_remaining
        );

        // Advance through the 4ms ramp + new pluck and ensure click-free finite output
        let mut new_energy = 0.0_f32;
        for i in 0..2000 {
            let (l, r) = wg.next_frame();
            assert!(l.is_finite() && r.is_finite());
            if i >= 220 {
                new_energy += l * l + r * r;
            }
        }
        assert_eq!(wg.pre_pluck_samples_remaining, 0);
        assert!(new_energy > 0.05, "Expected strong new pluck after 4ms pre-pluck damping");
    }

    #[test]
    fn test_sympathetic_bridge_coupling_transfers_energy_passively() {
        let sr = 48_000.0;
        let mut s1 = StringWaveguide::new(sr, 1);
        let mut s2 = StringWaveguide::new(sr, 2);

        let mut p = StringSynthParams::default();
        p.sympathetic_coupling = 0.004;
        s1.update_params(&p);
        s2.update_params(&p);

        // Tune string 2 to 220 Hz and let it become quiet (unmuted open string)
        s2.trigger(2, 220.0, 0.5, 0.2, 0.0, None);
        s2.delay_line_h.fill(0.0);
        s2.delay_line_v.fill(0.0);
        s2.exc_samples_elapsed = s2.exc_total_samples;
        s2.active = false;
        assert!(s2.was_tuned && !s2.muted_by_player);

        // Pluck string 1 at octave/unison (220 Hz)
        s1.trigger(1, 220.0, 0.95, 0.18, 0.0, None);

        let mut bridge_bus = 0.0_f32;
        let mut s2_symp_energy = 0.0_f32;
        let mut max_peak = 0.0_f32;

        for _ in 0..24_000 {
            s2.try_sympathetic_wake(bridge_bus);
            let coupled = (s1.is_sympathetically_coupled() as usize)
                + (s2.is_sympathetically_coupled() as usize);
            let (l1, r1, b1) = s1.next_frame_with_bridge(bridge_bus, coupled);
            let (l2, r2, b2) = s2.next_frame_with_bridge(bridge_bus, coupled);
            assert!(l1.is_finite() && l2.is_finite());
            max_peak = max_peak.max(l1.abs()).max(r1.abs()).max(l2.abs()).max(r2.abs());
            s2_symp_energy += l2 * l2 + r2 * r2;
            bridge_bus = flush_denormal(b1 + b2);
        }

        assert!(
            s2_symp_energy > 1.0e-4,
            "Unmuted open string tuned to unison should resonate sympathetically (energy={})",
            s2_symp_energy
        );
        assert!(
            max_peak < 1.5,
            "Passive sympathetic coupling must never blow up (peak={})",
            max_peak
        );
    }
}
