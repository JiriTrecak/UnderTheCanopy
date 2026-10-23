# Releases

How a change reaches players ([D43](decisions.md)). Releases use the old desktop app's
release log, update feed, signing key and package format, so installed copies of the old
app update to this game and this game updates the same way.

Everything goes through the release tool (`crates/release-tool`, binary `canopy-release`),
run from the repository root through `just`. The formats it shares with the game's updater
live in `canopy-release` (`crates/release`).

## Release notes

`releases/log.json` is the only changelog. Its `unreleased` section collects every
completed player-visible change; frozen releases feed the game's What's new, the feed's
notes and the published changelogs. Never keep a second changelog.

```sh
just release-note --kind fixed --text "Describe the player-visible fix."
just release-check
```

Kinds: `added`, `improved`, `fixed`, `knownIssues`. The command takes the log's lock
(`releases/log.json.lock`, created exclusively), keeps other agents' notes, ignores exact
duplicates and writes atomically. If it reports the lock, retry; remove a stale lock only
after checking its owner has stopped. Internal refactors need no note.

`release-check` validates the log and every gallery image: the file name is the SHA-256 of
its bytes, it is a still WebP of the declared size (at most 1600 px a side) and at most
768 KiB. Gallery images are Git LFS files: without `git lfs pull` the check stops at the
pointer and says so. CI has no LFS and runs `check --allow-lfs-pointers`, which reports
pointers and checks everything else.

On Windows `just` cannot pass quoted arguments through; run
`cargo run -q -p canopy-release-tool -- note --kind fixed --text "…"` (and likewise
`check`, `prepare`, `build`, `publish`).

### Gallery images

Releases may show up to eight screenshots (`images` in a change set, in display order). The
old `release:image` command, which made the optimised WebP, is not ported: Rust has no
ready lossy WebP encoder. Until it is, add an image by hand: encode a WebP of at most
1600 px a side and 768 KiB (for example `cwebp -q 82 -resize …`), name it
`<sha256 of the file>.webp` in `releases/media/`, add
`{"file", "alt", "caption"?, "width", "height"}` to Unreleased's `images`, and run
`just release-check`. Never change a published gallery.

## Version policy

- Routine work (features, polish, fixes) ships as **patch** releases: 0.3.1, 0.3.2, …
- **Minor** releases (`0.X.0`) are milestones for agreed major upgrades, such as a new
  playable race. 0.3.0 is the native rebuild. **0.7.0** is the planned public early access
  and **1.0.0** the full release. Do not move towards a milestone because notes piled up.
- `release-prepare` accepts the next patch; the next minor or major needs `--milestone`;
  skipped versions are refused. The flag records an intent someone agreed to. It does not
  let an agent choose a milestone.

The app version is the workspace version in the root `Cargo.toml`
(`[workspace.package] version`); preparing sets it. The bump is checked against the newer
of the app version and the newest released version.

**Never prepare or publish a release without the user's intent to release.** Bug-fix work
only maintains Unreleased.

## Releasing

From the repository root, with the intended changes merged and `just check` passing:

```sh
git lfs pull
just release-prepare 0.3.1 --title "Short release title"     # --milestone for 0.X.0
just server-deploy                                              # every release: the server's version follows the game
just release-build
just release-publish --artifact build/releases/0.3.1/darwin-aarch64/artifact.json
just release-publish --artifact build/releases/0.3.1/darwin-aarch64/artifact.json --apply
```

1. **Prepare** moves Unreleased into the version (published at now, UTC), clears
   Unreleased, and sets the app version in `Cargo.toml` and `Cargo.lock`, the base package's `package.json` and the match server's `server/package.json` (the server's health check reports it, so a deployed server shows which game it belongs to). It refuses an
   empty Unreleased. Commit the result. Frozen notes never change; corrections go into a
   newer release.
   Then deploy the match server (`just server-deploy`, docs/server.md) on **every** release,
   before publishing: prepare set its version, and its health check
   (`https://18.134.138.1/api/health`) must report the released game's version even when no
   server code changed.
2. **Build** requires the log to have a release for the app version. It verifies the
   galleries, loads the signing key (before the long build), runs the same packaging as
   `just package` (`cargo packager`), and checks the bundle's `Info.plist`: the version must
   be the release's and the identifier the old app's `com.jiritrecak.settlers`. It seals
   the bundle with an ad-hoc code signature, as the old Tauri packages were (unless the
   packager signed it with an identity), then archives the bundle as `Under the Canopy.app.tar.gz` (top-level entry
   `Under the Canopy.app/`, modes and symlinks kept, as the Tauri updater unpacks it) and
   signs it as the Tauri CLI did: minisign prehashed Ed25519 with the trusted comment
   `timestamp:<unix seconds>\tfile:Under the Canopy.app.tar.gz\tversion:<version>`. The
   signature must verify against the game's public key and bind the version. The result is
   staged under `build/releases/<version>/<platform>/`: the package
   `under-the-canopy-<version>-<platform>.app.tar.gz`, its `.sig`, `artifact.json`,
   `changelog.json` and the gallery in `media/`. `build/` is ignored by Git.
3. **Publish without `--apply`** verifies the staged package's hash, size and signature
   (bound to the version), that the notes still equal the log's, the gallery, and that the
   AWS identity is the release account. Nothing is uploaded.
4. **Publish with `--apply`** takes the publisher lock (`publishing.lock`, written only if
   absent), refuses to downgrade the feed or to strand other platforms, uploads every
   versioned file immutably (an existing key must hold the same bytes; packages over 64 MiB
   go up in 32 MiB parts and complete only if the key is still absent; failed multipart
   uploads are aborted), verifies the stored objects and the public CDN download byte for
   byte with ranged reads, writes the version's `changelog.json` and `<platform>.json`,
   updates the changelog index, and writes `stable.json` **last**. The lock is removed in
   every case.

A failed publish leaves the feed as it was. Retry with the **same** staged artifact. A
rebuilt package has another signature: once a package is published, use a new version.
Never overwrite release files or remove release folders to retry. If a publish crashed and
left `publishing.lock`, inspect it (version, start time), make sure that publish has
stopped, then delete only the lock.

Use `--profile canopy` when the release account is a named AWS profile. The tool refuses any
account but `533797168831`; other local accounts belong to unrelated projects. AWS
credentials stay in the AWS CLI, never in the game or the repository.

Releases are macOS Apple Silicon (`darwin-aarch64`) for now. The tool also builds the
Windows NSIS installer as the Windows package, untested. Before the feed serves more than
one platform, publishing must stage and promote all platforms together; the
single-platform publisher refuses a new version that would drop a platform from the feed.

### Before publishing

Test a real older-to-newer update in a disposable copy of the app: the old version finds
the release, shows its notes, downloads, verifies, installs, restarts into the new version
(check its `Info.plist`), and then reports itself up to date. Test every platform you
advertise; report only what you actually built and tested.

## Signing key

The updater's secret key is `~/.config/under-the-canopy/updater.key` (mode 0600; base64 of a
minisign secret key, as Tauri stores it). CI or another machine can instead set
`TAURI_SIGNING_PRIVATE_KEY` (the key or a path to it) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty when unset). The current key has an empty
password, which the old Tauri CLI's minisign still ran through scrypt; the tool opens such
keys as well as unencrypted and password-protected ones. Its public half is
`canopy_release::PUBLIC_KEY`, which every installed copy trusts.

**Never display, log, commit or upload the secret key, and never regenerate it**: installed
copies only accept packages it signed, so losing it ends updates for everyone. Back it up
securely.

Updater signing is not Apple code signing. macOS builds are ad-hoc signed and not
notarized; Developer ID signing and notarization are still needed for smooth public
distribution.

## Hosting

AWS account `533797168831`, region `eu-west-2`, CloudFormation stack
`under-canopy-releases`; identifiers in `releases/deployment.json`. The stack itself is
provisioned from the old repository (`infrastructure/releases`); this repository only
publishes to it.

- Base URL: https://d3dte7yaa1v3g8.cloudfront.net
- Update feed: https://d3dte7yaa1v3g8.cloudfront.net/stable.json
- Changelog index: https://d3dte7yaa1v3g8.cloudfront.net/changelog.json
- Download page: https://d3dte7yaa1v3g8.cloudfront.net/download, the one link to share. It
  reads `stable.json` when opened and offers the newest package with its title, gallery
  and install steps, so it never needs updating for a release. Its source is
  `releases/site/download.html`; `just release-site` uploads it (refusing any account but
  the release one).

S3 blocks public access; CloudFront serves only the `updates/` prefix:

```text
updates/stable.json                         → /stable.json
updates/changelog.json                      → /changelog.json
updates/download                            → /download (the download page, text/html)
updates/releases/<version>/changelog.json   → /releases/<version>/changelog.json
updates/releases/<version>/<platform>.json
updates/releases/<version>/<package>
updates/releases/<version>/<package>.sig
updates/releases/<version>/media/<sha256>.webp
publishing.lock                             (private; never served)
```

Versioned files are cached for a year as immutable; the feed and the index for 60 seconds,
so a release can take a minute to appear. No CDN invalidation is needed. Never put sources,
keys, logs or credentials under `updates/`.
