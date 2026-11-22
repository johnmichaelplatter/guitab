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

/// Broad acoustic family used to organize drum & percussion instruments in the Synth Lab navigator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumFamily {
    Shells,
    CymbalsAndHats,
    LatinAndHand,
    MetalsWoodsAndFx,
}

impl DrumFamily {
    pub const fn all() -> &'static [DrumFamily] {
        &[
            DrumFamily::Shells,
            DrumFamily::CymbalsAndHats,
            DrumFamily::LatinAndHand,
            DrumFamily::MetalsWoodsAndFx,
        ]
    }

    pub const fn label(self) -> &'static str {
        match self {
            DrumFamily::Shells => "SHELLS & CLAP",
            DrumFamily::CymbalsAndHats => "HI-HAT & CYMBALS",
            DrumFamily::LatinAndHand => "LATIN & HAND DRUMS",
            DrumFamily::MetalsWoodsAndFx => "METALS, WOODS & FX",
        }
    }
}

/// Target percussion instrument/group role for drum parameter updates and UI editing.
///
/// Covers all 47 standard General MIDI Level 1 percussion notes (`35..=81`) across 28 logical
/// individual instruments and acoustically matched groups.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrumRole {
    // --- Shells & Clap ---
    Kick,
    AcousticKick,
    Snare,
    ElectricSnare,
    HandClap,
    Toms,
    // --- Hi-Hat & Cymbals ---
    HiHats,
    Crash1,
    Crash2,
    Ride1AndBell,
    Ride2,
    Splash,
    China,
    // --- Latin & Hand Drums ---
    Bongos,
    Congas,
    Timbales,
    Cuica,
    // --- Metals, Woods & FX ---
    Cowbell,
    Agogo,
    Triangle,
    Claves,
    WoodBlocks,
    Tambourine,
    Cabasa,
    Maracas,
    Guiro,
    Whistle,
    Vibraslap,
}

impl DrumRole {
    /// All 28 logical drum & percussion roles in canonical step-through order.
    pub const fn all() -> &'static [DrumRole] {
        &[
            DrumRole::Kick,
            DrumRole::AcousticKick,
            DrumRole::Snare,
            DrumRole::ElectricSnare,
            DrumRole::HandClap,
            DrumRole::Toms,
            DrumRole::HiHats,
            DrumRole::Crash1,
            DrumRole::Crash2,
            DrumRole::Ride1AndBell,
            DrumRole::Ride2,
            DrumRole::Splash,
            DrumRole::China,
            DrumRole::Bongos,
            DrumRole::Congas,
            DrumRole::Timbales,
            DrumRole::Cuica,
            DrumRole::Cowbell,
            DrumRole::Agogo,
            DrumRole::Triangle,
            DrumRole::Claves,
            DrumRole::WoodBlocks,
            DrumRole::Tambourine,
            DrumRole::Cabasa,
            DrumRole::Maracas,
            DrumRole::Guiro,
            DrumRole::Whistle,
            DrumRole::Vibraslap,
        ]
    }

    /// Maps any General MIDI percussion note (`35..=81`, plus fallback) to its logical role.
    #[inline(always)]
    pub const fn from_gm_note(note: u8) -> DrumRole {
        match note {
            35 => DrumRole::AcousticKick,
            36 => DrumRole::Kick,
            37 | 38 => DrumRole::Snare,
            39 => DrumRole::HandClap,
            40 => DrumRole::ElectricSnare,
            41 | 43 | 45 | 47 | 48 | 50 => DrumRole::Toms,
            42 | 44 | 46 => DrumRole::HiHats,
            49 => DrumRole::Crash1,
            57 => DrumRole::Crash2,
            51 | 53 => DrumRole::Ride1AndBell,
            59 => DrumRole::Ride2,
            55 => DrumRole::Splash,
            52 => DrumRole::China,
            60 | 61 => DrumRole::Bongos,
            62 | 63 | 64 => DrumRole::Congas,
            65 | 66 => DrumRole::Timbales,
            78 | 79 => DrumRole::Cuica,
            56 => DrumRole::Cowbell,
            67 | 68 => DrumRole::Agogo,
            80 | 81 => DrumRole::Triangle,
            75 => DrumRole::Claves,
            76 | 77 => DrumRole::WoodBlocks,
            54 => DrumRole::Tambourine,
            69 => DrumRole::Cabasa,
            70 => DrumRole::Maracas,
            73 | 74 => DrumRole::Guiro,
            71 | 72 => DrumRole::Whistle,
            58 => DrumRole::Vibraslap,
            _ => DrumRole::Cowbell,
        }
    }

    /// Returns the General MIDI percussion note numbers belonging to this role.
    pub const fn gm_notes(self) -> &'static [u8] {
        match self {
            DrumRole::Kick => &[36],
            DrumRole::AcousticKick => &[35],
            DrumRole::Snare => &[38, 37],
            DrumRole::ElectricSnare => &[40],
            DrumRole::HandClap => &[39],
            DrumRole::Toms => &[41, 43, 45, 47, 48, 50],
            DrumRole::HiHats => &[42, 44, 46],
            DrumRole::Crash1 => &[49],
            DrumRole::Crash2 => &[57],
            DrumRole::Ride1AndBell => &[51, 53],
            DrumRole::Ride2 => &[59],
            DrumRole::Splash => &[55],
            DrumRole::China => &[52],
            DrumRole::Bongos => &[60, 61],
            DrumRole::Congas => &[62, 63, 64],
            DrumRole::Timbales => &[65, 66],
            DrumRole::Cuica => &[78, 79],
            DrumRole::Cowbell => &[56],
            DrumRole::Agogo => &[67, 68],
            DrumRole::Triangle => &[80, 81],
            DrumRole::Claves => &[75],
            DrumRole::WoodBlocks => &[76, 77],
            DrumRole::Tambourine => &[54],
            DrumRole::Cabasa => &[69],
            DrumRole::Maracas => &[70],
            DrumRole::Guiro => &[73, 74],
            DrumRole::Whistle => &[71, 72],
            DrumRole::Vibraslap => &[58],
        }
    }

    /// Primary default audition note when selecting this role.
    pub const fn default_note(self) -> u8 {
        match self {
            DrumRole::Kick => 36,
            DrumRole::AcousticKick => 35,
            DrumRole::Snare => 38,
            DrumRole::ElectricSnare => 40,
            DrumRole::HandClap => 39,
            DrumRole::Toms => 45,
            DrumRole::HiHats => 42,
            DrumRole::Crash1 => 49,
            DrumRole::Crash2 => 57,
            DrumRole::Ride1AndBell => 51,
            DrumRole::Ride2 => 59,
            DrumRole::Splash => 55,
            DrumRole::China => 52,
            DrumRole::Bongos => 60,
            DrumRole::Congas => 63,
            DrumRole::Timbales => 65,
            DrumRole::Cuica => 79,
            DrumRole::Cowbell => 56,
            DrumRole::Agogo => 67,
            DrumRole::Triangle => 81,
            DrumRole::Claves => 75,
            DrumRole::WoodBlocks => 76,
            DrumRole::Tambourine => 54,
            DrumRole::Cabasa => 69,
            DrumRole::Maracas => 70,
            DrumRole::Guiro => 74,
            DrumRole::Whistle => 72,
            DrumRole::Vibraslap => 58,
        }
    }

    /// Human-readable display name for UI headers and navigation rows.
    pub const fn display_name(self) -> &'static str {
        match self {
            DrumRole::Kick => "Bass Drum 1 (Kick)",
            DrumRole::AcousticKick => "Acoustic Bass Drum",
            DrumRole::Snare => "Acoustic Snare & Side Stick",
            DrumRole::ElectricSnare => "Electric / Alt Snare",
            DrumRole::HandClap => "Hand Clap",
            DrumRole::Toms => "Toms",
            DrumRole::HiHats => "Hi-Hat",
            DrumRole::Crash1 => "Crash Cymbal 1",
            DrumRole::Crash2 => "Crash Cymbal 2",
            DrumRole::Ride1AndBell => "Ride 1 & Bell",
            DrumRole::Ride2 => "Ride Cymbal 2",
            DrumRole::Splash => "Splash Cymbal",
            DrumRole::China => "China Cymbal",
            DrumRole::Bongos => "Bongos",
            DrumRole::Congas => "Congas",
            DrumRole::Timbales => "Timbales",
            DrumRole::Cuica => "Cuica",
            DrumRole::Cowbell => "Cowbell",
            DrumRole::Agogo => "Agogo Bells",
            DrumRole::Triangle => "Triangle",
            DrumRole::Claves => "Claves",
            DrumRole::WoodBlocks => "Wood Blocks",
            DrumRole::Tambourine => "Tambourine",
            DrumRole::Cabasa => "Cabasa",
            DrumRole::Maracas => "Maracas",
            DrumRole::Guiro => "Guiro",
            DrumRole::Whistle => "Samba Whistle",
            DrumRole::Vibraslap => "Vibraslap",
        }
    }

    /// Compact short name for the left-rail kit navigator.
    pub const fn short_name(self) -> &'static str {
        match self {
            DrumRole::Kick => "Kick 1",
            DrumRole::AcousticKick => "Acoustic Kick",
            DrumRole::Snare => "Snare & Stick",
            DrumRole::ElectricSnare => "Electric Snare",
            DrumRole::HandClap => "Hand Clap",
            DrumRole::Toms => "Toms",
            DrumRole::HiHats => "Hi-Hat",
            DrumRole::Crash1 => "Crash 1",
            DrumRole::Crash2 => "Crash 2",
            DrumRole::Ride1AndBell => "Ride 1 & Bell",
            DrumRole::Ride2 => "Ride 2",
            DrumRole::Splash => "Splash",
            DrumRole::China => "China",
            DrumRole::Bongos => "Bongos",
            DrumRole::Congas => "Congas",
            DrumRole::Timbales => "Timbales",
            DrumRole::Cuica => "Cuica",
            DrumRole::Cowbell => "Cowbell",
            DrumRole::Agogo => "Agogo Bells",
            DrumRole::Triangle => "Triangle",
            DrumRole::Claves => "Claves",
            DrumRole::WoodBlocks => "Wood Blocks",
            DrumRole::Tambourine => "Tambourine",
            DrumRole::Cabasa => "Cabasa",
            DrumRole::Maracas => "Maracas",
            DrumRole::Guiro => "Guiro",
            DrumRole::Whistle => "Whistle",
            DrumRole::Vibraslap => "Vibraslap",
        }
    }

    pub const fn family(self) -> DrumFamily {
        match self {
            DrumRole::Kick
            | DrumRole::AcousticKick
            | DrumRole::Snare
            | DrumRole::ElectricSnare
            | DrumRole::HandClap
            | DrumRole::Toms => DrumFamily::Shells,

            DrumRole::HiHats
            | DrumRole::Crash1
            | DrumRole::Crash2
            | DrumRole::Ride1AndBell
            | DrumRole::Ride2
            | DrumRole::Splash
            | DrumRole::China => DrumFamily::CymbalsAndHats,

            DrumRole::Bongos | DrumRole::Congas | DrumRole::Timbales | DrumRole::Cuica => {
                DrumFamily::LatinAndHand
            }

            DrumRole::Cowbell
            | DrumRole::Agogo
            | DrumRole::Triangle
            | DrumRole::Claves
            | DrumRole::WoodBlocks
            | DrumRole::Tambourine
            | DrumRole::Cabasa
            | DrumRole::Maracas
            | DrumRole::Guiro
            | DrumRole::Whistle
            | DrumRole::Vibraslap => DrumFamily::MetalsWoodsAndFx,
        }
    }
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
fn default_zero_db() -> f32 {
    0.0
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
    /// Absolute output volume in dB (`-48.0..=+12.0 dB`; `-48.0 dB` = silent mute).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            pitch_start_hz: 93.0,
            pitch_end_hz: 43.5,
            pitch_decay_ms: 55.5,
            click_level: 0.28,
            body_decay_ms: 210.0,
            head_tension_mod: 0.30,
            strike_position: 0.48,
            overtones_ring: 0.89,
            shell_wood_cavity: 0.43,
            hit_variation: 0.14,
            volume_db: 3.00,
        }
    }
}

impl KickParams {
    /// Warmer, slightly deeper acoustic bass drum (GM 35) default voicing.
    pub fn acoustic_kick_default() -> Self {
        Self {
            pitch_start_hz: 150.0,
            pitch_end_hz: 40.5,
            pitch_decay_ms: 58.0,
            click_level: 0.13,
            body_decay_ms: 415.0,
            head_tension_mod: 0.68,
            strike_position: 0.48,
            overtones_ring: 0.00,
            shell_wood_cavity: 0.00,
            hit_variation: 0.10,
            volume_db: 2.50,
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
    /// Absolute Acoustic/Electric Snare (GM 38, 40) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Absolute Side Stick / Rimshot (GM 37) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub rim_volume_db: f32,
    /// Absolute Hand Clap (GM 39) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub clap_volume_db: f32,
}

impl Default for SnareParams {
    fn default() -> Self {
        Self {
            tone_freq_hz: 300.0,
            mode2_freq_hz: 600.0,
            tone_decay_ms: 45.0,
            wire_cutoff_hz: 2850.0,
            noise_decay_ms: 50.0,
            noise_mix: 0.67,
            clap_spacing_ms: 28.3,
            head_tension_mod: 0.63,
            strike_position: 1.00,
            overtones_ring: 0.48,
            shell_wood_cavity: 0.74,
            wire_tension: 0.47,
            hit_variation: 0.038,
            volume_db: 0.00,
            rim_volume_db: 5.50,
            clap_volume_db: 0.00,
        }
    }
}

impl SnareParams {
    /// Crisper, punchier Electric / Alt Snare (GM 40) default voicing.
    pub fn electric_snare_default() -> Self {
        Self {
            tone_freq_hz: 190.0,
            mode2_freq_hz: 450.0,
            tone_decay_ms: 97.0,
            wire_cutoff_hz: 5000.0,
            noise_decay_ms: 110.0,
            noise_mix: 0.29,
            clap_spacing_ms: 12.0,
            head_tension_mod: 0.25,
            strike_position: 1.00,
            overtones_ring: 0.00,
            shell_wood_cavity: 0.00,
            wire_tension: 1.00,
            hit_variation: 0.00,
            volume_db: 0.00,
            rim_volume_db: 0.00,
            clap_volume_db: 0.00,
        }
    }
}

/// Dedicated multi-burst Hand Clap voice parameters (GM 39).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandClapParams {
    /// Absolute output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Multi-burst transient repetition spacing in milliseconds (4.0..=35.0 ms).
    pub clap_spacing_ms: f32,
    /// Hand-clap formant bandpass center frequency in Hz (600.0..=3600.0 Hz).
    pub bandpass_hz: f32,
    /// Formant bandpass Q resonance (0.6..=4.5).
    pub bandpass_q: f32,
    /// Ambient clap noise tail decay in milliseconds (40.0..=500.0 ms).
    pub tail_decay_ms: f32,
    /// Short room cavity reflection mix (0.0..=1.0).
    pub room_mix: f32,
    /// Per-hit velocity & timing humanization (0.0..=1.0).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for HandClapParams {
    fn default() -> Self {
        Self {
            volume_db: 0.00,
            clap_spacing_ms: 35.0,
            bandpass_hz: 1700.0,
            bandpass_q: 2.17,
            tail_decay_ms: 50.0,
            room_mix: 0.00,
            hit_variation: 1.00,
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
    /// Absolute output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Individual per-drum tuning offsets in semitones (`-12.0..=+12.0 st`) for each drum in this group:
    /// - Toms: `[41 Low Floor, 43 Hi Floor, 45 Low, 47 Low-Mid, 48 Hi-Mid, 50 High]`
    /// - Bongos: `[60 High, 61 Low, ...]`
    /// - Congas: `[62 Mute High, 63 Open High, 64 Low, ...]`
    /// - Timbales: `[65 High, 66 Low, ...]`
    #[serde(default)]
    pub tom_tuning_semitones: [f32; 6],
}

impl Default for TomParams {
    fn default() -> Self {
        Self {
            pitch_scale: 0.75,
            pitch_drop_ratio: 1.36,
            decay_ms: 195.0,
            stick_attack: 0.80,
            head_tension_mod: 0.66,
            strike_position: 0.00,
            overtones_ring: 1.00,
            shell_wood_cavity: 0.88,
            hit_variation: 0.65,
            volume_db: 3.00,
            tom_tuning_semitones: [0.0; 6],
        }
    }
}

impl TomParams {
    /// Matched wooden Bongo pair (GM 60 High, 61 Low) default voicing.
    pub fn bongos_default() -> Self {
        Self {
            pitch_scale: 2.00,
            pitch_drop_ratio: 1.05,
            decay_ms: 160.0,
            stick_attack: 1.24,
            head_tension_mod: 0.93,
            strike_position: 0.88,
            overtones_ring: 0.22,
            shell_wood_cavity: 0.62,
            hit_variation: 1.00,
            volume_db: 0.00,
            tom_tuning_semitones: [0.0, 3.80, 0.0, 0.0, 0.0, 0.0],
        }
    }

    /// Matched tall wooden Conga family (GM 62 Mute High, 63 Open High, 64 Low) default voicing.
    pub fn congas_default() -> Self {
        Self {
            pitch_scale: 1.10,
            pitch_drop_ratio: 1.26,
            decay_ms: 250.0,
            stick_attack: 1.00,
            head_tension_mod: 0.42,
            strike_position: 1.00,
            overtones_ring: 0.35,
            shell_wood_cavity: 0.78,
            hit_variation: 1.00,
            volume_db: 0.00,
            tom_tuning_semitones: [12.00, -3.00, -3.10, 0.0, 0.0, 0.0],
        }
    }

    /// Matched shallow metal-shell Timbales (GM 65 High, 66 Low) default voicing.
    pub fn timbales_default() -> Self {
        Self {
            pitch_scale: 1.35,
            pitch_drop_ratio: 1.02,
            decay_ms: 600.0,
            stick_attack: 1.00,
            head_tension_mod: 1.00,
            strike_position: 1.00,
            overtones_ring: 1.00,
            shell_wood_cavity: 1.00,
            hit_variation: 0.35,
            volume_db: 0.00,
            tom_tuning_semitones: [1.70, -2.00, 0.0, 0.0, 0.0, 0.0],
        }
    }

    /// Returns the per-drum tuning slot index (`0..6`) for a GM note inside `Toms`/`Bongos`/`Congas`/`Timbales`.
    #[inline(always)]
    pub const fn tuning_slot_for_note(note: u8) -> Option<usize> {
        match note {
            41 | 60 | 62 | 65 => Some(0),
            43 | 61 | 63 | 66 => Some(1),
            45 | 64 => Some(2),
            47 => Some(3),
            48 => Some(4),
            50 => Some(5),
            _ => None,
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
    /// Absolute Closed Hi-Hat (GM 42) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub closed_volume_db: f32,
    /// Absolute Pedal Hi-Hat (GM 44) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub pedal_volume_db: f32,
    /// Absolute Open Hi-Hat (GM 46) volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub open_volume_db: f32,
}

impl Default for HiHatParams {
    fn default() -> Self {
        Self {
            spread: 1.90,
            highpass_cutoff_hz: 5400.0,
            closed_decay_ms: 27.0,
            pedal_decay_ms: 55.0,
            open_decay_ms: 850.0,
            bronze_size_pitch: 1.01,
            plate_dispersion: 0.40,
            nonlin_shimmer: 0.51,
            bell_level: 0.44,
            stick_hardness: 0.49,
            hit_variation: 0.38,
            closed_volume_db: -3.0,
            pedal_volume_db: -3.0,
            open_volume_db: -4.0,
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

/// Individually shapeable cast B20 bronze cymbal parameters (Crash 1, Crash 2, Splash, China, Ride 2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SingleCymbalParams {
    /// Absolute output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Cymbal ring / shimmer decay duration in milliseconds (150.0..=3800.0 ms).
    pub decay_ms: f32,
    /// Thin bronze plate highpass/brightness cutoff frequency in Hz (1800.0..=8500.0).
    pub cutoff_hz: f32,
    /// Bronze plate diameter/pitch scaling multiplier (0.5 = large/dark, 2.0 = small/bright).
    #[serde(default = "default_bronze_size_pitch")]
    pub bronze_size_pitch: f32,
    /// Frequency spread multiplier on bronze modal spacing (0.5..=2.0).
    pub matrix_spread: f32,
    /// All-pass plate dispersion depth shaping initial stick ping character (0.0..=1.0).
    #[serde(default = "default_cymbal_plate_dispersion")]
    pub plate_dispersion: f32,
    /// Signal-modulated non-linear all-pass shimmer cascade bloom (0.0..=1.0).
    #[serde(default = "default_cymbal_nonlin_shimmer")]
    pub nonlin_shimmer: f32,
    /// Bell / cup harmonic presence peak level (0.0..=1.0).
    pub bell_level: f32,
    /// Drumstick tip hardness controlling excitation lowpass brightness (0.0..=1.0).
    #[serde(default = "default_cymbal_stick_hardness")]
    pub stick_hardness: f32,
    /// Per-hit humanization amount varying velocity & stick strike character (0.0..=1.0).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for SingleCymbalParams {
    fn default() -> Self {
        Self::crash1_default()
    }
}

impl SingleCymbalParams {
    /// Thin flexible 18" Crash Cymbal 1 (GM 49) default voicing.
    pub fn crash1_default() -> Self {
        Self {
            volume_db: 9.0,
            decay_ms: 1800.0,
            cutoff_hz: 3500.0,
            bronze_size_pitch: 1.71,
            matrix_spread: 1.69,
            plate_dispersion: 1.0,
            nonlin_shimmer: 0.89,
            bell_level: 0.0,
            stick_hardness: 1.0,
            hit_variation: 0.018,
        }
    }

    /// Slightly brighter 16"–17" Crash Cymbal 2 (GM 57) default voicing.
    pub fn crash2_default() -> Self {
        Self {
            volume_db: 8.5,
            decay_ms: 2600.0,
            cutoff_hz: 3650.0,
            bronze_size_pitch: 0.60,
            matrix_spread: 0.80,
            plate_dispersion: 0.0,
            nonlin_shimmer: 0.10,
            bell_level: 0.83,
            stick_hardness: 1.0,
            hit_variation: 0.018,
        }
    }

    /// Secondary Ride Cymbal 2 (GM 59) default voicing (slightly darker/washier than Ride 1).
    pub fn ride2_default() -> Self {
        Self {
            volume_db: 6.5,
            decay_ms: 770.0,
            cutoff_hz: 1800.0,
            bronze_size_pitch: 1.03,
            matrix_spread: 2.0,
            plate_dispersion: 1.0,
            nonlin_shimmer: 1.0,
            bell_level: 0.0,
            stick_hardness: 0.0,
            hit_variation: 0.03,
        }
    }

    /// Small, thin, fast-blooming 10" Splash Cymbal (GM 55) default voicing.
    pub fn splash_default() -> Self {
        Self {
            volume_db: 9.0,
            decay_ms: 750.0,
            cutoff_hz: 5450.0,
            bronze_size_pitch: 2.0,
            matrix_spread: 0.50,
            plate_dispersion: 1.0,
            nonlin_shimmer: 1.0,
            bell_level: 0.0,
            stick_hardness: 1.0,
            hit_variation: 0.10,
        }
    }

    /// Trashy, thin, high-shimmer Chinese Cymbal (GM 52) default voicing.
    pub fn china_default() -> Self {
        Self {
            volume_db: 12.0,
            decay_ms: 650.0,
            cutoff_hz: 3000.0,
            bronze_size_pitch: 1.61,
            matrix_spread: 2.0,
            plate_dispersion: 1.0,
            nonlin_shimmer: 0.91,
            bell_level: 0.16,
            stick_hardness: 1.0,
            hit_variation: 0.80,
        }
    }
}

/// Grouped Ride Cymbal 1 & Ride Bell parameters (GM 51 bow + GM 53 bell cup on the same bronze plate).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RideCymbalParams {
    /// Absolute Ride Bow (GM 51) output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub bow_volume_db: f32,
    /// Absolute Ride Bell (GM 53) output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub bell_volume_db: f32,
    /// Ride bow wash decay duration in milliseconds (300.0..=3500.0 ms).
    pub bow_decay_ms: f32,
    /// Ride bell singing cup decay duration in milliseconds (200.0..=3000.0 ms).
    pub bell_decay_ms: f32,
    /// Highpass/brightness cutoff frequency in Hz (1800.0..=8000.0).
    pub cutoff_hz: f32,
    /// Shared bronze plate diameter/pitch scaling multiplier (0.5..=2.0).
    #[serde(default = "default_bronze_size_pitch")]
    pub bronze_size_pitch: f32,
    /// Frequency spread multiplier on bronze modal spacing (0.5..=2.0).
    pub matrix_spread: f32,
    /// All-pass plate dispersion depth (0.0..=1.0).
    #[serde(default = "default_cymbal_plate_dispersion")]
    pub plate_dispersion: f32,
    /// Signal-modulated non-linear shimmer wash bloom (0.0..=1.0).
    #[serde(default = "default_cymbal_nonlin_shimmer")]
    pub nonlin_shimmer: f32,
    /// Ride bell harmonic presence peak level (0.0..=1.0).
    pub bell_peak_level: f32,
    /// Drumstick tip hardness (0.0..=1.0).
    #[serde(default = "default_cymbal_stick_hardness")]
    pub stick_hardness: f32,
    /// Per-hit velocity & stick strike humanization (0.0..=1.0).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for RideCymbalParams {
    fn default() -> Self {
        Self {
            bow_volume_db: 6.5,
            bell_volume_db: 8.0,
            bow_decay_ms: 3500.0,
            bell_decay_ms: 1300.0,
            cutoff_hz: 3450.0,
            bronze_size_pitch: 1.68,
            matrix_spread: 1.60,
            plate_dispersion: 0.01,
            nonlin_shimmer: 0.54,
            bell_peak_level: 1.0,
            stick_hardness: 1.0,
            hit_variation: 0.0,
        }
    }
}

/// Tuned metallic / hardwood modal percussion parameters (Cowbell, Agogo Bells, Triangle, Claves, Wood Blocks).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PercAuxParams {
    /// Fundamental modal frequency 1 in Hz (200.0..=4500.0).
    pub freq1_hz: f32,
    /// Second modal frequency 2 in Hz (300.0..=7000.0).
    pub freq2_hz: f32,
    /// Detune / spread offset in Hz (-80.0..=80.0).
    pub detune: f32,
    /// Resonant filter Q / modal sharpness (0.5..=14.0).
    pub bandpass_q: f32,
    /// Resonant ring decay in milliseconds (15.0..=1800.0 ms).
    pub decay_ms: f32,
    /// Absolute output volume in dB (`-48.0..=+12.0 dB`).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Per-hit velocity & strike humanization (0.0..=1.0).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
}

impl Default for PercAuxParams {
    fn default() -> Self {
        Self::cowbell_default()
    }
}

impl PercAuxParams {
    /// Cowbell (GM 56) default voicing.
    pub fn cowbell_default() -> Self {
        Self {
            freq1_hz: 480.0,
            freq2_hz: 1330.0,
            detune: 9.0,
            bandpass_q: 15.1,
            decay_ms: 315.0,
            volume_db: -8.0,
            hit_variation: 1.0,
        }
    }

    /// Agogo Bells (GM 67 High, 68 Low) default voicing.
    pub fn agogo_default() -> Self {
        Self {
            freq1_hz: 1150.0,
            freq2_hz: 2080.0,
            detune: 17.0,
            bandpass_q: 11.2,
            decay_ms: 375.0,
            volume_db: -7.0,
            hit_variation: 0.10,
        }
    }

    /// Steel Triangle (GM 80 Mute, 81 Open) default voicing.
    pub fn triangle_default() -> Self {
        Self {
            freq1_hz: 2650.0,
            freq2_hz: 4100.0,
            detune: 0.0,
            bandpass_q: 8.5,
            decay_ms: 300.0,
            volume_db: -9.0,
            hit_variation: 1.0,
        }
    }

    /// Rosewood Claves (GM 75) default voicing.
    pub fn claves_default() -> Self {
        Self {
            freq1_hz: 1520.0,
            freq2_hz: 2700.0,
            detune: 7.0,
            bandpass_q: 7.8,
            decay_ms: 34.0,
            volume_db: 6.5,
            hit_variation: 1.0,
        }
    }

    /// Hollow Hardwood Wood Blocks (GM 76 High, 77 Low) default voicing.
    pub fn wood_blocks_default() -> Self {
        Self {
            freq1_hz: 1160.0,
            freq2_hz: 1530.0,
            detune: -16.0,
            bandpass_q: 12.0,
            decay_ms: 88.0,
            volume_db: 0.0,
            hit_variation: 1.0,
        }
    }
}

fn default_shaker_aux_decay_ms() -> f32 {
    50.0
}

fn default_shaker_aux_amount() -> f32 {
    0.35
}

fn default_shaker_aux_volume_db() -> f32 {
    6.50
}

/// Acoustic shaker, ratchet, whistle, and friction-drum parameters
/// (Tambourine, Cabasa, Maracas, Guiro, Samba Whistle, Vibraslap, Cuica).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShakerAndEffectParams {
    /// Absolute output volume in dB (`-48.0..=+18.0 dB`; for Guiro, controls Long Guiro 74 volume).
    #[serde(default = "default_zero_db")]
    pub volume_db: f32,
    /// Pitch / frequency scaling multiplier (0.25..=3.5).
    pub pitch_scale: f32,
    /// Spectral brightness / bandpass center cutoff in Hz.
    pub brightness_hz: f32,
    /// Primary envelope decay / gesture duration in milliseconds
    /// (for Guiro: Long Guiro 74 scrape duration; for Whistle: Long Whistle 72 duration).
    pub decay_ms: f32,
    /// Instrument texture / modulation rate (jingle density, scrape rate, rattle rate, trill depth, pitch sweep).
    pub texture_rate: f32,
    /// Resonator body Q factor.
    pub resonance_q: f32,
    /// Per-hit humanization (0.0..=1.0).
    #[serde(default = "default_drum_hit_variation")]
    pub hit_variation: f32,
    /// Secondary decay duration in milliseconds
    /// (for Tambourine: drumhead decay; for Guiro: Short Guiro 73 scrape length; for Whistle: Short Whistle 71 duration).
    #[serde(default = "default_shaker_aux_decay_ms")]
    pub aux_decay_ms: f32,
    /// Secondary physical modeling control
    /// (for Maracas: random loose beads; for Whistle: velocity/accent pitch bend).
    #[serde(default = "default_shaker_aux_amount")]
    pub aux_amount: f32,
    /// Secondary absolute output volume in dB (for Guiro: Short Guiro 73 volume independent of Long Guiro 74).
    #[serde(default = "default_shaker_aux_volume_db")]
    pub aux_volume_db: f32,
}

impl Default for ShakerAndEffectParams {
    fn default() -> Self {
        Self::tambourine_default()
    }
}

impl ShakerAndEffectParams {
    pub fn tambourine_default() -> Self {
        Self {
            volume_db: -3.0,
            pitch_scale: 1.00,
            brightness_hz: 7200.0,
            decay_ms: 60.0,
            texture_rate: 0.42,
            resonance_q: 0.5,
            hit_variation: 1.0,
            aux_decay_ms: 160.0,
            aux_amount: 0.35,
            aux_volume_db: 6.5,
        }
    }

    pub fn cabasa_default() -> Self {
        Self {
            volume_db: 0.0,
            pitch_scale: 1.47,
            brightness_hz: 6200.0,
            decay_ms: 85.0,
            texture_rate: 0.52,
            resonance_q: 1.5,
            hit_variation: 1.0,
            aux_decay_ms: 50.0,
            aux_amount: 0.35,
            aux_volume_db: 6.5,
        }
    }

    pub fn maracas_default() -> Self {
        Self {
            volume_db: 4.0,
            pitch_scale: 0.40,
            brightness_hz: 10500.0,
            decay_ms: 20.0,
            texture_rate: 0.43,
            resonance_q: 6.0,
            hit_variation: 0.45,
            aux_decay_ms: 50.0,
            aux_amount: 0.78,
            aux_volume_db: 6.5,
        }
    }

    pub fn guiro_default() -> Self {
        Self {
            volume_db: 6.5,
            pitch_scale: 0.75,
            brightness_hz: 4750.0,
            decay_ms: 370.0,
            texture_rate: 0.97,
            resonance_q: 8.7,
            hit_variation: 1.0,
            aux_decay_ms: 20.0,
            aux_amount: 0.35,
            aux_volume_db: 13.0,
        }
    }

    pub fn whistle_default() -> Self {
        Self {
            volume_db: -3.0,
            pitch_scale: 0.60,
            brightness_hz: 3800.0,
            decay_ms: 200.0,
            texture_rate: 1.15,
            resonance_q: 2.6,
            hit_variation: 1.0,
            aux_decay_ms: 50.0,
            aux_amount: 0.87,
            aux_volume_db: 6.5,
        }
    }

    pub fn vibraslap_default() -> Self {
        Self {
            volume_db: 0.0,
            pitch_scale: 0.87,
            brightness_hz: 3350.0,
            decay_ms: 310.0,
            texture_rate: 1.50,
            resonance_q: 4.2,
            hit_variation: 1.0,
            aux_decay_ms: 50.0,
            aux_amount: 0.35,
            aux_volume_db: 6.5,
        }
    }

    pub fn cuica_default() -> Self {
        Self {
            volume_db: 0.0,
            pitch_scale: 0.90,
            brightness_hz: 420.0,
            decay_ms: 75.0,
            texture_rate: 0.75,
            resonance_q: 2.9,
            hit_variation: 1.0,
            aux_decay_ms: 50.0,
            aux_amount: 0.35,
            aux_volume_db: 6.5,
        }
    }
}

fn default_acoustic_kick() -> KickParams {
    KickParams::acoustic_kick_default()
}
fn default_electric_snare() -> SnareParams {
    SnareParams::electric_snare_default()
}
fn default_crash1() -> SingleCymbalParams {
    SingleCymbalParams::crash1_default()
}
fn default_crash2() -> SingleCymbalParams {
    SingleCymbalParams::crash2_default()
}
fn default_ride2() -> SingleCymbalParams {
    SingleCymbalParams::ride2_default()
}
fn default_splash() -> SingleCymbalParams {
    SingleCymbalParams::splash_default()
}
fn default_china() -> SingleCymbalParams {
    SingleCymbalParams::china_default()
}
fn default_bongos() -> TomParams {
    TomParams::bongos_default()
}
fn default_congas() -> TomParams {
    TomParams::congas_default()
}
fn default_timbales() -> TomParams {
    TomParams::timbales_default()
}
fn default_agogo() -> PercAuxParams {
    PercAuxParams::agogo_default()
}
fn default_triangle() -> PercAuxParams {
    PercAuxParams::triangle_default()
}
fn default_claves() -> PercAuxParams {
    PercAuxParams::claves_default()
}
fn default_wood_blocks() -> PercAuxParams {
    PercAuxParams::wood_blocks_default()
}
fn default_tambourine() -> ShakerAndEffectParams {
    ShakerAndEffectParams::tambourine_default()
}
fn default_cabasa() -> ShakerAndEffectParams {
    ShakerAndEffectParams::cabasa_default()
}
fn default_maracas() -> ShakerAndEffectParams {
    ShakerAndEffectParams::maracas_default()
}
fn default_guiro() -> ShakerAndEffectParams {
    ShakerAndEffectParams::guiro_default()
}
fn default_whistle() -> ShakerAndEffectParams {
    ShakerAndEffectParams::whistle_default()
}
fn default_vibraslap() -> ShakerAndEffectParams {
    ShakerAndEffectParams::vibraslap_default()
}
fn default_cuica() -> ShakerAndEffectParams {
    ShakerAndEffectParams::cuica_default()
}

fn is_default_acoustic_kick(v: &KickParams) -> bool {
    *v == default_acoustic_kick()
}
fn is_default_electric_snare(v: &SnareParams) -> bool {
    *v == default_electric_snare()
}
fn is_default_hand_clap(v: &HandClapParams) -> bool {
    *v == HandClapParams::default()
}
fn is_default_crash1(v: &SingleCymbalParams) -> bool {
    *v == default_crash1()
}
fn is_default_crash2(v: &SingleCymbalParams) -> bool {
    *v == default_crash2()
}
fn is_default_ride1(v: &RideCymbalParams) -> bool {
    *v == RideCymbalParams::default()
}
fn is_default_ride2(v: &SingleCymbalParams) -> bool {
    *v == default_ride2()
}
fn is_default_splash(v: &SingleCymbalParams) -> bool {
    *v == default_splash()
}
fn is_default_china(v: &SingleCymbalParams) -> bool {
    *v == default_china()
}
fn is_default_bongos(v: &TomParams) -> bool {
    *v == default_bongos()
}
fn is_default_congas(v: &TomParams) -> bool {
    *v == default_congas()
}
fn is_default_timbales(v: &TomParams) -> bool {
    *v == default_timbales()
}
fn is_default_cuica(v: &ShakerAndEffectParams) -> bool {
    *v == default_cuica()
}
fn is_default_agogo(v: &PercAuxParams) -> bool {
    *v == default_agogo()
}
fn is_default_triangle(v: &PercAuxParams) -> bool {
    *v == default_triangle()
}
fn is_default_claves(v: &PercAuxParams) -> bool {
    *v == default_claves()
}
fn is_default_wood_blocks(v: &PercAuxParams) -> bool {
    *v == default_wood_blocks()
}
fn is_default_tambourine(v: &ShakerAndEffectParams) -> bool {
    *v == default_tambourine()
}
fn is_default_cabasa(v: &ShakerAndEffectParams) -> bool {
    *v == default_cabasa()
}
fn is_default_maracas(v: &ShakerAndEffectParams) -> bool {
    *v == default_maracas()
}
fn is_default_guiro(v: &ShakerAndEffectParams) -> bool {
    *v == default_guiro()
}
fn is_default_whistle(v: &ShakerAndEffectParams) -> bool {
    *v == default_whistle()
}
fn is_default_vibraslap(v: &ShakerAndEffectParams) -> bool {
    *v == default_vibraslap()
}

/// Complete procedural drum & percussion kit parameters covering all 28 logical GM roles (`35..=81`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumKitParams {
    // --- Shells & Clap ---
    #[serde(default)]
    pub kick: KickParams,
    #[serde(default = "default_acoustic_kick", skip_serializing_if = "is_default_acoustic_kick")]
    pub acoustic_kick: KickParams,
    #[serde(default)]
    pub snare: SnareParams,
    #[serde(default = "default_electric_snare", skip_serializing_if = "is_default_electric_snare")]
    pub electric_snare: SnareParams,
    #[serde(default, skip_serializing_if = "is_default_hand_clap")]
    pub hand_clap: HandClapParams,
    #[serde(default)]
    pub toms: TomParams,

    // --- Hi-Hat & Cymbals ---
    #[serde(default)]
    pub hi_hats: HiHatParams,
    #[serde(default = "default_crash1", skip_serializing_if = "is_default_crash1")]
    pub crash1: SingleCymbalParams,
    #[serde(default = "default_crash2", skip_serializing_if = "is_default_crash2")]
    pub crash2: SingleCymbalParams,
    #[serde(default, skip_serializing_if = "is_default_ride1")]
    pub ride1_and_bell: RideCymbalParams,
    #[serde(default = "default_ride2", skip_serializing_if = "is_default_ride2")]
    pub ride2: SingleCymbalParams,
    #[serde(default = "default_splash", skip_serializing_if = "is_default_splash")]
    pub splash: SingleCymbalParams,
    #[serde(default = "default_china", skip_serializing_if = "is_default_china")]
    pub china: SingleCymbalParams,

    // --- Latin & Hand Drums ---
    #[serde(default = "default_bongos", skip_serializing_if = "is_default_bongos")]
    pub bongos: TomParams,
    #[serde(default = "default_congas", skip_serializing_if = "is_default_congas")]
    pub congas: TomParams,
    #[serde(default = "default_timbales", skip_serializing_if = "is_default_timbales")]
    pub timbales: TomParams,
    #[serde(default = "default_cuica", skip_serializing_if = "is_default_cuica")]
    pub cuica: ShakerAndEffectParams,

    // --- Metals, Woods & FX ---
    #[serde(default, alias = "aux")]
    pub cowbell: PercAuxParams,
    #[serde(default = "default_agogo", skip_serializing_if = "is_default_agogo")]
    pub agogo: PercAuxParams,
    #[serde(default = "default_triangle", skip_serializing_if = "is_default_triangle")]
    pub triangle: PercAuxParams,
    #[serde(default = "default_claves", skip_serializing_if = "is_default_claves")]
    pub claves: PercAuxParams,
    #[serde(default = "default_wood_blocks", skip_serializing_if = "is_default_wood_blocks")]
    pub wood_blocks: PercAuxParams,
    #[serde(default = "default_tambourine", skip_serializing_if = "is_default_tambourine")]
    pub tambourine: ShakerAndEffectParams,
    #[serde(default = "default_cabasa", skip_serializing_if = "is_default_cabasa")]
    pub cabasa: ShakerAndEffectParams,
    #[serde(default = "default_maracas", skip_serializing_if = "is_default_maracas")]
    pub maracas: ShakerAndEffectParams,
    #[serde(default = "default_guiro", skip_serializing_if = "is_default_guiro")]
    pub guiro: ShakerAndEffectParams,
    #[serde(default = "default_whistle", skip_serializing_if = "is_default_whistle")]
    pub whistle: ShakerAndEffectParams,
    #[serde(default = "default_vibraslap", skip_serializing_if = "is_default_vibraslap")]
    pub vibraslap: ShakerAndEffectParams,
}

impl Default for DrumKitParams {
    fn default() -> Self {
        Self {
            kick: KickParams::default(),
            acoustic_kick: default_acoustic_kick(),
            snare: SnareParams::default(),
            electric_snare: default_electric_snare(),
            hand_clap: HandClapParams::default(),
            toms: TomParams::default(),
            hi_hats: HiHatParams::default(),
            crash1: default_crash1(),
            crash2: default_crash2(),
            ride1_and_bell: RideCymbalParams::default(),
            ride2: default_ride2(),
            splash: default_splash(),
            china: default_china(),
            bongos: default_bongos(),
            congas: default_congas(),
            timbales: default_timbales(),
            cuica: default_cuica(),
            cowbell: PercAuxParams::cowbell_default(),
            agogo: default_agogo(),
            triangle: default_triangle(),
            claves: default_claves(),
            wood_blocks: default_wood_blocks(),
            tambourine: default_tambourine(),
            cabasa: default_cabasa(),
            maracas: default_maracas(),
            guiro: default_guiro(),
            whistle: default_whistle(),
            vibraslap: default_vibraslap(),
        }
    }
}

impl DrumKitParams {
    /// Extracts the compact [`DrumVoiceParams`] payload for a specific [`DrumRole`].
    pub fn params_for_role(&self, role: DrumRole) -> DrumVoiceParams {
        match role {
            DrumRole::Kick => DrumVoiceParams::Kick(self.kick),
            DrumRole::AcousticKick => DrumVoiceParams::Kick(self.acoustic_kick),
            DrumRole::Snare => DrumVoiceParams::Snare(self.snare),
            DrumRole::ElectricSnare => DrumVoiceParams::Snare(self.electric_snare),
            DrumRole::HandClap => DrumVoiceParams::HandClap(self.hand_clap),
            DrumRole::Toms => DrumVoiceParams::Toms(self.toms),
            DrumRole::HiHats => DrumVoiceParams::HiHats(self.hi_hats),
            DrumRole::Crash1 => DrumVoiceParams::SingleCymbal(self.crash1),
            DrumRole::Crash2 => DrumVoiceParams::SingleCymbal(self.crash2),
            DrumRole::Ride1AndBell => DrumVoiceParams::RideCymbal(self.ride1_and_bell),
            DrumRole::Ride2 => DrumVoiceParams::SingleCymbal(self.ride2),
            DrumRole::Splash => DrumVoiceParams::SingleCymbal(self.splash),
            DrumRole::China => DrumVoiceParams::SingleCymbal(self.china),
            DrumRole::Bongos => DrumVoiceParams::Toms(self.bongos),
            DrumRole::Congas => DrumVoiceParams::Toms(self.congas),
            DrumRole::Timbales => DrumVoiceParams::Toms(self.timbales),
            DrumRole::Cuica => DrumVoiceParams::ShakerEffect(self.cuica),
            DrumRole::Cowbell => DrumVoiceParams::PercAux(self.cowbell),
            DrumRole::Agogo => DrumVoiceParams::PercAux(self.agogo),
            DrumRole::Triangle => DrumVoiceParams::PercAux(self.triangle),
            DrumRole::Claves => DrumVoiceParams::PercAux(self.claves),
            DrumRole::WoodBlocks => DrumVoiceParams::PercAux(self.wood_blocks),
            DrumRole::Tambourine => DrumVoiceParams::ShakerEffect(self.tambourine),
            DrumRole::Cabasa => DrumVoiceParams::ShakerEffect(self.cabasa),
            DrumRole::Maracas => DrumVoiceParams::ShakerEffect(self.maracas),
            DrumRole::Guiro => DrumVoiceParams::ShakerEffect(self.guiro),
            DrumRole::Whistle => DrumVoiceParams::ShakerEffect(self.whistle),
            DrumRole::Vibraslap => DrumVoiceParams::ShakerEffect(self.vibraslap),
        }
    }

    /// Applies a [`DrumVoiceParams`] update to the matching [`DrumRole`] slot.
    pub fn set_params_for_role(&mut self, role: DrumRole, params: DrumVoiceParams) {
        match (role, params) {
            (DrumRole::Kick, DrumVoiceParams::Kick(v)) => self.kick = v,
            (DrumRole::AcousticKick, DrumVoiceParams::Kick(v)) => self.acoustic_kick = v,
            (DrumRole::Snare, DrumVoiceParams::Snare(v)) => self.snare = v,
            (DrumRole::ElectricSnare, DrumVoiceParams::Snare(v)) => self.electric_snare = v,
            (DrumRole::HandClap, DrumVoiceParams::HandClap(v)) => self.hand_clap = v,
            (DrumRole::Toms, DrumVoiceParams::Toms(v)) => self.toms = v,
            (DrumRole::HiHats, DrumVoiceParams::HiHats(v)) => self.hi_hats = v,
            (DrumRole::Crash1, DrumVoiceParams::SingleCymbal(v)) => self.crash1 = v,
            (DrumRole::Crash2, DrumVoiceParams::SingleCymbal(v)) => self.crash2 = v,
            (DrumRole::Ride1AndBell, DrumVoiceParams::RideCymbal(v)) => self.ride1_and_bell = v,
            (DrumRole::Ride2, DrumVoiceParams::SingleCymbal(v)) => self.ride2 = v,
            (DrumRole::Splash, DrumVoiceParams::SingleCymbal(v)) => self.splash = v,
            (DrumRole::China, DrumVoiceParams::SingleCymbal(v)) => self.china = v,
            (DrumRole::Bongos, DrumVoiceParams::Toms(v)) => self.bongos = v,
            (DrumRole::Congas, DrumVoiceParams::Toms(v)) => self.congas = v,
            (DrumRole::Timbales, DrumVoiceParams::Toms(v)) => self.timbales = v,
            (DrumRole::Cuica, DrumVoiceParams::ShakerEffect(v)) => self.cuica = v,
            (DrumRole::Cowbell, DrumVoiceParams::PercAux(v)) => self.cowbell = v,
            (DrumRole::Agogo, DrumVoiceParams::PercAux(v)) => self.agogo = v,
            (DrumRole::Triangle, DrumVoiceParams::PercAux(v)) => self.triangle = v,
            (DrumRole::Claves, DrumVoiceParams::PercAux(v)) => self.claves = v,
            (DrumRole::WoodBlocks, DrumVoiceParams::PercAux(v)) => self.wood_blocks = v,
            (DrumRole::Tambourine, DrumVoiceParams::ShakerEffect(v)) => self.tambourine = v,
            (DrumRole::Cabasa, DrumVoiceParams::ShakerEffect(v)) => self.cabasa = v,
            (DrumRole::Maracas, DrumVoiceParams::ShakerEffect(v)) => self.maracas = v,
            (DrumRole::Guiro, DrumVoiceParams::ShakerEffect(v)) => self.guiro = v,
            (DrumRole::Whistle, DrumVoiceParams::ShakerEffect(v)) => self.whistle = v,
            (DrumRole::Vibraslap, DrumVoiceParams::ShakerEffect(v)) => self.vibraslap = v,
            _ => {}
        }
    }

    /// Resets a single [`DrumRole`] back to its factory acoustic default without touching the rest of the kit.
    pub fn reset_role(&mut self, role: DrumRole) {
        let def = Self::default();
        self.set_params_for_role(role, def.params_for_role(role));
    }

    /// Returns `true` if the specified [`DrumRole`] has been customized away from factory defaults.
    pub fn is_role_customized(&self, role: DrumRole) -> bool {
        let def = Self::default();
        self.params_for_role(role) != def.params_for_role(role)
    }
}

/// Lightweight `Copy` parameter payload (`<= 68` bytes) for lock-free updates to any of the 28 drum roles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DrumVoiceParams {
    Kick(KickParams),
    Snare(SnareParams),
    HandClap(HandClapParams),
    Toms(TomParams),
    HiHats(HiHatParams),
    SingleCymbal(SingleCymbalParams),
    RideCymbal(RideCymbalParams),
    PercAux(PercAuxParams),
    ShakerEffect(ShakerAndEffectParams),
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
            drum_params: DrumKitParams::default(),
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

        // Drum params (all 28 logical GM roles)
        let write_kick = |out: &mut String, key: &str, k: &KickParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"pitch_start_hz\": {:.1},\n", k.pitch_start_hz));
            out.push_str(&format!("      \"pitch_end_hz\": {:.1},\n", k.pitch_end_hz));
            out.push_str(&format!("      \"pitch_decay_ms\": {:.1},\n", k.pitch_decay_ms));
            out.push_str(&format!("      \"click_level\": {:.4},\n", k.click_level));
            out.push_str(&format!("      \"body_decay_ms\": {:.1},\n", k.body_decay_ms));
            out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", k.head_tension_mod));
            out.push_str(&format!("      \"strike_position\": {:.4},\n", k.strike_position));
            out.push_str(&format!("      \"overtones_ring\": {:.4},\n", k.overtones_ring));
            out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", k.shell_wood_cavity));
            out.push_str(&format!("      \"hit_variation\": {:.4},\n", k.hit_variation));
            out.push_str(&format!("      \"volume_db\": {:.2}\n", k.volume_db));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_snare = |out: &mut String, key: &str, s: &SnareParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"tone_freq_hz\": {:.1},\n", s.tone_freq_hz));
            out.push_str(&format!("      \"mode2_freq_hz\": {:.1},\n", s.mode2_freq_hz));
            out.push_str(&format!("      \"tone_decay_ms\": {:.1},\n", s.tone_decay_ms));
            out.push_str(&format!("      \"wire_cutoff_hz\": {:.1},\n", s.wire_cutoff_hz));
            out.push_str(&format!("      \"noise_decay_ms\": {:.1},\n", s.noise_decay_ms));
            out.push_str(&format!("      \"noise_mix\": {:.4},\n", s.noise_mix));
            out.push_str(&format!("      \"clap_spacing_ms\": {:.1},\n", s.clap_spacing_ms));
            out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", s.head_tension_mod));
            out.push_str(&format!("      \"strike_position\": {:.4},\n", s.strike_position));
            out.push_str(&format!("      \"overtones_ring\": {:.4},\n", s.overtones_ring));
            out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", s.shell_wood_cavity));
            out.push_str(&format!("      \"wire_tension\": {:.4},\n", s.wire_tension));
            out.push_str(&format!("      \"hit_variation\": {:.4},\n", s.hit_variation));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", s.volume_db));
            out.push_str(&format!("      \"rim_volume_db\": {:.2},\n", s.rim_volume_db));
            out.push_str(&format!("      \"clap_volume_db\": {:.2}\n", s.clap_volume_db));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_hand_clap = |out: &mut String, key: &str, c: &HandClapParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", c.volume_db));
            out.push_str(&format!("      \"clap_spacing_ms\": {:.1},\n", c.clap_spacing_ms));
            out.push_str(&format!("      \"bandpass_hz\": {:.1},\n", c.bandpass_hz));
            out.push_str(&format!("      \"bandpass_q\": {:.3},\n", c.bandpass_q));
            out.push_str(&format!("      \"tail_decay_ms\": {:.1},\n", c.tail_decay_ms));
            out.push_str(&format!("      \"room_mix\": {:.4},\n", c.room_mix));
            out.push_str(&format!("      \"hit_variation\": {:.4}\n", c.hit_variation));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_tom = |out: &mut String, key: &str, t: &TomParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"pitch_scale\": {:.4},\n", t.pitch_scale));
            out.push_str(&format!("      \"pitch_drop_ratio\": {:.4},\n", t.pitch_drop_ratio));
            out.push_str(&format!("      \"decay_ms\": {:.1},\n", t.decay_ms));
            out.push_str(&format!("      \"stick_attack\": {:.4},\n", t.stick_attack));
            out.push_str(&format!("      \"head_tension_mod\": {:.4},\n", t.head_tension_mod));
            out.push_str(&format!("      \"strike_position\": {:.4},\n", t.strike_position));
            out.push_str(&format!("      \"overtones_ring\": {:.4},\n", t.overtones_ring));
            out.push_str(&format!("      \"shell_wood_cavity\": {:.4},\n", t.shell_wood_cavity));
            out.push_str(&format!("      \"hit_variation\": {:.4},\n", t.hit_variation));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", t.volume_db));
            out.push_str(&format!(
                "      \"tom_tuning_semitones\": [{:.2}, {:.2}, {:.2}, {:.2}, {:.2}, {:.2}]\n",
                t.tom_tuning_semitones[0],
                t.tom_tuning_semitones[1],
                t.tom_tuning_semitones[2],
                t.tom_tuning_semitones[3],
                t.tom_tuning_semitones[4],
                t.tom_tuning_semitones[5]
            ));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_single_cymbal = |out: &mut String, key: &str, c: &SingleCymbalParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", c.volume_db));
            out.push_str(&format!("      \"decay_ms\": {:.1},\n", c.decay_ms));
            out.push_str(&format!("      \"cutoff_hz\": {:.1},\n", c.cutoff_hz));
            out.push_str(&format!("      \"bronze_size_pitch\": {:.4},\n", c.bronze_size_pitch));
            out.push_str(&format!("      \"matrix_spread\": {:.4},\n", c.matrix_spread));
            out.push_str(&format!("      \"plate_dispersion\": {:.4},\n", c.plate_dispersion));
            out.push_str(&format!("      \"nonlin_shimmer\": {:.4},\n", c.nonlin_shimmer));
            out.push_str(&format!("      \"bell_level\": {:.4},\n", c.bell_level));
            out.push_str(&format!("      \"stick_hardness\": {:.4},\n", c.stick_hardness));
            out.push_str(&format!("      \"hit_variation\": {:.4}\n", c.hit_variation));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_ride_cymbal = |out: &mut String, key: &str, r: &RideCymbalParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"bow_volume_db\": {:.2},\n", r.bow_volume_db));
            out.push_str(&format!("      \"bell_volume_db\": {:.2},\n", r.bell_volume_db));
            out.push_str(&format!("      \"bow_decay_ms\": {:.1},\n", r.bow_decay_ms));
            out.push_str(&format!("      \"bell_decay_ms\": {:.1},\n", r.bell_decay_ms));
            out.push_str(&format!("      \"cutoff_hz\": {:.1},\n", r.cutoff_hz));
            out.push_str(&format!("      \"bronze_size_pitch\": {:.4},\n", r.bronze_size_pitch));
            out.push_str(&format!("      \"matrix_spread\": {:.4},\n", r.matrix_spread));
            out.push_str(&format!("      \"plate_dispersion\": {:.4},\n", r.plate_dispersion));
            out.push_str(&format!("      \"nonlin_shimmer\": {:.4},\n", r.nonlin_shimmer));
            out.push_str(&format!("      \"bell_peak_level\": {:.4},\n", r.bell_peak_level));
            out.push_str(&format!("      \"stick_hardness\": {:.4},\n", r.stick_hardness));
            out.push_str(&format!("      \"hit_variation\": {:.4}\n", r.hit_variation));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_perc_aux = |out: &mut String, key: &str, a: &PercAuxParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"freq1_hz\": {:.1},\n", a.freq1_hz));
            out.push_str(&format!("      \"freq2_hz\": {:.1},\n", a.freq2_hz));
            out.push_str(&format!("      \"detune\": {:.1},\n", a.detune));
            out.push_str(&format!("      \"bandpass_q\": {:.4},\n", a.bandpass_q));
            out.push_str(&format!("      \"decay_ms\": {:.1},\n", a.decay_ms));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", a.volume_db));
            out.push_str(&format!("      \"hit_variation\": {:.4}\n", a.hit_variation));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };
        let write_shaker = |out: &mut String, key: &str, s: &ShakerAndEffectParams, trailing_comma: bool| {
            out.push_str(&format!("    \"{}\": {{\n", key));
            out.push_str(&format!("      \"volume_db\": {:.2},\n", s.volume_db));
            out.push_str(&format!("      \"pitch_scale\": {:.4},\n", s.pitch_scale));
            out.push_str(&format!("      \"brightness_hz\": {:.1},\n", s.brightness_hz));
            out.push_str(&format!("      \"decay_ms\": {:.1},\n", s.decay_ms));
            out.push_str(&format!("      \"texture_rate\": {:.4},\n", s.texture_rate));
            out.push_str(&format!("      \"resonance_q\": {:.3},\n", s.resonance_q));
            out.push_str(&format!("      \"hit_variation\": {:.4},\n", s.hit_variation));
            out.push_str(&format!("      \"aux_decay_ms\": {:.1},\n", s.aux_decay_ms));
            out.push_str(&format!("      \"aux_amount\": {:.4},\n", s.aux_amount));
            out.push_str(&format!("      \"aux_volume_db\": {:.2}\n", s.aux_volume_db));
            out.push_str(if trailing_comma { "    },\n" } else { "    }\n" });
        };

        let dp = &self.drum_params;
        out.push_str("  \"drum_params\": {\n");
        write_kick(&mut out, "kick", &dp.kick, true);
        write_kick(&mut out, "acoustic_kick", &dp.acoustic_kick, true);
        write_snare(&mut out, "snare", &dp.snare, true);
        write_snare(&mut out, "electric_snare", &dp.electric_snare, true);
        write_hand_clap(&mut out, "hand_clap", &dp.hand_clap, true);
        write_tom(&mut out, "toms", &dp.toms, true);
        out.push_str("    \"hi_hats\": {\n");
        out.push_str(&format!("      \"spread\": {:.4},\n", dp.hi_hats.spread));
        out.push_str(&format!("      \"highpass_cutoff_hz\": {:.1},\n", dp.hi_hats.highpass_cutoff_hz));
        out.push_str(&format!("      \"closed_decay_ms\": {:.1},\n", dp.hi_hats.closed_decay_ms));
        out.push_str(&format!("      \"pedal_decay_ms\": {:.1},\n", dp.hi_hats.pedal_decay_ms));
        out.push_str(&format!("      \"open_decay_ms\": {:.1},\n", dp.hi_hats.open_decay_ms));
        out.push_str(&format!("      \"bronze_size_pitch\": {:.4},\n", dp.hi_hats.bronze_size_pitch));
        out.push_str(&format!("      \"plate_dispersion\": {:.4},\n", dp.hi_hats.plate_dispersion));
        out.push_str(&format!("      \"nonlin_shimmer\": {:.4},\n", dp.hi_hats.nonlin_shimmer));
        out.push_str(&format!("      \"bell_level\": {:.4},\n", dp.hi_hats.bell_level));
        out.push_str(&format!("      \"stick_hardness\": {:.4},\n", dp.hi_hats.stick_hardness));
        out.push_str(&format!("      \"hit_variation\": {:.4},\n", dp.hi_hats.hit_variation));
        out.push_str(&format!("      \"closed_volume_db\": {:.2},\n", dp.hi_hats.closed_volume_db));
        out.push_str(&format!("      \"pedal_volume_db\": {:.2},\n", dp.hi_hats.pedal_volume_db));
        out.push_str(&format!("      \"open_volume_db\": {:.2}\n", dp.hi_hats.open_volume_db));
        out.push_str("    },\n");
        write_single_cymbal(&mut out, "crash1", &dp.crash1, true);
        write_single_cymbal(&mut out, "crash2", &dp.crash2, true);
        write_ride_cymbal(&mut out, "ride1_and_bell", &dp.ride1_and_bell, true);
        write_single_cymbal(&mut out, "ride2", &dp.ride2, true);
        write_single_cymbal(&mut out, "splash", &dp.splash, true);
        write_single_cymbal(&mut out, "china", &dp.china, true);
        write_tom(&mut out, "bongos", &dp.bongos, true);
        write_tom(&mut out, "congas", &dp.congas, true);
        write_tom(&mut out, "timbales", &dp.timbales, true);
        write_shaker(&mut out, "cuica", &dp.cuica, true);
        write_perc_aux(&mut out, "cowbell", &dp.cowbell, true);
        write_perc_aux(&mut out, "agogo", &dp.agogo, true);
        write_perc_aux(&mut out, "triangle", &dp.triangle, true);
        write_perc_aux(&mut out, "claves", &dp.claves, true);
        write_perc_aux(&mut out, "wood_blocks", &dp.wood_blocks, true);
        write_shaker(&mut out, "tambourine", &dp.tambourine, true);
        write_shaker(&mut out, "cabasa", &dp.cabasa, true);
        write_shaker(&mut out, "maracas", &dp.maracas, true);
        write_shaker(&mut out, "guiro", &dp.guiro, true);
        write_shaker(&mut out, "whistle", &dp.whistle, true);
        write_shaker(&mut out, "vibraslap", &dp.vibraslap, false);
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
            assert!((p.drum_params.crash1.cutoff_hz - deserialized.drum_params.crash1.cutoff_hz).abs() < 1e-3);
            assert!((p.drum_params.crash1.bronze_size_pitch - deserialized.drum_params.crash1.bronze_size_pitch).abs() < 1e-3);
            assert!((p.drum_params.crash1.stick_hardness - deserialized.drum_params.crash1.stick_hardness).abs() < 1e-3);
            assert!((p.drum_params.cowbell.freq1_hz - deserialized.drum_params.cowbell.freq1_hz).abs() < 1e-3);
            assert!((p.drum_params.kick.volume_db - deserialized.drum_params.kick.volume_db).abs() < 1e-2);
            assert!((p.drum_params.acoustic_kick.volume_db - deserialized.drum_params.acoustic_kick.volume_db).abs() < 1e-2);
            assert!((p.drum_params.snare.volume_db - deserialized.drum_params.snare.volume_db).abs() < 1e-2);
            assert!((p.drum_params.snare.rim_volume_db - deserialized.drum_params.snare.rim_volume_db).abs() < 1e-2);
            assert!((p.drum_params.electric_snare.volume_db - deserialized.drum_params.electric_snare.volume_db).abs() < 1e-2);
            assert!((p.drum_params.hand_clap.volume_db - deserialized.drum_params.hand_clap.volume_db).abs() < 1e-2);
            assert!((p.drum_params.toms.volume_db - deserialized.drum_params.toms.volume_db).abs() < 1e-2);
            assert!((p.drum_params.hi_hats.closed_volume_db - deserialized.drum_params.hi_hats.closed_volume_db).abs() < 1e-2);
            assert!((p.drum_params.hi_hats.pedal_volume_db - deserialized.drum_params.hi_hats.pedal_volume_db).abs() < 1e-2);
            assert!((p.drum_params.hi_hats.open_volume_db - deserialized.drum_params.hi_hats.open_volume_db).abs() < 1e-2);
            assert!((p.drum_params.crash1.volume_db - deserialized.drum_params.crash1.volume_db).abs() < 1e-2);
            assert!((p.drum_params.ride1_and_bell.bow_volume_db - deserialized.drum_params.ride1_and_bell.bow_volume_db).abs() < 1e-2);
            assert!((p.drum_params.ride1_and_bell.bell_volume_db - deserialized.drum_params.ride1_and_bell.bell_volume_db).abs() < 1e-2);
            assert!((p.drum_params.cowbell.volume_db - deserialized.drum_params.cowbell.volume_db).abs() < 1e-2);
            assert!((p.fm_params.mod_ratio - deserialized.fm_params.mod_ratio).abs() < 1e-3);
        }

        // Also verify custom per-instrument drum volumes & tom tuning roundtrip cleanly
        let mut custom = SynthPreset::linn_808_kit();
        custom.drum_params.kick.volume_db = 3.5;
        custom.drum_params.acoustic_kick.volume_db = -1.5;
        custom.drum_params.snare.volume_db = -2.0;
        custom.drum_params.snare.rim_volume_db = -6.5;
        custom.drum_params.electric_snare.volume_db = 1.0;
        custom.drum_params.hand_clap.volume_db = -48.0;
        custom.drum_params.toms.volume_db = 1.5;
        custom.drum_params.toms.tom_tuning_semitones = [-3.0, -1.5, 0.0, 2.0, 4.5, 6.0];
        custom.drum_params.hi_hats.closed_volume_db = -3.0;
        custom.drum_params.hi_hats.pedal_volume_db = -4.0;
        custom.drum_params.hi_hats.open_volume_db = -5.0;
        custom.drum_params.crash1.volume_db = -1.5;
        custom.drum_params.crash2.decay_ms = 1850.0;
        custom.drum_params.ride1_and_bell.bow_volume_db = 2.0;
        custom.drum_params.ride1_and_bell.bell_volume_db = -3.5;
        custom.drum_params.congas.shell_wood_cavity = 0.85;
        custom.drum_params.cowbell.volume_db = -8.0;
        let json = custom.to_json_string().expect("custom JSON serialization failed");
        let parsed = SynthPreset::from_json_string(&json).expect("custom JSON deserialization failed");
        assert!((parsed.drum_params.kick.volume_db - 3.5).abs() < 1e-2);
        assert!((parsed.drum_params.acoustic_kick.volume_db + 1.5).abs() < 1e-2);
        assert!((parsed.drum_params.snare.rim_volume_db + 6.5).abs() < 1e-2);
        assert!((parsed.drum_params.hand_clap.volume_db + 48.0).abs() < 1e-2);
        assert!((parsed.drum_params.toms.tom_tuning_semitones[4] - 4.5).abs() < 1e-2);
        assert!((parsed.drum_params.hi_hats.open_volume_db + 5.0).abs() < 1e-2);
        assert!((parsed.drum_params.crash2.decay_ms - 1850.0).abs() < 1e-1);
        assert!((parsed.drum_params.ride1_and_bell.bell_volume_db + 3.5).abs() < 1e-2);
        assert!((parsed.drum_params.congas.shell_wood_cavity - 0.85).abs() < 1e-3);
        assert!((parsed.drum_params.cowbell.volume_db + 8.0).abs() < 1e-2);
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

        // 3. Update all 28 drum voice parameter roles
        let dp = DrumKitParams::default();
        for &role in DrumRole::all() {
            assert!(engine.send(SynthCommand::UpdateDrumParams {
                drum_role: role,
                params: dp.params_for_role(role),
            }));
        }

        // 4. Trigger every standard GM percussion note (35 through 81)
        for note in 35..=81 {
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
        assert_eq!(parsed.drum_params.toms.tom_tuning_semitones, [0.0; 6]);
        assert!((parsed.drum_params.hi_hats.bronze_size_pitch - 1.0).abs() < 1e-4);
        assert!((parsed.drum_params.crash1.nonlin_shimmer - 0.89).abs() < 1e-4);
        assert!((parsed.drum_params.crash1.stick_hardness - 1.0).abs() < 1e-4);
        assert!((parsed.drum_params.kick.hit_variation - 0.10).abs() < 1e-4);
        assert!((parsed.drum_params.snare.hit_variation - 0.10).abs() < 1e-4);
        assert!((parsed.drum_params.crash1.hit_variation - 0.018).abs() < 1e-4);
        assert!((parsed.drum_params.kick.volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.acoustic_kick.volume_db - 2.5).abs() < 1e-4);
        assert!((parsed.drum_params.snare.volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.snare.rim_volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.electric_snare.volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.hand_clap.volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.toms.volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.hi_hats.closed_volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.hi_hats.pedal_volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.hi_hats.open_volume_db - 0.0).abs() < 1e-4);
        assert!((parsed.drum_params.crash1.volume_db - 9.0).abs() < 1e-4);
        assert!((parsed.drum_params.ride1_and_bell.bow_volume_db - 6.5).abs() < 1e-4);
        assert!((parsed.drum_params.ride1_and_bell.bell_volume_db - 8.0).abs() < 1e-4);
        assert!((parsed.drum_params.cowbell.volume_db - (-8.0)).abs() < 1e-4);
    }

    #[test]
    fn test_default_percussion_all_json_matches_drum_kit_params_default() {
        let json_str = include_str!("../../context/default_percussion_all3.json");
        let parsed = SynthPreset::from_json_string(json_str)
            .expect("context/default_percussion_all3.json must deserialize cleanly");
        assert_eq!(parsed.drum_params, DrumKitParams::default());
    }
}
