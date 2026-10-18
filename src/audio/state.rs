//! Lock-free state shared by the UI thread, the decoder thread and the
//! real-time cpal callback. Every field is an atomic because the callback
//! must never take a lock. Same layout rules as the desktop engine's
//! `SharedPlayback`: `samples_played` is the single source of truth for the
//! position, it counts only samples really popped from the ring (never the
//! silence written on an underrun), and only the decoder resets it.

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

/// High-level player lifecycle, stored as an `AtomicU8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerState {
    Idle = 0,
    Loading = 1,
    Playing = 2,
    Paused = 3,
    Ended = 4,
}

pub struct SharedPlayback {
    pub state: AtomicU8,
    /// Interleaved samples the output has pulled from the ring since the
    /// clock was last restarted.
    pub samples_played: AtomicU64,
    /// Device rate and channel count, written once when the stream opens.
    pub sample_rate: AtomicU32,
    pub channels: AtomicU16,
    /// `f32` in `[0, 1]` stored via `to_bits`.
    pub volume_bits: AtomicU32,
    /// Position at the last load / seek, added to the delta derived from
    /// `samples_played`.
    pub base_offset_ms: AtomicU64,
    /// Callback writes silence and does **not** pop: pause is instant even
    /// with a second of audio buffered, and resume picks up exactly there.
    pub paused_output: AtomicBool,
    /// Callback bulk-pops the ring and writes silence: a load or a seek
    /// flushes the old position so its tail never reaches the device.
    pub drain_silent: AtomicBool,
}

impl SharedPlayback {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(PlayerState::Idle as u8),
            samples_played: AtomicU64::new(0),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU16::new(0),
            volume_bits: AtomicU32::new(1.0_f32.to_bits()),
            base_offset_ms: AtomicU64::new(0),
            paused_output: AtomicBool::new(false),
            drain_silent: AtomicBool::new(false),
        }
    }

    pub fn set_state(&self, state: PlayerState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        let v = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.volume_bits.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Acquire)
    }

    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Acquire).max(1) as usize
    }

    /// Current playback position. Cheap enough to read every frame.
    pub fn position_ms(&self) -> u64 {
        let rate = self.sample_rate() as u64;
        let channels = self.channels() as u64;
        let base = self.base_offset_ms.load(Ordering::Acquire);
        if rate == 0 {
            return base;
        }
        let played = self.samples_played.load(Ordering::Acquire) / channels;
        base + played.saturating_mul(1000) / rate
    }

    /// Reset the clock to `ms` — only the decoder calls this, after the
    /// ring has been drained, so no stale sample can be counted against the
    /// new origin.
    pub fn restart_clock(&self, ms: u64) {
        self.base_offset_ms.store(ms, Ordering::Release);
        self.samples_played.store(0, Ordering::Release);
    }
}

impl Default for SharedPlayback {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_derives_from_samples_and_base_offset() {
        let s = SharedPlayback::new();
        s.sample_rate.store(48_000, Ordering::Release);
        s.channels.store(2, Ordering::Release);
        s.restart_clock(10_000);
        // 1 s of stereo at 48 kHz.
        s.samples_played.store(96_000, Ordering::Release);
        assert_eq!(s.position_ms(), 11_000);
    }

    #[test]
    fn volume_is_clamped_and_nan_safe() {
        let s = SharedPlayback::new();
        s.set_volume(1.7);
        assert_eq!(s.volume(), 1.0);
        s.set_volume(-0.2);
        assert_eq!(s.volume(), 0.0);
        s.set_volume(f32::NAN);
        assert_eq!(s.volume(), 1.0);
    }
}
