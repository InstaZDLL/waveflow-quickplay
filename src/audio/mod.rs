//! QuickPlay's playback pipeline.
//!
//! The topology is the desktop engine's, reduced to what a local-file
//! player needs (see `docs/architecture/audio.md` in the WaveFlow repo):
//!
//! ```text
//! ┌─ UI thread (egui)          ┌─ Decoder thread (std)             ┌─ cpal callback (real-time)
//! │  AudioCmd ─ crossbeam ────►│  symphonia / DSD → f32            │  pop f32 from the SPSC ring
//! │  ◄─ EngineEvent ───────────│  channel-convert → rubato         │  × volume → device format
//! │  reads SharedPlayback      │  push into rtrb::Producer ───────►│  advance `samples_played`
//! └────────────────────────────┴───────────────────────────────────┴───────────────────────────
//! ```
//!
//! Why a reduced copy of the pipeline and not the engine itself: the
//! desktop engine (`crates/app/src/audio/`) is constructed from a
//! `tauri::AppHandle`, emits every state change as a Tauri event, spawns
//! its analytics task on `tauri::async_runtime`, and reads the profile
//! database for ReplayGain, crossfade, the output-device pin and the
//! exclusive-mode preference. None of that exists here. What *is* shared is
//! everything the engine itself shares with the rest of WaveFlow through
//! `waveflow-core`: the codec registry (so QuickPlay plays exactly the set
//! of formats the desktop plays, Opus included), the DSD parser and PCM
//! converter, and the same symphonia / rubato / rtrb / cpal versions.
//!
//! The contract the callback keeps is the desktop's: no allocation, no
//! locks, no logging — it pops from the ring and touches atomics.

pub mod decoder;
pub mod engine;
pub mod output;
pub mod state;
pub mod stream;

pub use engine::{AudioCmd, AudioEngine, EngineEvent};
pub use state::PlayerState;
