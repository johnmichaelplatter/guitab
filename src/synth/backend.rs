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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Target low-latency hardware buffer size in frames on macOS/Windows when supported by the host device.
const TARGET_BUFFER_FRAMES: u32 = 256;

/// Configurable audio output buffer size preference for the `cpal` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBufferSize {
    /// Let the OS audio backend (ALSA / PipeWire / PulseAudio / CoreAudio / WASAPI) choose.
    Auto,
    /// Request a fixed buffer size in frames (clamped to the device's supported range).
    Fixed(u32),
}

impl AudioBufferSize {
    pub const ALL_PRESETS: [AudioBufferSize; 6] = [
        AudioBufferSize::Auto,
        AudioBufferSize::Fixed(256),
        AudioBufferSize::Fixed(512),
        AudioBufferSize::Fixed(1024),
        AudioBufferSize::Fixed(2048),
        AudioBufferSize::Fixed(4096),
    ];

    /// Platform-appropriate default:
    /// - Linux defaults to `Auto` (`cpal::BufferSize::Default`) to avoid ALSA/Pulse/PipeWire
    ///   underruns (`snd_pcm_recover`) on non-RT desktop kernels and low-power CPUs.
    /// - macOS / Windows default to `Fixed(256)` for low-latency CoreAudio/WASAPI playback.
    pub const fn platform_default() -> Self {
        #[cfg(target_os = "linux")]
        {
            AudioBufferSize::Auto
        }
        #[cfg(not(target_os = "linux"))]
        {
            AudioBufferSize::Fixed(TARGET_BUFFER_FRAMES)
        }
    }

    pub fn label(self) -> String {
        match self {
            AudioBufferSize::Auto => "Auto".to_string(),
            AudioBufferSize::Fixed(n) => format!("{} frames", n),
        }
    }

    pub fn short_label(self) -> String {
        match self {
            AudioBufferSize::Auto => "Auto".to_string(),
            AudioBufferSize::Fixed(n) => format!("{}", n),
        }
    }
}

impl Default for AudioBufferSize {
    fn default() -> Self {
        Self::platform_default()
    }
}

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
    pub fn next_stereo_frame(&mut self) -> (f32, f32, bool) {
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

        let perc_active = self.percussion.has_active_voices();
        if perc_active {
            let (perc_l, perc_r) = self.percussion.next_stereo_frame();
            left += perc_l;
            right += perc_r;
        }

        let still_active = any_string_active
            || self.body_tail_samples > 0
            || self.bridge_bus.abs() > 2.0e-4
            || perc_active;

        (left, right, still_active)
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
    /// Bitmask of parts with active audio (bit `i` corresponds to `parts[i]`).
    /// Allows `next_stereo_frame()` to skip completely silent parts in O(1) without cache thrashing.
    active_parts_mask: u16,
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
            active_parts_mask: 0,
        }
    }

    #[inline(always)]
    pub fn active_parts_mask(&self) -> u16 {
        self.active_parts_mask
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
                let p_idx = self.active_part_idx;
                let part = &mut self.parts[p_idx];
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
                self.active_parts_mask |= 1u16 << p_idx;
            }
            SynthCommand::SlideString {
                string_idx,
                target_freq_hz,
                glide_samples,
            } => {
                let p_idx = self.active_part_idx;
                let part = &mut self.parts[p_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.slide_fm_string(string_idx, target_freq_hz);
                } else {
                    let slot = Self::map_string_slot(string_idx);
                    part.strings[slot].slide(target_freq_hz, glide_samples);
                }
                self.active_parts_mask |= 1u16 << p_idx;
            }
            SynthCommand::SlurString {
                string_idx,
                new_freq_hz,
                hammer_energy,
            } => {
                let p_idx = self.active_part_idx;
                let part = &mut self.parts[p_idx];
                if part.engine == PitchedEngine::FmSynth {
                    part.percussion.slur_fm_string(string_idx, new_freq_hz, hammer_energy);
                } else {
                    let slot = Self::map_string_slot(string_idx);
                    part.strings[slot].slur(new_freq_hz, hammer_energy);
                }
                self.active_parts_mask |= 1u16 << p_idx;
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
                let p_idx = self.active_part_idx;
                self.parts[p_idx]
                    .percussion
                    .trigger_drum(gm_drum_note, velocity);
                self.active_parts_mask |= 1u16 << p_idx;
            }
            SynthCommand::TriggerGmVoice {
                channel,
                freq_hz,
                velocity,
                program,
            } => {
                let p_idx = self.active_part_idx;
                self.parts[p_idx]
                    .percussion
                    .trigger_gm_voice(channel, freq_hz, velocity, program);
                self.active_parts_mask |= 1u16 << p_idx;
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
                let p_idx = self.active_part_idx;
                let part = &mut self.parts[p_idx];
                if part.engine != engine {
                    part.clear();
                    part.engine = engine;
                    self.active_parts_mask &= !(1u16 << p_idx);
                }
            }
            SynthCommand::SetPartEngine { part_idx, engine } => {
                let slot = Self::map_part_slot(part_idx);
                let part = &mut self.parts[slot];
                if part.engine != engine {
                    part.clear();
                    part.engine = engine;
                    self.active_parts_mask &= !(1u16 << slot);
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
                self.active_parts_mask = 0;
            }
        }
    }

    /// Renders the next stereo frame `(left, right)` in `[-1.0, 1.0]`, summing only active instrument parts.
    #[inline(always)]
    pub fn next_stereo_frame(&mut self) -> (f32, f32) {
        let mut mask = self.active_parts_mask;
        if mask == 0 {
            return (0.0, 0.0);
        }

        let mut left = 0.0_f32;
        let mut right = 0.0_f32;
        let mut still_active_mask = 0u16;

        while mask != 0 {
            let idx = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let (pl, pr, active) = self.parts[idx].next_stereo_frame();
            left += pl;
            right += pr;
            if active {
                still_active_mask |= 1u16 << idx;
            }
        }
        self.active_parts_mask = still_active_mask;

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
    pub device_name: String,
    pub buffer_size_label: String,
    pub stream_error_count: Arc<AtomicU64>,
}

/// Enumerates the names of all available `cpal` audio output devices on the current host.
pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                let trimmed = name.trim();
                if !trimmed.is_empty() && !names.iter().any(|n: &String| n == trimmed) {
                    names.push(trimmed.to_string());
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        fn linux_device_priority(name: &str) -> u8 {
            match name {
                "default" => 0,
                "pipewire" => 1,
                "pulse" => 2,
                _ if name.starts_with("default:") => 3,
                _ => 4,
            }
        }
        names.sort_by_key(|n| linux_device_priority(n));
    }
    names
}

/// Scores a candidate `cpal` supported output config range and chooses the best sample rate
/// within `[min_sr, max_sr]`. Returns `Some((score, chosen_sample_rate))` or `None` if unsupported.
///
/// Strongly prefers stereo (`2` ch), `48_000` Hz (or `44_100` Hz), and `F32` (falling back to
/// `I16`/`U16`) to avoid Linux ALSA/PipeWire `default_output_config()` selecting excessive
/// channel counts (e.g. 32 ch) or 192 kHz sample rates.
pub(crate) fn score_output_candidate(
    channels: u16,
    min_sr: u32,
    max_sr: u32,
    sample_format: cpal::SampleFormat,
) -> Option<(i32, u32)> {
    if channels == 0 || min_sr == 0 || max_sr < min_sr {
        return None;
    }

    let format_score = match sample_format {
        cpal::SampleFormat::F32 => 300,
        cpal::SampleFormat::I16 => 200,
        cpal::SampleFormat::U16 => 100,
        _ => return None,
    };

    let channel_score = match channels {
        2 => 2000,
        1 => 1000,
        c => 500 - (c as i32 * 20),
    };

    let (sr_score, chosen_sr) = if min_sr <= 48_000 && 48_000 <= max_sr {
        (1000, 48_000)
    } else if min_sr <= 44_100 && 44_100 <= max_sr {
        (900, 44_100)
    } else {
        let clamped = 48_000u32.clamp(min_sr, max_sr);
        let diff_khz = (clamped as i64 - 48_000).unsigned_abs() / 1000;
        let penalty = (diff_khz.min(500) as i32) * 4;
        (600 - penalty, clamped)
    };

    Some((channel_score + sr_score + format_score, chosen_sr))
}

/// Negotiates the best supported output config for `device`, preferring stereo 48/44.1 kHz `F32`,
/// and falling back to `device.default_output_config()` if enumeration fails.
fn select_best_output_config(
    device: &cpal::Device,
    device_name: &str,
) -> Result<cpal::SupportedStreamConfig, String> {
    if let Ok(configs) = device.supported_output_configs() {
        let mut best: Option<(i32, cpal::SupportedStreamConfig)> = None;
        for range in configs {
            if let Some((score, chosen_sr)) = score_output_candidate(
                range.channels(),
                range.min_sample_rate().0,
                range.max_sample_rate().0,
                range.sample_format(),
            ) {
                if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best = Some((score, range.with_sample_rate(cpal::SampleRate(chosen_sr))));
                }
            }
        }
        if let Some((_, cfg)) = best {
            return Ok(cfg);
        }
    }

    device
        .default_output_config()
        .map_err(|e| format!("Failed to query audio output config for '{}': {}", device_name, e))
}

/// Initializes and starts the default `cpal` output stream with the platform default buffer size.
///
/// Supports `f32`, `i16`, and `u16` hardware sample formats.
pub fn spawn_cpal_stream(
    consumer: rtrb::Consumer<SynthCommand>,
) -> Result<ActiveAudioStream, String> {
    spawn_cpal_stream_for_device_and_buffer(consumer, None, AudioBufferSize::platform_default())
}

/// Initializes and starts a `cpal` output stream on `preferred_device` (or the system default
/// output device when `preferred_device` is `None`) with the platform default buffer size.
pub fn spawn_cpal_stream_for_device(
    consumer: rtrb::Consumer<SynthCommand>,
    preferred_device: Option<&str>,
) -> Result<ActiveAudioStream, String> {
    spawn_cpal_stream_for_device_and_buffer(
        consumer,
        preferred_device,
        AudioBufferSize::platform_default(),
    )
}

/// Initializes and starts a `cpal` output stream on `preferred_device` with an explicit
/// [`AudioBufferSize`] preference.
pub fn spawn_cpal_stream_for_device_and_buffer(
    consumer: rtrb::Consumer<SynthCommand>,
    preferred_device: Option<&str>,
    preferred_buffer: AudioBufferSize,
) -> Result<ActiveAudioStream, String> {
    let host = cpal::default_host();
    let device = match preferred_device.map(str::trim).filter(|s| !s.is_empty()) {
        Some(target_name) => {
            let mut found = None;
            if let Ok(devices) = host.output_devices() {
                for dev in devices {
                    if let Ok(name) = dev.name() {
                        if name.trim() == target_name {
                            found = Some(dev);
                            break;
                        }
                    }
                }
            }
            found.ok_or_else(|| format!("Audio output device '{}' not found", target_name))?
        }
        None => host
            .default_output_device()
            .ok_or_else(|| "No default audio output device found".to_string())?,
    };

    let device_name = device
        .name()
        .unwrap_or_else(|_| "Default Audio Output".to_string());

    let selected_cfg = select_best_output_config(&device, &device_name)?;

    let sample_format = selected_cfg.sample_format();
    let channels = selected_cfg.channels();
    let sample_rate = selected_cfg.sample_rate().0 as f32;

    let (buffer_size, mut buffer_size_label) = match preferred_buffer {
        AudioBufferSize::Auto => (cpal::BufferSize::Default, "Auto".to_string()),
        AudioBufferSize::Fixed(req_frames) => match selected_cfg.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                let frames = req_frames.clamp(*min, *max);
                (cpal::BufferSize::Fixed(frames), format!("{} frames", frames))
            }
            cpal::SupportedBufferSize::Unknown => (cpal::BufferSize::Default, "Auto".to_string()),
        },
    };

    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: selected_cfg.sample_rate(),
        buffer_size,
    };

    let mixer = SynthMixer::new(sample_rate, consumer);
    let ch_usize = channels as usize;
    let stream_error_count = Arc::new(AtomicU64::new(0));

    let stream = match build_stream_for_format(
        &device,
        &stream_config,
        sample_format,
        mixer,
        ch_usize,
        stream_error_count.clone(),
    ) {
        Ok(s) => s,
        Err((err_msg, recovered_mixer)) => {
            if matches!(stream_config.buffer_size, cpal::BufferSize::Fixed(_)) {
                // Retry with OS default buffer size if a strict ALSA/hardware device rejected Fixed(N)
                let fallback_config = cpal::StreamConfig {
                    channels,
                    sample_rate: selected_cfg.sample_rate(),
                    buffer_size: cpal::BufferSize::Default,
                };
                buffer_size_label = "Auto".to_string();
                build_stream_for_format(
                    &device,
                    &fallback_config,
                    sample_format,
                    recovered_mixer,
                    ch_usize,
                    stream_error_count.clone(),
                )
                .map_err(|(e, _)| e)?
            } else {
                return Err(err_msg);
            }
        }
    };

    stream
        .play()
        .map_err(|e| format!("Failed to start cpal output stream on '{}': {}", device_name, e))?;

    Ok(ActiveAudioStream {
        stream,
        sample_rate,
        channels,
        device_name,
        buffer_size_label,
        stream_error_count,
    })
}

struct CallbackMixer {
    mixer: Option<SynthMixer>,
    return_slot: Arc<std::sync::Mutex<Option<SynthMixer>>>,
    stream_started: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for CallbackMixer {
    fn drop(&mut self) {
        if !self.stream_started.load(Ordering::Acquire) {
            if let Some(m) = self.mixer.take() {
                if let Ok(mut guard) = self.return_slot.lock() {
                    *guard = Some(m);
                }
            }
        }
    }
}

fn build_stream_for_format(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mixer: SynthMixer,
    channels: usize,
    error_counter: Arc<AtomicU64>,
) -> Result<cpal::Stream, (String, SynthMixer)> {
    let err_fn = move |_err: cpal::StreamError| {
        // Real-time safe: atomic increment only (zero locks, zero stdio)
        error_counter.fetch_add(1, Ordering::Relaxed);
    };

    let return_slot = Arc::new(std::sync::Mutex::new(None));
    let stream_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut cb_mixer = CallbackMixer {
        mixer: Some(mixer),
        return_slot: return_slot.clone(),
        stream_started: stream_started.clone(),
    };

    let res = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if let Some(m) = cb_mixer.mixer.as_mut() {
                    m.process_f32(data, channels);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                if let Some(m) = cb_mixer.mixer.as_mut() {
                    m.process_i16(data, channels);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            config,
            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                if let Some(m) = cb_mixer.mixer.as_mut() {
                    m.process_u16(data, channels);
                }
            },
            err_fn,
            None,
        ),
        other => {
            let recovered = cb_mixer.mixer.take().unwrap();
            return Err((format!("Unsupported cpal sample format: {:?}", other), recovered));
        }
    };

    match res {
        Ok(stream) => {
            stream_started.store(true, Ordering::Release);
            Ok(stream)
        }
        Err(e) => {
            let recovered = return_slot
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_else(|| {
                    SynthMixer::new(config.sample_rate.0 as f32, rtrb::RingBuffer::new(1).1)
                });
            Err((format!("Failed to build cpal output stream: {}", e), recovered))
        }
    }
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

    #[test]
    fn test_active_parts_mask_skips_silent_parts_and_clears_on_decay() {
        let (mut engine, mut mixer) = SynthEngine::new_headless(48_000.0, 2);

        // Initially idle: mask is 0 and output is exact silence
        assert_eq!(mixer.active_parts_mask(), 0);
        let mut buf = [1.0_f32; 64];
        mixer.process_f32(&mut buf, 2);
        assert!(buf.iter().all(|&s| s == 0.0));
        assert_eq!(mixer.active_parts_mask(), 0);

        // Trigger a string on Part 2 and a drum on Part 5
        assert!(engine.send(SynthCommand::SetActivePart(2)));
        assert!(engine.send(SynthCommand::TriggerString {
            string_idx: 1,
            freq_hz: 330.0,
            velocity: 0.8,
            pluck_pos: 0.2,
            palm_mute: 0.0,
            harmonic_node: None,
        }));
        assert!(engine.send(SynthCommand::SetActivePart(5)));
        assert!(engine.send(SynthCommand::TriggerDrum {
            gm_drum_note: 36,
            velocity: 0.9,
        }));

        let mut render_buf = [0.0_f32; 512];
        mixer.process_f32(&mut render_buf, 2);
        assert_eq!(
            mixer.active_parts_mask(),
            (1u16 << 2) | (1u16 << 5),
            "Only triggered parts 2 and 5 should be marked active"
        );
        let energy: f32 = render_buf.iter().map(|s| s * s).sum();
        assert!(energy > 1.0e-3, "Active parts must produce audible energy");

        // AllNotesOff immediately clears the active mask back to 0
        assert!(engine.all_notes_off());
        let mut silent_buf = [1.0_f32; 64];
        mixer.process_f32(&mut silent_buf, 2);
        assert_eq!(mixer.active_parts_mask(), 0);
        assert!(silent_buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_output_config_selection_prefers_stereo_48k_f32() {
        let stereo_48k_f32 =
            score_output_candidate(2, 44_100, 96_000, cpal::SampleFormat::F32).unwrap();
        let stereo_44k_f32 =
            score_output_candidate(2, 44_100, 44_100, cpal::SampleFormat::F32).unwrap();
        let ch32_192k_f32 =
            score_output_candidate(32, 192_000, 192_000, cpal::SampleFormat::F32).unwrap();
        let stereo_48k_i16 =
            score_output_candidate(2, 48_000, 48_000, cpal::SampleFormat::I16).unwrap();
        let mono_48k_f32 =
            score_output_candidate(1, 48_000, 48_000, cpal::SampleFormat::F32).unwrap();

        assert_eq!(stereo_48k_f32.1, 48_000);
        assert_eq!(stereo_44k_f32.1, 44_100);
        assert!(
            stereo_48k_f32.0 > stereo_44k_f32.0,
            "48 kHz should beat 44.1 kHz"
        );
        assert!(
            stereo_48k_f32.0 > stereo_48k_i16.0,
            "F32 should beat I16"
        );
        assert!(
            stereo_48k_f32.0 > mono_48k_f32.0,
            "Stereo should beat mono"
        );
        assert!(
            mono_48k_f32.0 > ch32_192k_f32.0,
            "Sane mono/stereo configs must beat 32-channel 192 kHz ALSA plugin configs"
        );
        assert!(score_output_candidate(0, 48_000, 48_000, cpal::SampleFormat::F32).is_none());
    }

    #[test]
    fn test_audio_buffer_size_platform_default_and_labels() {
        assert_eq!(AudioBufferSize::Auto.label(), "Auto");
        assert_eq!(AudioBufferSize::Auto.short_label(), "Auto");
        assert_eq!(AudioBufferSize::Fixed(1024).label(), "1024 frames");
        assert_eq!(AudioBufferSize::Fixed(1024).short_label(), "1024");
        #[cfg(target_os = "linux")]
        assert_eq!(AudioBufferSize::platform_default(), AudioBufferSize::Auto);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            AudioBufferSize::platform_default(),
            AudioBufferSize::Fixed(TARGET_BUFFER_FRAMES)
        );
    }
}
