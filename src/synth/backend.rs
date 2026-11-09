//! Real-time `cpal` audio stream setup, sample format conversion (`f32`, `i16`, `u16`),
//! mono/stereo channel distribution, and lock-free SPSC command consumer loop.
//!
//! ### Strict Real-Time Invariants
//! - Zero heap allocations, zero locks, zero syscalls, and zero `unwrap()`/`panic!()` inside the
//!   audio callback.
//! - All voice state is pre-allocated at stream initialization.
//! - Denormal numbers are flushed to zero in all exponential decay and feedback paths.

use crate::synth::body::{BodyResonator, InstrumentProfile};
use crate::synth::command::SynthCommand;
use crate::synth::waveguide::{StringWaveguide, MAX_STRINGS};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::f32::consts::TAU;

/// Maximum number of pre-allocated voices in the Phase 1 verification mixer.
pub const MAX_TEST_VOICES: usize = 16;

/// Target low-latency hardware buffer size in frames when supported by the host device.
const TARGET_BUFFER_FRAMES: u32 = 256;

/// Flushes IEEE-754 denormal (subnormal) floats and non-finite values to `0.0`
/// to prevent severe CPU spikes on x86 processors during exponential tail decay.
#[inline(always)]
pub fn flush_denormal(x: f32) -> f32 {
    if x.abs() < 1.0e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

/// A lightweight, zero-allocation test sine + impulse voice used in Phase 1 to verify
/// lock-free command dispatch, microtonal `f64` frequency control, glides, and audio output.
#[derive(Clone, Copy, Debug)]
pub struct TestSineVoice {
    pub active: bool,
    pub string_idx: u8,
    pub phase: f32,
    pub phase_inc: f32,
    pub target_phase_inc: f32,
    pub glide_step: f32,
    pub glide_remaining: u32,
    pub env: f32,
    pub decay_coeff: f32,
    pub impulse_env: f32,
    pub pan: f32,
}

impl Default for TestSineVoice {
    fn default() -> Self {
        Self {
            active: false,
            string_idx: 0,
            phase: 0.0,
            phase_inc: 0.0,
            target_phase_inc: 0.0,
            glide_step: 0.0,
            glide_remaining: 0,
            env: 0.0,
            decay_coeff: 0.9995,
            impulse_env: 0.0,
            pan: 0.5,
        }
    }
}

impl TestSineVoice {
    #[inline(always)]
    pub fn trigger(
        &mut self,
        string_idx: u8,
        freq_hz: f64,
        velocity: f32,
        palm_mute: f32,
        harmonic_node: Option<u8>,
        sample_rate: f32,
    ) {
        let inv_sr = if sample_rate > 1.0 {
            1.0 / sample_rate
        } else {
            1.0 / 48_000.0
        };
        let node_mult = match harmonic_node {
            Some(n) if n > 1 => n as f64,
            _ => 1.0,
        };
        let effective_freq = (freq_hz * node_mult).clamp(15.0, (sample_rate as f64) * 0.45) as f32;
        let inc = effective_freq * inv_sr;

        self.active = true;
        self.string_idx = string_idx;
        self.phase_inc = inc;
        self.target_phase_inc = inc;
        self.glide_step = 0.0;
        self.glide_remaining = 0;

        let vel_clamped = velocity.clamp(0.0, 1.0);
        self.env = vel_clamped * 0.25;
        self.impulse_env = vel_clamped * 0.35;

        // Calculate sample-rate-agnostic exponential decay time constant:
        // Open string ~ 2.5s tau, heavy palm mute ~ 0.12s tau.
        let mute_clamped = palm_mute.clamp(0.0, 1.0);
        let tau_seconds = 2.5 * (1.0 - 0.95 * mute_clamped);
        self.decay_coeff = (-inv_sr / tau_seconds.max(0.01)).exp();

        // Subtle stereo spread by string index
        self.pan = (0.35 + ((string_idx % 8) as f32) * 0.04).clamp(0.15, 0.85);
    }

    #[inline(always)]
    pub fn set_glide(&mut self, target_freq_hz: f64, glide_samples: u32, sample_rate: f32) {
        if !self.active {
            return;
        }
        let inv_sr = if sample_rate > 1.0 {
            1.0 / sample_rate
        } else {
            1.0 / 48_000.0
        };
        let target_inc =
            (target_freq_hz.clamp(15.0, (sample_rate as f64) * 0.45) as f32) * inv_sr;
        self.target_phase_inc = target_inc;
        if glide_samples == 0 {
            self.phase_inc = target_inc;
            self.glide_step = 0.0;
            self.glide_remaining = 0;
        } else {
            self.glide_step = (target_inc - self.phase_inc) / (glide_samples as f32);
            self.glide_remaining = glide_samples;
        }
    }

    #[inline(always)]
    pub fn slur(&mut self, new_freq_hz: f64, hammer_energy: f32, sample_rate: f32) {
        if !self.active {
            self.trigger(self.string_idx, new_freq_hz, hammer_energy, 0.0, None, sample_rate);
            return;
        }
        let inv_sr = if sample_rate > 1.0 {
            1.0 / sample_rate
        } else {
            1.0 / 48_000.0
        };
        let inc = (new_freq_hz.clamp(15.0, (sample_rate as f64) * 0.45) as f32) * inv_sr;
        self.phase_inc = inc;
        self.target_phase_inc = inc;
        self.glide_remaining = 0;
        self.env = (self.env + hammer_energy.clamp(0.0, 1.0) * 0.15).min(0.35);
        self.impulse_env = hammer_energy.clamp(0.0, 1.0) * 0.12;
    }

    #[inline(always)]
    pub fn mute(&mut self, release_rate: f32) {
        if self.active {
            self.decay_coeff = release_rate.clamp(0.90, 0.9999);
        }
    }

    /// Steps one audio sample and returns `(left, right)`.
    #[inline(always)]
    pub fn next_frame(&mut self) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        if self.glide_remaining > 0 {
            self.phase_inc += self.glide_step;
            self.glide_remaining -= 1;
            if self.glide_remaining == 0 {
                self.phase_inc = self.target_phase_inc;
            }
        }

        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Sine fundamental + 2nd harmonic + short pluck impulse transient
        let angle = self.phase * TAU;
        let osc = angle.sin() + 0.25 * (angle * 2.0).sin();
        let impulse = (angle * 4.0).sin() * self.impulse_env;
        let raw = osc * self.env + impulse;

        self.env = flush_denormal(self.env * self.decay_coeff);
        self.impulse_env = flush_denormal(self.impulse_env * 0.985);

        if self.env < 1.0e-5 && self.impulse_env < 1.0e-5 {
            self.env = 0.0;
            self.impulse_env = 0.0;
            self.active = false;
            return (0.0, 0.0);
        }

        let left = raw * (1.0 - self.pan);
        let right = raw * self.pan;
        (left, right)
    }
}

/// Pre-allocated, real-time audio mixer owned exclusively by the `cpal` audio callback thread.
pub struct SynthMixer {
    sample_rate: f32,
    consumer: rtrb::Consumer<SynthCommand>,
    /// Pre-allocated bank of 12 Extended Karplus-Strong physical string waveguides.
    strings: Box<[StringWaveguide; MAX_STRINGS]>,
    /// Post-string acoustic body / electric pickup + cabinet resonator.
    body: BodyResonator,
    /// Auxiliary test voices for drum/GM triggers (prior to Phase 4 percussion module).
    voices: [TestSineVoice; MAX_TEST_VOICES],
    voice_cursor: usize,
}

impl SynthMixer {
    /// Pre-allocates all mixer, waveguide, body resonator, and voice state prior to starting the `cpal` stream.
    pub fn new(sample_rate: f32, consumer: rtrb::Consumer<SynthCommand>) -> Self {
        let sr = sample_rate.max(8_000.0);
        let strings = Box::new(std::array::from_fn(|i| {
            StringWaveguide::new(sr, (i + 1) as u8)
        }));
        Self {
            sample_rate: sr,
            consumer,
            strings,
            body: BodyResonator::new(sr, InstrumentProfile::AcousticGuitar),
            voices: [TestSineVoice::default(); MAX_TEST_VOICES],
            voice_cursor: 0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn profile(&self) -> InstrumentProfile {
        self.body.profile()
    }

    #[inline(always)]
    fn map_string_slot(string_idx: u8) -> usize {
        (string_idx.saturating_sub(1) as usize) % MAX_STRINGS
    }

    /// Drains all pending `SynthCommand` messages from the lock-free SPSC queue.
    #[inline(always)]
    pub fn drain_commands(&mut self) {
        while let Ok(cmd) = self.consumer.pop() {
            self.dispatch_command(cmd);
        }
    }

    /// Dispatches a single `SynthCommand` in constant time with zero allocations.
    #[inline(always)]
    pub fn dispatch_command(&mut self, cmd: SynthCommand) {
        match cmd {
            SynthCommand::TriggerString {
                string_idx,
                freq_hz,
                velocity,
                pluck_pos,
                palm_mute,
                harmonic_node,
            } => {
                let slot = Self::map_string_slot(string_idx);
                self.strings[slot].trigger(
                    string_idx,
                    freq_hz,
                    velocity,
                    pluck_pos,
                    palm_mute,
                    harmonic_node,
                );
            }
            SynthCommand::SlideString {
                string_idx,
                target_freq_hz,
                glide_samples,
            } => {
                let slot = Self::map_string_slot(string_idx);
                self.strings[slot].slide(target_freq_hz, glide_samples);
            }
            SynthCommand::SlurString {
                string_idx,
                new_freq_hz,
                hammer_energy,
            } => {
                let slot = Self::map_string_slot(string_idx);
                self.strings[slot].slur(new_freq_hz, hammer_energy);
            }
            SynthCommand::MuteString {
                string_idx,
                release_rate,
            } => {
                let slot = Self::map_string_slot(string_idx);
                self.strings[slot].mute(release_rate);
            }
            SynthCommand::TriggerDrum {
                gm_drum_note,
                velocity,
            } => {
                let pseudo_string = 100u8.wrapping_add(gm_drum_note);
                let freq_hz = 60.0 + (gm_drum_note as f64) * 4.5;
                let slot = self.find_or_steal_voice(pseudo_string);
                self.voices[slot].trigger(
                    pseudo_string,
                    freq_hz,
                    velocity,
                    0.85,
                    None,
                    self.sample_rate,
                );
            }
            SynthCommand::TriggerGmVoice {
                channel,
                freq_hz,
                velocity,
                program: _,
            } => {
                let pseudo_string = 200u8.wrapping_add(channel);
                let slot = self.find_or_steal_voice(pseudo_string);
                self.voices[slot].trigger(
                    pseudo_string,
                    freq_hz,
                    velocity,
                    0.2,
                    None,
                    self.sample_rate,
                );
            }
            SynthCommand::SetInstrumentProfile(profile) => {
                self.body.set_profile(profile);
                for s in self.strings.iter_mut() {
                    s.set_profile(profile);
                }
            }
            SynthCommand::AllNotesOff => {
                for s in self.strings.iter_mut() {
                    s.clear();
                }
                self.body.reset();
                for v in &mut self.voices {
                    v.active = false;
                    v.env = 0.0;
                    v.impulse_env = 0.0;
                }
            }
        }
    }

    #[inline(always)]
    fn find_or_steal_voice(&mut self, string_idx: u8) -> usize {
        for (i, v) in self.voices.iter().enumerate() {
            if v.active && v.string_idx == string_idx {
                return i;
            }
        }
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                return i;
            }
        }
        let idx = self.voice_cursor % MAX_TEST_VOICES;
        self.voice_cursor = self.voice_cursor.wrapping_add(1);
        idx
    }

    /// Renders the next stereo frame `(left, right)` in `[-1.0, 1.0]`.
    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let mut str_left = 0.0_f32;
        let mut str_right = 0.0_f32;

        for s in self.strings.iter_mut() {
            if s.active {
                let (l, r) = s.next_frame();
                str_left += l;
                str_right += r;
            }
        }

        // Route summed string waveguides through the active acoustic body / electric cab resonator
        let (mut left, mut right) = self.body.process_stereo(str_left, str_right);

        for v in &mut self.voices {
            if v.active {
                let (l, r) = v.next_frame();
                left += l;
                right += r;
            }
        }

        // Smooth rational soft-clipper + hard clamp to prevent digital overs
        let soft_l = left / (1.0 + left.abs() * 0.25);
        let soft_r = right / (1.0 + right.abs() * 0.25);
        let left_out = flush_denormal(soft_l).clamp(-1.0, 1.0);
        let right_out = flush_denormal(soft_r).clamp(-1.0, 1.0);
        (left_out, right_out)
    }

    /// Processes an entire interleaved `f32` output buffer (drains commands once at start of buffer).
    #[inline(always)]
    pub fn process_f32(&mut self, data: &mut [f32], channels: usize) {
        self.drain_commands();
        if channels == 0 {
            return;
        }
        if channels == 1 {
            for sample in data.iter_mut() {
                let (l, r) = self.next_stereo_frame();
                *sample = (l + r) * 0.5;
            }
        } else {
            for frame in data.chunks_exact_mut(channels) {
                let (l, r) = self.next_stereo_frame();
                frame[0] = l;
                frame[1] = r;
                for extra in &mut frame[2..] {
                    *extra = 0.0;
                }
            }
        }
    }

    /// Processes an entire interleaved `i16` output buffer.
    #[inline(always)]
    pub fn process_i16(&mut self, data: &mut [i16], channels: usize) {
        self.drain_commands();
        if channels == 0 {
            return;
        }
        if channels == 1 {
            for sample in data.iter_mut() {
                let (l, r) = self.next_stereo_frame();
                let mono = ((l + r) * 0.5).clamp(-1.0, 1.0);
                *sample = (mono * 32767.0) as i16;
            }
        } else {
            for frame in data.chunks_exact_mut(channels) {
                let (l, r) = self.next_stereo_frame();
                frame[0] = (l * 32767.0) as i16;
                frame[1] = (r * 32767.0) as i16;
                for extra in &mut frame[2..] {
                    *extra = 0;
                }
            }
        }
    }

    /// Processes an entire interleaved `u16` output buffer.
    #[inline(always)]
    pub fn process_u16(&mut self, data: &mut [u16], channels: usize) {
        self.drain_commands();
        if channels == 0 {
            return;
        }
        if channels == 1 {
            for sample in data.iter_mut() {
                let (l, r) = self.next_stereo_frame();
                let mono = ((l + r) * 0.5).clamp(-1.0, 1.0);
                *sample = ((mono * 0.5 + 0.5) * 65535.0) as u16;
            }
        } else {
            for frame in data.chunks_exact_mut(channels) {
                let (l, r) = self.next_stereo_frame();
                frame[0] = ((l * 0.5 + 0.5) * 65535.0) as u16;
                frame[1] = ((r * 0.5 + 0.5) * 65535.0) as u16;
                for extra in &mut frame[2..] {
                    *extra = 32768;
                }
            }
        }
    }
}

/// Active `cpal` stream metadata returned upon successful initialization.
pub struct ActiveAudioStream {
    pub stream: cpal::Stream,
    pub sample_rate: f32,
    pub channels: u16,
}

/// Initializes and starts the default `cpal` output stream with the lowest-latency viable configuration.
///
/// Supports `f32`, `i16`, and `u16` hardware sample formats.
pub fn spawn_cpal_stream(
    consumer: rtrb::Consumer<SynthCommand>,
) -> Result<ActiveAudioStream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "No default audio output device found".to_string())?;

    let default_cfg = device
        .default_output_config()
        .map_err(|e| format!("Failed to query default audio output config: {}", e))?;

    let sample_format = default_cfg.sample_format();
    let channels = default_cfg.channels();
    let sample_rate = default_cfg.sample_rate().0 as f32;

    // Select a low-latency buffer size if the hardware supports a configurable range
    let buffer_size = match default_cfg.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            let frames = TARGET_BUFFER_FRAMES.clamp(*min, *max);
            cpal::BufferSize::Fixed(frames)
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };

    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: default_cfg.sample_rate(),
        buffer_size,
    };

    let mixer = SynthMixer::new(sample_rate, consumer);
    let ch_usize = channels as usize;

    let stream = build_stream_for_format(&device, &stream_config, sample_format, mixer, ch_usize)?;
    stream
        .play()
        .map_err(|e| format!("Failed to start cpal output stream: {}", e))?;

    Ok(ActiveAudioStream {
        stream,
        sample_rate,
        channels,
    })
}

fn build_stream_for_format(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mut mixer: SynthMixer,
    channels: usize,
) -> Result<cpal::Stream, String> {
    let err_fn = |_err: cpal::StreamError| {
        // Real-time safe: ignore or silently drop stream error without locking stdio
    };

    let res = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                mixer.process_f32(data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                mixer.process_i16(data, channels);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            config,
            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                mixer.process_u16(data, channels);
            },
            err_fn,
            None,
        ),
        other => {
            return Err(format!("Unsupported cpal sample format: {:?}", other));
        }
    };

    res.map_err(|e| format!("Failed to build cpal output stream: {}", e))
}
