# Security Policy

Thank you for reporting security vulnerabilities responsibly before any public
disclosure.

## Supported Versions

QuickPlay does not have long-term version support. The `main` branch and the
latest published release are the only versions that receive security fixes.

| Version                                           | Supported |
| ------------------------------------------------- | --------- |
| `main` / latest published release                 | Yes       |
| Older versions, snapshots, and unmaintained forks | No        |

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

1. **GitHub Security Advisories**: open the repository's _Security_ tab, then
   choose _Report a vulnerability_
   (<https://github.com/InstaZDLL/waveflow-quickplay/security/advisories/new>).
   This is the recommended confidential channel.
2. Contact the maintainers privately if GitHub Security Advisories are not
   available.

Your report should include:

- a clear description of the issue;
- the expected impact on users or their files;
- the affected version, platform, and installation mode (installer, `.deb`,
  AppImage, built from source);
- the affected surface: a file format, the single-instance socket, the
  file-manager registration, the installer, a user flow;
- detailed reproduction steps, with a sample file when the issue is in a
  decoder or parser;
- a minimal proof of concept if needed to understand the issue;
- a suggested mitigation if you have one;
- your contact information for follow-up.

## Response Targets

- Initial acknowledgement: within 3 business days.
- First assessment: within 7 business days.
- Follow-up updates: at least once per week until the fix is released.
- Public disclosure: coordinated after the fix is released, usually within 30
  to 90 days depending on severity and impact.

## Current Scope

QuickPlay is a local player with no network access of its own. Its sensitive
surfaces are:

- parsing of untrusted audio files and their tags and artwork, through
  `waveflow-core` (symphonia, libopus, the DSD reader) and lofty;
- parsing of `.lrc` sidecar files next to the audio;
- the single-instance socket (`src/ipc.rs`): a named pipe on Windows, a
  socket file in the per-user runtime directory on Unix, which accepts
  lists of paths to play from other processes of the same user;
- `--register` / `--unregister`, which write to the current user's registry
  (`HKCU\Software\Classes`) or `~/.local/share` (desktop entry, icon), and
  the installer that runs them;
- the settings file read at start (`settings.json` under the user's config
  directory);
- the release pipeline and the Authenticode signature of the Windows
  installer.

Vulnerabilities in a decoder or parser that QuickPlay gets from
`waveflow-core` or its dependencies are in scope for a report here; the fix
may land upstream in [WaveFlow](https://github.com/InstaZDLL/WaveFlow) or
the dependency and be picked up by QuickPlay.

## Out of Scope

The following reports are generally not considered exploitable security
vulnerabilities:

- cosmetic bugs or interface issues without security impact;
- generic automated reports without a demonstrated exploit path;
- vulnerabilities in unmodified third-party services or operating systems;
- issues that already require full access to the user's account or local
  machine without increasing impact (the socket is reachable by the same
  user by design);
- voluntary disclosure of files or secrets by the user;
- the SmartScreen warning on the installer: it is the expected consequence
  of a self-signed certificate, not a vulnerability.

## Rewards

QuickPlay does not offer a monetary bug bounty. Researchers who report a valid
vulnerability may be credited in the fix release notes if they want attribution.
