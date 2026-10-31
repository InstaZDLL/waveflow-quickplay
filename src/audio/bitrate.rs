//! The bitrate of what is playing, measured from the compressed packets
//! the decoder reads (`ActiveStream::packets_read`) rather than read from
//! the tags. A tag's figure is the whole file's average: a FLAC that
//! opens on a quiet passage shows the same number through its loud
//! middle. Each reading covers a second of audio and is due when the last
//! of that second plays, timed to the output clock as the spectrum frames
//! are.

use std::collections::VecDeque;

use waveflow_core::playback::stream::PacketTally;

/// Audio one reading covers, in milliseconds. Shorter jumps from frame to
/// frame (a FLAC frame of silence weighs a few bytes); longer lags behind
/// the music.
const WINDOW_MS: u64 = 1000;

/// Readings kept waiting for their play position: the ring holds a second
/// of stereo at 48 kHz but 12 s of mono at 8 kHz.
const MAX_QUEUED: usize = 32;

/// Sums the packets the decoder reads until they hold a window of audio.
pub struct BitrateMeter {
    /// The stream's totals when the current window started.
    start: PacketTally,
}

impl BitrateMeter {
    pub fn new(now: PacketTally) -> Self {
        Self { start: now }
    }

    /// Start the next window here: after a seek, the packets read before
    /// it are not what plays next.
    pub fn restart(&mut self, now: PacketTally) {
        self.start = now;
    }

    /// The kbps of the packets read since the window started, once they
    /// hold [`WINDOW_MS`] of audio at `rate`; the next window starts there.
    pub fn read(&mut self, now: PacketTally, rate: u32) -> Option<u32> {
        let frames = now.frames.saturating_sub(self.start.frames);
        let rate = u64::from(rate);
        if rate == 0 || frames == 0 || frames * 1000 < WINDOW_MS * rate {
            return None;
        }
        let bytes = now.bytes.saturating_sub(self.start.bytes);
        self.start = now;
        // bytes · 8 bits over frames / rate seconds, in thousands, rounded.
        let kbps = (u128::from(bytes) * 8 * u128::from(rate) + u128::from(frames) * 500)
            / (u128::from(frames) * 1000);
        u32::try_from(kbps).ok()
    }
}

/// Readings the decoder has taken and the UI has not shown yet. Decoder
/// and UI threads only, never the audio callback: a mutex is fine.
#[derive(Default)]
pub struct BitrateQueue {
    /// `(due_samples, kbps)`, due in the unit of
    /// `SharedPlayback::samples_played`.
    readings: VecDeque<(u64, u32)>,
}

impl BitrateQueue {
    pub fn push(&mut self, due_samples: u64, kbps: u32) {
        if self.readings.len() >= MAX_QUEUED {
            self.readings.pop_front();
        }
        self.readings.push_back((due_samples, kbps));
    }

    /// The newest reading whose time has come at `played`, dropping the
    /// older ones it supersedes.
    pub fn take_due(&mut self, played: u64) -> Option<u32> {
        let mut latest = None;
        while let Some(&(due, kbps)) = self.readings.front() {
            if due > played {
                break;
            }
            self.readings.pop_front();
            latest = Some(kbps);
        }
        latest
    }

    pub fn clear(&mut self) {
        self.readings.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(bytes: u64, frames: u64) -> PacketTally {
        PacketTally { bytes, frames }
    }

    #[test]
    fn a_second_of_packets_gives_its_bitrate() {
        let mut meter = BitrateMeter::new(tally(1_000, 4_096));
        // Half a second at 44.1 kHz: not a reading yet.
        assert_eq!(meter.read(tally(50_000, 4_096 + 22_050), 44_100), None);
        // A second: 196 750 bytes is 1 574 kbps.
        assert_eq!(
            meter.read(tally(197_750, 4_096 + 44_100), 44_100),
            Some(1_574)
        );
        // The next window starts where that one ended: a quiet second.
        assert_eq!(
            meter.read(tally(197_750 + 8_000, 4_096 + 88_200), 44_100),
            Some(64)
        );
    }

    #[test]
    fn a_seek_starts_the_window_afresh() {
        let mut meter = BitrateMeter::new(tally(0, 0));
        assert_eq!(meter.read(tally(100_000, 30_000), 48_000), None);
        // The 100 000 bytes read before the seek do not count after it.
        meter.restart(tally(100_000, 30_000));
        assert_eq!(
            meter.read(tally(100_000 + 32_000, 30_000 + 48_000), 48_000),
            Some(256)
        );
    }

    #[test]
    fn nothing_is_read_before_the_rate_is_known() {
        let mut meter = BitrateMeter::new(tally(0, 0));
        assert_eq!(meter.read(tally(500_000, 96_000), 0), None);
    }

    #[test]
    fn a_reading_waits_for_its_play_position() {
        let mut queue = BitrateQueue::default();
        queue.push(96_000, 900);
        queue.push(192_000, 1_500);
        assert_eq!(queue.take_due(50_000), None);
        assert_eq!(queue.take_due(100_000), Some(900));
        // Both due at once: the newer one wins.
        queue.push(288_000, 700);
        assert_eq!(queue.take_due(300_000), Some(700));
        assert_eq!(queue.take_due(400_000), None);
    }
}
