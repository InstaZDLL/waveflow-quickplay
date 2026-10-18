//! The handle the UI owns: sends commands, polls events, reads the shared
//! position, and releases the device + threads on drop.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Receiver, Sender};

use super::decoder::{spawn_decoder_thread, EventSink};
use super::output::{spawn_output_thread, OutputHandle};
use super::state::{PlayerState, SharedPlayback};

/// Commands accepted by the decoder thread.
#[derive(Debug)]
pub enum AudioCmd {
    Load {
        /// Monotonic token stamped on the events this load produces, so
        /// the UI can ignore the tail of a load it has since replaced.
        generation: u64,
        path: PathBuf,
        start_ms: u64,
        autoplay: bool,
    },
    Pause,
    Resume,
    Stop,
    Seek(u64),
    SetVolume(f32),
    Shutdown,
}

/// What the engine reports back. Delivered on a channel the UI drains once
/// per frame; every emit also wakes the UI loop.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    Loaded {
        generation: u64,
        duration_ms: Option<u64>,
        codec: Option<String>,
    },
    State(PlayerState),
    Ended {
        generation: u64,
    },
    Error {
        generation: Option<u64>,
        message: String,
    },
    /// The output device reported a problem (unplugged, invalidated).
    Output(String),
}

pub struct AudioEngine {
    cmd_tx: Sender<AudioCmd>,
    events_rx: Receiver<EngineEvent>,
    shared: Arc<SharedPlayback>,
    output: Option<OutputHandle>,
    decoder: Option<JoinHandle<()>>,
}

impl AudioEngine {
    /// Open the default output device and start the decoder thread. `notify`
    /// is called from the audio threads whenever an event is queued (the UI
    /// passes `egui::Context::request_repaint`).
    pub fn new(notify: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let shared = Arc::new(SharedPlayback::new());
        let (events_tx, events_rx) = unbounded::<EngineEvent>();
        let sink = EventSink::new(events_tx, notify);

        let error_sink = sink.clone();
        let (producer, output) = spawn_output_thread(
            shared.clone(),
            Arc::new(move |message: String| {
                tracing::warn!(%message, "output stream error");
                error_sink.emit(EngineEvent::Output(message));
            }),
        )?;

        let (cmd_tx, cmd_rx) = unbounded::<AudioCmd>();
        let decoder = spawn_decoder_thread(cmd_rx, producer, shared.clone(), sink)
            .map_err(|e| format!("spawn decoder thread: {e}"))?;

        Ok(Self {
            cmd_tx,
            events_rx,
            shared,
            output: Some(output),
            decoder: Some(decoder),
        })
    }

    pub fn send(&self, cmd: AudioCmd) {
        if self.cmd_tx.send(cmd).is_err() {
            tracing::warn!("decoder thread is gone; command dropped");
        }
    }

    pub fn shared(&self) -> &SharedPlayback {
        &self.shared
    }

    pub fn device_name(&self) -> Option<&str> {
        self.output.as_ref().and_then(|o| o.device_name.as_deref())
    }

    /// Everything queued since the last call.
    pub fn poll_events(&self) -> Vec<EngineEvent> {
        self.events_rx.try_iter().collect()
    }

    /// Stop the decoder, close the device, join both threads.
    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(AudioCmd::Shutdown);
        if let Some(join) = self.decoder.take() {
            let _ = join.join();
        }
        if let Some(output) = self.output.take() {
            output.stop();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
