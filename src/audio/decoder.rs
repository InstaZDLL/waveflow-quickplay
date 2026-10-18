//! The decoder thread: blocks on the command channel when idle, owns the
//! open `ActiveStream` and the producer half of the ring while a track
//! plays, and polls commands between packets so pause / seek / stop stay
//! responsive. Reports back through [`EventSink`] rather than Tauri events.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use rtrb::Producer;

use super::engine::{AudioCmd, EngineEvent};
use super::output::RING_CAPACITY;
use super::spectrum::SpectrumAnalyzer;
use super::state::{PlayerState, SharedPlayback};
use super::stream::ActiveStream;

/// Where the decoder's events go, plus a wake-up for the UI loop (egui
/// repaints only when asked; the engine asks).
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<EngineEvent>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl EventSink {
    pub fn new(tx: Sender<EngineEvent>, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, notify }
    }

    pub fn emit(&self, event: EngineEvent) {
        let _ = self.tx.send(event);
        (self.notify)();
    }
}

/// Longest the decoder waits for the callback to empty the ring before
/// giving up on a flush — a stalled device must not hang the thread.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

/// Longest a finished track waits for its tail to play out.
const TAIL_TIMEOUT: Duration = Duration::from_secs(5);

pub fn spawn_decoder_thread(
    cmd_rx: Receiver<AudioCmd>,
    producer: Producer<f32>,
    shared: Arc<SharedPlayback>,
    events: EventSink,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("quickplay-audio-decoder".into())
        .spawn(move || {
            let mut producer = producer;
            decoder_loop(&cmd_rx, &mut producer, &shared, &events);
        })
}

fn set_state(shared: &SharedPlayback, events: &EventSink, state: PlayerState) {
    shared.set_state(state);
    events.emit(EngineEvent::State(state));
}

/// Flush and return to a clean idle: nothing queued, nothing paused, clock
/// at zero.
fn stop_playback(producer: &Producer<f32>, shared: &SharedPlayback, events: &EventSink) {
    drain_ring(producer, shared);
    shared.drain_silent.store(false, Ordering::Release);
    shared.paused_output.store(false, Ordering::Release);
    shared.restart_clock(0);
    set_state(shared, events, PlayerState::Idle);
}

/// What `play_track` hands back to the idle loop.
enum Exit {
    /// Track finished or was stopped; back to idle.
    Idle,
    /// A load arrived mid-track; process it before blocking again.
    Pending(AudioCmd),
    Shutdown,
}

fn decoder_loop(
    cmd_rx: &Receiver<AudioCmd>,
    producer: &mut Producer<f32>,
    shared: &Arc<SharedPlayback>,
    events: &EventSink,
) {
    let mut pending: Option<AudioCmd> = None;
    loop {
        let cmd = match pending.take() {
            Some(c) => c,
            None => match cmd_rx.recv() {
                Ok(c) => c,
                Err(_) => return,
            },
        };
        match cmd {
            AudioCmd::Load {
                generation,
                path,
                start_ms,
                autoplay,
            } => {
                shared.set_state(PlayerState::Loading);
                // Flush whatever the previous track left in the ring. The
                // callback only pops when not paused.
                shared.paused_output.store(false, Ordering::Release);
                drain_ring(producer, shared);
                let mut stream = match ActiveStream::open(&path) {
                    Ok(s) => s,
                    Err(message) => {
                        tracing::warn!(path = %path.display(), %message, "track failed to open");
                        shared.drain_silent.store(false, Ordering::Release);
                        set_state(shared, events, PlayerState::Idle);
                        events.emit(EngineEvent::Error {
                            generation: Some(generation),
                            message,
                        });
                        continue;
                    }
                };
                if start_ms > 0 {
                    stream.seek_ms(start_ms);
                    stream.reset_decoder();
                }
                shared.restart_clock(start_ms);
                shared.paused_output.store(!autoplay, Ordering::Release);
                shared.drain_silent.store(false, Ordering::Release);
                events.emit(EngineEvent::Loaded {
                    generation,
                    duration_ms: stream.duration_ms(),
                    codec: stream.codec().map(str::to_owned),
                });
                set_state(
                    shared,
                    events,
                    if autoplay {
                        PlayerState::Playing
                    } else {
                        PlayerState::Paused
                    },
                );
                match play_track(cmd_rx, producer, shared, events, &mut stream, generation) {
                    Exit::Idle => {}
                    Exit::Pending(cmd) => pending = Some(cmd),
                    Exit::Shutdown => return,
                }
            }
            AudioCmd::SetVolume(v) => shared.set_volume(v),
            AudioCmd::Stop => stop_playback(producer, shared, events),
            AudioCmd::Shutdown => return,
            // Nothing is loaded: these have nothing to act on.
            AudioCmd::Pause | AudioCmd::Resume | AudioCmd::Seek(_) => {}
        }
    }
}

/// Everything a command needs while a track is open.
struct Session<'a> {
    producer: &'a mut Producer<f32>,
    shared: &'a SharedPlayback,
    events: &'a EventSink,
    stream: &'a mut ActiveStream,
    /// Decoded samples not yet pushed into the ring.
    out: Vec<f32>,
    /// The stream has ended; its tail is still playing out of the ring.
    eof: Option<Instant>,
    spectrum: SpectrumAnalyzer,
}

impl Session<'_> {
    /// Hand the samples decoded since `before` to the analyser, timed to
    /// the output clock: everything pushed so far plus everything still
    /// waiting in `out` precedes them.
    fn feed_spectrum(&mut self, before: usize, rate: u32, channels: usize) {
        if self.out.len() <= before || !self.shared.spectrum_enabled.load(Ordering::Relaxed) {
            return;
        }
        let ahead = self.shared.samples_pushed.load(Ordering::Acquire) + self.out.len() as u64;
        let mut queue = self
            .shared
            .spectrum
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.spectrum
            .feed(&self.out[before..], channels, rate, ahead, &mut queue);
    }

    /// Apply one command. `Some(exit)` ends the track for the caller.
    fn handle(&mut self, cmd: AudioCmd) -> Option<Exit> {
        match cmd {
            AudioCmd::Pause => {
                self.shared.paused_output.store(true, Ordering::Release);
                set_state(self.shared, self.events, PlayerState::Paused);
                None
            }
            AudioCmd::Resume => {
                self.shared.paused_output.store(false, Ordering::Release);
                set_state(self.shared, self.events, PlayerState::Playing);
                None
            }
            AudioCmd::SetVolume(v) => {
                self.shared.set_volume(v);
                None
            }
            AudioCmd::Seek(ms) => {
                let ms = match self.stream.duration_ms() {
                    Some(d) if d > 0 => ms.min(d.saturating_sub(1)),
                    _ => ms,
                };
                // The callback does not pop while paused, so lift the pause
                // for the flush and put it back after.
                let was_paused = self.shared.paused_output.load(Ordering::Acquire);
                self.shared.paused_output.store(false, Ordering::Release);
                drain_ring(self.producer, self.shared);
                self.out.clear();
                self.spectrum.discard_pending();
                self.stream.seek_ms(ms);
                self.stream.reset_decoder();
                self.shared.restart_clock(ms);
                self.shared
                    .paused_output
                    .store(was_paused, Ordering::Release);
                self.shared.drain_silent.store(false, Ordering::Release);
                // Seeking back into a track that had reached its end.
                self.eof = None;
                None
            }
            AudioCmd::Stop => {
                stop_playback(self.producer, self.shared, self.events);
                Some(Exit::Idle)
            }
            load @ AudioCmd::Load { .. } => Some(Exit::Pending(load)),
            AudioCmd::Shutdown => Some(Exit::Shutdown),
        }
    }
}

/// Decode `stream` into the ring until it ends or a command interrupts.
fn play_track(
    cmd_rx: &Receiver<AudioCmd>,
    producer: &mut Producer<f32>,
    shared: &Arc<SharedPlayback>,
    events: &EventSink,
    stream: &mut ActiveStream,
    generation: u64,
) -> Exit {
    let dst_rate = shared.sample_rate();
    let dst_channels = shared.channels();
    let mut scratch: Vec<f32> = Vec::with_capacity(16_384);
    let mut logged_rate = false;
    let mut session = Session {
        producer,
        shared,
        events,
        stream,
        out: Vec::with_capacity(16_384),
        eof: None,
        spectrum: SpectrumAnalyzer::new(),
    };

    loop {
        // 1. Every command that is already waiting.
        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    if let Some(exit) = session.handle(cmd) {
                        return exit;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Exit::Shutdown,
            }
        }

        // 2. Paused: nothing to decode, block for the next command. Zero
        //    CPU while paused.
        if shared.paused_output.load(Ordering::Acquire) {
            if let Ok(cmd) = cmd_rx.recv_timeout(Duration::from_millis(250)) {
                if let Some(exit) = session.handle(cmd) {
                    return exit;
                }
            }
            continue;
        }

        // 3. Samples decoded earlier that did not fit: top the ring up, and
        //    when it is full come back to the command drain after a nap.
        if !session.out.is_empty() {
            let before = session.out.len();
            push_samples(session.producer, &mut session.out);
            let written = (before - session.out.len()) as u64;
            shared.samples_pushed.fetch_add(written, Ordering::AcqRel);
            if !session.out.is_empty() {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
        }

        // 4. Finished: let the ring play out before announcing the end, or
        //    the last second of the track would be cut.
        if let Some(since) = session.eof {
            if session.producer.slots() >= RING_CAPACITY || since.elapsed() > TAIL_TIMEOUT {
                set_state(shared, events, PlayerState::Ended);
                events.emit(EngineEvent::Ended { generation });
                return Exit::Idle;
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        // 5. Decode the next packet.
        let before = session.out.len();
        match session
            .stream
            .decode_next(&mut session.out, &mut scratch, dst_rate, dst_channels)
        {
            Ok(false) => {
                session.feed_spectrum(before, dst_rate, dst_channels);
                if !logged_rate && session.stream.source_sample_rate() > 0 {
                    logged_rate = true;
                    tracing::debug!(
                        src_rate = session.stream.source_sample_rate(),
                        dst_rate,
                        dst_channels,
                        "decoding"
                    );
                }
            }
            Ok(true) => session.eof = Some(Instant::now()),
            Err(message) => {
                tracing::warn!(%message, "decode failed");
                stop_playback(session.producer, shared, events);
                events.emit(EngineEvent::Error {
                    generation: Some(generation),
                    message,
                });
                return Exit::Idle;
            }
        }
    }
}

/// Push as much of `buf` into the ring as fits; what did not fit stays at
/// the front of `buf`.
fn push_samples(producer: &mut Producer<f32>, buf: &mut Vec<f32>) {
    let slots = producer.slots();
    if slots == 0 || buf.is_empty() {
        return;
    }
    let n = slots.min(buf.len());
    if let Ok(chunk) = producer.write_chunk_uninit(n) {
        let written = chunk.fill_from_iter(buf.iter().copied());
        buf.drain(..written);
    }
}

/// Flush the ring through the callback (`drain_silent`), bounded. The caller
/// clears `drain_silent` once it has reset the clock.
fn drain_ring(producer: &Producer<f32>, shared: &SharedPlayback) {
    shared.drain_silent.store(true, Ordering::Release);
    let start = Instant::now();
    while producer.slots() < RING_CAPACITY && start.elapsed() < DRAIN_TIMEOUT {
        std::thread::sleep(Duration::from_millis(2));
    }
}
