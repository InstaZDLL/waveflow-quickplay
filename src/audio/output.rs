//! The cpal output stream, hosted on its own thread.
//!
//! `cpal::Stream` is `!Send` on Windows (WASAPI / COM handles), so the
//! stream is built and kept on a dedicated thread that parks on a shutdown
//! channel for the engine's lifetime — the desktop engine's arrangement.
//! The callback pops from the SPSC ring, applies the volume and advances
//! `samples_played`; it never allocates, locks or logs.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{bounded, Receiver, Sender};
use rtrb::{Consumer, Producer, RingBuffer};

use super::state::SharedPlayback;

/// Interleaved f32 slots in the ring — ~1 s of stereo at 48 kHz, the
/// desktop figure.
pub const RING_CAPACITY: usize = 96_000;

/// Reported from the cpal error callback (a device that went away, a
/// stream the backend invalidated). The callback runs on an arbitrary
/// thread, so this is forwarded, not acted on, there.
pub type ErrorSink = Arc<dyn Fn(String) + Send + Sync>;

/// What the output thread hands back once the stream is playing.
type OpenResult = Result<(Producer<f32>, Option<String>), String>;

pub struct OutputHandle {
    shutdown_tx: Sender<()>,
    join: Option<JoinHandle<()>>,
    /// The device the stream opened on, when the backend can name it.
    pub device_name: Option<String>,
}

impl OutputHandle {
    /// Tear the stream down and wait for the thread.
    pub fn stop(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Open the OS default output device on its own thread. Returns the
/// producer half of the ring once the stream is playing, so the caller can
/// read the device layout from `shared` before spawning the decoder.
pub fn spawn_output_thread(
    shared: Arc<SharedPlayback>,
    on_error: ErrorSink,
) -> Result<(Producer<f32>, OutputHandle), String> {
    let (ring_tx, ring_rx) = bounded::<OpenResult>(1);
    let (shutdown_tx, shutdown_rx) = bounded::<()>(1);
    let join = std::thread::Builder::new()
        .name("quickplay-audio-output".into())
        .spawn(move || output_thread_main(shared, on_error, ring_tx, shutdown_rx))
        .map_err(|e| format!("spawn output thread: {e}"))?;

    match ring_rx.recv() {
        Ok(Ok((producer, device_name))) => Ok((
            producer,
            OutputHandle {
                shutdown_tx,
                join: Some(join),
                device_name,
            },
        )),
        Ok(Err(err)) => {
            let _ = join.join();
            Err(err)
        }
        Err(_) => {
            let _ = join.join();
            Err("output thread exited before opening a stream".into())
        }
    }
}

fn output_thread_main(
    shared: Arc<SharedPlayback>,
    on_error: ErrorSink,
    ring_tx: Sender<OpenResult>,
    shutdown_rx: Receiver<()>,
) {
    let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
    let built = build_stream(shared, consumer, on_error);
    let stream = match built {
        Ok((stream, device_name)) => {
            if let Err(err) = stream.play() {
                let _ = ring_tx.send(Err(format!("stream play: {err}")));
                return;
            }
            let _ = ring_tx.send(Ok((producer, device_name)));
            stream
        }
        Err(err) => {
            let _ = ring_tx.send(Err(err));
            return;
        }
    };
    // Park for the engine's lifetime; dropping `stream` closes the device.
    let _ = shutdown_rx.recv();
    drop(stream);
}

fn device_name(device: &cpal::Device) -> Option<String> {
    let desc = device.description().ok()?;
    if let Some(friendly) = desc.extended().first() {
        return Some(friendly.clone());
    }
    Some(desc.name().to_string())
}

fn build_stream(
    shared: Arc<SharedPlayback>,
    consumer: Consumer<f32>,
    on_error: ErrorSink,
) -> Result<(Stream, Option<String>), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default audio output device".to_string())?;
    let name = device_name(&device);
    let default_cfg = device
        .default_output_config()
        .map_err(|e| format!("default output config: {e}"))?;
    let sample_format = default_cfg.sample_format();
    let channels = default_cfg.channels();
    let sample_rate = default_cfg.sample_rate();
    let config: StreamConfig = default_cfg.into();

    // Stamp the device layout before the decoder exists.
    shared.sample_rate.store(sample_rate, Ordering::Release);
    shared.channels.store(channels, Ordering::Release);
    tracing::info!(sample_rate, channels, ?sample_format, device = ?name, "output stream opened");

    let stream = match sample_format {
        SampleFormat::F32 => open_stream::<f32>(&device, &config, consumer, shared, on_error),
        SampleFormat::I16 => open_stream::<i16>(&device, &config, consumer, shared, on_error),
        SampleFormat::U16 => open_stream::<u16>(&device, &config, consumer, shared, on_error),
        SampleFormat::I32 => open_stream::<i32>(&device, &config, consumer, shared, on_error),
        SampleFormat::I8 => open_stream::<i8>(&device, &config, consumer, shared, on_error),
        SampleFormat::I24 => open_stream::<cpal::I24>(&device, &config, consumer, shared, on_error),
        SampleFormat::I64 => open_stream::<i64>(&device, &config, consumer, shared, on_error),
        SampleFormat::U8 => open_stream::<u8>(&device, &config, consumer, shared, on_error),
        SampleFormat::U24 => open_stream::<cpal::U24>(&device, &config, consumer, shared, on_error),
        SampleFormat::U32 => open_stream::<u32>(&device, &config, consumer, shared, on_error),
        SampleFormat::U64 => open_stream::<u64>(&device, &config, consumer, shared, on_error),
        SampleFormat::F64 => open_stream::<f64>(&device, &config, consumer, shared, on_error),
        other => Err(format!("unsupported sample format: {other:?}")),
    }?;
    Ok((stream, name))
}

fn open_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: Consumer<f32>,
    shared: Arc<SharedPlayback>,
    on_error: ErrorSink,
) -> Result<Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32> + Send + 'static,
{
    let err_fn = move |err: cpal::StreamError| on_error(format!("audio device error: {err}"));
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _info: &cpal::OutputCallbackInfo| {
                // Drain before pause: a flush requested while paused must
                // empty the ring, or Stop from Paused would wait out the
                // timeout and then play the stale samples.
                if shared.drain_silent.load(Ordering::Acquire) {
                    while consumer.pop().is_ok() {}
                    for slot in out.iter_mut() {
                        *slot = T::from_sample(0.0_f32);
                    }
                    return;
                }
                if shared.paused_output.load(Ordering::Acquire) {
                    for slot in out.iter_mut() {
                        *slot = T::from_sample(0.0_f32);
                    }
                    return;
                }
                let volume = shared.volume();
                let mut written: u64 = 0;
                for slot in out.iter_mut() {
                    *slot = match consumer.pop() {
                        Ok(s) => {
                            written += 1;
                            T::from_sample(s * volume)
                        }
                        Err(_) => T::from_sample(0.0_f32),
                    };
                }
                if written > 0 {
                    shared.samples_played.fetch_add(written, Ordering::AcqRel);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("build output stream: {e}"))
}
