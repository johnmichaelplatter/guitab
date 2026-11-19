//! Lock-free, `Copy` command messages sent from the UI/sequencer thread to the real-time audio thread.
//!
//! All frequencies are specified in exact pre-MIDI `f64` Hz so that arbitrary microtonal EDOs,
//! Just Intonation ratios, and custom fretlet layouts are rendered with zero pitch quantization.

use crate::synth::patch::{
    BodyResonatorParams, DrumRole, DrumVoiceParams, FmVoiceParams, PitchedEngine, StringSynthParams,
};

/// Commands sent over the lock-free SPSC ringbuffer (`rtrb`) to the `cpal` audio callback.
///
/// Must remain `Copy`, `Clone`, and free of any heap-allocated types (`String`, `Vec`, `Box`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SynthCommand {
    /// Pluck or strike a fretted/open string waveguide.
    TriggerString {
        /// 1-based or 0-based string index (mapped modulo `MAX_STRINGS` on the audio thread).
        string_idx: u8,
        /// Exact fundamental frequency in Hz (`f64`).
        freq_hz: f64,
        /// Normalized excitation velocity in `[0.0, 1.0]`.
        velocity: f32,
        /// Normalized pluck position along the string in `(0.0, 0.5]` (bridge = `0.0`, neck/middle = `0.5`).
        pluck_pos: f32,
        /// Normalized palm-mute damping factor in `[0.0, 1.0]` (`0.0` = open ring, `1.0` = heavy palm mute).
        palm_mute: f32,
        /// Optional natural/artificial harmonic node divider (e.g., `Some(2)` for octave harmonic at 12th fret).
        harmonic_node: Option<u8>,
    },
    /// Smoothly glide a ringing string's delay line to `target_freq_hz` over `glide_samples`.
    SlideString {
        string_idx: u8,
        target_freq_hz: f64,
        glide_samples: u32,
    },
    /// Hammer-on / pull-off excitation on an already-vibrating string without full pluck reset.
    SlurString {
        string_idx: u8,
        new_freq_hz: f64,
        hammer_energy: f32,
    },
    /// Dampen a specific string with the specified exponential release rate.
    MuteString {
        string_idx: u8,
        release_rate: f32,
    },
    /// Trigger a procedural drum/percussion voice using General MIDI percussion note numbers (35..=81+).
    TriggerDrum {
        gm_drum_note: u8,
        velocity: f32,
    },
    /// Trigger an accompaniment General MIDI voice at an exact microtonal `freq_hz`.
    TriggerGmVoice {
        channel: u8,
        freq_hz: f64,
        velocity: f32,
        program: u8,
    },
    /// Select the active part index (`0..MAX_SYNTH_PARTS`) for subsequent note/articulation commands.
    SetActivePart(u8),
    /// Switch the active physical instrument & body resonator profile (`AcousticGuitar`, `ElectricClean`, etc.).
    SetInstrumentProfile(crate::synth::body::InstrumentProfile),
    /// Switch the physical instrument & body resonator profile for a specific part/track.
    SetPartProfile {
        part_idx: u8,
        profile: crate::synth::body::InstrumentProfile,
    },
    /// Switch the active pitched synthesis engine (`PhysicalString` vs `FmSynth`).
    SetPitchedEngine(PitchedEngine),
    /// Assign the active pitched synthesis engine for a specific part/track.
    SetPartEngine {
        part_idx: u8,
        engine: PitchedEngine,
    },
    /// Immediately silence or fast-release all active strings, drums, and GM voices.
    AllNotesOff,

    /// Live real-time parameter updates for Extended Karplus-Strong string waveguides.
    UpdateStringParams(StringSynthParams),
    /// Live real-time parameter updates for body resonator, drive, and 4-band EQ/cabinet filters.
    UpdateBodyParams(BodyResonatorParams),
    /// Live real-time parameter updates for procedural drum kit voices (Kick, Snare, Toms, Hi-Hats, Cymbals, Aux).
    UpdateDrumParams {
        drum_role: DrumRole,
        params: DrumVoiceParams,
    },
    /// Live real-time parameter updates for 2-operator FM melodic backing voices.
    UpdateGmVoiceParams(FmVoiceParams),

    /// Scoped real-time parameter updates for Extended Karplus-Strong string waveguides on a specific part.
    UpdatePartStringParams {
        part_idx: u8,
        params: StringSynthParams,
    },
    /// Scoped real-time parameter updates for body resonator on a specific part.
    UpdatePartBodyParams {
        part_idx: u8,
        params: BodyResonatorParams,
    },
    /// Scoped real-time parameter updates for FM melodic voice on a specific part.
    UpdatePartFmParams {
        part_idx: u8,
        params: FmVoiceParams,
    },
    /// Scoped real-time parameter updates for procedural drum kit voices on a specific part.
    UpdatePartDrumParams {
        part_idx: u8,
        drum_role: DrumRole,
        params: DrumVoiceParams,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synth_command_is_copy_and_compact() {
        fn assert_copy_send_sync<T: Copy + Clone + Send + Sync>() {}
        assert_copy_send_sync::<SynthCommand>();
        assert!(
            std::mem::size_of::<SynthCommand>() <= 96,
            "SynthCommand size ({}) should remain cache-friendly (<= 96 bytes)",
            std::mem::size_of::<SynthCommand>()
        );
    }
}
