# Architecture

How QuickPlay is put together, for anyone who wants to change it. The
README has the short version; this is the long one.

## Crates and modules

```text
WaveFlow/src-tauri/crates/core   (waveflow-core, git dependency pinned to a WaveFlow commit,
   │                              with its `playback` feature)
   ├── playback::stream::ActiveStream open a track, decode it to interleaved f32 at the
   │                                  output's rate and channel count, seek; DSD → PCM;
   │                                  the container's length and the codec's name
   ├── playback::resampler, channels  rubato FFT resampler with its end-of-track tail,
   │                                  channel conversion (mono ↔ stereo, Lo/Ro fold-down)
   ├── audio_format::opus::codecs()   the one codec registry (symphonia + libopus)
   ├── audio_format::dsd              DSF / DFF parser, 1-bit → PCM converter, tags
   ├── handoff                        the file QuickPlay hands its queue and position over in
   └── scanner::extract               AUDIO_EXTENSIONS, is_scannable_audio,
                                      read_tagged_file, probe_playable_stream, file_type_label

waveflow-quickplay
   ├── src/main.rs        CLI, single-instance hand-over, logging, eframe window
   ├── src/app.rs         QuickPlayApp: queue + engine + loader + window state, applies UI actions
   ├── src/audio/         the transport around core's decoding (see below)
   ├── src/library.rs     tags / duration / cover reads and folder scans on worker threads
   ├── src/lyrics.rs      .lrc sidecar parser, line in force at a position
   ├── src/queue.rs       the playlist model
   ├── src/config.rs      settings.json
   ├── src/i18n/          one strings table per language, system-language detection
   ├── src/ipc.rs         single instance: local socket, play / enqueue hand-over
   ├── src/waveflow.rs    finds WaveFlow, hands the playback over to it (core's `handoff`)
   ├── src/update.rs      update check at launch, signed download, install on close
   ├── src/restart.rs     restart under the other window system for the pin (Wayland)
   ├── src/integration.rs --register / --unregister (registry verbs, .desktop file)
   ├── src/drop_verb.rs   Windows: Explorer's verbs as COM drop targets, one process per selection
   ├── src/platform.rs    Windows: owner link between the playlist window and the player
   ├── src/macos.rs       macOS: the files Finder opens (application:openURLs:)
   ├── src/ui/            theme, painted icons, widgets, player pane, playlist window, settings page
   └── packaging/         NSIS template and Linux desktop entry for cargo-packager,
                          the .rpm definition, the macOS Info.plist and bundle script
```

The UI never touches the engine or the queue directly: every pane reads a
view of the state and pushes an `Action`; the app applies the actions after
the frame. That is what keeps the widgets free of borrows and the audio
thread free of UI concerns.

## Why QuickPlay has its own pipeline

The desktop engine (`crates/app/src/audio/`, about 10 000 lines across
`engine.rs`, `decoder.rs`, `output.rs`) is built from a `tauri::AppHandle`:
every state change is a Tauri event, the analytics task runs on
`tauri::async_runtime` and writes `play_event` rows through the profile
pool, and ReplayGain / crossfade / the output pin / exclusive mode are read
from the profile database. It cannot be instantiated without a Tauri
runtime, which QuickPlay must not start.

The decoding that engine wraps is not tied to Tauri, and since WaveFlow
#922 it lives in `waveflow-core` behind the `playback` feature:
`ActiveStream` opens a file, decodes it to interleaved `f32`, fits its
channels to the output's and resamples it to the output's rate, with the
desktop's handling of DSD, of FLAC files that carry stray bytes after
their last frame, and of the resampler's tail at the end of a track.
QuickPlay plays through exactly that code. What it carries itself is the
transport around it (`src/audio/`, about 1 300 lines) with the desktop's
topology and invariants: the decoder thread and its commands, the
lock-free ring, the cpal callback, the spectrum analyser. "Powered by
WaveFlow" is the honest description: same formats, same decoding, its own
small transport.

## Playback pipeline

```text
UI thread (egui)  ──AudioCmd (crossbeam)──►  decoder thread (std)  ──f32 (rtrb SPSC)──►  cpal callback
                  ◄──EngineEvent + repaint──  waveflow_core::playback: decode → channels → resample
                  reads SharedPlayback atomics (position, state, volume)
```

- The cpal callback never allocates, locks or logs; it pops the ring,
  applies the volume and advances `samples_played`, the only position clock.
- `drain_silent` (bulk pop + silence) and `paused_output` (silence, no pop)
  give clean seeks and instant pause, as in the desktop engine; the callback
  tests the drain first, so a flush requested while paused still empties the
  ring.
- Streams open through `ActiveStream::open` with the desktop's codec
  registry (`waveflow_core::audio_format::opus::codecs()`), so QuickPlay
  plays exactly the formats the desktop plays, Opus included. The library
  identifiers of that call (track id, source) stay empty; the length comes
  from the container (`container_duration_ms`) and the codec name from the
  registry (`codec`), spelled as the desktop's format chip spells it.
- The bitrate chip is measured while the track plays: the decoder sums
  the compressed bytes and the frames of the packets it decodes
  (`ActiveStream::packets_read`) and takes a reading per second of audio,
  due when that second plays, as the spectrum frames are. The tags' figure,
  the whole file's average, shows until the first reading; a pause keeps
  the last one. A FLAC that opens on silence reads a few kbps there and its
  real rate once the music starts. DSD is not counted: its rate is
  constant, one bit per sample and channel, and the chip shows the one its
  header gives (5 645 kbps for DSD64 stereo).
- At the end of a track `decode_next` appends the resampler's tail, cut to
  the track's real length, along with its `Ok(true)`; the decoder thread
  pushes what it was handed before it acts on the end, so the last
  fraction of a second plays and no silence is added after it. A seek
  resets the decoder, resampler included, so nothing from the old
  position plays first.
- A file on a network share is read ahead on a thread of core's own: the
  open waits for the header only, and Stop or a seek during the transfer
  are answered at once.
- DSD is converted with the desktop's default 256-tap filter; fewer taps
  let the DSD shaping noise through as a steady hiss.
- Every cpal sample format is handled in the output.
- A track ends only once its tail has left the ring, resampler delay
  included; the UI then loads the next queue entry. State events are tagged
  with a generation and ignored until the `Loaded` event of the current
  track, so a slow previous load cannot flip the state of the new one.
- The engine is created lazily on the first load, released on exit, and
  released on an output-stream error so the next play reopens the device.
  An xrun is not one: the stream recovers from it by itself, and macOS
  reports one whenever the process stalls (the first launch of a new build
  while it is scanned), so it is logged and playback carries on.
- The spectrum analyser (`src/audio/spectrum.rs`, the desktop's FFT and band
  ladder without the Tauri emit) runs on the decoder thread and timestamps
  every frame with the output position it belongs to; the UI shows the
  frame due at the position the callback reports, so the bars follow what
  is heard.

What is deliberately **not** here: crossfade, gapless, ReplayGain, EQ,
exclusive output, device selection, radio / remote sources, analytics.

## Windows and docking

The main window has a custom title bar (drag, settings, pin, minimize,
close) and resize edges: `with_decorations(false)` plus
`ViewportCommand::StartDrag` / `BeginResize`. The bar reads "QuickPlay"
beside the WaveFlow logo; the OS title stays "WaveFlow QuickPlay", which
is how the playlist window finds the player on Windows. Compact mode
locks the height to the controls block; artwork mode adds the cover above
it and makes the window freely resizable; the settings page takes the
content area in both modes.

The pin is a window level (`WindowLevel::AlwaysOnTop`) except in a Wayland
session, where winit cannot set one: the compositor alone stacks windows.
There `src/restart.rs` runs QuickPlay under XWayland while it is pinned.
A click on the pin writes where playback is (queue, current row, position,
playing or paused) to a file of its own: in `$XDG_RUNTIME_DIR` when this
user owns it with nobody else allowed in, in the temporary directory
otherwise (on Linux usually the shared `/tmp`), with mode 0600 and a name
nobody can guess either way. It then starts a new copy with
`--restart-state <file> --after-pid <pid>`, stops the audio and closes.
The new copy waits up to ten seconds for the old one to exit (it holds the
single-instance socket and the device), then tries the socket for up to
ten more if it is still taken, builds its event loop with `with_x11()`
because the pin is now on, and resumes from the file. Unpinning does the
same back to Wayland. XWayland is checked by opening the display as winit
would (`XInitThreads`, `XOpenDisplay`) before it is chosen, at start-up
when pinned and on the click, never otherwise, since in GNOME the first X
client starts XWayland; on the click it is asked on a thread, so the
window does not freeze while XWayland starts, and further clicks wait for
the answer. winit builds one event loop per process, so when
the window still cannot be made under XWayland (the display answered, the
window or its OpenGL context did not), the process starts a copy of
itself on Wayland with `--no-xwayland` and the same state or files.
A copy started with `--restart-state` that finds no state it can read
(the file gone, or not one QuickPlay wrote) still opens, with an empty
playlist, and says so in the status line.

The playlist is a second native window (`show_viewport_immediate`) docked
beside the player, as PotPlayer's playlist is:

- it keeps its offset from the player's top-right corner when the player
  is moved or widened (drag the playlist itself to change that offset);
- it minimizes and restores with the player (`-` button, taskbar, Win+D or
  the Dock);
- it has no taskbar button of its own.

On Windows the playlist is made an *owned* window of the player
(`src/platform.rs`, the one native call egui does not wrap), which gives the
pair a single taskbar entry and a shared z-order. On X11 it declares the
utility window type, which taskbars skip too. Wayland does not let an
application position its windows, so there the playlist opens where the
compositor puts it and does not follow the player. Where the backend cannot
open a second window at all, egui embeds it as a floating panel.

On macOS egui-winit only polls the minimized state at window creation
(egui#3494), so the app derives it from the presence of the window rect and
re-sends `Minimized(false)` to the root after a restore from the Dock;
without that, eframe keeps treating the window as minimized, paints nothing
and drops the close request.

## Single instance and file-manager integration

A launch first tries to hand its files to a running instance over a local
socket (`src/ipc.rs`): a named pipe on Windows, a socket file in the
per-user runtime directory (`$XDG_RUNTIME_DIR`, `$TMPDIR` on macOS) on
Unix, the Linux abstract namespace only when there is no such directory.
A socket path must fit in `sun_path` (104 bytes on macOS, 108 on Linux):
when the usual name does not (a `$TMPDIR` set by hand, a long user name)
the file takes a short one, and when even that is too long it goes to
`/tmp/wfqp-<uid>`, a directory made with mode 0700 and refused if it is
not this user's, a link, or open to others.
The protocol is a few UTF-8 lines (`play` or `enqueue`, one path per line,
a blank line, `ok` back); messages are capped in size and count, and one
that is cut short or over a cap gets no `ok`. A path that is not UTF-8 or
holds a line break is not sent: that launch opens a window of its own. The
server reads each connection on a thread of its own, eight at most, and
gives each message two seconds in all (a Unix socket is read nonblocking, a
pipe is peeked before each read, since it has no timeouts); a ninth waits
for a slot. Each hand-off gives up after three seconds and, once it has,
sends nothing even if its connection gets through later. A client that
never finishes holds up the other launches for two seconds at most; an
instance that never answers keeps each launch trying for up to thirty
seconds, as below. A launch whose message was taken but whose `ok` was
lost tries again; the server remembers for a minute what each process
handed over and answers such a repeat without queueing the files twice.

The single instance is strict: a launch that finds the socket taken (a pipe
created exclusive reports "access denied", read as `AddrInUse`) or the
player too busy to answer tries again for up to thirty seconds and then
exits. In that case it never opens a window of its own. A socket file it
cannot bind is removed only when connecting to it is refused: one that
answers, or keeps the connect waiting, belongs to a live player. A file
manager that starts one process per selected file (a Linux one, Explorer's
"Open with"; Explorer's own verbs start one, see below) makes a selection
of a thousand files a thousand launches at once: up to 1024 messages wait
for the window to come up, and `play` messages arriving within half a second of
each other are merged into one queue replacement.

Pipe names and the abstract namespace are machine-wide, so another user
could take the name first: there each end checks that the other runs as the
same user (the token's user SID on Windows, the peer's uid on Linux), and a
launch that finds a stranger opens its own window. The runtime directory is
only used when this user owns it and nobody else can enter it.

`--register` writes, for the current user only, the context-menu verbs and
an "Open with" entry on Windows (`HKCU\Software\Classes`), or a `.desktop`
entry with the audio MIME types and an "Add to playlist" action on Linux.
On Windows the two verbs are `DropTarget` verbs, not command lines: a
command-line verb starts one process per selected file, a thousand for a
thousand files. Each verb names a COM class (`CLSID\{…}\LocalServer32`,
the executable itself, per user like the rest), and Explorer drops the
whole selection on it as one data object; Explorer sets no limit on its
size, but the socket does: a selection over its caps (10 000 paths, 4 MiB)
is not handed to the running player and opens a window of its own. COM
starts QuickPlay once with `-Embedding`; `src/drop_verb.rs` serves the two
classes (play, add to playlist) from a multithreaded apartment on a thread
of its own, so winit still gets the main thread for OLE, reads the
selection's paths from the data object, withdraws the classes and hands
the paths to the usual start: over the socket to the running player, or a
window of its own. A launch that is dropped nothing exits after thirty
seconds. Registration deletes the `command` keys of earlier versions, so
an upgrade does not keep the per-file command.
The labels come from the interface language; the desktop entry carries all
of them, and on Windows a language picked in the settings rewrites the
registry labels when the running executable is the registered one (checked
against the "Open with" command, so a development build never redirects
the installed menu). `--unregister` removes everything. The Windows
installer runs both at install and uninstall time.

On macOS there is nothing to register: the bundle's Info.plist declares
the audio formats (by extension, ranked Alternate, as WaveFlow Desktop
does), which puts QuickPlay under Finder's "Open With". Finder does not
pass the file on the command line; it sends an "open documents" Apple
event, which AppKit hands to the application delegate as
`application:openURLs:`. winit 0.30 installs a delegate of its own,
panics if it is replaced, and does not answer that selector, so
`src/macos.rs` builds the event loop itself, adds the method to winit's
`WinitApplicationDelegate` class through the Objective-C runtime before
the loop runs (a file opened at launch arrives before the first frame),
and hands the loop to eframe with `create_native`. The method queues the
paths, one list per open, and wakes the UI. Each open replaces the queue
as `play` does, without the burst rule (Explorer's one process per file);
a multi-selection arrives in one piece.

## Continuing in WaveFlow

When a WaveFlow that takes handoffs is installed, the player shows its
logo beside the artwork button. A press writes a `waveflow_core::handoff::Handoff` (the queue as
absolute paths, the current entry, the position, playing or paused) to a
file in the temporary directory, starts `waveflow --handoff <file>`, stops
the sound and closes. Both sides build the format from the same core
module; WaveFlow removes the file once read, and a WaveFlow already
running receives the arguments through its single-instance plugin. If the
start fails, the file is removed and the player stays as it was with the
error in its status line.

WaveFlow is looked for once, on a thread, so the first frame does not wait
for it: on Windows the uninstall entry named "WaveFlow" (NSIS or MSI, which
follows a folder the user picked) and then the default folders, on macOS
`WaveFlow.app` in `/Applications` or `~/Applications`, on Linux `waveflow`
on `PATH`. The Flatpak exports its application id instead, which is what
keeps the button hidden there: its sandbox could not see the file.

An older WaveFlow ignores `--handoff` and opens empty, so the install's
version must be `MIN_WAVEFLOW_HANDOFF` (2.0.0-beta.1, pre-releases
before releases as in semver) or later. It is read from metadata, never
by starting WaveFlow, which has no `--version` option and would open its
window: `DisplayVersion` of the uninstall entry, else the executable's
version resource, on Windows; `CFBundleShortVersionString` on macOS;
`dpkg-query`, `rpm` or `pacman` on Linux. No version read, no button.
`QUICKPLAY_WAVEFLOW` overrides the lookup and skips the check, since a
development build reports the version of the last release cut.

## Updates

At launch, unless `check_updates` is off in the settings, `src/update.rs`
reads `latest.json` from the latest GitHub release on a thread of its own
(`reqwest` blocking, the client `waveflow-core` already builds). The
manifest gives the version and, per package QuickPlay can install by
itself, its URL and its minisign signature: `windows-x86_64` (the NSIS
installer), `darwin-universal` (the disk image) and
`linux-x86_64-appimage`. The release workflow signs each with the release
key (secret `QUICKPLAY_UPDATE_KEY`; the public half is `PUBLIC_KEY` in the
module) under the trusted comment `waveflow-quickplay <version> <file>`.
The manifest itself is not signed, so the app checks that comment against
the version and the file name it was given: a genuine older package cannot
be passed off as a newer version.

How the copy was installed decides the rest (`Install::detect`):

- an `uninstall.exe` beside the executable is the per-user NSIS install:
  on close the installer runs with `/S /NS` (silent, no shortcut the user
  removed comes back); it ends a QuickPlay still running before it copies;
- an executable inside `<name>.app/Contents/MacOS/`, whose bundle and
  folder this user can write, is replaced on close by a shell that waits
  for the process to exit, mounts the disk image, copies the bundle beside
  the old one with `ditto` and swaps the two, the old one coming back if
  the swap fails (its output goes to a log beside the image in the cache).
  The image stays in the cache until the new bundle is in place, so a
  failed attempt is retried at the next close without a new download;
- `APPIMAGE` names the AppImage, whose folder this user can write, when
  the executable runs from inside its mount (`APPDIR`): both variables are
  inherited by whatever an AppImage starts, a QuickPlay installed from a
  package included. The new file is copied beside it and renamed over it on
  close;
- anything else (a `.deb` or an `.rpm` under `/usr`, a build from source, a
  read-only copy) only shows that a new version is out, with a link to the
  release in the settings.

The package is downloaded to `<cache dir>/waveflow-quickplay/updates`,
verified before it is kept, and verified again when a later launch finds it
there instead of downloading it again (a restart for the pin closes without
installing). A debug build looks only when `QUICKPLAY_UPDATE_URL` names a
manifest to test with. A restart for the pin starts the AppImage again
rather than the executable inside it, whose mount goes away with the
process that is closing.

## Languages

Every user-visible string lives in a `Strings` table, one `const` per
language under `src/i18n/`, with named placeholders (`{n}`, `{t}`, `{e}`,
`{v}`, `{l}`) filled at use. The language is the stored preference when it
names one, the system's otherwise, English as the last resort. A test checks
that every language keeps the placeholders of the English template. Window
titles the OS shows stay in English: they are also how the two windows find
each other on Windows.

## Testing from a script

`QUICKPLAY_CAPTURE_DIR=<dir>` (and `QUICKPLAY_CAPTURE_AFTER_MS`) makes the
app write a PNG of the player window after start, for displays that cannot
be grabbed from outside such as GNOME on Wayland or an ssh session on
macOS. `RUST_LOG=debug` turns the log up. The repository's CI runs fmt,
clippy and the tests on Windows, Ubuntu and macOS; releases are built from
a `v*` tag by `.github/workflows/release.yml`, the macOS disk image by
`packaging/macos/bundle.sh` around a universal binary, signed ad hoc.
