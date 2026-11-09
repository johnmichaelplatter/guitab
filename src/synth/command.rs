//! Lock-free, `Copy` command messages sent from the UI/sequencer thread to the real-time audio thread.
//!
//! All frequencies are specified in exact pre-MIDI `f64` Hz so that arbitrary microtonal EDOs,
//! Just Intonation ratios, and custom fretlet layouts are rendered with zero pitch quantization.

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
    /// Switch the active physical instrument & body resonator profile (`AcousticGuitar`, `ElectricClean`, etc.).
    SetInstrumentProfile(crate::synth::body::InstrumentProfile),
    /// Immediately silence or fast-release all active strings, drums, and GM voices.
    AllNotesOff,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synth_command_is_copy_and_compact() {
        fn assert_copy_send_sync<T: Copy + Clone + Send + Sync>() {}
        assert_copy_send_sync::<SynthCommand>();
        assert!(
            std::mem::size_of::<SynthCommand>() <= 32,
            "SynthCommand size ({}) should remain cache-friendly (<= 32 bytes)",
            std::mem::size_of::<SynthCommand>()
        );
    }
}
