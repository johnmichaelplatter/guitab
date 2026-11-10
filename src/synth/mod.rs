//! Internal low-latency, microtonal physical-modeling and percussion synthesizer.
//!
//! ### Architecture
//! - [`SynthEngine`]: Public handle owned by the UI/sequencer thread. Sends [`SynthCommand`]
//!   messages over a bounded, lock-free SPSC ringbuffer (`rtrb`) to the `cpal` audio thread.
//! - [`backend`]: Real-time `cpal` stream initialization, format conversion (`f32`, `i16`, `u16`),
//!   and lock-free command dispatch.
//! - [`command`]: Pre-MIDI `Copy` command enum using exact `f64` Hz frequencies.

pub mod backend;
pub mod body;
pub mod command;
pub mod patch;
pub mod percussion;
pub mod waveguide;

pub use backend::{flush_denormal, SynthMixer};
pub use body::{BiquadFilter, BodyResonator, InstrumentProfile};
pub use command::SynthCommand;
pub use patch::*;
pub use percussion::{GmFmPreset, PercussionAndGmPool, PolyVoice, MAX_POLY_VOICES};
pub use waveguide::{StringWaveguide, MAX_STRINGS};

use std::sync::{Arc, Mutex};

/// Capacity of the lock-free SPSC command ringbuffer between UI/sequencer and audio threads.
pub const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Cloneable producer handle allowing the UI thread, sequencer playback thread, and grid live-play
/// input thread to enqueue [`SynthCommand`] messages into the `rtrb` ringbuffer.
///
/// **Note on Real-Time Invariants:** The `Mutex` here only synchronizes producer threads (UI / sequencer);
/// the `cpal` audio callback thread exclusively owns the `rtrb::Consumer<SynthCommand>` and never
/// touches this mutex.
#[derive(Clone)]
pub struct SynthSender {
    producer: Arc<Mutex<rtrb::Producer<SynthCommand>>>,
    sample_rate: f32,
}

impl SynthSender {
    /// Pushes a [`SynthCommand`] into the SPSC ringbuffer toward the audio thread.
    #[inline]
    pub fn send(&self, cmd: SynthCommand) -> bool {
        if let Ok(mut guard) = self.producer.lock() {
            guard.push(cmd).is_ok()
        } else {
            false
        }
    }

    /// Enqueues [`SynthCommand::AllNotesOff`] to immediately silence all voices.
    #[inline]
    pub fn all_notes_off(&self) -> bool {
        self.send(SynthCommand::AllNotesOff)
    }

    #[inline]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
}

/// Tab category for the Sound Design & Patch Editor inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynthEditorTab {
    #[default]
    StringAndPluck,
    BodyAndAmp,
    DrumsAndCymbals,
    GmFmSynth,
}

/// UI state for the interactive "Synth Lab" human-in-the-loop audio verification and sound design panel.
#[derive(Debug, Clone)]
pub struct SynthTestLabState {
    pub open: bool,
    pub route_score_preview: bool,
    pub profile: InstrumentProfile,
    pub string_idx: u8,
    pub freq_hz: f64,
    pub velocity: f32,
    pub pluck_pos: f32,
    pub palm_mute: f32,
    pub harmonic_node: Option<u8>,
    pub glide_ms: f32,
    pub gm_program: u8,
    pub status_msg: Option<String>,

    // Sound Design & Patch Editor state
    pub active_tab: SynthEditorTab,
    pub preset: SynthPreset,
    pub user_presets: Vec<SynthPreset>,
    pub selected_preset_idx: usize,
    pub master_gain_db: f32,
    pub save_dialog_open: bool,
    pub save_preset_name: String,
    pub preview_peak_level: f32,
}

impl Default for SynthTestLabState {
    fn default() -> Self {
        let preset = SynthPreset::default();
        Self {
            open: false,
            route_score_preview: true,
            profile: InstrumentProfile::AcousticGuitar,
            string_idx: 1,
            freq_hz: 220.0,
            velocity: 0.85,
            pluck_pos: 0.18,
            palm_mute: 0.0,
            harmonic_node: None,
            glide_ms: 220.0,
            gm_program: 0,
            status_msg: None,

            active_tab: SynthEditorTab::StringAndPluck,
            preset,
            user_presets: Vec::new(),
            selected_preset_idx: 0,
            master_gain_db: 0.0,
            save_dialog_open: false,
            save_preset_name: "My Custom Preset".to_string(),
            preview_peak_level: 0.0,
        }
    }
}

/// Public handle to the real-time synthesizer engine owned by the UI/playback thread.
pub struct SynthEngine {
    sender: SynthSender,
    /// Kept alive for the lifetime of the engine (`None` in headless/offline test mode).
    _stream: Option<cpal::Stream>,
    sample_rate: f32,
    channels: u16,
}

impl SynthEngine {
    /// Initializes the real-time `cpal` audio output stream and pre-allocates the SPSC command queue.
    pub fn new() -> Result<Self, String> {
        let (producer, consumer) = rtrb::RingBuffer::new(COMMAND_QUEUE_CAPACITY);
        let active = backend::spawn_cpal_stream(consumer)?;
        let sender = SynthSender {
            producer: Arc::new(Mutex::new(producer)),
            sample_rate: active.sample_rate,
        };
        Ok(Self {
            sender,
            _stream: Some(active.stream),
            sample_rate: active.sample_rate,
            channels: active.channels,
        })
    }

    /// Creates a headless engine and its paired real-time [`SynthMixer`] for deterministic
    /// offline rendering and unit testing without requiring hardware audio devices.
    pub fn new_headless(sample_rate: f32, channels: u16) -> (Self, SynthMixer) {
        let (producer, consumer) = rtrb::RingBuffer::new(COMMAND_QUEUE_CAPACITY);
        let mixer = SynthMixer::new(sample_rate, consumer);
        let sender = SynthSender {
            producer: Arc::new(Mutex::new(producer)),
            sample_rate,
        };
        let engine = Self {
            sender,
            _stream: None,
            sample_rate,
            channels: channels.max(1),
        };
        (engine, mixer)
    }

    /// Returns a cloneable [`SynthSender`] for use by background sequencer or controller threads.
    #[inline]
    pub fn sender(&self) -> SynthSender {
        self.sender.clone()
    }

    /// Pushes a [`SynthCommand`] to the lock-free SPSC ringbuffer without blocking the audio thread.
    ///
    /// Returns `true` if the command was enqueued, or `false` if the ringbuffer was full.
    #[inline]
    pub fn send(&mut self, cmd: SynthCommand) -> bool {
        self.sender.send(cmd)
    }

    /// Enqueues [`SynthCommand::AllNotesOff`] to immediately silence all voices.
    #[inline]
    pub fn all_notes_off(&mut self) -> bool {
        self.send(SynthCommand::AllNotesOff)
    }

    /// Active hardware (or offline) sample rate in Hz.
    #[inline]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Active hardware (or offline) output channel count.
    #[inline]
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase1_headless_lock_free_dispatch_and_audio_output() {
        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);

        // 1. Trigger a microtonal string note (e.g. 41-EDO pitch at 442.5 Hz)
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 442.5,
            velocity: 0.9,
            pluck_pos: 0.25,
            palm_mute: 0.0,
            harmonic_node: None,
        }));

        // 2. Render a 256-frame stereo f32 buffer and verify non-zero, finite audio output
        let mut buf_f32 = [0.0_f32; 512];
        mixer.process_f32(&mut buf_f32, 2);

        let energy: f32 = buf_f32.iter().map(|s| s * s).sum();
        assert!(energy > 1.0e-3, "Expected audible signal energy, got {}", energy);
        assert!(buf_f32.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        // 3. Test SlideString and i16 / u16 sample format conversion
        assert!(engine.send(SynthCommand::SlideString {
            string_idx: 1,
            target_freq_hz: 465.0,
            glide_samples: 128,
        }));
        let mut buf_i16 = [0_i16; 256];
        mixer.process_i16(&mut buf_i16, 2);
        assert!(buf_i16.iter().any(|&s| s != 0));

        let mut buf_u16 = [32768_u16; 256];
        mixer.process_u16(&mut buf_u16, 2);
        assert!(buf_u16.iter().any(|&s| s != 32768));

        // 4. Test AllNotesOff silences output immediately
        assert!(engine.all_notes_off());
        let mut silent_buf = [1.0_f32; 128];
        mixer.process_f32(&mut silent_buf, 2);
        assert!(
            silent_buf.iter().all(|&s| s == 0.0),
            "AllNotesOff must immediately zero all active voices"
        );
    }

    #[test]
    fn test_denormal_flush_protection() {
        assert_eq!(flush_denormal(1.0e-30), 0.0);
        assert_eq!(flush_denormal(-1.0e-25), 0.0);
        assert_eq!(flush_denormal(f32::NAN), 0.0);
        assert_eq!(flush_denormal(f32::INFINITY), 0.0);
        assert_eq!(flush_denormal(0.5), 0.5);
    }
}
