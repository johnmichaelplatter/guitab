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
use crate::synth::percussion::PercussionAndGmPool;
use crate::synth::waveguide::{StringWaveguide, MAX_STRINGS};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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

/// Pre-allocated, real-time audio mixer owned exclusively by the `cpal` audio callback thread.
pub struct SynthMixer {
    sample_rate: f32,
    consumer: rtrb::Consumer<SynthCommand>,
    /// Pre-allocated bank of 12 Extended Karplus-Strong physical string waveguides.
    strings: Box<[StringWaveguide; MAX_STRINGS]>,
    /// Post-string acoustic body / electric pickup + cabinet resonator.
    body: BodyResonator,
    /// Fixed 16-voice polyphonic pool for GM percussion (kick, snare, 808 hats/cymbals) and 2-op FM GM backing.
    percussion: PercussionAndGmPool,
}

impl SynthMixer {
    /// Pre-allocates all mixer, waveguide, body resonator, and percussion/GM voice state prior to starting the `cpal` stream.
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
            percussion: PercussionAndGmPool::new(sr),
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
                self.percussion.trigger_drum(gm_drum_note, velocity);
            }
            SynthCommand::TriggerGmVoice {
                channel,
                freq_hz,
                velocity,
                program,
            } => {
                self.percussion
                    .trigger_gm_voice(channel, freq_hz, velocity, program);
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
                self.percussion.all_notes_off();
            }
        }
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

        // Sum 16-voice polyphonic percussion & 2-op FM GM backing pool
        let (perc_l, perc_r) = self.percussion.next_stereo_frame();
        left += perc_l;
        right += perc_r;

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
