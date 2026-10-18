# Contributing to WaveFlow QuickPlay

Thanks for wanting to help. QuickPlay is the small player next to
[WaveFlow](https://github.com/InstaZDLL/WaveFlow): **Rust + egui/eframe** on
top of `waveflow-core`, the desktop app's codec and tag crate. WaveFlow's
[CONTRIBUTING.md](https://github.com/InstaZDLL/WaveFlow/blob/main/CONTRIBUTING.md)
is the canonical guide for the WaveFlow family; this file adds what is
specific to QuickPlay.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## What QuickPlay is, and is not

QuickPlay plays the file in front of you. It deliberately has **no** EQ,
crossfade, gapless, ReplayGain, device picker, exclusive output, library,
network source or analytics: those belong to WaveFlow Desktop. A pull
request that adds one of them will be closed with a pointer there, however
good the code. Fixes, platform work (Linux desktops, macOS), languages,
accessibility and anything that makes "double-click, listen" faster or more
reliable are very welcome.

## Getting set up

Rust 1.98 is pinned by `rust-toolchain.toml` (let rustup install it) and
`cmake` is needed for the libopus that `waveflow-core` builds. On Linux add
the ALSA, xkbcommon and Wayland development packages; on macOS the Xcode
command line tools.

```bash
cargo run --release -- <files or folders>
cargo build --release          # → target/release/waveflow-quickplay(.exe)
```

`waveflow-core` comes from the WaveFlow commit pinned in `Cargo.toml`. To
work against a local checkout:

```bash
cargo build --config 'patch."https://github.com/InstaZDLL/WaveFlow".waveflow-core.path="../WaveFlow/src-tauri/crates/core"'
```

`RUST_LOG=debug` turns the log up. `QUICKPLAY_CAPTURE_DIR=<dir>` (and
`QUICKPLAY_CAPTURE_AFTER_MS`) writes a PNG of the player window after
start, which is how the window is checked on displays that cannot be
grabbed from outside (GNOME on Wayland, an ssh session on macOS).

## Before you open a PR

CI runs exactly these on Windows and Ubuntu, so run them first:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Touching the window, the playlist docking or the audio pipeline? Say in the
PR on which platform(s) you ran it; the behaviours that differ per platform
are listed in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Touching a
cross-cutting pattern (actions, the pipeline, the IPC protocol, the
settings file) means updating that document in the same PR.

## Translations

The interface ships in eight languages, one `Strings` table per language
under `src/i18n/` (`en.rs` is the reference). When you add a string:

- add the field to `Strings` in `src/i18n/mod.rs` with a doc comment saying
  where it appears;
- fill it in **every** language file, in the same place, keeping the
  `{n}`, `{t}`, `{e}`, `{v}`, `{l}` placeholders and the shortcut in
  parentheses untouched; product names (`WaveFlow QuickPlay`,
  `waveflow-core`) stay as they are;
- keep it short: the compact bar is 420 points wide and status lines
  truncate.

A new language is one file, one `Lang` variant, its code and native name in
`src/i18n/mod.rs`, and a line in the README. The `translations_keep_placeholders`
test checks the placeholders.

## Commits and pull requests

- Commit messages: a short lowercase area prefix and an imperative subject,
  `app: restore the playlist after a Dock restore`, `ipc: keep the macOS
  socket out of /tmp`, `docs: …`, `ci: …`; the body says why.
- The PR title follows [Conventional Commits](https://www.conventionalcommits.org/),
  `type(scope): subject`, which drives the `type:` label; the diff size
  drives the `size:` label and the touched paths the `scope:` labels.
- Keep a PR focused on one thing; fill in the template (what, why, how you
  tested it, on which platform); link issues with `Closes #123`.
- No `Claude-Session:` or other tool trailers in commit messages.

## Releases

Version in `Cargo.toml`, notes in `docs/releases/v<version>.md`, then a
`v<version>` tag: `.github/workflows/release.yml` builds the signed Windows
installer, the `.deb` and the AppImage and publishes the GitHub release.
Only the maintainers tag.

## Reporting bugs and security issues

- Bugs and feature requests: use the [issue templates](.github/ISSUE_TEMPLATE/).
- Security vulnerabilities: **do not** open a public issue, follow
  [SECURITY.md](.github/SECURITY.md).
- A bug in the decoders or tag reading usually belongs to `waveflow-core`;
  report it here anyway, it will be moved if needed.

## License

QuickPlay is **GPL-3.0-only**. By submitting a pull request you agree that
your contribution is licensed under those terms for inclusion in this
repository.
