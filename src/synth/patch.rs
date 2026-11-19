//! Serializable patch architecture and factory presets for the physical-modeling and percussion synthesizer (`src/synth/patch.rs`).
//!
//! Provides `SynthPreset` and its component parameter structs (`StringSynthParams`, `BodyResonatorParams`,
//! `DrumKitParams`, `FmVoiceParams`), factory presets, and JSON serialization.

use crate::synth::body::InstrumentProfile;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Active synthesis algorithm for pitched tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PitchedEngine {
    /// Extended Karplus-Strong physical string waveguide + acoustic/pickup body resonator.
    #[default]
    PhysicalString,
    /// 2-operator frequency modulation synthesizer for melodic backing and keyboard sounds.
    FmSynth,
}

fn default_polarization_detune_cents() -> f32 {
    0.7
}
fn default_polarization_coupling() -> f32 {
    0.012
}
fn default_tension_mod_amount() -> f32 {
    0.30
}
fn default_fret_buzz_sizzle() -> f32 {
    0.20
}
fn default_sympathetic_coupling() -> f32 {
    0.0025
}
fn default_modal_knock_air() -> f32 {
    0.55
}

/// Extended Karplus-Strong physical string waveguide parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StringSynthParams {
    /// Normalized pluck position in `[0.05, 0.50]` (0.05 = bridge/bright, 0.50 = middle/hollow).
    pub pluck_pos: f32,
    /// Pick hardness / lowpass cutoff frequency on initial excitation burst in Hz (500.0..=16000.0).
    pub pick_hardness: f32,
    /// Ratio of transient winding-scrape noise vs. raised-cosine pluck displacement pulse in `[0.0, 1.0]`.
    pub noise_mix: f32,
    /// Target $T_{60}$ loop decay sustain time in seconds (0.1..=10.0 s).
    pub t60_decay_sec: f32,
    /// Brightness / stretching factor $S$ for one-zero loop lowpass filter $H(z) = g \cdot ((1-S) + S z^{-1})$ (0.01..=0.99).
    pub brightness_s: f32,
    /// 1st-order all-pass stiffness dispersion coefficient for wound strings (0.0..=0.80).
    pub inharmonicity_dispersion: f32,
    /// Palm mute damping multiplier applied to loop decay (0.01..=0.80).
    pub palm_mute_damping: f32,
    /// Duration of harmonic node isolation tap coupling in milliseconds (10.0..=500.0 ms).
    pub harmonic_damping_ms: f32,
    /// Detuning of the vertical polarization delay line relative to horizontal in cents (0.0..=3.0 cents, default ~0.7).
    #[serde(default = "default_polarization_detune_cents")]
    pub polarization_detune_cents: f32,
    /// Bridge $2 \times 2$ orthogonal mixing angle between horizontal and vertical polarizations (0.0..=0.05, default 0.012).
    #[serde(default = "default_polarization_coupling")]
    pub polarization_coupling: f32,
    /// Strength of energy-driven tension modulation pitch drop on hard plucks (`0.0..=1.0`).
    #[serde(default = "default_tension_mod_amount")]
    pub tension_mod_amount: f32,
    /// Acoustic fret buzz / sizzle sensitivity controlling the asymmetric `fret_clearance` barrier (`0.0..=1.0`).
    #[serde(default = "default_fret_buzz_sizzle")]
    pub fret_buzz_sizzle: f32,
    /// Fraction of summed bridge force coupled back into unmuted waveguides for sympathetic resonance (`0.0..=0.015`, default `0.0025`).
    #[serde(default = "default_sympathetic_coupling")]
    pub sympathetic_coupling: f32,
}

impl Default for StringSynthParams {
    fn default() -> Self {
        Self {
            pluck_pos: 0.18,
            pick_hardness: 8_500.0,
            noise_mix: 0.65,
            t60_decay_sec: 3.5,
            brightness_s: 0.16,
            inharmonicity_dispersion: 0.12,
            palm_mute_damping: 0.22,
            harmonic_damping_ms: 30.0,
            polarization_detune_cents: default_polarization_detune_cents(),
            polarization_coupling: default_polarization_coupling(),
            tension_mod_amount: default_tension_mod_amount(),
            fret_buzz_sizzle: default_fret_buzz_sizzle(),
            sympathetic_coupling: default_sympathetic_coupling(),
        }
    }
}

/// Parametric 2nd-order Biquad filter band parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiquadBandParams {
    /// Center or corner cutoff frequency in Hz (20.0..=20000.0).
    pub freq_hz: f32,
    /// Filter resonance / quality factor $Q$ (0.1..=15.0).
    pub q: f32,
    /// Peak boost or cut gain in dB (-24.0..=+24.0 dB).
    pub gain_db: f32,
    /// Whether this filter band is active in the DSP chain.
    pub enabled: bool,
}

impl Default for BiquadBandParams {
    fn default() -> Self {
        Self {
            freq_hz: 1000.0,
            q: 1.0,
            gain_db: 0.0,
            enabled: true,
        }
    }
}

/// Acoustic soundboard body & electric pickup / amplifier / cabinet resonator parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyResonatorParams {
    /// Baseline physical resonator archetype.
    pub profile_base: InstrumentProfile,
    /// Wet / dry mix between resonator modes and direct string signal (0.0..=1.0).
    pub body_wet_mix: f32,
    /// Pre-filter overdrive / saturation drive factor (0.0..=5.0).
    pub pre_drive: f32,
    /// Output makeup gain in dB (-18.0..=+12.0 dB).
    pub post_gain_db: f32,
    /// Intensity of the 12-mode wooden body resonant bank + 4-tap Householder micro soundboard diffusion (`0.0..=1.0`).
    #[serde(default = "default_modal_knock_air")]
    pub modal_knock_air: f32,
    /// 4 parametric biquad bands:
    /// - `bands[0]`: Helmholtz / air cavity resonance (~98 Hz)
    /// - `bands[1]`: Top plate / wood mode dipole (~205 Hz)
    /// - `bands[2]`: Pickup resonance / presence peak (~2.8 kHz–3.5 kHz)
    /// - `bands[3]`: Cabinet lowpass / high shelf speaker cone roll-off (~5.0 kHz)
    pub bands: [BiquadBandParams; 4],
}

impl Default for BodyResonatorParams {
    fn default() -> Self {
        Self {
            profile_base: InstrumentProfile::AcousticGuitar,
            body_wet_mix: 0.35,
            pre_drive: 1.0,
            post_gain_db: 0.0,
            modal_knock_air: default_modal_knock_air(),
            bands: [
                BiquadBandParams {
                    freq_hz: 98.0,
                    q: 6.0,
                    gain_db: 3.0,
                    enabled: true,
                },
                BiquadBandParams {
                    freq_hz: 205.0,
                    q: 5.2,
                    gain_db: 2.0,
                    enabled: true,
                },
                BiquadBandParams {
                    freq_hz: 2800.0,
                    q: 1.8,
                    gain_db: 2.5,
                    enabled: true,
                },
                BiquadBandParams {
                    freq_hz: 5200.0,
                    q: 0.85,
                    gain_db: -6.0,
                    enabled: true,
                },
            ],
        }
    }
}

/// Target percussion voice role for drum parameter updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumRole {
    Kick,
    Snare,
    Toms,
    HiHats,
    Cymbals,
    PercAux,
}

fn default_kick_body_decay() -> f32 {
    240.0
}
fn default_kick_tension_mod() -> f32 {
    0.55
}
fn default_kick_strike_pos() -> f32 {
    0.15
}
fn default_kick_overtones_ring() -> f32 {
    0.30
}
fn default_kick_shell_cavity() -> f32 {
    0.50
}
fn default_drum_hit_variation() -> f32 {
    0.10
}

/// Kick drum voice parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KickParams {
    /// Initial frequency of the exponential pitch sweep in Hz (80.0..=250.0).
    pub pitch_start_hz: f32,
    /// Resting fundamental sub-bass frequency in Hz (30.0..=65.0).
    pub pitch_end_hz: f32,
    /// Duration of the pitch drop envelope in milliseconds (10.0..=80.0 ms).
    pub pitch_decay_ms: f32,
    /// High-frequency beater attack click transient level (0.0..=1.0).
    pub click_level: f32,
    /// Main drum shell body resonant decay duration in milliseconds (50.0..=600.0 ms).
    #[serde(default = "default_kick_body_decay")]
    pub body_decay_ms: f32,
    /// Non-linear drumhead tension pitch modulation depth on hard hits (0.0..=1.0).
    #[serde(default = "default_kick_tension_mod")]
    pub head_tension_mod: f32,
    /// Strike position across the circular membrane (0.0 = dead center punch, 1.0 = rim/edge ring).
    #[serde(default = "default_kick_strike_pos")]
    pub strike_position: f32,
    /// Higher Bessel overtone decay persistence relative to the (0,1) fundamental (0.0..=1.0).
    #[serde(default = "default_kick_overtones_ring")]
    pub overtones_ring: f32,
    /// Wooden shell air-cavity mode & 2-tap micro-reflection body mix (0.0..=1.0).
    #[serde(default = "default_kick_shell_cavity")]
    pub shell_wood_cavity: f32,
    /// Per-hit humanization amount varying velocity & strike position (0.0 = locked/identical, 1.0 = heavy variation).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            pitch_start_hz: 160.0,
            pitch_end_hz: 48.0,
            pitch_decay_ms: 25.0,
            click_level: 0.65,
            body_decay_ms: 240.0,
            head_tension_mod: default_kick_tension_mod(),
            strike_position: default_kick_strike_pos(),
            overtones_ring: default_kick_overtones_ring(),
            shell_wood_cavity: default_kick_shell_cavity(),
            hit_variation: default_drum_hit_variation(),
        }
    }
}

fn default_snare_mode2() -> f32 {
    330.0
}
fn default_snare_tone_decay() -> f32 {
    55.0
}
fn default_snare_wire_cutoff() -> f32 {
    2400.0
}
fn default_snare_clap_spacing() -> f32 {
    12.0
}
fn default_snare_tension_mod() -> f32 {
    0.40
}
fn default_snare_strike_pos() -> f32 {
    0.32
}
fn default_snare_overtones_ring() -> f32 {
    0.45
}
fn default_snare_shell_cavity() -> f32 {
    0.52
}
fn default_snare_wire_tension() -> f32 {
    0.55
}

/// Snare drum, rimshot, and hand clap voice parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnareParams {
    /// Fundamental shell body tone frequency in Hz (120.0..=350.0).
    pub tone_freq_hz: f32,
    /// Second resonant drumhead mode frequency in Hz (200.0..=600.0).
    #[serde(default = "default_snare_mode2")]
    pub mode2_freq_hz: f32,
    /// Shell body tone resonance decay duration in milliseconds (20.0..=200.0 ms).
    #[serde(default = "default_snare_tone_decay")]
    pub tone_decay_ms: f32,
    /// Snare wire noise bandpass center frequency in Hz (1000.0..=5000.0).
    #[serde(default = "default_snare_wire_cutoff")]
    pub wire_cutoff_hz: f32,
    /// Snare wire noise decay duration in milliseconds (50.0..=400.0 ms).
    pub noise_decay_ms: f32,
    /// Mix ratio between physically coupled snare wires and resonant membrane body (0.0..=1.0).
    pub noise_mix: f32,
    /// Burst repetition spacing for multi-clap transient in milliseconds (5.0..=30.0 ms).
    #[serde(default = "default_snare_clap_spacing")]
    pub clap_spacing_ms: f32,
    /// Non-linear drumhead tension pitch modulation depth on hard hits (0.0..=1.0).
    #[serde(default = "default_snare_tension_mod")]
    pub head_tension_mod: f32,
    /// Strike position across the circular membrane (0.0 = dead center punch, 1.0 = rim/edge ring).
    #[serde(default = "default_snare_strike_pos")]
    pub strike_position: f32,
    /// Higher Bessel overtone decay persistence relative to the (0,1) fundamental (0.0..=1.0).
    #[serde(default = "default_snare_overtones_ring")]
    pub overtones_ring: f32,
    /// Wooden shell air-cavity mode & 2-tap micro-reflection body mix (0.0..=1.0).
    #[serde(default = "default_snare_shell_cavity")]
    pub shell_wood_cavity: f32,
    /// Snare wire strainer tension / rattle threshold (0.0 = loose buzz, 1.0 = tight snap).
    #[serde(default = "default_snare_wire_tension")]
    pub wire_tension: f32,
    /// Per-hit humanization amount varying velocity & strike position (0.0 = locked/identical, 1.0 = heavy variation).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for SnareParams {
    fn default() -> Self {
        Self {
            tone_freq_hz: 185.0,
            mode2_freq_hz: 330.0,
            tone_decay_ms: 55.0,
            wire_cutoff_hz: 2400.0,
            noise_decay_ms: 180.0,
            noise_mix: 0.55,
            clap_spacing_ms: 12.0,
            head_tension_mod: default_snare_tension_mod(),
            strike_position: default_snare_strike_pos(),
            overtones_ring: default_snare_overtones_ring(),
            shell_wood_cavity: default_snare_shell_cavity(),
            wire_tension: default_snare_wire_tension(),
            hit_variation: default_drum_hit_variation(),
        }
    }
}

fn default_tom_tension_mod() -> f32 {
    0.50
}
fn default_tom_strike_pos() -> f32 {
    0.28
}
fn default_tom_overtones_ring() -> f32 {
    0.50
}
fn default_tom_shell_cavity() -> f32 {
    0.58
}

/// Tuned tom-tom voice parameters across all 6 General MIDI toms.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TomParams {
    /// Global pitch scaling multiplier across all 6 toms (0.5..=2.0).
    pub pitch_scale: f32,
    /// Exponential pitch drop sweep ratio $f_{start} / f_{end}$ (1.2..=3.0).
    pub pitch_drop_ratio: f32,
    /// Tomhead resonant decay duration in milliseconds (50.0..=800.0 ms).
    pub decay_ms: f32,
    /// Stick beater attack level (0.0..=1.0).
    pub stick_attack: f32,
    /// Non-linear drumhead tension pitch modulation depth on hard hits (0.0..=1.0).
    #[serde(default = "default_tom_tension_mod")]
    pub head_tension_mod: f32,
    /// Strike position across the circular membrane (0.0 = dead center punch, 1.0 = rim/edge ring).
    #[serde(default = "default_tom_strike_pos")]
    pub strike_position: f32,
    /// Higher Bessel overtone decay persistence relative to the (0,1) fundamental (0.0..=1.0).
    #[serde(default = "default_tom_overtones_ring")]
    pub overtones_ring: f32,
    /// Wooden shell air-cavity mode & 2-tap micro-reflection body mix (0.0..=1.0).
    #[serde(default = "default_tom_shell_cavity")]
    pub shell_wood_cavity: f32,
    /// Per-hit humanization amount varying velocity & strike position (0.0 = locked/identical, 1.0 = heavy variation).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for TomParams {
    fn default() -> Self {
        Self {
            pitch_scale: 1.0,
            pitch_drop_ratio: 1.85,
            decay_ms: 240.0,
            stick_attack: 0.60,
            head_tension_mod: default_tom_tension_mod(),
            strike_position: default_tom_strike_pos(),
            overtones_ring: default_tom_overtones_ring(),
            shell_wood_cavity: default_tom_shell_cavity(),
            hit_variation: default_drum_hit_variation(),
        }
    }
}

fn default_bronze_size_pitch() -> f32 {
    1.0
}
fn default_hat_plate_dispersion() -> f32 {
    0.48
}
fn default_hat_nonlin_shimmer() -> f32 {
    0.42
}
fn default_hat_bell_level() -> f32 {
    0.35
}
fn default_hat_stick_hardness() -> f32 {
    0.68
}

/// Cast B20 bronze hi-hat parameters for closed, pedal, and open modes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HiHatParams {
    /// Frequency spread multiplier on bronze modal spacing (0.5..=2.0).
    pub spread: f32,
    /// Highpass filter cutoff frequency in Hz (3000.0..=12000.0).
    pub highpass_cutoff_hz: f32,
    /// Closed hi-hat decay time in milliseconds (15.0..=150.0 ms).
    pub closed_decay_ms: f32,
    /// Pedal hi-hat decay time in milliseconds (20.0..=200.0 ms).
    pub pedal_decay_ms: f32,
    /// Open hi-hat decay time in milliseconds (100.0..=2000.0 ms).
    pub open_decay_ms: f32,
    /// Bronze plate diameter/pitch scaling multiplier (0.5 = large/dark, 2.0 = small/bright).
    #[serde(default = "default_bronze_size_pitch")]
    pub bronze_size_pitch: f32,
    /// All-pass plate dispersion depth shaping initial stick ping character (0.0..=1.0).
    #[serde(default = "default_hat_plate_dispersion")]
    pub plate_dispersion: f32,
    /// Signal-modulated non-linear all-pass shimmer cascade bloom (0.0..=1.0).
    #[serde(default = "default_hat_nonlin_shimmer")]
    pub nonlin_shimmer: f32,
    /// High-Q bronze bell / cup modal resonance emphasis (0.0..=1.0).
    #[serde(default = "default_hat_bell_level")]
    pub bell_level: f32,
    /// Drumstick tip hardness controlling excitation lowpass brightness (0.0..=1.0).
    #[serde(default = "default_hat_stick_hardness")]
    pub stick_hardness: f32,
    /// Per-hit humanization amount varying velocity & stick strike character (0.0 = locked/identical, 1.0 = heavy variation).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for HiHatParams {
    fn default() -> Self {
        Self {
            spread: 1.0,
            highpass_cutoff_hz: 7000.0,
            closed_decay_ms: 45.0,
            pedal_decay_ms: 60.0,
            open_decay_ms: 650.0,
            bronze_size_pitch: default_bronze_size_pitch(),
            plate_dispersion: default_hat_plate_dispersion(),
            nonlin_shimmer: default_hat_nonlin_shimmer(),
            bell_level: default_hat_bell_level(),
            stick_hardness: default_hat_stick_hardness(),
            hit_variation: default_drum_hit_variation(),
        }
    }
}

fn default_cymbal_plate_dispersion() -> f32 {
    0.52
}
fn default_cymbal_nonlin_shimmer() -> f32 {
    0.58
}
fn default_cymbal_stick_hardness() -> f32 {
    0.62
}

/// Cast B20 bronze crash and ride cymbal parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CymbalParams {
    /// Frequency spread multiplier on bronze modal spacing (0.5..=2.0).
    pub matrix_spread: f32,
    /// Bandpass/highpass filter cutoff frequency in Hz (2000.0..=8000.0).
    pub cutoff_hz: f32,
    /// Ride bell harmonic presence peak level (0.0..=1.0).
    pub bell_peak_level: f32,
    /// Crash cymbal decay time in milliseconds (300.0..=3000.0 ms).
    pub crash_decay_ms: f32,
    /// Ride cymbal decay time in milliseconds (400.0..=3500.0 ms).
    pub ride_decay_ms: f32,
    /// Bronze plate diameter/pitch scaling multiplier (0.5 = large/dark, 2.0 = small/bright).
    #[serde(default = "default_bronze_size_pitch")]
    pub bronze_size_pitch: f32,
    /// All-pass plate dispersion depth shaping initial stick ping character (0.0..=1.0).
    #[serde(default = "default_cymbal_plate_dispersion")]
    pub plate_dispersion: f32,
    /// Signal-modulated non-linear all-pass shimmer cascade bloom (0.0..=1.0).
    #[serde(default = "default_cymbal_nonlin_shimmer")]
    pub nonlin_shimmer: f32,
    /// Drumstick tip hardness controlling excitation lowpass brightness (0.0..=1.0).
    #[serde(default = "default_cymbal_stick_hardness")]
    pub stick_hardness: f32,
    /// Per-hit humanization amount varying velocity & stick strike character (0.0 = locked/identical, 1.0 = heavy variation).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for CymbalParams {
    fn default() -> Self {
        Self {
            matrix_spread: 1.0,
            cutoff_hz: 3500.0,
            bell_peak_level: 0.50,
            crash_decay_ms: 1200.0,
            ride_decay_ms: 1300.0,
            bronze_size_pitch: default_bronze_size_pitch(),
            plate_dispersion: default_cymbal_plate_dispersion(),
            nonlin_shimmer: default_cymbal_nonlin_shimmer(),
            stick_hardness: default_cymbal_stick_hardness(),
            hit_variation: default_drum_hit_variation(),
        }
    }
}

/// Tuned metallic auxiliary percussion parameters (Cowbell, Agogo, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PercAuxParams {
    /// Fundamental modal frequency 1 in Hz (300.0..=1000.0).
    pub freq1_hz: f32,
    /// Second modal frequency 2 in Hz (400.0..=1400.0).
    pub freq2_hz: f32,
    /// Detune offset in Hz (-50.0..=50.0).
    pub detune: f32,
    /// Resonant bandpass filter Q factor (0.5..=10.0).
    pub bandpass_q: f32,
    /// Resonant metallic ring decay in milliseconds (20.0..=500.0 ms).
    pub decay_ms: f32,
}

impl Default for PercAuxParams {
    fn default() -> Self {
        Self {
            freq1_hz: 540.0,
            freq2_hz: 800.0,
            detune: 0.0,
            bandpass_q: 2.0,
            decay_ms: 85.0,
        }
    }
}

/// Complete procedural drum kit parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct DrumKitParams {
    #[serde(default)]
    pub kick: KickParams,
    #[serde(default)]
    pub snare: SnareParams,
    #[serde(default)]
    pub toms: TomParams,
    #[serde(default)]
    pub hi_hats: HiHatParams,
    #[serde(default)]
    pub cymbals: CymbalParams,
    #[serde(default)]
    pub aux: PercAuxParams,
}

/// Lightweight `Copy` parameter payload for updating a specific drum role.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DrumVoiceParams {
    Kick(KickParams),
    Snare(SnareParams),
    Toms(TomParams),
    HiHats(HiHatParams),
    Cymbals(CymbalParams),
    PercAux(PercAuxParams),
}

impl DrumVoiceParams {
    pub fn role(&self) -> DrumRole {
        match self {
            DrumVoiceParams::Kick(_) => DrumRole::Kick,
            DrumVoiceParams::Snare(_) => DrumRole::Snare,
            DrumVoiceParams::Toms(_) => DrumRole::Toms,
            DrumVoiceParams::HiHats(_) => DrumRole::HiHats,
            DrumVoiceParams::Cymbals(_) => DrumRole::Cymbals,
            DrumVoiceParams::PercAux(_) => DrumRole::PercAux,
        }
    }
}

/// 4-stage ADSR envelope parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdsrEnvelopeParams {
    /// Attack duration in milliseconds (0.5..=500.0 ms).
    pub attack_ms: f32,
    /// Decay duration in milliseconds (10.0..=2000.0 ms).
    pub decay_ms: f32,
    /// Sustain level in `[0.0, 1.0]`.
    pub sustain: f32,
    /// Release duration in milliseconds (10.0..=3000.0 ms).
    pub release_ms: f32,
}

impl Default for AdsrEnvelopeParams {
    fn default() -> Self {
        Self {
            attack_ms: 5.0,
            decay_ms: 120.0,
            sustain: 0.6,
            release_ms: 250.0,
        }
    }
}

/// 2-operator FM melodic backing synthesizer parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmVoiceParams {
    /// Frequency ratio between modulator and carrier (0.5..=12.0).
    pub mod_ratio: f32,
    /// Peak modulation index / FM brightness (0.0..=10.0).
    pub mod_index: f32,
    /// Modulator self-feedback amount for brassy or abrasive harmonics (0.0..=1.5).
    pub feedback: f32,
    /// Carrier amplitude ADSR envelope.
    pub carrier_adsr: AdsrEnvelopeParams,
    /// Modulator modulation index ADSR envelope.
    pub mod_adsr: AdsrEnvelopeParams,
}

impl Default for FmVoiceParams {
    fn default() -> Self {
        Self {
            mod_ratio: 1.0,
            mod_index: 1.8,
            feedback: 0.2,
            carrier_adsr: AdsrEnvelopeParams {
                attack_ms: 2.0,
                decay_ms: 180.0,
                sustain: 0.5,
                release_ms: 300.0,
            },
            mod_adsr: AdsrEnvelopeParams {
                attack_ms: 1.0,
                decay_ms: 120.0,
                sustain: 0.25,
                release_ms: 200.0,
            },
        }
    }
}

/// Complete synthesizer patch capturing the entire physical-modeling, resonator, drum, and FM state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthPreset {
    pub name: String,
    pub author: String,
    #[serde(default)]
    pub engine: PitchedEngine,
    pub string_params: StringSynthParams,
    pub body_params: BodyResonatorParams,
    pub drum_params: DrumKitParams,
    pub fm_params: FmVoiceParams,
}

impl Default for SynthPreset {
    fn default() -> Self {
        Self::acoustic_dreadnought()
    }
}

impl SynthPreset {
    /// Factory preset: Full-bodied acoustic dreadnought guitar.
    pub fn acoustic_dreadnought() -> Self {
        Self {
            name: "Acoustic Dreadnought".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::PhysicalString,
            string_params: StringSynthParams {
                pluck_pos: 0.18,
                pick_hardness: 8_500.0,
                noise_mix: 0.65,
                t60_decay_sec: 4.2,
                brightness_s: 0.15,
                inharmonicity_dispersion: 0.15,
                palm_mute_damping: 0.22,
                harmonic_damping_ms: 30.0,
                polarization_detune_cents: 0.75,
                polarization_coupling: 0.013,
                tension_mod_amount: 0.30,
                fret_buzz_sizzle: 0.22,
                sympathetic_coupling: 0.0030,
            },
            body_params: BodyResonatorParams {
                profile_base: InstrumentProfile::AcousticGuitar,
                body_wet_mix: 0.35,
                pre_drive: 1.0,
                post_gain_db: 0.0,
                modal_knock_air: 0.72,
                bands: [
                    BiquadBandParams {
                        freq_hz: 98.0,
                        q: 6.0,
                        gain_db: 3.5,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 205.0,
                        q: 5.2,
                        gain_db: 2.5,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 410.0,
                        q: 4.5,
                        gain_db: 1.8,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 5200.0,
                        q: 0.85,
                        gain_db: -4.0,
                        enabled: true,
                    },
                ],
            },
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams::default(),
        }
    }

    /// Factory preset: Bright, twangy electric single-coil clean.
    pub fn tele_clean() -> Self {
        Self {
            name: "Tele Clean".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::PhysicalString,
            string_params: StringSynthParams {
                pluck_pos: 0.14,
                pick_hardness: 11_000.0,
                noise_mix: 0.50,
                t60_decay_sec: 5.0,
                brightness_s: 0.09,
                inharmonicity_dispersion: 0.10,
                palm_mute_damping: 0.25,
                harmonic_damping_ms: 25.0,
                polarization_detune_cents: 0.60,
                polarization_coupling: 0.010,
                tension_mod_amount: 0.25,
                fret_buzz_sizzle: 0.18,
                sympathetic_coupling: 0.0020,
            },
            body_params: BodyResonatorParams {
                profile_base: InstrumentProfile::ElectricClean,
                body_wet_mix: 1.0,
                pre_drive: 1.35,
                post_gain_db: -1.0,
                modal_knock_air: 0.38,
                bands: [
                    BiquadBandParams {
                        freq_hz: 85.0,
                        q: 0.707,
                        gain_db: -2.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 400.0,
                        q: 1.2,
                        gain_db: -4.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 3300.0,
                        q: 1.85,
                        gain_db: 4.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 5000.0,
                        q: 0.85,
                        gain_db: -8.0,
                        enabled: true,
                    },
                ],
            },
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams::default(),
        }
    }

    /// Factory preset: Saturated high-gain lead guitar with sustain and presence.
    pub fn heavy_lead() -> Self {
        Self {
            name: "Heavy Lead".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::PhysicalString,
            string_params: StringSynthParams {
                pluck_pos: 0.12,
                pick_hardness: 12_500.0,
                noise_mix: 0.45,
                t60_decay_sec: 6.5,
                brightness_s: 0.08,
                inharmonicity_dispersion: 0.08,
                palm_mute_damping: 0.32,
                harmonic_damping_ms: 20.0,
                polarization_detune_cents: 0.50,
                polarization_coupling: 0.009,
                tension_mod_amount: 0.35,
                fret_buzz_sizzle: 0.15,
                sympathetic_coupling: 0.0018,
            },
            body_params: BodyResonatorParams {
                profile_base: InstrumentProfile::ElectricOverdrive,
                body_wet_mix: 1.0,
                pre_drive: 3.8,
                post_gain_db: -2.5,
                modal_knock_air: 0.42,
                bands: [
                    BiquadBandParams {
                        freq_hz: 85.0,
                        q: 0.75,
                        gain_db: -3.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 400.0,
                        q: 1.25,
                        gain_db: -4.5,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 2850.0,
                        q: 2.2,
                        gain_db: 5.5,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 5000.0,
                        q: 0.90,
                        gain_db: -12.0,
                        enabled: true,
                    },
                ],
            },
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams::default(),
        }
    }

    /// Factory preset: Deep, warm fingered upright/acoustic bass.
    pub fn upright_bass() -> Self {
        Self {
            name: "Upright Bass".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::PhysicalString,
            string_params: StringSynthParams {
                pluck_pos: 0.28,
                pick_hardness: 4_200.0,
                noise_mix: 0.40,
                t60_decay_sec: 5.5,
                brightness_s: 0.28,
                inharmonicity_dispersion: 0.35,
                palm_mute_damping: 0.18,
                harmonic_damping_ms: 35.0,
                polarization_detune_cents: 0.85,
                polarization_coupling: 0.015,
                tension_mod_amount: 0.45,
                fret_buzz_sizzle: 0.30,
                sympathetic_coupling: 0.0025,
            },
            body_params: BodyResonatorParams {
                profile_base: InstrumentProfile::FingeredBass,
                body_wet_mix: 0.90,
                pre_drive: 1.2,
                post_gain_db: 1.5,
                modal_knock_air: 0.68,
                bands: [
                    BiquadBandParams {
                        freq_hz: 38.0,
                        q: 0.707,
                        gain_db: -1.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 100.0,
                        q: 1.1,
                        gain_db: 6.0,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 320.0,
                        q: 1.2,
                        gain_db: -2.5,
                        enabled: true,
                    },
                    BiquadBandParams {
                        freq_hz: 3200.0,
                        q: 0.75,
                        gain_db: -8.0,
                        enabled: true,
                    },
                ],
            },
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams::default(),
        }
    }

    /// Factory preset: Classic analog & 808 percussion kit.
    pub fn linn_808_kit() -> Self {
        Self {
            name: "Linn-808 Kit".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::PhysicalString,
            string_params: StringSynthParams::default(),
            body_params: BodyResonatorParams::default(),
            drum_params: DrumKitParams {
                kick: KickParams {
                    pitch_start_hz: 180.0,
                    pitch_end_hz: 45.0,
                    pitch_decay_ms: 32.0,
                    click_level: 0.75,
                    body_decay_ms: 260.0,
                    ..KickParams::default()
                },
                snare: SnareParams {
                    tone_freq_hz: 210.0,
                    mode2_freq_hz: 340.0,
                    tone_decay_ms: 60.0,
                    wire_cutoff_hz: 2600.0,
                    noise_decay_ms: 220.0,
                    noise_mix: 0.65,
                    clap_spacing_ms: 12.0,
                    ..SnareParams::default()
                },
                toms: TomParams::default(),
                hi_hats: HiHatParams {
                    spread: 1.05,
                    highpass_cutoff_hz: 7500.0,
                    closed_decay_ms: 40.0,
                    pedal_decay_ms: 55.0,
                    open_decay_ms: 750.0,
                    ..HiHatParams::default()
                },
                cymbals: CymbalParams::default(),
                aux: PercAuxParams::default(),
            },
            fm_params: FmVoiceParams::default(),
        }
    }

    /// Factory preset: DX7-style 2-operator electric piano.
    pub fn fm_dx_electric_piano() -> Self {
        Self {
            name: "FM DX Electric Piano".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::FmSynth,
            string_params: StringSynthParams::default(),
            body_params: BodyResonatorParams::default(),
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams {
                mod_ratio: 1.0,
                mod_index: 2.2,
                feedback: 0.25,
                carrier_adsr: AdsrEnvelopeParams {
                    attack_ms: 1.5,
                    decay_ms: 240.0,
                    sustain: 0.40,
                    release_ms: 400.0,
                },
                mod_adsr: AdsrEnvelopeParams {
                    attack_ms: 1.0,
                    decay_ms: 140.0,
                    sustain: 0.15,
                    release_ms: 200.0,
                },
            },
        }
    }

    /// Factory preset: Warm analog brass pad with dynamic filter bloom.
    pub fn analog_brass() -> Self {
        Self {
            name: "Analog Brass".to_string(),
            author: "guitab".to_string(),
            engine: PitchedEngine::FmSynth,
            string_params: StringSynthParams::default(),
            body_params: BodyResonatorParams::default(),
            drum_params: DrumKitParams::default(),
            fm_params: FmVoiceParams {
                mod_ratio: 1.0,
                mod_index: 3.2,
                feedback: 0.35,
                carrier_adsr: AdsrEnvelopeParams {
                    attack_ms: 22.0,
                    decay_ms: 350.0,
                    sustain: 0.75,
                    release_ms: 500.0,
                },
                mod_adsr: AdsrEnvelopeParams {
                    attack_ms: 35.0,
                    decay_ms: 280.0,
                    sustain: 0.55,
                    release_ms: 350.0,
                },
            },
        }
    }

    /// All built-in factory presets in display order.
    pub fn factory_presets() -> Vec<Self> {
        vec![
            Self::acoustic_dreadnought(),
            Self::tele_clean(),
            Self::heavy_lead(),
            Self::upright_bass(),
            Self::linn_808_kit(),
            Self::fm_dx_electric_piano(),
            Self::analog_brass(),
        ]
    }

    /// Serializes this preset to a clean JSON string.
    pub fn to_json_string(&self) -> Result<String, String> {
        let mut out = String::with_capacity(2048);
        out.push_str("{\n");
        out.push_str(&format!("  \"name\": \"{}\",\n", self.name));
        out.push_str(&format!("  \"author\": \"{}\",\n", self.author));
        out.push_str(&format!("  \"engine\": \"{:?}\",\n", self.engine));

        // String params
        out.push_str("  \"string_params\": {\n");
        out.push_str(&format!("    \"pluck_pos\": {:.4},\n", self.string_params.pluck_pos));
        out.push_str(&format!("    \"pick_hardness\": {:.1},\n", self.string_params.pick_hardness));
        out.push_str(&format!("    \"noise_mix\": {:.4},\n", self.string_params.noise_mix));
        out.push_str(&format!("    \"t60_decay_sec\": {:.3},\n", self.string_params.t60_decay_sec));
        out.push_str(&format!("    \"brightness_s\": {:.4},\n", self.string_params.brightness_s));
        out.push_str(&format!("    \"inharmonicity_dispersion\": {:.4},\n", self.string_params.inharmonicity_dispersion));
        out.push_str(&format!("    \"palm_mute_damping\": {:.4},\n", self.string_params.palm_mute_damping));
        out.push_str(&format!("    \"harmonic_damping_ms\": {:.1},\n", self.string_params.harmonic_damping_ms));
        out.push_str(&format!("    \"polarization_detune_cents\": {:.4},\n", self.string_params.polarization_detune_cents));
        out.push_str(&format!("    \"polarization_coupling\": {:.5},\n", self.string_params.polarization_coupling));
        out.push_str(&format!("    \"tension_mod_amount\": {:.4},\n", self.string_params.tension_mod_amount));
        out.push_str(&format!("    \"fret_buzz_sizzle\": {:.4},\n", self.string_params.fret_buzz_sizzle));
        out.push_str(&format!("    \"sympathetic_coupling\": {:.5}\n", self.string_params.sympathetic_coupling));
        out.push_str("  },\n");

        // Body params
        out.push_str("  \"body_params\": {\n");
        out.push_str(&format!("    \"profile_base\": \"{:?}\",\n", self.body_params.profile_base));
        out.push_str(&format!("    \"body_wet_mix\": {:.4},\n", self.body_params.body_wet_mix));
        out.push_str(&format!("    \"pre_drive\": {:.4},\n", self.body_params.pre_drive));
        out.push_str(&format!("    \"post_gain_db\": {:.2},\n", self.body_params.post_gain_db));
        out.push_str(&format!("    \"modal_knock_air\": {:.4},\n", self.body_params.modal_knock_air));
        out.push_str("    \"bands\": [\n");
        for (i, b) in self.body_params.bands.iter().enumerate() {
            let comma = if i + 1 < self.body_params.bands.len() { "," } else { "" };
            out.push_str(&format!(
                "      {{ \"freq_hz\": {:.1}, \"q\": {:.3}, \"gain_db\": {:.2}, \"enabled\": {} }}{}\n",
                b.freq_hz, b.q, b.gain_db, b.enabled, comma
            ));
        }
        out.push_str("    ]\n");
        out.push_str("  },\n");

        // Drum params
        out.push_str("  \"drum_params\": {\n");
        out.push_str("    \"kick\": {\n");
        out.push_str(&format!("      \"pitch_start_hz\": {:.1},\n", self.drum_params.kick.pitch_start_hz));
        out.push_str(&format!("      \"pitch_end_hz\": {:.1},\n", self.drum_params.kick.pitch_end_hz));
        out.push_str(&format!("      \"pitch_decay_ms\": {:.1},\n", self.drum_params.kick.pitch_decay_ms));
        out.push_str(&format!("      \"click_level\": {:.4},\n", self.drum_params.kick.click_level));
        out.push_str(&format!("      \"body_decay_ms\": {:.1},\n", self.drum_params.kick.body_decay_ms));
        out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", self.drum_params.kick.head_tension_mod));
        out.push_str(&format!("      \"strike_position\": {:.4},\n", self.drum_params.kick.strike_position));
        out.push_str(&format!("      \"overtones_ring\": {:.4},\n", self.drum_params.kick.overtones_ring));
        out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", self.drum_params.kick.shell_wood_cavity));
        out.push_str(&format!("      \"hit_variation\": {:.4}\n", self.drum_params.kick.hit_variation));
        out.push_str("    },\n");
        out.push_str("    \"snare\": {\n");
        out.push_str(&format!("      \"tone_freq_hz\": {:.1},\n", self.drum_params.snare.tone_freq_hz));
        out.push_str(&format!("      \"mode2_freq_hz\": {:.1},\n", self.drum_params.snare.mode2_freq_hz));
        out.push_str(&format!("      \"tone_decay_ms\": {:.1},\n", self.drum_params.snare.tone_decay_ms));
        out.push_str(&format!("      \"wire_cutoff_hz\": {:.1},\n", self.drum_params.snare.wire_cutoff_hz));
        out.push_str(&format!("      \"noise_decay_ms\": {:.1},\n", self.drum_params.snare.noise_decay_ms));
        out.push_str(&format!("      \"noise_mix\": {:.4},\n", self.drum_params.snare.noise_mix));
        out.push_str(&format!("      \"clap_spacing_ms\": {:.1},\n", self.drum_params.snare.clap_spacing_ms));
        out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", self.drum_params.snare.head_tension_mod));
        out.push_str(&format!("      \"strike_position\": {:.4},\n", self.drum_params.snare.strike_position));
        out.push_str(&format!("      \"overtones_ring\": {:.4},\n", self.drum_params.snare.overtones_ring));
        out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", self.drum_params.snare.shell_wood_cavity));
        out.push_str(&format!("      \"wire_tension\": {:.4},\n", self.drum_params.snare.wire_tension));
        out.push_str(&format!("      \"hit_variation\": {:.4}\n", self.drum_params.snare.hit_variation));
        out.push_str("    },\n");
        out.push_str("    \"toms\": {\n");
        out.push_str(&format!("      \"pitch_scale\": {:.4},\n", self.drum_params.toms.pitch_scale));
        out.push_str(&format!("      \"pitch_drop_ratio\": {:.4},\n", self.drum_params.toms.pitch_drop_ratio));
        out.push_str(&format!("      \"decay_ms\": {:.1},\n", self.drum_params.toms.decay_ms));
        out.push_str(&format!("      \"stick_attack\": {:.4},\n", self.drum_params.toms.stick_attack));
        out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", self.drum_params.toms.head_tension_mod));
        out.push_str(&format!("      \"strike_position\": {:.4},\n", self.drum_params.toms.strike_position));
        out.push_str(&format!("      \"overtones_ring\": {:.4},\n", self.drum_params.toms.overtones_ring));
        out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", self.drum_params.toms.shell_wood_cavity));
        out.push_str(&format!("      \"hit_variation\": {:.4}\n", self.drum_params.toms.hit_variation));
        out.push_str("    },\n");
        out.push_str("    \"hi_hats\": {\n");
        out.push_str(&format!("      \"spread\": {:.4},\n", self.drum_params.hi_hats.spread));
        out.push_str(&format!("      \"highpass_cutoff_hz\": {:.1},\n", self.drum_params.hi_hats.highpass_cutoff_hz));
        out.push_str(&format!("      \"closed_decay_ms\": {:.1},\n", self.drum_params.hi_hats.closed_decay_ms));
        out.push_str(&format!("      \"pedal_decay_ms\": {:.1},\n", self.drum_params.hi_hats.pedal_decay_ms));
        out.push_str(&format!("      \"open_decay_ms\": {:.1},\n", self.drum_params.hi_hats.open_decay_ms));
        out.push_str(&format!("      \"bronze_size_pitch\": {:.4},\n", self.drum_params.hi_hats.bronze_size_pitch));
        out.push_str(&format!("      \"plate_dispersion\": {:.4},\n", self.drum_params.hi_hats.plate_dispersion));
        out.push_str(&format!("      \"nonlin_shimmer\": {:.4},\n", self.drum_params.hi_hats.nonlin_shimmer));
        out.push_str(&format!("      \"bell_level\": {:.4},\n", self.drum_params.hi_hats.bell_level));
        out.push_str(&format!("      \"stick_hardness\": {:.4},\n", self.drum_params.hi_hats.stick_hardness));
        out.push_str(&format!("      \"hit_variation\": {:.4}\n", self.drum_params.hi_hats.hit_variation));
        out.push_str("    },\n");
        out.push_str("    \"cymbals\": {\n");
        out.push_str(&format!("      \"matrix_spread\": {:.4},\n", self.drum_params.cymbals.matrix_spread));
        out.push_str(&format!("      \"cutoff_hz\": {:.1},\n", self.drum_params.cymbals.cutoff_hz));
        out.push_str(&format!("      \"bell_peak_level\": {:.4},\n", self.drum_params.cymbals.bell_peak_level));
        out.push_str(&format!("      \"crash_decay_ms\": {:.1},\n", self.drum_params.cymbals.crash_decay_ms));
        out.push_str(&format!("      \"ride_decay_ms\": {:.1},\n", self.drum_params.cymbals.ride_decay_ms));
        out.push_str(&format!("      \"bronze_size_pitch\": {:.4},\n", self.drum_params.cymbals.bronze_size_pitch));
        out.push_str(&format!("      \"plate_dispersion\": {:.4},\n", self.drum_params.cymbals.plate_dispersion));
        out.push_str(&format!("      \"nonlin_shimmer\": {:.4},\n", self.drum_params.cymbals.nonlin_shimmer));
        out.push_str(&format!("      \"stick_hardness\": {:.4},\n", self.drum_params.cymbals.stick_hardness));
        out.push_str(&format!("      \"hit_variation\": {:.4}\n", self.drum_params.cymbals.hit_variation));
        out.push_str("    },\n");
        out.push_str("    \"aux\": {\n");
        out.push_str(&format!("      \"freq1_hz\": {:.1},\n", self.drum_params.aux.freq1_hz));
        out.push_str(&format!("      \"freq2_hz\": {:.1},\n", self.drum_params.aux.freq2_hz));
        out.push_str(&format!("      \"detune\": {:.1},\n", self.drum_params.aux.detune));
        out.push_str(&format!("      \"bandpass_q\": {:.4},\n", self.drum_params.aux.bandpass_q));
        out.push_str(&format!("      \"decay_ms\": {:.1}\n", self.drum_params.aux.decay_ms));
        out.push_str("    }\n");
        out.push_str("  },\n");

        // FM params
        out.push_str("  \"fm_params\": {\n");
        out.push_str(&format!("    \"mod_ratio\": {:.4},\n", self.fm_params.mod_ratio));
        out.push_str(&format!("    \"mod_index\": {:.4},\n", self.fm_params.mod_index));
        out.push_str(&format!("    \"feedback\": {:.4},\n", self.fm_params.feedback));
        out.push_str("    \"carrier_adsr\": {\n");
        out.push_str(&format!("      \"attack_ms\": {:.1},\n", self.fm_params.carrier_adsr.attack_ms));
        out.push_str(&format!("      \"decay_ms\": {:.1},\n", self.fm_params.carrier_adsr.decay_ms));
        out.push_str(&format!("      \"sustain\": {:.4},\n", self.fm_params.carrier_adsr.sustain));
        out.push_str(&format!("      \"release_ms\": {:.1}\n", self.fm_params.carrier_adsr.release_ms));
        out.push_str("    },\n");
        out.push_str("    \"mod_adsr\": {\n");
        out.push_str(&format!("      \"attack_ms\": {:.1},\n", self.fm_params.mod_adsr.attack_ms));
        out.push_str(&format!("      \"decay_ms\": {:.1},\n", self.fm_params.mod_adsr.decay_ms));
        out.push_str(&format!("      \"sustain\": {:.4},\n", self.fm_params.mod_adsr.sustain));
        out.push_str(&format!("      \"release_ms\": {:.1}\n", self.fm_params.mod_adsr.release_ms));
        out.push_str("    }\n");
        out.push_str("  }\n");
        out.push_str("}\n");

        Ok(out)
    }





    /// Deserializes a preset from a JSON (or YAML) string.
    pub fn from_json_string(s: &str) -> Result<Self, String> {
        serde_saphyr::from_str::<Self>(s).map_err(|e| format!("Failed to parse preset JSON: {}", e))
    }

    /// Saves this preset to a file on disk.
    pub fn save_to_json_file(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json_string()?;
        std::fs::write(path, json).map_err(|e| format!("Failed to write preset to {:?}: {}", path, e))
    }

    /// Loads a preset from a file on disk.
    pub fn load_from_json_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read preset from {:?}: {}", path, e))?;
        Self::from_json_string(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_presets_and_json_roundtrip() {
        let presets = SynthPreset::factory_presets();
        assert_eq!(presets.len(), 7);

        for p in presets {
            let json = p.to_json_string().expect("JSON serialization failed");
            let deserialized = SynthPreset::from_json_string(&json).expect("JSON deserialization failed");
            assert_eq!(p.name, deserialized.name);
            assert_eq!(p.author, deserialized.author);
            assert_eq!(p.engine, deserialized.engine);
            assert_eq!(p.body_params.profile_base, deserialized.body_params.profile_base);
            assert!((p.string_params.pluck_pos - deserialized.string_params.pluck_pos).abs() < 1e-3);
            assert!((p.string_params.polarization_detune_cents - deserialized.string_params.polarization_detune_cents).abs() < 1e-3);
            assert!((p.string_params.polarization_coupling - deserialized.string_params.polarization_coupling).abs() < 1e-4);
            assert!((p.string_params.tension_mod_amount - deserialized.string_params.tension_mod_amount).abs() < 1e-3);
            assert!((p.string_params.fret_buzz_sizzle - deserialized.string_params.fret_buzz_sizzle).abs() < 1e-3);
            assert!((p.string_params.sympathetic_coupling - deserialized.string_params.sympathetic_coupling).abs() < 1e-4);
            assert!((p.body_params.modal_knock_air - deserialized.body_params.modal_knock_air).abs() < 1e-3);
            assert!((p.drum_params.kick.pitch_start_hz - deserialized.drum_params.kick.pitch_start_hz).abs() < 1e-3);
            assert!((p.drum_params.kick.body_decay_ms - deserialized.drum_params.kick.body_decay_ms).abs() < 1e-3);
            assert!((p.drum_params.kick.head_tension_mod - deserialized.drum_params.kick.head_tension_mod).abs() < 1e-3);
            assert!((p.drum_params.kick.strike_position - deserialized.drum_params.kick.strike_position).abs() < 1e-3);
            assert!((p.drum_params.snare.mode2_freq_hz - deserialized.drum_params.snare.mode2_freq_hz).abs() < 1e-3);
            assert!((p.drum_params.snare.wire_tension - deserialized.drum_params.snare.wire_tension).abs() < 1e-3);
            assert!((p.drum_params.toms.pitch_drop_ratio - deserialized.drum_params.toms.pitch_drop_ratio).abs() < 1e-3);
            assert!((p.drum_params.toms.shell_wood_cavity - deserialized.drum_params.toms.shell_wood_cavity).abs() < 1e-3);
            assert!((p.drum_params.hi_hats.spread - deserialized.drum_params.hi_hats.spread).abs() < 1e-3);
            assert!((p.drum_params.hi_hats.nonlin_shimmer - deserialized.drum_params.hi_hats.nonlin_shimmer).abs() < 1e-3);
            assert!((p.drum_params.cymbals.cutoff_hz - deserialized.drum_params.cymbals.cutoff_hz).abs() < 1e-3);
            assert!((p.drum_params.cymbals.bronze_size_pitch - deserialized.drum_params.cymbals.bronze_size_pitch).abs() < 1e-3);
            assert!((p.drum_params.cymbals.stick_hardness - deserialized.drum_params.cymbals.stick_hardness).abs() < 1e-3);
            assert!((p.drum_params.aux.freq1_hz - deserialized.drum_params.aux.freq1_hz).abs() < 1e-3);
            assert!((p.fm_params.mod_ratio - deserialized.fm_params.mod_ratio).abs() < 1e-3);
        }
    }

    #[test]
    fn test_live_parameter_updates_and_extreme_sweep_stability() {
        use crate::synth::{SynthCommand, SynthEngine};

        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);

        // 1. Trigger a note
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 196.0,
            velocity: 0.8,
            pluck_pos: 0.2,
            palm_mute: 0.0,
            harmonic_node: None,
        }));

        let mut buf = [0.0_f32; 256];
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));

        // 2. Perform live sweep of biquad filter frequencies, Q, and gain
        for freq in [20.0, 100.0, 500.0, 2500.0, 8000.0, 18000.0, 21500.0] {
            for q in [0.1, 0.707, 2.5, 8.0, 15.0] {
                let mut body_p = BodyResonatorParams::default();
                body_p.bands[0].freq_hz = freq;
                body_p.bands[0].q = q;
                body_p.bands[0].gain_db = 12.0;
                body_p.pre_drive = 2.5;

                assert!(engine.send(SynthCommand::UpdateBodyParams(body_p)));
                mixer.process_f32(&mut buf, 2);
                assert!(
                    buf.iter().all(|s| s.is_finite() && !s.is_nan()),
                    "NaN or Inf detected during filter sweep at freq={}, Q={}",
                    freq,
                    q
                );
            }
        }

        // 3. Test string parameter update
        let mut str_p = StringSynthParams::default();
        str_p.t60_decay_sec = 8.5;
        str_p.brightness_s = 0.05;
        assert!(engine.send(SynthCommand::UpdateStringParams(str_p)));
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));

        // 4. Test drum and FM parameter update
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::Kick,
            params: DrumVoiceParams::Kick(KickParams {
                pitch_start_hz: 220.0,
                pitch_end_hz: 35.0,
                pitch_decay_ms: 45.0,
                click_level: 0.9,
                body_decay_ms: 220.0,
                ..KickParams::default()
            }),
        }));
        assert!(engine.send(SynthCommand::UpdateGmVoiceParams(FmVoiceParams::default())));
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn test_pitched_engine_switching_and_all_drum_roles() {
        use crate::synth::{SynthCommand, SynthEngine};

        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);
        let mut buf = [0.0_f32; 256];

        // 1. Pitched engine switching
        assert!(engine.send(SynthCommand::SetPitchedEngine(PitchedEngine::PhysicalString)));
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 110.0,
            velocity: 0.8,
            pluck_pos: 0.2,
            palm_mute: 0.0,
            harmonic_node: None,
        }));
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));

        assert!(engine.send(SynthCommand::SetPitchedEngine(PitchedEngine::FmSynth)));
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 220.0,
            velocity: 0.8,
            pluck_pos: 0.2,
            palm_mute: 0.0,
            harmonic_node: None,
        }));
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));

        // 2. Part engine assignments
        assert!(engine.send(SynthCommand::SetPartEngine {
            part_idx: 0,
            engine: PitchedEngine::PhysicalString,
        }));
        assert!(engine.send(SynthCommand::SetPartEngine {
            part_idx: 1,
            engine: PitchedEngine::FmSynth,
        }));

        // 3. Update all 6 drum voice parameter variants
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::Kick,
            params: DrumVoiceParams::Kick(KickParams::default()),
        }));
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::Snare,
            params: DrumVoiceParams::Snare(SnareParams::default()),
        }));
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::Toms,
            params: DrumVoiceParams::Toms(TomParams::default()),
        }));
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::HiHats,
            params: DrumVoiceParams::HiHats(HiHatParams::default()),
        }));
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::Cymbals,
            params: DrumVoiceParams::Cymbals(CymbalParams::default()),
        }));
        assert!(engine.send(SynthCommand::UpdateDrumParams {
            drum_role: DrumRole::PercAux,
            params: DrumVoiceParams::PercAux(PercAuxParams::default()),
        }));

        // 4. Trigger each GM drum note (35 through 59)
        for note in 35..=59 {
            assert!(engine.send(SynthCommand::TriggerDrum {
                gm_drum_note: note,
                velocity: 0.75,
            }));
            mixer.process_f32(&mut buf, 2);
            assert!(buf.iter().all(|s| s.is_finite()));
        }

        // 5. Scoped part parameter updates
        assert!(engine.send(SynthCommand::UpdatePartStringParams {
            part_idx: 0,
            params: StringSynthParams::default(),
        }));
        assert!(engine.send(SynthCommand::UpdatePartBodyParams {
            part_idx: 0,
            params: BodyResonatorParams::default(),
        }));
        assert!(engine.send(SynthCommand::UpdatePartFmParams {
            part_idx: 1,
            params: FmVoiceParams::default(),
        }));
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn test_per_part_isolation_and_fm_slur_does_not_trigger_waveguide() {
        use crate::synth::{SynthCommand, SynthEngine};

        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);
        let mut buf = [0.0_f32; 256];

        // Configure Part 0 as PhysicalString (ElectricOverdrive) and Part 1 as FmSynth
        assert!(engine.send(SynthCommand::SetPartEngine {
            part_idx: 0,
            engine: PitchedEngine::PhysicalString,
        }));
        assert!(engine.send(SynthCommand::SetPartProfile {
            part_idx: 0,
            profile: InstrumentProfile::ElectricOverdrive,
        }));
        assert!(engine.send(SynthCommand::SetPartEngine {
            part_idx: 1,
            engine: PitchedEngine::FmSynth,
        }));
        assert!(engine.send(SynthCommand::SetPartProfile {
            part_idx: 1,
            profile: InstrumentProfile::AcousticGuitar,
        }));
        mixer.drain_commands();

        // Verify Part 0 and Part 1 have completely independent engine and profile states
        assert_eq!(mixer.parts[0].engine, PitchedEngine::PhysicalString);
        assert_eq!(mixer.parts[0].body.profile(), InstrumentProfile::ElectricOverdrive);
        assert_eq!(mixer.parts[1].engine, PitchedEngine::FmSynth);
        assert_eq!(mixer.parts[1].body.profile(), InstrumentProfile::AcousticGuitar);

        // Select Part 1 (FM Synth) and trigger a note followed by a hammer-on/pull-off slur and a slide
        assert!(engine.send(SynthCommand::SetActivePart(1)));
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 220.0,
            velocity: 0.85,
            pluck_pos: 0.18,
            palm_mute: 0.0,
            harmonic_node: None,
        }));
        mixer.process_f32(&mut buf, 2);
        assert!(
            mixer.parts[1].percussion.has_active_voices(),
            "FM Synth part must have active FM voices after TriggerString"
        );
        assert!(
            mixer.parts[1].strings.iter().all(|s| !s.active),
            "FM Synth part must NOT activate physical string waveguides on TriggerString"
        );

        // Now send SlurString (hammer-on / pull-off) and SlideString on Part 1
        assert!(engine.send(SynthCommand::SlurString {
            string_idx: 1,
            new_freq_hz: 246.94,
            hammer_energy: 0.75,
        }));
        assert!(engine.send(SynthCommand::SlideString {
            string_idx: 1,
            target_freq_hz: 261.63,
            glide_samples: 128,
        }));
        mixer.process_f32(&mut buf, 2);

        // Physical string waveguides on Part 1 MUST remain completely inactive!
        assert!(
            mixer.parts[1].strings.iter().all(|s| !s.active),
            "Hammer-ons/pull-offs (SlurString) and slides on an FM Synth part must NEVER revert to physical string waveguides"
        );
        assert!(
            mixer.parts[1].percussion.has_active_voices(),
            "FM Synth voice must remain active during hammer-on/pull-off slur"
        );
    }

    #[test]
    fn test_legacy_preset_json_deserializes_with_realism_defaults() {
        let legacy_json = r#"{
  "name": "Legacy Acoustic",
  "author": "user",
  "string_params": {
    "pluck_pos": 0.2,
    "pick_hardness": 8000.0,
    "noise_mix": 0.5,
    "t60_decay_sec": 3.5,
    "brightness_s": 0.16,
    "inharmonicity_dispersion": 0.12,
    "palm_mute_damping": 0.22,
    "harmonic_damping_ms": 30.0
  },
  "body_params": {
    "profile_base": "AcousticGuitar",
    "body_wet_mix": 0.35,
    "pre_drive": 1.0,
    "post_gain_db": 0.0,
    "bands": [
      { "freq_hz": 98.0, "q": 6.0, "gain_db": 3.0, "enabled": true },
      { "freq_hz": 205.0, "q": 5.2, "gain_db": 2.0, "enabled": true },
      { "freq_hz": 410.0, "q": 4.5, "gain_db": 1.5, "enabled": true },
      { "freq_hz": 5200.0, "q": 0.85, "gain_db": -4.0, "enabled": true }
    ]
  },
  "drum_params": {
    "kick": { "pitch_start_hz": 160.0, "pitch_end_hz": 48.0, "pitch_decay_ms": 25.0, "click_level": 0.65 },
    "snare": { "tone_freq_hz": 185.0, "noise_decay_ms": 180.0, "noise_mix": 0.55 },
    "toms": { "pitch_scale": 1.0, "pitch_drop_ratio": 1.85, "decay_ms": 240.0, "stick_attack": 0.60 },
    "hi_hats": { "spread": 1.0, "highpass_cutoff_hz": 7000.0, "closed_decay_ms": 45.0, "pedal_decay_ms": 60.0, "open_decay_ms": 650.0 },
    "cymbals": { "matrix_spread": 1.0, "cutoff_hz": 3500.0, "bell_peak_level": 0.50, "crash_decay_ms": 1200.0, "ride_decay_ms": 1300.0 }
  }
}"#;
        let parsed = SynthPreset::from_json_string(legacy_json)
            .expect("Legacy preset without realism fields must deserialize cleanly");
        assert!((parsed.string_params.polarization_detune_cents - 0.7).abs() < 1e-4);
        assert!((parsed.string_params.polarization_coupling - 0.012).abs() < 1e-5);
        assert!((parsed.string_params.tension_mod_amount - 0.30).abs() < 1e-4);
        assert!((parsed.string_params.fret_buzz_sizzle - 0.20).abs() < 1e-4);
        assert!((parsed.string_params.sympathetic_coupling - 0.0025).abs() < 1e-5);
        assert!((parsed.body_params.modal_knock_air - 0.55).abs() < 1e-4);
        assert!((parsed.drum_params.kick.head_tension_mod - 0.55).abs() < 1e-4);
        assert!((parsed.drum_params.kick.strike_position - 0.15).abs() < 1e-4);
        assert!((parsed.drum_params.snare.wire_tension - 0.55).abs() < 1e-4);
        assert!((parsed.drum_params.toms.shell_wood_cavity - 0.58).abs() < 1e-4);
        assert!((parsed.drum_params.hi_hats.bronze_size_pitch - 1.0).abs() < 1e-4);
        assert!((parsed.drum_params.cymbals.nonlin_shimmer - 0.58).abs() < 1e-4);
        assert!((parsed.drum_params.cymbals.stick_hardness - 0.62).abs() < 1e-4);
        assert!((parsed.drum_params.kick.hit_variation - 0.10).abs() < 1e-4);
        assert!((parsed.drum_params.snare.hit_variation - 0.10).abs() < 1e-4);
        assert!((parsed.drum_params.cymbals.hit_variation - 0.10).abs() < 1e-4);
    }
}
