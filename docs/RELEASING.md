# Releasing

Version in `Cargo.toml`, notes in `docs/releases/v<version>.md`, then a
signed `v<version>` tag. [`release.yml`](../.github/workflows/release.yml)
does the rest: it builds and signs the packages, publishes the GitHub
release with its update manifest, and copies the whole release to R2.

## Repository secrets

| Secret                                         | What it is                                                                                    |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `QUICKPLAY_UPDATE_KEY`                         | the release key: the rsign secret key file, without a password. Its public half is `PUBLIC_KEY` in `src/update.rs` |
| `SIGNTOOL_PFX_BASE64`, `SIGNTOOL_PFX_PASSWORD` | WaveFlow's Authenticode certificate, the same two secrets as the WaveFlow repository          |
| `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`     | an R2 API token with Object Read & Write on WaveFlow's bucket; the same token WaveFlow uses   |
| `R2_ACCOUNT_ID`, `R2_BUCKET`                   | the Cloudflare account id (or its S3 endpoint, `https://<id>.r2.cloudflarestorage.com`) and the bucket's name (`waveflow-releases`) |

The four `R2_*` secrets are optional on GitHub: with none of them the
`Copy to R2` job ends green after saying so, with only some of them it
fails, alone, after the GitHub release is out. A repository **variable**
`R2_PUBLIC_URL` is needed only if the bucket's domain is not
`https://download.waveflow.app`, and then the address in `src/update.rs`
has to change with it, in a release that ships before anything relies on
it.

## The R2 copy

Every installed QuickPlay finds its updates at the addresses compiled into
it. Up to 1.4.0 that is one GitHub address: the day the account stops
answering, those copies stop updating for good, and nothing a later release
does can tell them otherwise. From 1.5.0 the app also reads a copy of each
release on WaveFlow's R2 bucket, served under the project's own domain, in
a folder of its own next to WaveFlow's:

```text
https://download.waveflow.app/quickplay/
├── releases/<tag>/…    every file of the release, same names, same bytes
└── latest.json         the manifest of the latest release, pointing there
```

The copy's manifest is GitHub's with each address moved under
`releases/<tag>/`, nothing else changed. The signatures are the ones the
release made: each covers the package's bytes, its version and its file
name, not the address it came from, so the app verifies an update from
either host under its one key, and neither host can pass an older package
off as a newer version. How the app reads the two hosts is in
[ARCHITECTURE.md](ARCHITECTURE.md#updates).

The `Copy to R2` job runs [`publish-r2.sh`](../scripts/release/publish-r2.sh)
once the GitHub release is out:

1. downloads the whole release back from GitHub, so a Linux-only rebuild
   copies the Windows and macOS packages too;
2. uploads every file under `quickplay/releases/<tag>/` through R2's S3 API;
3. asks the public domain for each package the manifest names, and stops
   unless it answers with the local file's size;
4. leaves `latest.json` alone when the one already published is newer (a
   rebuild of an older tag would otherwise hide every newer version from
   this host); the same version again is a rebuild and goes through, and
   `R2_FORCE=1` overrides for a deliberate rollback;
5. uploads the manifest (`no-cache`) last, so it never names a file that
   is not there yet, and reads it back through the domain.

The packages are cached five minutes at the edge: a rebuild replaces them
under the same names, and a stale copy would not match the new signature.
Nothing is ever deleted from the bucket.

`R2_DRY_RUN=1` runs the script without credentials or network:

```bash
R2_DRY_RUN=1 TAG_NAME=v1.5.0 bash scripts/release/publish-r2.sh <files dir> <files dir>/latest.json
```

## Switching to GitLab

[`.gitlab-ci.yml`](../.gitlab-ci.yml) is a standby copy of `ci.yml` and
`release.yml`. Nothing runs it on GitHub. It exists so that losing the
GitHub account costs a few hours of setup rather than a rewrite of the
release pipeline. On GitLab, R2 is the only host: the manifest points
there directly, and the GitLab release holds links to the files rather
than the files.

### Already in place

- The private project
  [`waveflow-groupe/waveflow-quickplay`](https://gitlab.com/waveflow-groupe/waveflow-quickplay),
  with `main` and every tag. It is not a mirror (pull mirroring is a
  Premium feature): refresh it after each release, from a clone whose
  `gitlab` remote points there:

  ```bash
  git fetch origin --tags && git push gitlab origin/main:refs/heads/main && git push gitlab --tags
  ```

- The R2 copy of every release, which the 1.5.0+ builds already read.
- WaveFlow's own copy,
  [`waveflow-groupe/WaveFlow`](https://gitlab.com/waveflow-groupe/WaveFlow).
  `waveflow-core` is a git dependency pinned to a WaveFlow commit
  (`Cargo.toml`): without GitHub, cargo finds that commit only there, so
  the `rev` a QuickPlay release pins must have been pushed to it.

### Before anything happens

Some of it needs GitHub to still answer:

- **Import the issues and pull requests** if they are worth keeping: on
  GitLab, New project → Import project → GitHub, into a separate project
  (the importer wants an empty one), with your GitHub token. It reads
  GitHub's API, so it cannot run after the account is gone.
- Keep a local clone with every tag.

### Runners

GitLab.com's hosted runners: Linux (`saas-linux-medium-amd64`) and Windows
(`saas-windows-medium-amd64`, beta) on every tier; macOS
(`saas-macos-medium-m1`) on Premium and Ultimate only, or through the
GitLab for Open Source program. The Free tier's 400 compute minutes a month
cover a release or two at most. Otherwise register a runner of your own:
`gitlab-runner` with the shell executor on the Mac, and set the
`MACOS_RUNNER_TAG` variable to its tag.

### Variables to create

Settings → CI/CD → Variables. Mark them **protected** and protect the `v*`
tags (Settings → Repository → Protected tags): a protected variable is
given only to pipelines of protected refs. Mask what GitLab accepts to
mask, and give each one the type **Variable**, not File.

**Set each secret's environment scope** as the table says. GitHub hands a
secret only to the step that names it; GitLab gives a variable to the whole
job, and a shell runner writes the job's variables into the script file it
runs, where any program of the job can read them, whatever the job does
with them afterwards. A scoped variable reaches only the jobs that name its
environment: `sign-windows` (the certificate), `sign-manifest` (the release
key) and `publish` (the R2 token), which compile nothing and run tools built
by jobs of their own (`tools-windows`, `tools-linux`). No build script and
no test ever runs in a job that has a secret, and no secret shares a job
with another: the release key never sits in the job that downloads and runs
the AWS CLI installer. A secret left at the default scope, `*`, undoes that:
keep the scopes, and keep any job added that compiles out of those three
environments. They show up under Operate → Environments once a release has
run.

| Variable                                                                 | Environment scope | Notes                                                     |
| ------------------------------------------------------------------------ | ----------------- | --------------------------------------------------------- |
| `QUICKPLAY_UPDATE_KEY`                                                   | `release-manifest` | the same key: installed copies trust only its public half |
| `SIGNTOOL_PFX_BASE64`, `SIGNTOOL_PFX_PASSWORD`                           | `release-signing` | the same certificate                                      |
| `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_ACCOUNT_ID`, `R2_BUCKET` | `release-publish` | **required** here: R2 is the only host                    |
| `R2_PUBLIC_URL`                                                          | `*`               | only if the domain is not `https://download.waveflow.app` |
| `MACOS_RUNNER_TAG`                                                       | `*`               | the tag of your own Mac runner, if not the hosted one     |

### What does not carry over

| On GitHub                          | On GitLab                                                                                                                                                                      |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `waveflow-core` from GitHub        | Point `git =` in `Cargo.toml` at `https://gitlab.com/waveflow-groupe/WaveFlow` and run `cargo update -p waveflow-core` (the `rev` stays). If WaveFlow's project is private, three things: this project in its CI/CD job token allowlist; read access to it (Reporter or more) for every user who runs a pipeline here, since the job token acts with that user's rights and the allowlist alone does not grant them; and `CARGO_NET_GIT_FETCH_WITH_CLI=true` with a `url.<…>.insteadOf` that puts `CI_JOB_TOKEN` in the URL. A public project needs none of it. |
| Dependabot, CodeRabbit, PR labels  | Not ported. Renovate is the nearest replacement for updates.                                                                                                                   |
| Release notes and update link      | The settings' "what's new" link (`RELEASE_PAGE` in `src/update.rs`) and the repository link (`REPOSITORY` in `src/ui/settings.rs`) name GitHub: change them in the next release. |
| Install links                      | The README and waveflow.app link to GitHub releases: point them at `https://download.waveflow.app/quickplay/releases/<tag>/`.                                                  |
| Linux-only rebuild of a tag        | Not ported: re-run the tag's pipeline, which rebuilds every platform.                                                                                                          |
| Copies up to 1.4.0                 | They know only GitHub and cannot be moved; their users reinstall once.                                                                                                         |

### The switch, in order

1. Push the latest code and tags to the GitLab project, and WaveFlow's to
   its own.
2. Point `waveflow-core` at GitLab (above), check that the pipeline of
   `main` runs green.
3. Create the variables with their environment scopes and protect the `v*` tags; register runners if the
   tier needs them.
4. Release as usual, with the tag pushed to GitLab. Its manifest replaces
   `quickplay/latest.json` on R2, and every 1.5.0+ copy sees it.
5. Update the install links.
