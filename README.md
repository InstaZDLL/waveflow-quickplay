<!-- markdownlint-disable MD033 MD041 -->
<p align="center">
  <img src="assets/icon-128.png" width="96" alt="WaveFlow QuickPlay logo" />
</p>

<h1 align="center">WaveFlow QuickPlay</h1>

<p align="center">
  <strong>Double-click a track, hear it now — WaveFlow's audio engine in a native window that opens in a quarter of a second.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-1.0.0-10b981?style=flat-square" alt="Version 1.0.0" />
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey?style=flat-square" alt="Windows and Linux" />
  <img src="https://img.shields.io/badge/rust-1.98-orange?style=flat-square&logo=rust" alt="Rust 1.98" />
  <img src="https://img.shields.io/badge/UI-egui%20%2F%20eframe%200.36-2563eb?style=flat-square" alt="egui / eframe 0.36" />
  <img src="https://img.shields.io/badge/startup-%3C%20300%20ms-10b981?style=flat-square" alt="Starts in under 300 ms" />
  <img src="https://img.shields.io/badge/tests-24%20passing-2ea44f?style=flat-square" alt="24 tests passing" />
  <img src="https://img.shields.io/badge/license-GPL--3.0-green?style=flat-square" alt="GPL-3.0" />
</p>

<p align="center">
  QuickPlay is the small player that sits next to <a href="https://github.com/InstaZDLL/WaveFlow">WaveFlow</a>. When you only want to hear a file, not manage a library, it plays it from the Explorer context menu or a double-click, with the same decoders the desktop app uses — Opus and DSD included — in a window the size of a notification. No database, no scan, no account.
</p>

<p align="center">
  <a href="#install"><b>Install</b></a>
  &nbsp;·&nbsp;
  <a href="#features"><b>Features</b></a>
  &nbsp;·&nbsp;
  <a href="#screenshots"><b>Screenshots</b></a>
  &nbsp;·&nbsp;
  <a href="#keyboard"><b>Keyboard</b></a>
  &nbsp;·&nbsp;
  <a href="#build-and-test"><b>Build</b></a>
  &nbsp;·&nbsp;
  <a href="#architecture"><b>Architecture</b></a>
</p>

<p align="center">
  <img src="docs/screenshots/player-and-playlist.png" width="788" alt="QuickPlay in artwork mode playing Feel Special, the synced lyric line replacing the title, the spectrum running, and the playlist window docked to its right" />
  <br />
  <sub><b>Artwork mode</b> · synced lyric in place of the title, spectrum, and the playlist docked beside the player</sub>
</p>

---

## Why QuickPlay

<table>
  <tr>
    <td>⚡&nbsp;<b>Instant</b></td>
    <td>First frame in 62 ms on Linux and under 300 ms on Windows, audio engine created lazily on the first play. The release binary is one file, 31 MB, half of it the CJK font.</td>
  </tr>
  <tr>
    <td>🎛️&nbsp;<b>Same sound as WaveFlow</b></td>
    <td>It links the desktop app's own <code>waveflow-core</code>: one codec registry (symphonia + libopus), the same DSD converter, the same resampler and lock-free ring between decoder and device.</td>
  </tr>
  <tr>
    <td>🪟&nbsp;<b>Behaves like a native player</b></td>
    <td>One instance per user, "Play with" / "Add to playlist" in the file manager, a playlist window that moves, minimizes and restores with the player, one taskbar entry for the pair.</td>
  </tr>
  <tr>
    <td>🚫&nbsp;<b>Deliberately small</b></td>
    <td>No EQ, crossfade, gapless, ReplayGain, device picker or exclusive mode. Those live in WaveFlow Desktop; QuickPlay plays the file.</td>
  </tr>
</table>

## Features

### 🎧 Playback

- **Every format the desktop plays**: MP3, FLAC, WAV, AIFF, Vorbis, Opus, AAC, ALAC, DSF and DFF, through `waveflow_core::audio_format::opus::codecs()`.
- **Files and folders** from the command line, a drop on either window or the open dialog; folders are walked four levels deep on a background thread.
- **Instant pause, clean seeks**: the cpal callback never allocates, locks or logs; pause and flush are atomics it reads, as in the desktop engine.
- **Auto-advance** with the track's tail played out, broken files skipped and marked in the playlist.
- **Spectrum** timed to the output clock, so the bars follow what is heard, not what is decoded.

### 🖼️ Two sizes, one window

- **Compact**: clock, title, format chips, seek, volume and transport in 440 × 154 points. Height locked, width free.
- **Artwork**: the cover above the controls, edge to edge, freely resizable. Press `A` or the artwork button to switch.
- Custom title bar with pin (always on top), minimize and close; window size and mode are remembered.

### 📋 Playlist

- A second native window (`show_viewport_immediate`) placed beside the player, virtualised for long queues, with number, title, artist, duration, the current row in the accent colour and failed rows flagged.
- **Docked**: it keeps its offset from the player's top-right corner when the player moves or widens, drag it to change that offset. It minimizes and restores with the player and has no taskbar button of its own; on Windows it is an owned window of the player.
- Double-click to play, right-click for Play / Remove, `Delete` to remove, add / remove / clear in the toolbar.

### 🎤 Lyrics

- A `song.lrc` next to `song.flac` is read when the track starts; the line in force replaces the title as the song plays, and an empty line, or the lead-in, shows the title again, as PotPlayer does.
- `[offset:]` and enhanced-LRC word stamps are honoured; nothing is fetched from the network.
- A title or lyric line wider than its column holds for a moment, then scrolls across and comes round again.

### 🖱️ From the file manager

- **One instance per user**: a second launch hands its files to the running player over a local socket and exits. Plain launch replaces the queue and plays, `--enqueue` appends; a multi-selection that Explorer starts as one process per file is merged.
- `waveflow-quickplay --register` adds *Play with WaveFlow QuickPlay* and *Add to WaveFlow QuickPlay playlist* to the context menu of every audio extension and of folders, plus an "Open with" entry, for the current user and without administrator rights. On Linux it writes a `.desktop` entry with the audio MIME types. `--unregister` removes it all.
- The default handler for the extensions is left to you (Settings › Default apps, or `xdg-mime default app.waveflow.quickplay.desktop audio/flac`).

### ✨ And also

- Inter and Noto Sans CJK are embedded: Japanese, Korean and Chinese titles render the same on every machine.
- Preferences live in their own namespace, `<config dir>/waveflow-quickplay/settings.json`, never read by the desktop app.
- `RUST_LOG=debug` turns the log up; `QUICKPLAY_CAPTURE_DIR=<dir>` writes a PNG of the player window 2 s after start, for displays that cannot be grabbed from outside such as GNOME on Wayland.

## Screenshots

<table>
  <tr>
    <td width="50%" align="center" valign="top"><img src="docs/screenshots/player-compact.png" alt="Compact mode: clock, the current lyric line, format chips, spectrum, seek bar, volume and transport in a 440 by 154 window" /></td>
    <td width="50%" align="center" valign="top"><img src="docs/screenshots/player-expanded.png" width="330" alt="Artwork mode: the cover above the same controls block" /></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Compact</b> · the whole player in 440 × 154</sub></td>
    <td align="center"><sub><b>Artwork</b> · the cover edge to edge, window freely resizable</sub></td>
  </tr>
</table>

## Install

Grab the package for your system from the [latest release](https://github.com/InstaZDLL/waveflow-quickplay/releases/latest):

- **Windows**: `waveflow-quickplay_<version>_x64-setup.exe`, a per-user installer (no administrator rights) that also registers the context-menu entries and removes them on uninstall.
- **Linux**: a `.deb` for Debian / Ubuntu or an `.AppImage` for any distribution, both with the desktop entry and the audio MIME types.

Or build it yourself (see [Build and test](#build-and-test)): the binary is a single file you can copy anywhere.

```text
waveflow-quickplay "C:\Music\song.flac"
waveflow-quickplay song1.flac song2.mp3 song3.m4a
waveflow-quickplay "C:\Music\Some Album"      # folders are walked (4 levels)
waveflow-quickplay --enqueue more.flac        # append to the running player's queue
waveflow-quickplay --register                 # file-manager integration, current user
waveflow-quickplay --unregister
```

`--register` is what the Windows installer runs for you; a hand-copied binary needs it once, and again if it moves, since the registry entries and the `.desktop` file point at the executable's path.

## Keyboard

| Key                 | Action                      |
| ------------------- | --------------------------- |
| Space               | Play / pause                |
| N / P               | Next / previous             |
| ← / → (Shift: 30 s) | Seek ±5 s                   |
| ↑ / ↓               | Volume ±5 %                 |
| A                   | Show / hide the artwork     |
| L                   | Show / hide the playlist    |
| S (or click it)     | Show / hide the spectrum    |
| T                   | Always on top               |
| Ctrl+O              | Open files                  |
| Delete (playlist)   | Remove the selected track   |

Shortcuts work whichever of the two windows has focus.

## Build and test

Requirements: Rust 1.98 (pinned by `rust-toolchain.toml`) and cmake (for
the vendored libopus that `waveflow-core` builds, as the desktop app does).
`waveflow-core` is fetched from the WaveFlow commit pinned in `Cargo.toml`; to
build against a local checkout instead, pass
`--config 'patch."https://github.com/InstaZDLL/WaveFlow".waveflow-core.path="../WaveFlow/src-tauri/crates/core"'`.
On Linux add the ALSA, xkbcommon and Wayland development packages.

```bash
cargo run --release -- <files or folders>
cargo build --release          # → target/release/waveflow-quickplay(.exe)

cargo fmt --check
cargo clippy --all-targets
cargo test                     # 24 tests

cargo install cargo-packager
cargo packager --release --formats nsis          # Windows: per-user installer
cargo packager --release --formats deb,appimage  # Linux
```

The installers come out in `target/packager/`. The NSIS template in
`packaging/installer.nsi` is cargo-packager's with the `--register` /
`--unregister` steps added; `packaging/quickplay.desktop` is the Linux
desktop entry. Releases are built by `.github/workflows/release.yml` from a
`v*` tag whose notes live in `docs/releases/`.

## Architecture

```text
WaveFlow/src-tauri/crates/core   (waveflow-core, path dependency)
   ├── audio_format::opus::codecs()   the one codec registry (symphonia + libopus)
   ├── audio_format::dsd              DSF / DFF parser, 1-bit → PCM converter, tags
   └── scanner::extract               AUDIO_EXTENSIONS, is_scannable_audio,
                                      read_tagged_file, probe_playable_stream, file_type_label

waveflow-quickplay
   ├── src/main.rs        CLI, single-instance hand-over, logging, eframe window
   ├── src/app.rs         QuickPlayApp: queue + engine + loader + window state, applies UI actions
   ├── src/audio/         the playback pipeline (see below)
   ├── src/library.rs     tags / duration / cover reads and folder scans on worker threads
   ├── src/lyrics.rs      .lrc sidecar parser, line in force at a position
   ├── src/queue.rs       the playlist model
   ├── src/config.rs      settings.json
   ├── src/ipc.rs         single instance: local socket, play / enqueue hand-over
   ├── src/integration.rs --register / --unregister (registry verbs, .desktop file)
   ├── src/platform.rs    Windows: owner link between the playlist window and the player
   ├── src/ui/            theme, painted icons, widgets, player pane, playlist window
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
- `drain_silent` (bulk pop + silence) and `paused_output` (silence, no pop)
  give clean seeks and instant pause, as in the desktop engine; the callback
  tests the drain first, so a flush requested while paused still empties the ring;
- streams open through `waveflow_core::audio_format::opus::codecs()`, so
  QuickPlay plays exactly the formats the desktop plays, Opus included;
- a track ends only once its tail has left the ring, resampler delay
  included; the UI then loads the next queue entry;
- the engine is created lazily on the first load, released on exit, and
  released on an output-stream error so the next play reopens the device;
- the spectrum analyser (`src/audio/spectrum.rs`, the desktop's FFT and
  band ladder without the Tauri emit) runs on the decoder thread and
  timestamps every frame with the output position it belongs to.

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
gives the pair a single taskbar entry and a shared z-order; on X11 it
declares the utility window type, which taskbars skip too. Wayland does not
let an application position its windows, so there the playlist opens where
the compositor puts it and does not follow the player. Where the backend
cannot open a second window at all, egui embeds it as a floating panel.

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

Verified there: rendering identical to Windows, Opus / AAC / FLAC playback,
the single-instance hand-over over the Unix socket, `--register` showing up
in "Open with", and under XWayland the playlist following the player.

The working set is dominated by the OpenGL driver and the decoded cover
(up to 1024² RGBA); the audio threads themselves are small.

## Known limitations

- **macOS** is untested, and file associations there need an application
  bundle; `--register` says so and does nothing.
- **Output device**: the OS default, opened at its default configuration
  (shared mode). No device picker, no exclusive mode. If the device goes
  away, the engine is released and the next play reopens the new default.
- **Fonts**: Inter and Noto Sans CJK are embedded; a system face is
  appended as a last fallback for anything else. The CJK file is 16 MB of
  the binary.
- One copy of the pipeline per application. If the prototype is adopted,
  the next step is a Tauri-free `waveflow-audio` crate with an event-sink
  trait, which both the desktop engine and QuickPlay would consume.

## Bundled assets

| File | Author | Source | License |
| --- | --- | --- | --- |
| `assets/fonts/Inter-Regular.ttf`, `Inter-SemiBold.ttf` | The Inter Project Authors | https://github.com/rsms/inter | OFL-1.1 (`assets/fonts/OFL-Inter.txt`) |
| `assets/fonts/NotoSansCJKsc-Regular.otf` | Google, Adobe (Noto CJK) | https://github.com/notofonts/noto-cjk | OFL-1.1 (`assets/fonts/OFL-NotoSansCJK.txt`) |
| `assets/icon-128.png`, `assets/icon.ico` | WaveFlow | `src-tauri/crates/app/icons/` | GPL-3.0-only (project) |

The screenshots show *Feel Special* by TWICE, used for illustration only.

## License

```
Copyright (C) 2026 InstaZDLL

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.
```

See [LICENSE](LICENSE) for the full text.
