---
name: release-new-version
description: Releases a new version of Under the Canopy end to end — merges the work to main, completes the release notes, checks everything, deploys the match server when it changed, prepares, builds, signs, verifies AWS and publishes the update feed. Only when Jiri runs /release-new-version.
disable-model-invocation: true
---

# Release a new version

Jiri running `/release-new-version` is his intent to release, which AGENTS.md requires before
any release. Nothing else is: never start this skill on your own. Read
[docs/releases.md](../../../docs/releases.md) first; it is the authority, and this skill is its
checklist. The steps run in order, and each must succeed before the next. On any failure, stop,
leave nothing half-done that the docs say not to, and report exactly where it stopped and why.

## Arguments

Free text after the command:
- **A version** (`0.3.8`): that version. Otherwise the next patch after the newest release in
  `releases/log.json`.
- **A title in quotes:** the release title. Otherwise write a short title from the release's
  biggest changes, in the style of the earlier ones ("Replays and feedback").
- **"milestone":** allows a `0.X.0` (`--milestone`). Only when Jiri says so; never choose a
  milestone yourself (Version policy in docs/releases.md).
- **"push":** push `main` to `origin` at the end. Otherwise don't push (it isn't part of the
  routine; `origin/main` has lagged behind releases).

## Steps

1. **Start clean.**
   - `git status` must be clean. If it isn't, stop and list what's uncommitted: never release
     someone's unfinished work.
   - No other agent may be mid-change on the branch being released: check the linked sessions
     and agents, and wait for them or report.
2. **Bring the work to main.** Releases are cut on `main`. If the work is on another branch
   (such as `art-direction`), check it contains `main` (`git merge-base --is-ancestor main
   <branch>`), switch to `main` and fast-forward it (`git merge --ff-only <branch>`). If it
   can't fast-forward, merge it, resolving `releases/log.json` by keeping every note, and say so
   in the report.
3. **Complete the release notes.** The release must describe what players get.
   - List the commits since the last release (`git log --oneline <last "Release …" commit>..HEAD`)
     and read `unreleased` in `releases/log.json`.
   - For every player-visible change without a note, add one with
     `just release-note --kind added|improved|fixed|knownIssues --text "…"`.
   - Write player-facing language, one change per note, merging near-duplicates.
   - Never invent changes or results, and don't note internal-only work.
   - Commit the notes ("Release notes for <version>").
4. **Check everything.**
   ```sh
   git lfs pull
   just check
   just release-check
   ```
   Read both results. Any failure stops the release: report it (or fix it, commit, and rerun
   from here if it's clearly a small mechanical fix).
5. **The match server.**
   - If `server/` changed since the last release (`git diff --stat <last release>..HEAD --
     server/`), deploy it before publishing the game, as docs/server.md says:
     `just server-deploy`.
   - It refuses while a match is being played. Don't force it; report and stop.
   - Then check its health over HTTPS as the deploy script reports.
6. **Prepare.**
   ```sh
   just release-prepare <version> --title "<title>"     # + --milestone only if Jiri said so
   ```
   Then commit what it changed (Cargo.toml, Cargo.lock, base/package.json,
   server/package.json, releases/log.json) as `Release <version>: <title>`, as earlier
   releases are.
7. **Build, sign and stage.**
   ```sh
   just release-build
   ```
   - This loads the updater signing key from `~/.config/under-the-canopy/updater.key`.
   - **Never display, log, copy, commit or upload the key**, and never regenerate it.
   - If the key is missing, stop and tell Jiri; don't work around it.
   - The staged files land in `build/releases/<version>/darwin-aarch64/`.
8. **Verify, AWS included (dry run).**
   ```sh
   just release-publish --artifact build/releases/<version>/darwin-aarch64/artifact.json
   ```
   - It verifies the package, signature, notes, gallery and that the AWS identity is the
     release account `533797168831`.
   - If the default AWS identity is another account, retry with `--profile canopy`.
   - If no credentials work, stop and tell Jiri to log in (`aws sso login` or his usual way).
     Never put credentials in files or commands.
9. **The update test.** docs/releases.md asks for a real older-to-newer update test before
   publishing.
   - Do it if you can: a disposable copy of the previous release finds the update, installs it
     and restarts into the new version.
   - Otherwise say plainly in the report that it wasn't tested. Don't claim it was.
10. **Publish.**
    ```sh
    just release-publish --artifact build/releases/<version>/darwin-aarch64/artifact.json --apply
    ```
    - A failed publish leaves the feed as it was: retry with the same staged artifact, never a
      rebuilt one.
    - If a crashed publish left `publishing.lock`, check its owner has stopped before deleting
      only the lock (docs/releases.md).
11. **Confirm it's live.** Fetch https://d3dte7yaa1v3g8.cloudfront.net/stable.json (it can take
    a minute) and check it names the new version.
12. **Push** `main` only if Jiri said "push". Otherwise, report how far `origin/main` is behind.

## Report

In a few lines:
- the version and title;
- what it contains (the note headlines);
- the checks run and their results;
- whether the server was deployed;
- the feed showing the version;
- the update test, done or not;
- whether `main` was pushed.

Then switch back to the branch the work came from, so ongoing work continues where it was.
