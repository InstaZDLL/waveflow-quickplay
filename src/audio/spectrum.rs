//! Real-time spectrum analyser for the bar visualizer — the desktop
//! engine's `audio/spectrum.rs` without the Tauri emit.
//!
//! Lives on the decoder thread, fed with the samples about to be pushed
//! into the ring. Mixes to mono, Hann-windows 4096 frames, runs a real FFT
//! and buckets the magnitudes into log-spaced bands scaled against the
//! track's own recent level.
//!
//! **Timed to the output.** The decoder runs up to a ring's length ahead
//! of what is heard, so every frame carries the output sample count at
//! which its window's middle is played; the UI holds it until the
//! callback's `samples_played` gets there.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Frames analysed per FFT pass: a 10.8 Hz bin at 44.1 kHz, fine enough
/// that the bass bands read distinct bins.
const FFT_SIZE: usize = 4096;
/// How far the window slides between two passes.
const HOP: usize = 1024;
/// How fast the level reference follows the music, up and down.
const LEVEL_ATTACK: Duration = Duration::from_millis(250);
const LEVEL_RELEASE: Duration = Duration::from_secs(3);
/// The level reference never drops below this, so silence is not
/// amplified into noise.
const MIN_LEVEL: f32 = 40.0;
/// Room above the reference: bars top out on peaks, not on the average.
const HEADROOM: f32 = 1.15;
/// Log-spaced bands between [`MIN_HZ`] and [`MAX_HZ`].
pub const BAND_COUNT: usize = 32;
const MIN_HZ: f32 = 30.0;
const MAX_HZ: f32 = 16_000.0;
/// Audio between two emitted frames (~30 Hz).
const EMIT_INTERVAL: Duration = Duration::from_millis(33);
/// Frames kept waiting for their play position. The decoder runs up to a
/// ring ahead of the output, and the ring is a fixed count of interleaved
/// samples: a second of stereo at 48 kHz, but 12 s of mono at 8 kHz, which
/// is 360 frames at 30 Hz. Dropping the oldest frames because the queue
/// is too short would drop the ones due soonest and freeze the bars; a
/// seek clears the queue anyway.
const MAX_QUEUED_FRAMES: usize = 512;

/// One analysed window, due when the output clock reaches `due_samples`
/// (interleaved samples, the unit of `SharedPlayback::samples_played`).
#[derive(Debug, Clone)]
pub struct SpectrumFrame {
    pub due_samples: u64,
    pub bands: [f32; BAND_COUNT],
}

/// Frames the decoder has analysed and the UI has not shown yet. Shared
/// between the decoder thread and the UI thread only — never touched by
/// the audio callback, so a mutex is fine.
#[derive(Default)]
pub struct SpectrumQueue {
    frames: VecDeque<SpectrumFrame>,
}

impl SpectrumQueue {
    pub fn push(&mut self, frame: SpectrumFrame) {
        if self.frames.len() >= MAX_QUEUED_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// The newest frame whose time has come at `played`, dropping the
    /// older ones it supersedes.
    pub fn take_due(&mut self, played: u64) -> Option<[f32; BAND_COUNT]> {
        let mut latest = None;
        while let Some(front) = self.frames.front() {
            if front.due_samples > played {
                break;
            }
            latest = self.frames.pop_front().map(|f| f.bands);
        }
        latest
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

pub struct SpectrumAnalyzer {
    plan: Arc<dyn RealToComplex<f32>>,
    /// Mono-mixed samples accumulating until a window is full.
    pending: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    bands: [f32; BAND_COUNT],
    raw: [f32; BAND_COUNT],
    level: f32,
    /// Audio slid through the window since the last emitted frame, less
    /// the emission interval's carry: when the next frame is due.
    since_emit: usize,
    /// Audio slid through the window since the last analysed frame: the
    /// time the level follower moves by.
    since_frame: usize,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let plan = planner.plan_fft_forward(FFT_SIZE);
        let scratch = plan.make_scratch_vec();
        let spectrum = plan.make_output_vec();
        Self {
            plan,
            pending: Vec::with_capacity(FFT_SIZE * 2),
            input: vec![0.0; FFT_SIZE],
            spectrum,
            window: hann_window(FFT_SIZE),
            scratch,
            bands: [0.0; BAND_COUNT],
            raw: [0.0; BAND_COUNT],
            level: MIN_LEVEL,
            since_emit: usize::MAX,
            since_frame: 0,
        }
    }

    /// After a seek: the next frame is built from the new position alone.
    /// The level reference is kept — same track.
    pub fn discard_pending(&mut self) {
        self.pending.clear();
        self.since_emit = usize::MAX;
    }

    /// Feed the interleaved samples just decoded. `ahead_samples` is how
    /// many interleaved samples precede the end of this buffer on the
    /// output clock: everything already pushed into the ring plus whatever
    /// is still waiting to be pushed, this buffer included.
    pub fn feed(
        &mut self,
        samples: &[f32],
        channels: usize,
        sample_rate: u32,
        ahead_samples: u64,
        out: &mut SpectrumQueue,
    ) {
        if channels == 0 || sample_rate == 0 || samples.is_empty() {
            return;
        }
        let rate = sample_rate as f32;
        for frame in samples.chunks_exact(channels) {
            let sum: f32 = frame.iter().sum();
            self.pending.push(sum / channels as f32);
        }

        let interval = ((rate * EMIT_INTERVAL.as_secs_f32()) as usize).max(1);
        while self.pending.len() >= FFT_SIZE {
            if self.since_emit >= interval {
                let first = self.since_emit == usize::MAX;
                let audio = if first { interval } else { self.since_frame };
                let elapsed = Duration::from_secs_f32((audio as f32 / rate).min(1.0));
                self.run_one_frame(rate, elapsed);
                self.since_frame = 0;
                // The window's middle sits this many mono frames before
                // the newest sample fed.
                let behind = (self.pending.len() as u64).saturating_sub(FFT_SIZE as u64 / 2);
                let due_samples = ahead_samples.saturating_sub(behind * channels as u64);
                out.push(SpectrumFrame {
                    due_samples,
                    bands: self.bands,
                });
                self.since_emit = if first {
                    0
                } else {
                    (self.since_emit - interval).min(interval)
                };
            }
            let slid = HOP.min(self.pending.len());
            self.pending.drain(..slid);
            self.since_emit = self.since_emit.saturating_add(slid);
            self.since_frame = self.since_frame.saturating_add(slid);
        }
    }

    fn run_one_frame(&mut self, sample_rate: f32, elapsed: Duration) {
        for (i, dst) in self.input.iter_mut().enumerate() {
            *dst = self.pending[i] * self.window[i];
        }
        if let Err(err) =
            self.plan
                .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
        {
            tracing::warn!(?err, "spectrum FFT failed");
            return;
        }
        band_magnitudes(&self.spectrum, sample_rate, &mut self.raw);
        let frame_peak = self.raw.iter().copied().fold(0.0f32, f32::max);
        self.level = follow_level(self.level, frame_peak, elapsed);
        scale_bands(&self.raw, self.level, &mut self.bands);
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn hann_window(n: usize) -> Vec<f32> {
    use std::f32::consts::PI;
    let denom = (n - 1) as f32;
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / denom).cos()))
        .collect()
}

/// One-pole follower: quick up, slow down, never below [`MIN_LEVEL`].
fn follow_level(level: f32, frame_peak: f32, elapsed: Duration) -> f32 {
    let tau = if frame_peak > level {
        LEVEL_ATTACK
    } else {
        LEVEL_RELEASE
    };
    let alpha = 1.0 - (-elapsed.as_secs_f32() / tau.as_secs_f32()).exp();
    (level + (frame_peak - level) * alpha).max(MIN_LEVEL)
}

/// Peak bin inside each log-spaced band, or an interpolated read at the
/// band's centre when the band is narrower than a bin.
fn band_magnitudes(spectrum: &[Complex<f32>], sample_rate: f32, bands: &mut [f32]) {
    let bin_count = spectrum.len();
    if bin_count == 0 {
        bands.fill(0.0);
        return;
    }
    let bin_hz = sample_rate / (FFT_SIZE as f32);
    let log_min = MIN_HZ.ln();
    let log_max = MAX_HZ.ln();
    let band_count = bands.len();
    for (b, band) in bands.iter_mut().enumerate() {
        let lo_hz = (log_min + (log_max - log_min) * b as f32 / band_count as f32).exp();
        let hi_hz = (log_min + (log_max - log_min) * (b + 1) as f32 / band_count as f32).exp();
        let lo = lo_hz / bin_hz;
        let hi = hi_hz / bin_hz;
        if hi - lo < 1.0 {
            let centre = (lo * hi).sqrt();
            let i = (centre.floor() as usize).min(bin_count - 1);
            let j = (i + 1).min(bin_count - 1);
            let t = centre - i as f32;
            *band = spectrum[i].norm() * (1.0 - t) + spectrum[j].norm() * t;
            continue;
        }
        let lo_bin = lo.ceil() as usize;
        let hi_bin = (hi.floor() as usize + 1).max(lo_bin + 1).min(bin_count);
        let mut peak_sq = 0.0f32;
        for bin in spectrum.iter().take(hi_bin).skip(lo_bin) {
            peak_sq = peak_sq.max(bin.norm_sqr());
        }
        *band = peak_sq.sqrt();
    }
}

/// Squash raw magnitudes into 0..1 against the level reference: a soft
/// knee above it, a small floor below, a square-root curve in between.
fn scale_bands(raw: &[f32], level: f32, bands: &mut [f32]) {
    const FLOOR: f32 = 0.02;
    let reference = level.max(MIN_LEVEL) * HEADROOM;
    for (band, &mag) in bands.iter_mut().zip(raw) {
        let normalised = soft_limit((mag / reference).max(0.0));
        let cut = (normalised - FLOOR).max(0.0) / (1.0 - FLOOR);
        *band = cut.sqrt();
    }
}

const KNEE: f32 = 0.8;

fn soft_limit(x: f32) -> f32 {
    if x <= KNEE {
        return x;
    }
    let room = 1.0 - KNEE;
    let over = (x - KNEE) / room;
    KNEE + room * over / (1.0 + over)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_is_symmetric_and_unit_peak() {
        let w = hann_window(FFT_SIZE);
        assert!((w[FFT_SIZE / 2] - 1.0).abs() < 1e-3);
        assert!((w[1] - w[FFT_SIZE - 2]).abs() < 1e-6);
    }

    #[test]
    fn silence_produces_zero_bands_due_after_the_ring() {
        let mut an = SpectrumAnalyzer::new();
        let mut q = SpectrumQueue::default();
        an.feed(&vec![0.0; FFT_SIZE * 2], 2, 44_100, 96_000, &mut q);
        let frame = q.take_due(u64::MAX).expect("a frame");
        assert!(frame.iter().all(|b| *b == 0.0));
    }

    #[test]
    fn a_tone_lands_in_one_band_and_frames_wait_for_their_time() {
        let mut an = SpectrumAnalyzer::new();
        let mut q = SpectrumQueue::default();
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..FFT_SIZE * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin())
            .collect();
        an.feed(&samples, 1, rate, 10_000, &mut q);
        assert!(
            q.take_due(0).is_none(),
            "nothing is due before its position"
        );
        let frame = q.take_due(u64::MAX).expect("a frame");
        let peak = frame
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .expect("band");
        // 1 kHz on a 30 Hz → 16 kHz log ladder of 32 bands is band ~17.
        assert!((15..=19).contains(&peak), "peak band {peak}");
    }

    #[test]
    fn a_full_ring_of_low_rate_mono_keeps_the_frames_due_first() {
        // 96 000 samples of 8 kHz mono: 12 s ahead, 360 frames at 30 Hz.
        let mut q = SpectrumQueue::default();
        for i in 0..360u64 {
            q.push(SpectrumFrame {
                due_samples: i * 266,
                bands: [i as f32; BAND_COUNT],
            });
        }
        assert_eq!(q.take_due(0).map(|b| b[0]), Some(0.0));
        assert_eq!(q.take_due(266).map(|b| b[0]), Some(1.0));
    }

    #[test]
    fn the_soft_limit_is_continuous_at_the_knee() {
        assert!((soft_limit(KNEE) - KNEE).abs() < 1e-6);
        assert!(soft_limit(10.0) < 1.0);
        assert!(soft_limit(10.0) > soft_limit(2.0));
    }
}
