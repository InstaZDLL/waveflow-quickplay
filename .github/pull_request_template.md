<!--
Thanks for the PR! A few quick reminders so review goes fast:

1. PR title follows Conventional Commits with a kebab-case scope:
     feat(playlist): sort a dropped folder by track number
     fix(audio): flush the resampler at the end of a track
     docs(readme): …
   The type drives the `type:` label; the diff size drives `size:`.

2. Before opening, run locally (CI runs exactly these):
     cargo fmt --check
     cargo clippy --locked --all-targets -- -D warnings
     cargo test --locked

3. If you touched user-facing strings, fill the new field in every file of
   src/i18n/ (8 languages — en.rs is the reference), placeholders intact.

4. If you touched a cross-cutting pattern (actions, pipeline, IPC, docking,
   settings file), update docs/ARCHITECTURE.md in the same PR.

5. EQ, crossfade, gapless, ReplayGain, device picker, exclusive output,
   library or network sources are out of scope: they belong to WaveFlow
   Desktop and will not be merged here.
-->

## Summary

<!-- 1-3 bullets describing what changes and why. Focus on the why. -->

-
-

## How I tested

<!-- Concrete steps a reviewer could repeat, and on which platform(s): Windows, Linux (Wayland / X11), macOS. Skip "it builds" — that's CI's job. -->

-
-

## Screenshots / clips

<!-- For UI changes. QUICKPLAY_CAPTURE_DIR=<dir> writes a PNG of the player window if your display cannot be grabbed. -->

## Checklist

- [ ] Title uses Conventional Commits (`type(scope): subject`, kebab-case scope)
- [ ] `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` pass locally
- [ ] If UI strings changed: every language in `src/i18n/` updated, placeholders kept
- [ ] If a cross-cutting pattern changed: `docs/ARCHITECTURE.md` updated
- [ ] Platform-specific behaviour: tested on the platform(s) named above
- [ ] Breaking change (settings file, CLI flags, IPC protocol)? Called out in the summary

## Linked issues

<!-- Use "Closes #123" / "Refs #456" so GitHub auto-closes on merge. -->

Closes #
