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
use crate::synth::patch::PitchedEngine;
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

/// Maximum number of independent instrument parts/tracks supported simultaneously by the real-time mixer.
pub const MAX_SYNTH_PARTS: usize = 16;

/// Independent per-instrument synthesis state (own string waveguides, body resonator, FM synth, and drum kit).
pub struct PartSynthState {
    pub engine: PitchedEngine,
    pub strings: Box<[StringWaveguide; MAX_STRINGS]>,
    pub body: BodyResonator,
    pub percussion: PercussionAndGmPool,
    /// Shared scalar bridge vibration bus summed across all strings each sample frame
    /// for passive sympathetic resonance coupling.
    pub bridge_bus: f32,
    /// Number of tail samples remaining for the body resonator after all strings go silent.
    pub body_tail_samples: u32,
    /// Sample-rate scaled duration of the body resonator tail (~250 ms for high-Q wood modes).
    body_tail_len: u32,
}

impl PartSynthState {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        // Allocate each StringWaveguide onto a heap Vec one at a time so stack usage remains minimal.
        let mut str_vec = Vec::with_capacity(MAX_STRINGS);
        for i in 0..MAX_STRINGS {
            str_vec.push(StringWaveguide::new(sr, (i + 1) as u8));
        }
        let strings: Box<[StringWaveguide; MAX_STRINGS]> =
            match str_vec.into_boxed_slice().try_into() {
                Ok(arr) => arr,
                Err(_) => unreachable!(),
            };
        let body_tail_len = (0.25 * sr).round().clamp(2048.0, 24_000.0) as u32;
        Self {
            engine: PitchedEngine::PhysicalString,
            strings,
            body: BodyResonator::new(sr, InstrumentProfile::AcousticGuitar),
            percussion: PercussionAndGmPool::new(sr),
            bridge_bus: 0.0,
            body_tail_samples: 0,
            body_tail_len,
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        for s in self.strings.iter_mut() {
            s.clear();
        }
        self.body.reset();
        self.percussion.all_notes_off();
        self.bridge_bus = 0.0;
        self.body_tail_samples = 0;
    }

    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let prev_bridge = self.bridge_bus;
        let mut coupled_count = 0usize;
        let mut any_string_active = false;

        // Wake any already-tuned, unmuted silent strings when sympathetic bridge energy is present
        if prev_bridge.abs() > 2.0e-4 {
            for s in self.strings.iter_mut() {
                if !s.active {
                    s.try_sympathetic_wake(prev_bridge);
                }
                if s.active {
                    any_string_active = true;
                    if s.is_sympathetically_coupled() {
                        coupled_count += 1;
                    }
                }
            }
        } else {
            for s in self.strings.iter() {
                if s.active {
                    any_string_active = true;
                    if s.is_sympathetically_coupled() {
                        coupled_count += 1;
                    }
                }
            }
        }

        let mut str_left = 0.0_f32;
        let mut str_right = 0.0_f32;
        let mut next_bridge_bus = 0.0_f32;

        if any_string_active {
            for s in self.strings.iter_mut() {
                if s.active {
                    let (l, r, bridge_force) = s.next_frame_with_bridge(prev_bridge, coupled_count);
                    str_left += l;
                    str_right += r;
                    next_bridge_bus += bridge_force;
                }
            }
            self.body_tail_samples = self.body_tail_len;
        }
        self.bridge_bus = flush_denormal(next_bridge_bus.clamp(-4.0, 4.0));

        let (mut left, mut right) = if any_string_active || self.body_tail_samples > 0 {
            if !any_string_active {
                self.body_tail_samples -= 1;
            }
            self.body.process_stereo(str_left, str_right)
        } else {
            (0.0, 0.0)
        };

        if self.percussion.has_active_voices() {
            let (perc_l, perc_r) = self.percussion.next_stereo_frame();
            left += perc_l;
            right += perc_r;
        }

        (left, right)
    }
}

/// Pre-allocated, real-time audio mixer owned exclusively by the `cpal` audio callback thread.
pub struct SynthMixer {
    sample_rate: f32,
    consumer: rtrb::Consumer<SynthCommand>,
    /// Pre-allocated bank of 16 independent instrument part synthesizers.
    pub parts: Box<[PartSynthState; MAX_SYNTH_PARTS]>,
    /// Currently targeted part index (`0..MAX_SYNTH_PARTS`) for subsequent note/articulation commands.
    pub active_part_idx: usize,
}

impl SynthMixer {
    /// Pre-allocates all mixer, waveguide, body resonator, and percussion/GM voice state prior to starting the `cpal` stream.
    pub fn new(sample_rate: f32, consumer: rtrb::Consumer<SynthCommand>) -> Self {
        let sr = sample_rate.max(8_000.0);
        // Allocate each PartSynthState directly on the heap via Vec -> Box<[PartSynthState; MAX_SYNTH_PARTS]>
        // to avoid large stack frames during initialization.
        let mut part_vec = Vec::with_capacity(MAX_SYNTH_PARTS);
        for _ in 0..MAX_SYNTH_PARTS {
            part_vec.push(PartSynthState::new(sr));
        }
        let parts: Box<[PartSynthState; MAX_SYNTH_PARTS]> = match part_vec.into_boxed_slice().try_into() {
            Ok(arr) => arr,
            Err(_) => unreachable!(),
        };
        Self {
            sample_rate: sr,
            consumer,
            parts,
            active_part_idx: 0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn profile(&self) -> InstrumentProfile {
        self.parts[self.active_part_idx].body.profile()
    }

    #[inline(always)]
    fn map_string_slot(string_idx: u8) -> usize {
        (string_idx.saturating_sub(1) as usize) % MAX_STRINGS
    }

    #[inline(always)]
    fn map_part_slot(part_idx: u8) -> usize {
        (part_idx as usize) % MAX_SYNTH_PARTS
    }

    /// Drains all pending `SynthCommand` messages from the lock-free SPSC queue.
    #[inline(always)]
    pub fn drain_commands(&mut self) {
        while let Ok(cmd) = self.consumer.pop() {
            self.dispatch_command(cmd);
        }
    }

    /// Pops a single pending command from the SPSC consumer queue without dispatching it.
    #[inline]
    pub fn pop_command(&mut self) -> Option<SynthCommand> {
        self.consumer.pop().ok()
    }

    /// Dispatches a single `SynthCommand` in constant time with zero allocations.
    #[inline(always)]
    pub fn dispatch_command(&mut self, cmd: SynthCommand) {
        match cmd {
            SynthCommand::SetActivePart(part_idx) => {
                self.active_part_idx = Self::map_part_slot(part_idx);
            }
            SynthCommand::TriggerString {
                string_idx,
                freq_hz,
                velocity,
                pluck_pos,
                palm_mute,
                harmonic_node,
            } => {
                let part = &mut self.parts[self.active_part_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.trigger_fm_string(string_idx, freq_hz, velocity);
                } else {
                    let slot = Self::map_string_slot(string_idx);
                    part.strings[slot].trigger(
                        string_idx,
                        freq_hz,
                        velocity,
                        pluck_pos,
                        palm_mute,
                        harmonic_node,
                    );
                }
            }
            SynthCommand::SlideString {
                string_idx,
                target_freq_hz,
                glide_samples,
            } => {
                let part = &mut self.parts[self.active_part_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.slide_fm_string(string_idx, target_freq_hz);
                } else {
                    let slot = Self::map_string_slot(string_idx);
                    part.strings[slot].slide(target_freq_hz, glide_samples);
                }
            }
            SynthCommand::SlurString {
                string_idx,
                new_freq_hz,
                hammer_energy,
            } => {
                let part = &mut self.parts[self.active_part_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.slur_fm_string(string_idx, new_freq_hz, hammer_energy);
                } else {
                    let slot = Self::map_string_slot(string_idx);
                    part.strings[slot].slur(new_freq_hz, hammer_energy);
                }
            }
            SynthCommand::MuteString {
                string_idx,
                release_rate,
            } => {
                let part = &mut self.parts[self.active_part_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.mute_fm_string(string_idx, release_rate);
                }
                let slot = Self::map_string_slot(string_idx);
                part.strings[slot].mute(release_rate);
            }
            SynthCommand::TriggerDrum {
                gm_drum_note,
                velocity,
            } => {
                self.parts[self.active_part_idx]
                    .percussion
                    .trigger_drum(gm_drum_note, velocity);
            }
            SynthCommand::TriggerGmVoice {
                channel,
                freq_hz,
                velocity,
                program,
            } => {
                self.parts[self.active_part_idx]
                    .percussion
                    .trigger_gm_voice(channel, freq_hz, velocity, program);
            }
            SynthCommand::SetInstrumentProfile(profile) => {
                let part = &mut self.parts[self.active_part_idx];
                part.body.set_profile(profile);
                for s in part.strings.iter_mut() {
                    s.set_profile(profile);
                }
            }
            SynthCommand::SetPartProfile { part_idx, profile } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                part.body.set_profile(profile);
                for s in part.strings.iter_mut() {
                    s.set_profile(profile);
                }
            }
            SynthCommand::SetPitchedEngine(engine) => {
                let part = &mut self.parts[self.active_part_idx];
                if part.engine != engine {
                    part.clear();
                    part.engine = engine;
                }
            }
            SynthCommand::SetPartEngine { part_idx, engine } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                if part.engine != engine {
                    part.clear();
                    part.engine = engine;
                }
            }
            SynthCommand::UpdateStringParams(params) => {
                let part = &mut self.parts[self.active_part_idx];
                for s in part.strings.iter_mut() {
                    s.update_params(&params);
                }
            }
            SynthCommand::UpdateBodyParams(params) => {
                self.parts[self.active_part_idx].body.update_params(&params);
            }
            SynthCommand::UpdateDrumParams { drum_role, params } => {
                self.parts[self.active_part_idx]
                    .percussion
                    .update_drum_params(drum_role, params);
            }
            SynthCommand::UpdateGmVoiceParams(params) => {
                self.parts[self.active_part_idx]
                    .percussion
                    .update_fm_params(params);
            }
            SynthCommand::UpdatePartStringParams { part_idx, params } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                for s in part.strings.iter_mut() {
                    s.update_params(&params);
                }
            }
            SynthCommand::UpdatePartBodyParams { part_idx, params } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                part.body.update_params(&params);
            }
            SynthCommand::UpdatePartFmParams { part_idx, params } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                part.percussion.update_fm_params(params);
            }
            SynthCommand::UpdatePartDrumParams {
                part_idx,
                drum_role,
                params,
            } => {
                let part = &mut self.parts[Self::map_part_slot(part_idx)];
                part.percussion.update_drum_params(drum_role, params);
            }
            SynthCommand::AllNotesOff => {
                for part in self.parts.iter_mut() {
                    part.clear();
                }
            }
        }
    }

    /// Renders the next stereo frame `(left, right)` in `[-1.0, 1.0]`, summing all active instrument parts.
    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;

        for part in self.parts.iter_mut() {
            let (pl, pr) = part.next_stereo_frame();
            left += pl;
            right += pr;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{SynthCommand, SynthEngine};

    #[test]
    fn test_mixer_sympathetic_bridge_bus_and_12_string_unison_stability() {
        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);

        // Configure max sympathetic resonance & high sustain on Part 0
        let mut sp = crate::synth::patch::StringSynthParams::default();
        sp.t60_decay_sec = 9.0;
        sp.sympathetic_coupling = 0.012;
        sp.polarization_coupling = 0.024;
        sp.polarization_detune_cents = 1.2;
        assert!(engine.send(SynthCommand::UpdatePartStringParams {
            part_idx: 0,
            params: sp,
        }));

        // Worst-case stability stress test: trigger all 12 strings simultaneously at exact unison (220 Hz)
        for s_idx in 1..=(MAX_STRINGS as u8) {
            assert!(engine.send(SynthCommand::TriggerString {
                string_idx: s_idx,
                freq_hz: 220.0,
                velocity: 1.0,
                pluck_pos: 0.18,
                palm_mute: 0.0,
                harmonic_node: None,
            }));
        }

        // Process 1 second (48,000 stereo frames) in 256-frame blocks
        let mut buf = [0.0_f32; 512];
        let mut first_half_rms = 0.0_f32;
        let mut second_half_rms = 0.0_f32;
        for block in 0..188 {
            mixer.process_f32(&mut buf, 2);
            for &s in &buf {
                assert!(
                    s.is_finite() && s.abs() <= 1.0,
                    "12-string unison sympathetic stress test produced invalid sample: {}",
                    s
                );
            }
            let e: f32 = buf.iter().map(|s| s * s).sum();
            if block < 94 {
                first_half_rms += e;
            } else {
                second_half_rms += e;
            }
        }
        assert!(
            first_half_rms > second_half_rms && second_half_rms > 1.0e-3,
            "12-string unison sympathetic bridge bus must decay stably without runaway (first={}, second={})",
            first_half_rms,
            second_half_rms
        );
    }
}
