//! Polyphonic procedural percussion and 2-operator FM General MIDI synthesis (`src/synth/percussion.rs`).
//!
//! ### Architecture
//! Implements a fixed-size pool of 16 pre-allocated polyphonic voices ([`PercussionAndGmPool`])
//! covering:
//! 1. **Kick & Toms (GM Notes 35, 36, 41–50):** Sine oscillator with fast exponential pitch sweep
//!    (dropping from ~160 Hz to 48 Hz over 25 ms for kick) + 5 ms filtered click transient for beater attack.
//! 2. **Snare & Claps (GM Notes 37, 38, 39, 40):** Dual body modes (185 Hz and 330 Hz sine oscillators
//!    with fast decay) mixed with bandpass-filtered white noise passed through an exponential decay
//!    envelope (snare wires / multi-burst clap).
//! 3. **Hi-Hats & Cymbals (GM Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59):** TR-808/modal metallic
//!    matrix of 6 square/pulse oscillators at non-harmonic frequencies
//!    (`[205.3, 304.4, 369.6, 522.7, 540.0, 800.0] Hz`), ring-modulated/XORed and summed, passed through
//!    a 7 kHz highpass filter (hats) or 3.5 kHz bandpass + highpass (crash/ride), shaped by tight
//!    (closed hat ~45 ms) or long (ride/crash ~1.2 s) dual-stage decay envelopes.
//! 4. **Basic GM Melodic Voices (`TriggerGmVoice`):** Compact 2-operator FM voice with selectable
//!    presets (`EPiano`, `Organ`, `Pad`, `Brass`, `SynthBass`) mapped from GM program numbers,
//!    accepting exact `f64` Hz frequencies for microtonal backing tracks.
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, and zero `unwrap()`/`panic!()` on the audio thread.
//! - All 16 voices and their biquad filters are pre-allocated at stream initialization.
//! - Denormal floats are flushed to zero in all envelope and filter states.

use crate::synth::backend::flush_denormal;
use crate::synth::body::{tanh_pade, BiquadFilter};
use std::f32::consts::TAU;

/// Fixed polyphonic voice count for percussion and GM melodic backing.
pub const MAX_POLY_VOICES: usize = 16;

/// Non-harmonic TR-808/modal metallic oscillator frequencies in Hz.
pub const METALLIC_OSC_FREQS_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

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
    /// Sine oscillator with fast exponential pitch envelope + 5 ms filtered click transient.
    KickOrTom,
    /// Dual body sine modes (185 Hz & 330 Hz) + bandpass-filtered snare wire / clap noise.
    SnareOrClap,
    /// 6-oscillator non-harmonic metallic matrix + highpass/bandpass filter + dual-stage decay.
    HatOrCymbal,
    /// 2-operator microtonal FM voice for GM melodic backing.
    GmFmMelodic(GmFmPreset),
}

/// Pre-allocated polyphonic voice capable of rendering any drum archetype or 2-operator FM melodic note.
#[derive(Clone, Copy, Debug)]
pub struct PolyVoice {
    pub mode: PolyVoiceMode,
    /// Identifier for voice stealing and hi-hat choke groups (`gm_drum_note` for drums, `200 + channel` for GM).
    pub voice_tag: u8,
    pub is_open_hat: bool,
    pub age_samples: u32,
    pub pan: f32,
    pub level: f32,

    // Oscillators & phases
    pub phase1: f32,
    pub phase2: f32,
    pub metal_phases: [f32; 6],
    pub metal_incs: [f32; 6],

    // Frequency & pitch envelope state (normalized phase increment per sample = Hz / sample_rate)
    pub base_inc: f32,
    pub sec_inc: f32,
    pub pitch_env: f32,
    pub pitch_decay: f32,
    pub pitch_mod_span_inc: f32,

    // Amplitude & modulation envelopes
    pub amp_env: f32,
    pub amp_attack_inc: f32,
    pub in_attack: bool,
    pub fast_env: f32,
    pub fast_decay: f32,
    pub slow_env: f32,
    pub slow_decay: f32,
    pub click_env: f32,
    pub click_decay: f32,

    // Archetype parameters
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
        Self {
            mode: PolyVoiceMode::Inactive,
            voice_tag: 0,
            is_open_hat: false,
            age_samples: 0,
            pan: 0.5,
            level: 0.0,
            phase1: 0.0,
            phase2: 0.0,
            metal_phases: [0.0; 6],
            metal_incs: [0.0; 6],
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
            rng_state: 0x6D2B_79F5,
        }
    }
}

impl PolyVoice {
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.mode != PolyVoiceMode::Inactive
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.mode = PolyVoiceMode::Inactive;
        self.amp_env = 0.0;
        self.fast_env = 0.0;
        self.slow_env = 0.0;
        self.click_env = 0.0;
        self.pitch_env = 0.0;
        self.fm_last_mod = 0.0;
        self.filter1.reset();
        self.filter2.reset();
    }

    /// Chokes an active voice (e.g. closed hi-hat choking an open hi-hat) with a fast ~8 ms decay.
    #[inline(always)]
    pub fn choke(&mut self, sample_rate: f32) {
        if self.is_active() {
            let sr = sample_rate.max(8_000.0);
            let fast = (-1.0 / (0.008 * sr)).exp();
            self.fast_decay = self.fast_decay.min(fast);
            self.slow_decay = self.slow_decay.min(fast);
            self.is_open_hat = false;
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

    /// Configures this voice as a Kick or Tom (GM notes 35, 36, 41–50).
    ///
    /// - Kick (35, 36): Sine oscillator with fast exponential pitch envelope dropping from
    ///   ~160 Hz to 48 Hz over ~25 ms + a 5 ms filtered click transient for beater attack.
    /// - Toms (41–50): Sine oscillator with exponential pitch drop ($1.85 f_0 \to f_0$) + beater click.
    #[inline(always)]
    pub fn trigger_kick_or_tom(
        &mut self,
        gm_note: u8,
        base_freq_hz: f32,
        start_freq_hz: f32,
        pitch_tau_sec: f32,
        decay_tau_sec: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.mode = PolyVoiceMode::KickOrTom;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * 0.65;

        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.base_inc = base_freq_hz.clamp(25.0, sr * 0.4) * inv_sr;
        self.pitch_mod_span_inc =
            (start_freq_hz.max(base_freq_hz) - base_freq_hz).clamp(0.0, sr * 0.4) * inv_sr;
        self.pitch_env = 1.0;
        // Exponential decay where tau_sec reaches 1/e (so ~25 ms drops >90% across the transient)
        self.pitch_decay = (-inv_sr / pitch_tau_sec.max(0.003)).exp();

        self.amp_env = 1.0;
        self.in_attack = false;
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / decay_tau_sec.max(0.03)).exp();
        self.slow_env = 0.0;
        self.slow_decay = 0.99;

        // 5 ms filtered click transient for beater attack
        self.click_env = 1.0;
        self.click_decay = (-inv_sr / 0.005).exp();
        self.filter1 = BiquadFilter::lowpass(sr, 2_200.0, 1.1);
        self.filter2.reset();
    }

    /// Configures this voice as a Snare, Rimshot, or Clap (GM notes 37, 38, 39, 40).
    ///
    /// - Dual body modes (`185 Hz` and `330 Hz` sine oscillators with fast decay) mixed with
    ///   bandpass-filtered white noise passed through an exponential decay envelope (snare wires).
    #[inline(always)]
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
        is_clap: bool,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.mode = PolyVoiceMode::SnareOrClap;
        self.voice_tag = gm_note;
        self.is_open_hat = false;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * 0.58;

        self.phase1 = 0.0;
        self.phase2 = 0.0;
        self.base_inc = mode1_hz.clamp(60.0, sr * 0.45) * inv_sr;
        self.sec_inc = mode2_hz.clamp(80.0, sr * 0.45) * inv_sr;

        // Small initial drumhead pitch snap on the dual body modes
        self.pitch_env = 1.0;
        self.pitch_decay = (-inv_sr / 0.010).exp();
        self.pitch_mod_span_inc = (mode1_hz * 0.35) * inv_sr;

        // Fast body envelope (`fast_env`) + snare wire / clap envelope (`slow_env`)
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / body_tau_sec.max(0.008)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / noise_tau_sec.max(0.020)).exp();

        self.body_mix = body_mix;
        self.noise_mix = noise_mix;
        self.is_clap = is_clap;

        self.filter1 = BiquadFilter::bandpass_peak(sr, noise_bp_hz, noise_bp_q);
        self.filter2 = BiquadFilter::highpass(sr, 450.0, 0.707);
    }

    /// Configures this voice as a Hi-Hat or Cymbal (GM notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59)
    /// using the 6-oscillator TR-808/modal metallic matrix (`[205.3, 304.4, 369.6, 522.7, 540.0, 800.0] Hz`).
    #[inline(always)]
    pub fn trigger_hat_or_cymbal(
        &mut self,
        gm_note: u8,
        pitch_scale: f32,
        is_cymbal_bandpass: bool,
        is_open_hat: bool,
        fast_tau_sec: f32,
        slow_tau_sec: f32,
        pan: f32,
        velocity: f32,
        sample_rate: f32,
    ) {
        let sr = sample_rate.max(8_000.0);
        let inv_sr = 1.0 / sr;
        let vel = velocity.clamp(0.05, 1.0);

        self.mode = PolyVoiceMode::HatOrCymbal;
        self.voice_tag = gm_note;
        self.is_open_hat = is_open_hat;
        self.age_samples = 0;
        self.pan = pan.clamp(0.1, 0.9);
        self.level = vel * 0.42;

        for (i, &freq) in METALLIC_OSC_FREQS_HZ.iter().enumerate() {
            let scaled = (freq * pitch_scale).clamp(80.0, sr * 0.46);
            self.metal_incs[i] = scaled * inv_sr;
        }

        self.is_cymbal_bandpass = is_cymbal_bandpass;
        // Dual-stage decay envelope: sharp stick attack (`fast_env`) + metallic ring (`slow_env`)
        self.fast_env = 1.0;
        self.fast_decay = (-inv_sr / fast_tau_sec.max(0.005)).exp();
        self.slow_env = 1.0;
        self.slow_decay = (-inv_sr / slow_tau_sec.max(0.015)).exp();

        if is_cymbal_bandpass {
            // Crash / Ride / Splash / China: bandpass at 3.5 kHz + highpass at 4.5 kHz
            self.filter1 = BiquadFilter::bandpass_peak(sr, 3_500.0, 1.15);
            self.filter2 = BiquadFilter::highpass(sr, 4_500.0, 0.75);
        } else {
            // Closed / Pedal / Open Hi-Hats: crisp highpass at 7.0 kHz
            let hp_cutoff = (7_000.0_f32).min(sr * 0.42);
            self.filter1 = BiquadFilter::highpass(sr, hp_cutoff, 0.85);
            self.filter2 = BiquadFilter::highpass(sr, (5_200.0_f32).min(sr * 0.40), 0.707);
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
                // Classic tine + warm body: 1:1 ratio with bright tine transient decaying into warm fundamental
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
                // Drawbar harmonic ratio 1:2, fast key-click attack, long sustain envelope
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
                // Lush slow-attack detuned 1:1.003 shimmer pad
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
                // Expressive 1:1 brass swell where FM modulation index blooms with attack
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
                // Punchy 1:2 FM bass with snappy filter/index decay
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
                let inst_inc = self.base_inc + self.pitch_mod_span_inc * self.pitch_env;
                self.phase1 += inst_inc;
                if self.phase1 >= 1.0 {
                    self.phase1 -= 1.0;
                }

                // Sine body with subtle 2nd-harmonic warmth driven through soft saturation
                let sine_body = (self.phase1 * TAU).sin() * self.fast_env;

                // 5ms filtered click + noise beater attack transient
                let click_sig = if self.click_env > 1.0e-4 {
                    let noise = self.next_white_noise();
                    let impulse = (1.0 - self.phase1 * 2.0) * 0.6 + noise * 0.4;
                    self.filter1.process(impulse) * self.click_env * 0.55
                } else {
                    0.0
                };

                self.pitch_env = flush_denormal(self.pitch_env * self.pitch_decay);
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.click_env = flush_denormal(self.click_env * self.click_decay);

                if self.fast_env < 1.0e-5 && self.click_env < 1.0e-5 {
                    self.clear();
                    return (0.0, 0.0);
                }

                tanh_pade((sine_body + click_sig) * 1.45) * self.level
            }

            PolyVoiceMode::SnareOrClap => {
                // Dual body modes (185 Hz and 330 Hz) with small initial drumhead pitch snap
                let inc1 = self.base_inc + self.pitch_mod_span_inc * self.pitch_env;
                let inc2 = self.sec_inc + (self.pitch_mod_span_inc * 0.8) * self.pitch_env;
                self.phase1 += inc1;
                if self.phase1 >= 1.0 {
                    self.phase1 -= 1.0;
                }
                self.phase2 += inc2;
                if self.phase2 >= 1.0 {
                    self.phase2 -= 1.0;
                }

                let body =
                    ((self.phase1 * TAU).sin() * 0.62 + (self.phase2 * TAU).sin() * 0.38) * self.fast_env;

                // Bandpass-filtered white noise for snare wires / hand clap
                let white = self.next_white_noise();
                let bp_noise = self.filter1.process(white);
                let shaped_noise = self.filter2.process(bp_noise + white * 0.22);

                let noise_env = if self.is_clap {
                    // Classic 3-burst micro-stutter over the first ~28 ms for hand clap (note 39)
                    let burst_phase = (self.age_samples as f32) * self.base_inc * 0.45;
                    if self.fast_env > 0.25 {
                        0.45 + 0.55 * (burst_phase * TAU).cos().abs()
                    } else {
                        self.slow_env
                    }
                } else {
                    self.slow_env
                };

                let wires = shaped_noise * noise_env;

                self.pitch_env = flush_denormal(self.pitch_env * self.pitch_decay);
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                if self.fast_env < 1.0e-5 && self.slow_env < 1.0e-5 {
                    self.clear();
                    return (0.0, 0.0);
                }

                tanh_pade((body * self.body_mix + wires * self.noise_mix) * 1.35) * self.level
            }

            PolyVoiceMode::HatOrCymbal => {
                // 6 square/pulse oscillators at non-harmonic metallic ratios
                let mut sq = [0.0_f32; 6];
                for i in 0..6 {
                    let mut p = self.metal_phases[i] + self.metal_incs[i];
                    if p >= 1.0 {
                        p -= 1.0;
                    }
                    self.metal_phases[i] = p;
                    // Asymmetric pulse wave (duty ~0.46) for richer even/odd metallic partials
                    sq[i] = if p < 0.46 { 1.0 } else { -1.0 };
                }

                // Ring-modulated (XOR on bipolar square waves) pairs + linear modal sum + crisp stick noise
                let xor_01 = sq[0] * sq[1];
                let xor_23 = sq[2] * sq[3];
                let xor_45 = sq[4] * sq[5];
                let cross_14 = sq[1] * sq[4];
                let modal_sum = (sq[0] + sq[1] + sq[2] + sq[3] + sq[4] + sq[5]) * (1.0 / 6.0);
                let stick_noise = self.next_white_noise() * self.fast_env * 0.30;

                let matrix = (xor_01 + xor_23 + xor_45 + cross_14) * 0.18 + modal_sum * 0.35 + stick_noise;

                let filtered = if self.is_cymbal_bandpass {
                    // Bandpass @ 3.5 kHz body shimmer + highpass @ 4.5 kHz sizzle
                    let bp = self.filter1.process(matrix);
                    let hp = self.filter2.process(matrix);
                    bp * 0.55 + hp * 0.65
                } else {
                    // Crisp 2-stage highpass @ 7 kHz for hi-hats
                    let hp1 = self.filter1.process(matrix);
                    self.filter2.process(hp1)
                };

                // Dual-stage decay envelope: sharp stick transient + metallic tail
                let env = self.fast_env * 0.55 + self.slow_env * 0.45;
                self.fast_env = flush_denormal(self.fast_env * self.fast_decay);
                self.slow_env = flush_denormal(self.slow_env * self.slow_decay);

                if self.fast_env < 1.0e-5 && self.slow_env < 1.0e-5 {
                    self.clear();
                    return (0.0, 0.0);
                }

                tanh_pade(filtered * env * 1.4) * self.level
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

/// Fixed-size 16-voice polyphonic pool for General MIDI percussion and 2-operator FM melodic voices.
pub struct PercussionAndGmPool {
    sample_rate: f32,
    voices: [PolyVoice; MAX_POLY_VOICES],
    steal_cursor: usize,
}

impl PercussionAndGmPool {
    /// Pre-allocates all 16 polyphonic voices at stream initialization.
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut voices = [PolyVoice::default(); MAX_POLY_VOICES];
        for (i, v) in voices.iter_mut().enumerate() {
            v.rng_state = 0x6D2B_79F5_u32.wrapping_add((i as u32).wrapping_mul(0x9E37_79B9));
        }
        Self {
            sample_rate: sr,
            voices,
            steal_cursor: 0,
        }
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

        // Closed (42) or Pedal (44) Hi-Hat chokes any active Open Hi-Hat (46)
        if gm_drum_note == 42 || gm_drum_note == 44 {
            for v in &mut self.voices {
                if v.is_active() && v.is_open_hat {
                    v.choke(sr);
                }
            }
        }

        let slot = self.allocate_voice(gm_drum_note, true);
        let voice = &mut self.voices[slot];

        match gm_drum_note {
            // 1. Kick & Toms (Notes 35, 36, 41–50)
            // Acoustic Bass Drum (35) & Bass Drum 1 (36): 160 Hz -> 48 Hz over 25 ms + 5 ms click
            35 => voice.trigger_kick_or_tom(35, 46.0, 155.0, 0.025, 0.24, 0.50, velocity, sr),
            36 => voice.trigger_kick_or_tom(36, 48.0, 160.0, 0.025, 0.22, 0.50, velocity, sr),
            // Floor Toms (41, 43)
            41 => voice.trigger_kick_or_tom(41, 78.0, 145.0, 0.030, 0.28, 0.66, velocity, sr),
            43 => voice.trigger_kick_or_tom(43, 95.0, 175.0, 0.028, 0.26, 0.60, velocity, sr),
            // Low & Low-Mid Toms (45, 47)
            45 => voice.trigger_kick_or_tom(45, 118.0, 215.0, 0.026, 0.24, 0.52, velocity, sr),
            47 => voice.trigger_kick_or_tom(47, 145.0, 260.0, 0.024, 0.22, 0.45, velocity, sr),
            // Hi-Mid & High Toms (48, 50)
            48 => voice.trigger_kick_or_tom(48, 172.0, 305.0, 0.022, 0.20, 0.38, velocity, sr),
            50 => voice.trigger_kick_or_tom(50, 205.0, 355.0, 0.020, 0.19, 0.32, velocity, sr),

            // 2. Snare & Claps (Notes 37, 38, 39, 40)
            // Side Stick / Rimshot (37): High woody modes + tight snap
            37 => voice.trigger_snare_or_clap(
                37, 320.0, 590.0, 0.022, 0.030, 2_800.0, 2.2, 0.80, 0.35, false, 0.46, velocity, sr,
            ),
            // Acoustic Snare (38): Dual body modes (185 Hz & 330 Hz) + bandpass snare wires
            38 => voice.trigger_snare_or_clap(
                38, 185.0, 330.0, 0.055, 0.145, 2_400.0, 1.1, 0.58, 0.65, false, 0.46, velocity, sr,
            ),
            // Hand Clap (39): Multi-burst bandpass noise @ 1.2 kHz
            39 => voice.trigger_snare_or_clap(
                39, 185.0, 330.0, 0.028, 0.170, 1_250.0, 1.6, 0.18, 0.90, true, 0.54, velocity, sr,
            ),
            // Electric Snare (40): Punchier 185 Hz & 330 Hz body + brighter longer wires
            40 => voice.trigger_snare_or_clap(
                40, 185.0, 330.0, 0.065, 0.175, 2_750.0, 1.0, 0.62, 0.72, false, 0.46, velocity, sr,
            ),

            // 3. Hi-Hats & Cymbals (Notes 42, 44, 46, 49, 51, 52, 53, 55, 57, 59)
            // Closed Hi-Hat (42): ~45 ms tight decay, 7 kHz highpass
            42 => voice.trigger_hat_or_cymbal(42, 1.00, false, false, 0.015, 0.045, 0.34, velocity, sr),
            // Pedal Hi-Hat (44): ~60 ms tight decay, 7 kHz highpass
            44 => voice.trigger_hat_or_cymbal(44, 0.96, false, false, 0.018, 0.060, 0.34, velocity, sr),
            // Open Hi-Hat (46): ~420 ms decay, choked by 42/44
            46 => voice.trigger_hat_or_cymbal(46, 1.00, false, true, 0.045, 0.420, 0.34, velocity, sr),
            // Crash Cymbal 1 (49): 3.5 kHz bandpass + highpass, ~1.2 s decay
            49 => voice.trigger_hat_or_cymbal(49, 0.92, true, false, 0.090, 1.200, 0.30, velocity, sr),
            // Ride Cymbal 1 (51): 3.5 kHz bandpass + highpass, ~1.25 s decay
            51 => voice.trigger_hat_or_cymbal(51, 1.12, true, false, 0.055, 1.250, 0.68, velocity, sr),
            // Chinese Cymbal (52): trashier detuned pitch scale, ~1.1 s decay
            52 => voice.trigger_hat_or_cymbal(52, 0.84, true, false, 0.110, 1.100, 0.72, velocity, sr),
            // Ride Bell (53): higher modal scale, clear ping
            53 => voice.trigger_hat_or_cymbal(53, 1.38, true, false, 0.070, 0.950, 0.68, velocity, sr),
            // Splash Cymbal (55): bright fast crash
            55 => voice.trigger_hat_or_cymbal(55, 1.18, true, false, 0.050, 0.650, 0.38, velocity, sr),
            // Crash Cymbal 2 (57): ~1.2 s decay
            57 => voice.trigger_hat_or_cymbal(57, 0.98, true, false, 0.095, 1.200, 0.64, velocity, sr),
            // Ride Cymbal 2 (59)
            59 => voice.trigger_hat_or_cymbal(59, 1.06, true, false, 0.060, 1.180, 0.66, velocity, sr),

            // Cowbell (56): 540 Hz & 800 Hz metallic pair
            56 => voice.trigger_snare_or_clap(
                56, 540.0, 800.0, 0.085, 0.015, 2_000.0, 2.0, 0.92, 0.12, false, 0.58, velocity, sr,
            ),
            // Tambourine (54) / Shaker (70) / Maracas (70)
            54 | 69 | 70 => {
                voice.trigger_hat_or_cymbal(gm_drum_note, 1.25, false, false, 0.025, 0.095, 0.60, velocity, sr)
            }
            // Fallback for other GM Latin/aux percussion notes (congas, bongos, woodblocks)
            other => {
                let base_hz = (90.0 + ((other % 24) as f32) * 14.0).clamp(65.0, 420.0);
                voice.trigger_kick_or_tom(
                    other,
                    base_hz,
                    base_hz * 1.45,
                    0.016,
                    0.14,
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
        // Allow polyphonic chords on the same GM channel by preferring free voices first
        let slot = self.allocate_voice(tag, false);
        self.voices[slot].trigger_gm_fm(channel, freq_hz, velocity, program, self.sample_rate);
    }

    /// Immediately silences all 16 voices.
    #[inline(always)]
    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.clear();
        }
    }

    /// Renders and sums all active percussion and GM melodic voices for the next stereo frame.
    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;
        for v in &mut self.voices {
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
        assert!(energy > 1.0, "Kick + Snare must produce strong finite energy, got {}", energy);

        // 2. Trigger Open Hi-Hat (46), then choke it with Closed Hi-Hat (42)
        pool.all_notes_off();
        pool.trigger_drum(46, 0.90);
        assert!(pool.voices.iter().any(|v| v.is_active() && v.is_open_hat));

        // Triggering closed hi-hat (42) must clear is_open_hat and accelerate decay on voice 46
        pool.trigger_drum(42, 0.85);
        assert!(
            !pool.voices.iter().any(|v| v.is_active() && v.is_open_hat),
            "Closed hi-hat (42) must choke open hi-hat (46)"
        );
    }

    #[test]
    fn test_phase4_gm_fm_melodic_presets_microtonal() {
        let sr = 48_000.0;
        let mut pool = PercussionAndGmPool::new(sr);

        // Trigger all 5 GM FM preset categories with microtonal frequencies
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
        assert!(energy > 0.5, "2-op FM GM voices must produce audible signal, got {}", energy);
    }
}
