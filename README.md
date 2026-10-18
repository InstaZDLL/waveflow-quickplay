# WaveFlow QuickPlay

A standalone, lightweight audio player for WaveFlow — a native egui/eframe
window that plays local files without launching the Tauri desktop app.
Experimental: its purpose is to evaluate whether egui + eframe is a practical
companion to WaveFlow Desktop.

```text
waveflow-quickplay "C:\Music\song.flac"
waveflow-quickplay song1.flac song2.mp3 song3.m4a
waveflow-quickplay "C:\Music\Some Album"      # folders are walked (4 levels)
```

## Build and run

Requirements: the WaveFlow repository checked out next to this folder
(`../WaveFlow`), Rust 1.98 (pinned by `rust-toolchain.toml`) and cmake (for
the vendored libopus that `waveflow-core` builds, as the desktop app does).

```bash
cargo run --release -- <files or folders>
cargo build --release          # → target/release/waveflow-quickplay(.exe)

cargo fmt --check
cargo clippy --all-targets
cargo test
```

`RUST_LOG=debug` turns the log up (stderr). `QUICKPLAY_CAPTURE_DIR=<dir>` writes a
PNG of the player window (`player.png`) 2 s after start (or
`QUICKPLAY_CAPTURE_AFTER_MS`), for testing on displays that cannot be grabbed
from outside such as GNOME on Wayland. Preferences live in
`<config dir>/waveflow-quickplay/settings.json` (`%APPDATA%` on Windows,
`~/.config` on Linux) — a namespace of its own, never read by the desktop
app.

### Keyboard

| Key                | Action                       |
| ------------------ | ---------------------------- |
| Space              | Play / pause                 |
| N / P              | Next / previous              |
| ← / → (Shift: 30 s) | Seek ±5 s                    |
| ↑ / ↓              | Volume ±5 %                  |
| A                  | Show / hide the artwork      |
| L                  | Show / hide the playlist     |
| S (or click it)    | Show / hide the spectrum     |
| T                  | Always on top                |
| Ctrl+O             | Open files                   |
| Delete (playlist)  | Remove the selected track    |

Files and folders can be dropped on either window; double-click a playlist
row to play it; right-click for Play / Remove.

### Lyrics

A `song.lrc` next to `song.flac` is read when the track starts and its
timed lines replace the title in the controls block as the song plays (the
window title keeps the track name). An empty timed line, or the gap before
the first one, shows the title again. `[offset:]` and enhanced-LRC word
stamps are honoured; nothing is fetched from the network.

## Architecture

```text
WaveFlow/src-tauri/crates/core   (waveflow-core, path dependency)
   ├── audio_format::opus::codecs()   the one codec registry (symphonia + libopus)
   ├── audio_format::dsd              DSF / DFF parser, 1-bit → PCM converter, tags
   └── scanner::extract               AUDIO_EXTENSIONS, is_scannable_audio,
                                      read_tagged_file, probe_playable_stream, file_type_label

waveflow-quickplay
   ├── src/main.rs        CLI, logging, eframe window
   ├── src/app.rs         QuickPlayApp: queue + engine + loader + window state, applies UI actions
   ├── src/audio/         the playback pipeline (see below)
   ├── src/library.rs     tags / duration / cover reads on two worker threads
   ├── src/lyrics.rs      .lrc sidecar parser, line in force at a position
   ├── src/platform.rs    Windows: owner link between the playlist window and the player
   ├── src/queue.rs       the playlist model
   ├── src/config.rs      settings.json
   └── src/ui/            theme, painted icons, widgets, player pane, playlist window
```

### Playback pipeline

The desktop engine (`crates/app/src/audio/`, ~10 000 lines across
`engine.rs`, `decoder.rs`, `output.rs`) is built from a `tauri::AppHandle`:
every state change is a Tauri event, the analytics task runs on
`tauri::async_runtime` and writes `play_event` rows through the profile
pool, and ReplayGain / crossfade / the output pin / exclusive mode are read
from the profile database. It cannot be instantiated without a Tauri
runtime, which QuickPlay must not start.

QuickPlay therefore carries a reduced pipeline (`src/audio/`, ~700 lines)
with the desktop's topology and invariants:

```text
UI thread (egui)  ──AudioCmd (crossbeam)──►  decoder thread (std)  ──f32 (rtrb SPSC)──►  cpal callback
                  ◄──EngineEvent + repaint──  symphonia / DSD → convert_channels → rubato
                  reads SharedPlayback atomics (position, state, volume)
```

- the cpal callback never allocates, locks or logs; it pops the ring,
  applies the volume and advances `samples_played`, the only position clock;
- `paused_output` (silence, no pop) and `drain_silent` (bulk pop + silence)
  give instant pause and clean seeks, as in the desktop engine;
- streams open through `waveflow_core::audio_format::opus::codecs()`, so
  QuickPlay plays exactly the formats the desktop plays, Opus included
  (MP3, FLAC, WAV, AIFF, Vorbis, Opus, AAC, ALAC, DSF, DFF);
- a track ends only once its tail has left the ring; the UI then loads the
  next queue entry;
- the engine is created lazily on the first load and released (decoder
  joined, stream dropped) on exit;
- the spectrum analyser (`src/audio/spectrum.rs`, the desktop's FFT and
  band ladder without the Tauri emit) runs on the decoder thread and
  timestamps every frame with the output position it belongs to, so the
  bars follow what is heard rather than what is decoded.

What is deliberately **not** here: crossfade, gapless, ReplayGain, EQ,
exclusive output, device selection, radio / remote sources, analytics.

### Windows

The main window has a custom title bar (drag, pin, minimize, close) and
resize edges — `with_decorations(false)` plus `ViewportCommand::StartDrag`
/ `BeginResize`. Compact mode locks the height to the controls block;
artwork mode adds the cover above it and makes the window freely
resizable. The playlist is a second native window (`show_viewport_immediate`)
docked beside the player, as PotPlayer's playlist is: it keeps its offset
from the player's top-right corner when the player is moved or widened
(drag the playlist itself to change that offset), it minimizes and restores
with the player (`-` button, taskbar or Win+D), and it has no taskbar button
of its own. On Windows the playlist is made an *owned* window of the player
(`src/platform.rs`, the one native call egui does not wrap), which is what
gives the pair a single taskbar entry and a shared z-order; elsewhere the
docking and minimize mirroring are done from the app and the window only
skips the taskbar. Where the backend cannot open a second window at all,
egui embeds it as a floating panel instead.

## Measurements

Windows 11, release profile (`opt-level = 3`, thin LTO, stripped), 2026-10-18,
playing a 48 kHz AAC file resampled to a 44.1 kHz 7.1 output. No comparison
with the Tauri desktop app was made, so no claim is made about it.

| Metric                                   | Value   | How                                                        |
| ---------------------------------------- | ------- | ---------------------------------------------------------- |
| Process start → first frame              | 276 ms  | `window ready elapsed_ms=…` in the log (Instant at main)    |
| Working set, playing with cover shown    | 88 MB   | `Get-Process` WorkingSet64 / PrivateMemorySize64 (85 MB)   |
| CPU while playing, spectrum hidden       | 3.8 %   | TotalProcessorTime delta over 10 s, one core = 100 %        |
| CPU while playing, spectrum shown        | 11.4 %  | same; the visualizer repaints the window at ~30 Hz          |
| CPU while paused                         | 0.0 %   | same, egui repaints only on input                           |
| Release binary                           | 30.7 MB | `target/release/waveflow-quickplay.exe` (16.4 MB of it is the embedded Noto Sans CJK, 0.8 MB Inter) |

Fedora 44, GNOME 50.5 on Wayland (also run under XWayland), same build
profile, same file, stereo 44.1 kHz output, 2026-10-18:

| Metric                                   | Value   | How                                                        |
| ---------------------------------------- | ------- | ---------------------------------------------------------- |
| Process start → first frame              | 62 ms   | `window ready elapsed_ms=…` (66 ms under XWayland)          |
| Resident set, playing with cover shown   | 92 MB   | `/proc/<pid>/status` RSS (96 MB under XWayland)             |
| CPU while playing, spectrum shown        | 6.8 %   | utime+stime delta over 10 s, one core = 100 %               |
| Release binary                           | 33.1 MB | `target/release/waveflow-quickplay`                         |
| Clean release build                      | 2 min 42 s | 8 cores, cmake + libopus included                        |

Verified there: rendering (fonts, cover, spectrum) identical to Windows,
Opus / AAC / FLAC playback, the playlist window opening beside the player,
and under XWayland the playlist following the player when it is moved
(`wmctrl`). Wayland does not let an application position its windows, so
the playlist opens where GNOME puts it and does not follow the player.

The working set is dominated by the OpenGL driver and the decoded cover
(up to 1024² RGBA); the audio threads themselves are small.

## Known limitations

- Fonts: Inter (Regular + SemiBold, the desktop app's face) and Noto Sans
  CJK (Chinese, Japanese, Korean) are embedded; a system face is appended as
  a last fallback for anything else. The CJK file is 16 MB of the binary.
- Output device: the OS default, opened at its default configuration
  (shared mode). No device picker, no exclusive mode.
- The `[patch.crates-io] glib` RUSTSEC backport of the WaveFlow workspace
  does not apply here (patches must sit at the workspace root); only Linux
  builds pull glib, through rfd's GTK dialog.
- One copy of the pipeline per application. If the prototype is adopted,
  the next step is a Tauri-free `waveflow-audio` crate with an event-sink
  trait, which both the desktop engine and QuickPlay would consume.

## Bundled assets

| File | Author | Source | License |
| --- | --- | --- | --- |
| `assets/fonts/Inter-Regular.ttf`, `Inter-SemiBold.ttf` | The Inter Project Authors | https://github.com/rsms/inter | OFL-1.1 (`assets/fonts/OFL-Inter.txt`) |
| `assets/fonts/NotoSansCJKsc-Regular.otf` | Google, Adobe (Noto CJK) | https://github.com/notofonts/noto-cjk | OFL-1.1 (`assets/fonts/OFL-NotoSansCJK.txt`) |
| `assets/icon-128.png`, `assets/icon.ico` | WaveFlow | `src-tauri/crates/app/icons/` | GPL-3.0-only (project) |
