//! QuickPlay's playback pipeline.
//!
//! The topology is the desktop engine's, reduced to what a local-file
//! player needs (see `docs/architecture/audio.md` in the WaveFlow repo):
//!
//! ```text
//! ┌─ UI thread (egui)          ┌─ Decoder thread (std)             ┌─ cpal callback (real-time)
//! │  AudioCmd ─ crossbeam ────►│  waveflow_core::playback          │  pop f32 from the SPSC ring
//! │  ◄─ EngineEvent ───────────│  decode → channels → resample     │  × volume → device format
//! │  reads SharedPlayback      │  push into rtrb::Producer ───────►│  advance `samples_played`
//! └────────────────────────────┴───────────────────────────────────┴───────────────────────────
//! ```
//!
//! The decoding itself — open a file, decode it to interleaved `f32`, fit
//! its channels to the output's, resample it to the output's rate, seek —
//! is `waveflow_core::playback::stream::ActiveStream`, the code the desktop
//! engine plays through (the crate's `playback` feature). A fix made there
//! reaches QuickPlay by bumping the pinned `rev`.
//!
//! Why the transport around it is QuickPlay's own and not the engine's:
//! the desktop engine (`crates/app/src/audio/`) is constructed from a
//! `tauri::AppHandle`, emits every state change as a Tauri event, spawns
//! its analytics task on `tauri::async_runtime`, and reads the profile
//! database for ReplayGain, crossfade, the output-device pin and the
//! exclusive-mode preference. None of that exists here.
//!
//! The contract the callback keeps is the desktop's: no allocation, no
//! locks, no logging — it pops from the ring and touches atomics.

pub mod decoder;
pub mod engine;
pub mod output;
pub mod spectrum;
pub mod state;

pub use engine::{AudioCmd, AudioEngine, EngineEvent};
pub use state::PlayerState;
