//! Extended Karplus-Strong (EKS) physical string waveguide model (`src/synth/waveguide.rs`).
//!
//! ### DSP Pipeline per String (`StringWaveguide`)
//! 1. **Excitation & Pick-Position Comb Filter:**
//!    - Filtered white noise + rounded triangle pluck transient scaled by `velocity`.
//!    - Dynamic one-pole lowpass filter where lower velocity darkens the initial pluck.
//!    - Feed-forward comb filter delayed by `pluck_pos * period` to carve natural pick-position
//!      spectral notches (`0.05` = bridge/bright, `0.50` = neck/hollow).
//! 2. **Fractional Delay Line:**
//!    - Pre-allocated power-of-two circular buffer (`[f32; 8192]`) supporting fundamentals down to
//!      $< 15\text{ Hz}$ at $96\text{ kHz}$.
//!    - Sub-sample delay interpolation (3rd-order Lagrange + Thiran all-pass phase compensation)
//!      for exact microtonal `f64` Hz tuning and zipper-free pitch slides.
//! 3. **Loop Damping, Stiffness Dispersion & DC Blocking:**
//!    - One-zero loop lowpass filter $H(z) = g \cdot ((1 - S) + S \cdot z^{-1})$.
//!    - 1st-order all-pass stiffness dispersion filter for wound/bass string inharmonicity.
//!    - DC blocker ($R = 0.995$) and strict denormal flushing in all state variables.
//! 4. **Articulations:**
//!    - Smooth delay slewing for `SlideString` and `SlurString` without clearing the waveguide.
//!    - Heavy loop damping $S$ and reduced feedback gain $g$ for `palm_mute`.
//!    - Secondary fractional node tap at `delay / n` during the first $30\text{ ms}$ for natural/artificial harmonics.

use crate::synth::backend::flush_denormal;
use crate::synth::body::InstrumentProfile;
use crate::synth::patch::StringSynthParams;
use std::f32::consts::PI;

/// Maximum number of simultaneous physical strings supported per waveguide bank.
pub const MAX_STRINGS: usize = 12;

/// Pre-allocated circular buffer length per string (power of two: 8192 samples).
/// Supports down to $96000 / 8192 \approx 11.7\text{ Hz}$ at $96\text{ kHz}$.
pub const DELAY_BUFFER_SIZE: usize = 8192;
const DELAY_BUFFER_MASK: usize = DELAY_BUFFER_SIZE - 1;

/// Pre-allocated excitation scratch buffer size for pick-position comb filtering.
const EXCITATION_BUFFER_SIZE: usize = 2048;
const EXCITATION_BUFFER_MASK: usize = EXCITATION_BUFFER_SIZE - 1;

/// Single physical string waveguide state using the Extended Karplus-Strong (EKS) algorithm.
#[derive(Clone)]
pub struct StringWaveguide {
    /// Circular waveguide delay line buffer.
    delay_line: [f32; DELAY_BUFFER_SIZE],
    ///Circular buffer for pick-position feed-forward comb filter on the excitation signal.
    exc_comb_buf: [f32; EXCITATION_BUFFER_SIZE],
    write_idx: usize,
    exc_write_idx: usize,

    pub active: bool,
    pub string_idx: u8,
    pub profile: InstrumentProfile,
    sample_rate: f32,

    /// Current effective delay length in samples (slewed sample-by-sample during slides/slurs).
    current_delay: f32,
    target_delay: f32,
    delay_slew_step: f32,
    glide_samples_remaining: u32,

    /// Loop filter parameters: $H(z) = g \cdot ((1 - S) + S \cdot z^{-1})$.
    loop_gain_g: f32,
    loop_stretch_s: f32,
    palm_mute: f32,
    lp_z1: f32,

    /// 1st-order Thiran / stiffness dispersion all-pass filter state.
    ap_coeff: f32,
    ap_x1: f32,
    ap_y1: f32,

    /// DC blocker state ($R = 0.995$).
    dc_x1: f32,
    dc_y1: f32,

    /// Excitation burst generator state (noise + rounded triangle + velocity lowpass + comb).
    exc_samples_remaining: u32,
    exc_total_samples: u32,
    exc_amplitude: f32,
    exc_lp_coeff: f32,
    exc_lp_z1: f32,
    exc_comb_delay: usize,

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
        Self {
            delay_line: [0.0; DELAY_BUFFER_SIZE],
            exc_comb_buf: [0.0; EXCITATION_BUFFER_SIZE],
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
            loop_stretch_s: 0.15,
            palm_mute: 0.0,
            lp_z1: 0.0,
            ap_coeff: -0.1,
            ap_x1: 0.0,
            ap_y1: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
            exc_samples_remaining: 0,
            exc_total_samples: 0,
            exc_amplitude: 0.0,
            exc_lp_coeff: 0.3,
            exc_lp_z1: 0.0,
            exc_comb_delay: 8,
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
        if self.active {
            let clamped_freq = (self.sample_rate / self.current_delay.max(2.0)).clamp(15.0, self.sample_rate * 0.42);
            let t60 = if self.palm_mute > 0.001 {
                params.t60_decay_sec * (1.0 - 0.92 * self.palm_mute * (params.palm_mute_damping / 0.22)).max(0.04)
            } else {
                params.t60_decay_sec
            };
            let periods_per_t60 = (t60 * clamped_freq).max(1.0);
            let mut gain_g = 10.0_f32.powf(-3.0 / periods_per_t60);
            if self.palm_mute > 0.001 {
                gain_g *= 1.0 - 0.045 * self.palm_mute;
            }
            self.loop_gain_g = gain_g.clamp(0.85, 0.9999);
            self.loop_stretch_s = (params.brightness_s + self.palm_mute * 0.52).clamp(0.01, 0.95);
            self.ap_coeff = (-params.inharmonicity_dispersion).clamp(-0.80, -0.005);
        }
    }

    #[inline(always)]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
    }

    #[inline(always)]
    pub fn set_profile(&mut self, profile: InstrumentProfile) {
        self.profile = profile;
    }

    /// Fast, real-time xorshift32 PRNG returning uniform white noise in `[-1.0, 1.0]`.
    #[inline(always)]
    fn next_white_noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = if x == 0 { 0x6D2B_79F5 } else { x };
        // Convert u32 to [-1.0, 1.0]
        ((x as i32) as f32) * (1.0 / 2_147_483_648.0)
    }

    /// Computes loop filter coefficients (`loop_gain_g`, `loop_stretch_s`, `ap_coeff`) and
    /// phase-compensated fractional delay length for `freq_hz`.
    #[inline(always)]
    fn compute_loop_parameters(&self, freq_hz: f64, palm_mute: f32) -> (f32, f32, f32, f32) {
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
        // Higher strings have smaller S (brighter); palm muting dramatically increases S.
        let stretch_s = (base_s + mute * 0.52).clamp(0.01, 0.95);

        // Target T60 decay time in seconds: longer for low strings & bass profiles, shorter when palm-muted.
        let t60 = if mute > 0.001 {
            let mute_scale = self.custom_params.map(|cp| cp.palm_mute_damping / 0.22).unwrap_or(1.0);
            base_t60 * (1.0 - 0.92 * mute * mute_scale).max(0.04)
        } else {
            base_t60
        };
        // Per-period attenuation for -60 dB (10^-3) over t60 seconds:
        let periods_per_t60 = (t60 * (clamped_freq as f32)).max(1.0);
        let mut gain_g = 10.0_f32.powf(-3.0 / periods_per_t60);
        if mute > 0.001 {
            gain_g *= 1.0 - 0.045 * mute;
        }
        let gain_g = gain_g.clamp(0.85, 0.9999);
        let ap_coeff = base_ap;

        // Phase delay compensation at fundamental omega_0 = 2 * pi * f0 / sr:
        let omega = (2.0 * std::f64::consts::PI * clamped_freq / sr) as f32;
        // One-zero lowpass phase delay:
        let lp_num = stretch_s * omega.sin();
        let lp_den = (1.0 - stretch_s) + stretch_s * omega.cos();
        let lp_phase_delay = if omega > 1.0e-5 {
            lp_num.atan2(lp_den) / omega
        } else {
            stretch_s
        };
        // 1st-order all-pass phase delay at low frequency: (1 - a) / (1 + a)
        let ap_phase_delay = ((1.0 - ap_coeff) / (1.0 + ap_coeff).max(0.1)).clamp(0.0, 4.0);

        let effective_delay =
            (raw_period - lp_phase_delay - ap_phase_delay).clamp(4.0, (DELAY_BUFFER_SIZE - 8) as f32);

        (effective_delay, gain_g, stretch_s, ap_coeff)
    }

    /// Plucks the string with the given `freq_hz`, `velocity`, `pluck_pos`, `palm_mute`, and `harmonic_node`.
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
        self.string_idx = string_idx;
        self.palm_mute = palm_mute.clamp(0.0, 1.0);
        self.pan = (0.30 + ((string_idx % MAX_STRINGS as u8) as f32) * 0.035).clamp(0.15, 0.85);
        let prof = self.profile.waveguide_params();

        let (delay, g, s, ap) = self.compute_loop_parameters(freq_hz, self.palm_mute);
        self.current_delay = delay;
        self.target_delay = delay;
        self.delay_slew_step = 0.0;
        self.glide_samples_remaining = 0;
        self.loop_gain_g = g;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;

        // Softly attenuate existing waveguide energy (simulating pick/finger contact before pluck release)
        for sample in &mut self.delay_line {
            *sample = flush_denormal(*sample * 0.18);
        }

        let vel = velocity.clamp(0.02, 1.0);
        // Excitation burst spans one fundamental period (clamped to reasonable bounds)
        let burst_len = (delay.round() as u32).clamp(16, (EXCITATION_BUFFER_SIZE / 2) as u32);
        self.exc_total_samples = burst_len;
        self.exc_samples_remaining = burst_len;
        self.exc_amplitude = vel * 0.42;

        // Pick-direction / dynamics one-pole lowpass: lower velocity or FingeredBass -> softer/darker
        self.exc_lp_coeff = if let Some(cp) = self.custom_params {
            self.exc_noise_mix = cp.noise_mix.clamp(0.0, 1.0);
            let norm_hardness = (cp.pick_hardness / 16_000.0).clamp(0.05, 0.95);
            (0.92 - 0.70 * norm_hardness * vel).clamp(0.06, 0.92)
        } else {
            (0.82 - 0.68 * vel + prof.exc_softness_offset).clamp(0.06, 0.92)
        };
        self.exc_lp_z1 = 0.0;

        // Pick-position comb filter delay: pluck_pos in [0.05, 0.5] of fundamental period
        // Blend requested pluck_pos with profile's characteristic pluck position
        let base_pluck = self.custom_params.map(|cp| cp.pluck_pos).unwrap_or(prof.default_pluck_pos);
        let effective_pos = (0.75 * pluck_pos + 0.25 * base_pluck).clamp(0.05, 0.50);
        self.exc_comb_delay =
            ((effective_pos * delay).round() as usize).clamp(1, EXCITATION_BUFFER_SIZE - 2);

        // Harmonic node coupling during the first 30ms of excitation
        match harmonic_node {
            Some(n) if n >= 2 => {
                self.harmonic_node = n.min(8);
                let harm_ms = self.custom_params.map(|cp| cp.harmonic_damping_ms).unwrap_or(30.0);
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

        self.quiet_samples = 0;
        self.is_muting = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;
        self.active = true;
    }

    /// Smoothly glides the string's fractional read pointer to `target_freq_hz` over `glide_samples`
    /// without clearing the waveguide buffer.
    #[inline(always)]
    pub fn slide(&mut self, target_freq_hz: f64, glide_samples: u32) {
        if !self.active {
            return;
        }
        let (target_delay, g, s, ap) = self.compute_loop_parameters(target_freq_hz, self.palm_mute);
        self.target_delay = target_delay;
        self.loop_gain_g = g;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;
        self.is_muting = false;
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
    /// WITHOUT clearing the waveguide buffer, injecting only a small low-passed impulse (`hammer_energy`).
    #[inline(always)]
    pub fn slur(&mut self, new_freq_hz: f64, hammer_energy: f32) {
        if !self.active {
            self.trigger(self.string_idx, new_freq_hz, hammer_energy, 0.25, 0.0, None);
            return;
        }
        let (target_delay, g, s, ap) = self.compute_loop_parameters(new_freq_hz, self.palm_mute);
        self.target_delay = target_delay;
        self.loop_gain_g = g;
        self.loop_stretch_s = s;
        self.ap_coeff = ap;
        self.is_muting = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;

        // Short ~2.5ms slew to prevent step discontinuity while keeping fretting crisp
        let slew_samples = (0.0025 * self.sample_rate).round().max(8.0) as u32;
        self.delay_slew_step = (target_delay - self.current_delay) / (slew_samples as f32);
        self.glide_samples_remaining = slew_samples;

        // Inject a tiny, warm low-passed impulse representing the finger hammering onto the fret
        let energy = hammer_energy.clamp(0.02, 1.0);
        let imp_samples = (target_delay * 0.5).round().clamp(8.0, 256.0) as u32;
        self.exc_total_samples = imp_samples;
        self.exc_samples_remaining = imp_samples;
        self.exc_amplitude = energy * 0.14;
        self.exc_lp_coeff = 0.78; // Warm/dark hammer transient
        self.quiet_samples = 0;
    }

    /// Dampens the string with the specified exponential release rate without altering
    /// loop filter phase delay, guaranteeing zero pitch drift between note on and note off.
    #[inline(always)]
    pub fn mute(&mut self, release_rate: f32) {
        if self.active {
            self.is_muting = true;
            // release_rate is a per-sample decay factor (e.g. 0.995 to 0.998).
            self.mute_rate = release_rate.clamp(0.90, 0.9995);

            // Compute per-period loop gain corresponding to the per-sample release rate
            let per_period = if release_rate > 0.90 {
                release_rate.clamp(0.90, 0.9999).powf(self.current_delay)
            } else {
                release_rate.clamp(0.01, 0.90)
            };
            self.loop_gain_g = self.loop_gain_g.min(per_period).clamp(0.01, 0.9999);

            // CRITICAL INVARIANT: We strictly PRESERVE `loop_stretch_s` here!
            // The one-zero loop lowpass filter H(z) = g * ((1 - S) + S * z^-1) has a phase
            // delay of ~S samples. In trigger(), the fractional delay line length was phase-compensated
            // using S. Mutating S on note-off increases the filter's phase delay without shortening the
            // delay line, which lengthens the overall loop period and causes the decaying tail
            // to audibly drop in pitch (go flat). Preserving S ensures pitch stability (0 cents drift).
        }
    }

    /// Immediately silences the string and clears all filter/delay state.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.active = false;
        self.is_muting = false;
        self.mute_env = 1.0;
        self.mute_rate = 1.0;
        self.delay_line.fill(0.0);
        self.exc_comb_buf.fill(0.0);
        self.lp_z1 = 0.0;
        self.ap_x1 = 0.0;
        self.ap_y1 = 0.0;
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.exc_samples_remaining = 0;
        self.harmonic_samples_remaining = 0;
        self.glide_samples_remaining = 0;
        self.quiet_samples = 0;
    }

    /// Reads from `self.delay_line` at `delay_samples` behind `self.write_idx` using
    /// 3rd-order (4-point) Lagrange fractional delay interpolation.
    ///
    /// This provides sub-sample microtonal accuracy and zero zipper noise when `delay_samples`
    /// is modulated continuously sample-by-sample during slides and slurs.
    #[inline(always)]
    fn read_fractional_lagrange(&self, delay_samples: f32) -> f32 {
        let d_clamped = delay_samples.clamp(2.0, (DELAY_BUFFER_SIZE - 4) as f32);
        let d_int = d_clamped.floor() as usize;
        let frac = d_clamped - (d_int as f32);

        let idx0 = self.write_idx.wrapping_sub(d_int.saturating_sub(1)) & DELAY_BUFFER_MASK;
        let idx1 = self.write_idx.wrapping_sub(d_int) & DELAY_BUFFER_MASK;
        let idx2 = self.write_idx.wrapping_sub(d_int + 1) & DELAY_BUFFER_MASK;
        let idx3 = self.write_idx.wrapping_sub(d_int + 2) & DELAY_BUFFER_MASK;

        let y0 = self.delay_line[idx0];
        let y1 = self.delay_line[idx1];
        let y2 = self.delay_line[idx2];
        let y3 = self.delay_line[idx3];

        // 4-point cubic Lagrange polynomials for fractional offset `frac` in [0, 1)
        let c0 = -frac * (frac - 1.0) * (frac - 2.0) * (1.0 / 6.0);
        let c1 = (frac + 1.0) * (frac - 1.0) * (frac - 2.0) * 0.5;
        let c2 = -(frac + 1.0) * frac * (frac - 2.0) * 0.5;
        let c3 = (frac + 1.0) * frac * (frac - 1.0) * (1.0 / 6.0);

        c0 * y0 + c1 * y1 + c2 * y2 + c3 * y3
    }

    /// Advances the waveguide by one sample and returns `(left, right)`.
    #[inline(always)]
    pub fn next_frame(&mut self) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // 1. Slew fractional delay pointer if sliding or slurring
        if self.glide_samples_remaining > 0 {
            self.current_delay += self.delay_slew_step;
            self.glide_samples_remaining -= 1;
            if self.glide_samples_remaining == 0 {
                self.current_delay = self.target_delay;
            }
        }

        // 2. Generate excitation sample (noise + rounded triangle transient + velocity LP + pick-pos comb)
        let excitation = if self.exc_samples_remaining > 0 {
            let progress = 1.0
                - (self.exc_samples_remaining as f32) / (self.exc_total_samples.max(1) as f32);
            self.exc_samples_remaining -= 1;

            let noise = self.next_white_noise();
            // Rounded triangle / half-sine pluck transient
            let tri_transient = (PI * progress).sin() * (1.0 - progress);
            let raw_exc = (self.exc_noise_mix * noise + (1.0 - self.exc_noise_mix) * tri_transient)
                * self.exc_amplitude;

            // Pick-direction / velocity dynamics one-pole lowpass filter
            let lp_exc = flush_denormal(
                (1.0 - self.exc_lp_coeff) * raw_exc + self.exc_lp_coeff * self.exc_lp_z1,
            );
            self.exc_lp_z1 = lp_exc;

            // Feed-forward pick-position comb filter: e_comb[n] = e[n] - e[n - M]
            self.exc_comb_buf[self.exc_write_idx] = lp_exc;
            let delayed_idx =
                self.exc_write_idx.wrapping_sub(self.exc_comb_delay) & EXCITATION_BUFFER_MASK;
            let comb_out = lp_exc - 0.85 * self.exc_comb_buf[delayed_idx];
            self.exc_write_idx = (self.exc_write_idx + 1) & EXCITATION_BUFFER_MASK;
            comb_out
        } else {
            0.0
        };

        // 3. Read fractional delay tap (and optional harmonic node tap during first 30ms)
        let mut delayed = self.read_fractional_lagrange(self.current_delay);
        if self.harmonic_samples_remaining > 0 && self.harmonic_node >= 2 {
            self.harmonic_samples_remaining -= 1;
            let node_delay = (self.current_delay / (self.harmonic_node as f32)).max(2.0);
            let node_tap = self.read_fractional_lagrange(node_delay);
            // Couple secondary tap at delay / n to cancel the fundamental and isolate the n-th harmonic
            let blend = (self.harmonic_samples_remaining as f32)
                / (self.harmonic_total_samples.max(1) as f32);
            let coupling = 0.55 * blend;
            delayed = (1.0 - coupling) * delayed + coupling * node_tap;
        }

        // 4. One-zero loop lowpass filter: H(z) = g * ((1 - S) + S * z^-1)
        let s = self.loop_stretch_s;
        let lp_out = self.loop_gain_g * ((1.0 - s) * delayed + s * self.lp_z1);
        self.lp_z1 = flush_denormal(delayed);

        // 5. 1st-order Thiran / stiffness dispersion all-pass filter:
        // y[n] = a * x[n] + x[n-1] - a * y[n-1]
        let a = self.ap_coeff;
        let ap_out = flush_denormal(a * lp_out + self.ap_x1 - a * self.ap_y1);
        self.ap_x1 = flush_denormal(lp_out);
        self.ap_y1 = ap_out;

        // 6. Combine feedback with excitation and apply DC blocker (R = 0.995)
        let loop_in = ap_out + excitation;
        let dc_out = flush_denormal(loop_in - self.dc_x1 + 0.995 * self.dc_y1);
        self.dc_x1 = flush_denormal(loop_in);
        self.dc_y1 = dc_out;

        // Apply smooth per-sample release envelope when muting
        if self.is_muting {
            self.mute_env = flush_denormal(self.mute_env * self.mute_rate);
            if self.mute_env < 1.0e-4 {
                self.active = false;
                self.is_muting = false;
                self.mute_env = 0.0;
                return (0.0, 0.0);
            }
        }

        // Soft saturation inside waveguide to prevent runaway peaks on rapid re-plucks
        let clamped_loop = dc_out.clamp(-2.0, 2.0);
        self.delay_line[self.write_idx] = flush_denormal(clamped_loop);
        self.write_idx = (self.write_idx + 1) & DELAY_BUFFER_MASK;

        // 7. Auto-deactivate once tail decays below audible threshold
        if clamped_loop.abs() < 1.0e-5 && self.exc_samples_remaining == 0 {
            self.quiet_samples += 1;
            if self.quiet_samples > (self.current_delay as u32).saturating_mul(2).max(256) {
                self.active = false;
                self.is_muting = false;
                return (0.0, 0.0);
            }
        } else {
            self.quiet_samples = 0;
        }

        let out = clamped_loop * self.mute_env;
        let left = out * (1.0 - self.pan);
        let right = out * self.pan;
        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Helper to estimate fundamental period via normalized autocorrelation with parabolic interpolation
        let calc_period = |samples: &[f32], min_lag: usize, max_lag: usize| -> f32 {
            let mut best_lag = min_lag;
            let mut max_corr = f32::MIN;
            for lag in min_lag..=max_lag {
                let mut corr = 0.0;
                let count = samples.len() - lag;
                for i in 0..count {
                    corr += samples[i] * samples[i + lag];
                }
                corr /= count as f32;
                if corr > max_corr {
                    max_corr = corr;
                    best_lag = lag;
                }
            }
            if best_lag > min_lag && best_lag < max_lag {
                let eval_corr = |l: usize| -> f32 {
                    let mut c = 0.0;
                    let count = samples.len() - l;
                    for i in 0..count {
                        c += samples[i] * samples[i + l];
                    }
                    c / count as f32
                };
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
        };

        let period_sustain = calc_period(&sustain_samples, 90, 130);

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

        let period_mute = calc_period(&mute_samples, 90, 130);

        // 5. Pitch drift in cents between sustain and mute (< 0.5 cents, well below human 3-5 cent discrimination threshold)
        let drift_cents = 1200.0 * (period_sustain / period_mute).log2();
        assert!(
            drift_cents.abs() < 0.50,
            "Note-off pitch went flat by {:.2} cents! (sustain period: {:.4}, mute period: {:.4})",
            drift_cents,
            period_sustain,
            period_mute
        );
    }
}
