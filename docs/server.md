# Match server

`server/` is the multiplayer match server: the lobby (`/api/…`) and the lockstep relay (`/match/:id`, WebSocket). It also takes players' feedback reports and files them as issues in Linear ([Feedback](#feedback), D59). It never runs the game; it collects each player's commands and sends every tick's commands to all players. Players need the same game version (`ContentIdentity`), which the server checks before a match starts.

It is the old game's TypeScript server moved here in the smallest form (D48): its own code (`index.ts`, `host.ts`, `room.ts`, `latency.ts`) unchanged apart from imports, plus `protocol.ts` and `rules.ts` with the few pieces of the old game it used. The protocol was frozen (D10) and is reopened once by D107 ([Protocol 2](#protocol-2)); `crates/net` is its client.

| File | What |
|---|---|
| `src/index.ts` | HTTP routes, CORS, WebSocket upgrade |
| `src/host.ts` | rooms: lobby, race and hero picks (passed on unchecked), start, saves, restart, latency probes, content agreement, desync hashes, chat |
| `src/room.ts` | the lockstep mailbox: commits a tick once every player has confirmed it |
| `src/latency.ts` | measured round trips and the command delay chosen from them |
| `src/protocol.ts` | wire messages, match config, action shapes, chat clean-up, the parts of a save the host reads |
| `src/feedback.ts` | feedback reports: checks, rate limits, the spool, screenshots, replays and saves uploaded to Linear, Linear issues |
| `src/ladder/` | the ladder website: pages, JSON API, replay downloads, SQLite, Glicko-2 and leagues ([Ladder](#ladder)) |
| `deploy/` | deploy script, systemd service, Nginx config |

## Running and testing

```bash
just server          # installs node_modules and serves 127.0.0.1:8787 (PORT, BIND override)
```

`CANOPY_MATCH_SERVER=http://127.0.0.1:8787 just play` points the game at it. `npm run check` in `server/` type-checks it and `npm test` runs its tests; `just check` does both once `node_modules` exists. The network tests (`crates/net/tests/match_server.rs`, the game's `screens/net_tests.rs`) start this server and play matches against it. `old_lockstep.rs` checks the lockstep messages byte for byte against the old game's TypeScript code when the old repository is checked out beside this one.

The server knows no content (D53): it passes each seat's race and hero on as chosen, checking only that they look like ids, and a seat without a choice plays the content's default race and its default hero. Clients list races and heroes from their own content and agree on content before a match starts. New action types need `protocol.ts` changed (and the protocol is frozen, D10).

## Lobby seats

Each seat is **Open** (a human may join), **Closed** (locked by the host) or **Computer**; a joined human's seat shows their name. Every seat has a team (1–8, by default its own number: free for all) and a palette colour (0–7, by default its own), unique among the seats taking part.

| Message | Who | Does |
|---|---|---|
| `setSeat {player, seat}` | host | opens, closes or hands a seat to the computer; never a joined human's |
| `selectRace {race, player?}`, `selectHero {hero, player?}` | a player for their own seat, the host also for a computer's | race and starting hero |
| `selectTeam {team, player?}`, `selectColor {color, player?}` | as above | team, colour (refused with `INVALID_COLOR` when taken) |

The room view gives each seat `kind` (`open`, `closed`, `ai` or `human`), `team` and `color`; computer seats are named "Computer" so older games show them as taken. At start only seats someone plays enter the match: open seats nobody joined and closed seats are left out, and the game builds no base at their starts. Computer seats are `kind: "ai"` slots: every player's game runs them inside the deterministic simulation, so they send nothing and the relay never waits for them. Colours travel in the config's `colors` list (one per slot, in slot order), not on the slots, which game versions up to 0.3.1 read strictly; those versions still play on this server, without colours.

## Differences from the old server

- **Pings carry a position.** The `ping` action may hold `position` (a cell); the game shows it to the pinging player's team on the minimap for four seconds. Older games send it without one, which marks nothing. Deploy the server before a game version that pings: an older server rejects the whole turn that holds one.

- **Abandoned rooms go.** Once a second the server discards rooms nobody can come back to: a finished or desynced match as soon as no player is connected, and a lobby or match whose players have all been disconnected for 2 minutes (`ABANDONED_AFTER_MS`; room for a crashed player to reconnect). Before, rooms stayed until the server restarted: closing the game or finishing a match never removed them. Clients need no change.
- **Only open lobbies are listed.** `GET /api/rooms` no longer shows matches under way; nobody could join them. `GET /api/rooms/:id` still finds them.
- **Maintenance is local.** `POST /api/rooms/clear` (ends every session) answers 403 unless it comes from the host itself: loopback, and not forwarded by Nginx. Before, anyone on the internet could call it. Use it on the host with `curl -X POST http://127.0.0.1:8787/api/rooms/clear`.

`npm test` in `server/` covers these (`test/`).

## Protocol 2

D107 reopened the protocol for one pass; this is its contract. The Rust client is `crates/net` (`wire.rs`, `session.rs`); the server is `src/`. Everything not named here stays as it was.

**Version and identity**
- `POST /api/rooms` and `POST /api/rooms/:id/join` bodies carry `protocol: 2` and `playerId` (1–32 of `A–Z a–z 0–9 . _ -`). A body without `protocol: 2` is refused with **426** `{"error": "update_required", "message": "Update Under the Canopy to play online."}`; a bad `playerId` with 400 `bad_player_id`. `GET /api/health` adds `protocol: 2`.
- The server logs the player id with every create, join, leave, drop and result. Room views show it on human seats (`playerId`).

**Lobby**
- `GET /api/rooms` lists matches under way too (`state: "playing"`), so they can be spectated; ended and desynced rooms stay out.
- `join` with `role: "spectator"` takes a spectator place while the lobby waits (refused `full` when none is free) and is always allowed while a match plays.
- Room view additions: `spectatorPlaces` (0–4, default 2), `observers: [{id, name, playerId}]` (`id` a short public id, never the token; `spectators` stays their count), `observerDelay` (seconds), and on seats: `difficulty` (`easy`, `hard`; Normal absent) for computer seats, `away: true` for a player who is away during a match.
- Host only, while waiting: `setSeat {player, seat, difficulty?}` (difficulty for `ai` seats; the seat's slot carries it), `setSpectatorPlaces {count}` (0–4; not below the spectators present), `setObserverDelay {seconds}` (0, 60, 120 or 180), `kick {player}` or `kick {observer}` (the member is removed and its socket receives `kicked` then closes; its token is void).
- Anyone, while waiting: `takeSeat {player}` moves the sender (player or spectator) to an open seat; `spectate {}` moves a player to a free spectator place (the host too; starting needs at least one seated human, as before).

**Match config:** `v: 2`; `observerDelay` (beats); slots may carry `difficulty`.

**Commits and rejoining**
- The server keeps every commit of the match. The match socket takes `?token=…&since=N` (default 0). When a member binds during a match it receives `start {config, you}` and then `history {through, commits}`: every commit after `since` up to `through` that holds an action (beats not listed were empty for every slot), and live `commit`s after it. For a spectator `through` (and every live commit it receives) stays `observerDelay` beats behind; when the match ends the rest is sent before `ended`.
- A player who binds during a match catches up and then sends `ready` again. If the player was **away**, the server takes them back into the lockstep at the next beat (their `through` counts from the last commit) and inserts `{"type": "back"}` into their slot of that beat; `go` follows as for any late `ready`.
- `away` and `back` are actions only the server writes: clients' bundles holding them are refused whole.

**Leaving, waiting, dropping**
- A socket that closes during a match no longer drops the seat: the beats wait for it.
- While the mailbox has waited for one or more human seats for 5 s, the server sends `waiting {players, seconds}` every second (`players` the waited-for seats, `seconds` how long), and `waiting {players: [], seconds: 0}` once beats flow again. Not while paused.
- `dropPlayer {player}` from a connected player of the match, once that seat has been waited for 30 s: the seat is dropped. `POST …/leave` during a match drops the sender at once and voids its token.
- A dropped seat no longer holds beats back, and the next commit carries `{"type": "away"}` in its slot. A dropped member keeps its token (unless it left) and may rejoin as above; its socket, if still open, receives `dropped` and closes.

**Pause**
- `pause {}` from a player with pauses left (3 a match) while the match plays and is not paused: no further beats commit; everyone receives `paused {player, name, pausesLeft}`.
- `resume {}` from the player who paused at any time, from any other player 30 s after the pause: everyone receives `resuming {seconds: 3}`, and 3 s later `resumed {}` and the beats flow again.
- The waiting notices pause with the match.

**Chat:** a spectator's line reaches spectators only; a player's reaches everyone.

**As built** (where the contract left room):
- Any `ready` after `go` is a rejoin: the seat's `through` becomes the last commit and anything it held is discarded, so a reconnecting client confirms from there.
- The waiting clock starts at the last commit (or `go`) and stands still while paused; the waited-for seats share it, so a 30 s `dropPlayer` is refused while paused. With no beat committed the all-clear goes on the next pulse.
- `away` and `back` lead their slot's actions. A seat dropped and back before any beat commits gets neither.
- Pausing needs `go`; the pause lasts until `resumed` (no second pause or resume in the countdown). Restart and load give back every pause and cancel one.
- A spectator's remainder at the end comes as `commit`s before `ended`; `history.through` for a spectator is the later of the load point and the tick less the delay.
- Kick, `takeSeat` and `spectate` are lobby-only; the mover gets a fresh `welcome`. The host cannot kick themselves; a spectating host still sets up computer seats. `setSeat` without a difficulty keeps a computer's level and its race and hero.
- Leaving during a match also sends `dropped` and closes the leaver's socket; drops and returns send a `room` view, so `away` shows.
- A hash arriving after its beat was agreed (a rejoin replaying) is checked against the agreement; a mismatch is a desync.
- Refusals are `error` messages: `INVALID_SEAT`, `INVALID_SPECTATOR_PLACES`, `INVALID_OBSERVER_DELAY`, `INVALID_KICK`, `SPECTATORS_FULL`, `DROP_REFUSED`, `PAUSE_REFUSED`, `RESUME_REFUSED`.
- The game keeps the seat of a match under way in the user folder (`multiplayer-seat.json`) and offers **Rejoin your match** in Multiplayer while that match still plays; a deliberate leave or the match's end forgets it.

## Ladder

The public face of ranked play ([D107](decisions.md)): a website under `/ladder` with the season, the best players of each race, the league distribution, per-race leaderboards, player profiles and recent matches with their replays. The pages are rendered on the server (no scripts) and styled with Tailwind's zinc palette; race accents come from the races table, leagues have cool neutral colours.

| Route | What |
|---|---|
| `GET /ladder` | the season, its numbers, the top 10 of each race, the league distribution, heroes' pick and win rates, recent matches |
| `GET /ladder/race/:race` | a race's leaderboard, 50 a page (`?page=`), `?league=` (0 lowest … 4) filters it |
| `GET /ladder/player/:playerId` | a player's rating, league, rank and record per race this season, and their matches (`?page=`); Player IDs match without case |
| `GET /ladder/matches` | the season's matches, newest first, 30 a page; `?race=` filters |
| `GET /ladder/search?q=` | players whose Player ID or name contains `q`; an exact Player ID redirects to its profile |
| `GET /ladder/replays/:match` | the match's replay as `<map> <date> <player> vs <player>.utcr` (`application/vnd.canopy.replay`); 410 once older than 90 days, 404 without one. Seeded matches download a text file labelled as a sample, not a replay |
| `GET /ladder/api/season` | JSON: the season, leagues, races, numbers, distribution and top 10s |
| `GET /ladder/api/leaderboard/:race`, `/api/players/:playerId`, `/api/players?q=`, `/api/matches?player=&race=&page=` | JSON of the pages above |
| `GET /ladder/static/…`, `/ladder/media/…` | the built CSS, the site's images and the sample file (`src/ladder/public/`); hero portraits and map previews copied from the content (`$LADDER_DATA/media/`) |

**Rules** live in `src/ladder/leagues.ts` and nowhere else: the league names (placeholders: Seedling, Sapling, Thicket, Elder, Canopy) and the ratings they start at, three tiers each (I highest), 5 placement matches, 3-month seasons starting in January, April, July and October (season 1 began on 1 October 2026), replays kept 90 days, and `SHOW_RATING`, whether the site shows ratings as numbers (it does; the game never does). Rename a league there: the database stores leagues by index. `src/ladder/glicko.ts` is Glicko-2 as Glickman's paper gives it (τ 0.5, new players 1500 ± 350, σ 0.06); every ranked match is its own rating period, and a soft reset for the next season moves ratings a quarter of the way back to 1500 and raises deviations to at least 200.

**Data**: SQLite through Node's built-in `node:sqlite` (Node 24; no dependency). `LADDER_DATA` (default `ladder-data` in the working directory, ignored by Git) holds `ladder.sqlite`, `replays/` and `media/`; `LADDER_DB` may put the database elsewhere. Tables (`src/ladder/db.ts`): `seasons`; `races`, `heroes` and `maps` (names and pictures the pages show, written from the content, since the server knows no content, D53); `players` (Player ID, display name); `ratings` (one per player, season and race: Glicko-2 rating, deviation, volatility, games, wins, losses, league and tier once placed); `matches` (season, time, map, duration, winning slot, replay file name or `sample`); `match_players` (both sides: player, race, hero, rating before and after). `LadderDb.recordMatch` records and rates a ranked 1v1; the seed is its only caller until ranked play lands. The server opens the database at start; a Node without `node:sqlite` still runs matches and answers `/ladder` with 503.

**Seeding**: `npm run seed-ladder` fills an empty database with 320 random players, the current season and 4,800 matches between them, paired by rating and rated through `recordMatch`, so ratings, records and leagues agree. Races, heroes, portraits and maps come from `base/` (`--base`); the map pool is 5 random two-player skirmish maps, or `--maps frozen-dream,sc2-oxide,…`. `--players`, `--matches` and `--seed` change the rest; `--reset` replaces a database that already has players. Seeded matches' replays are the labelled sample file.

**Styles**: `npm run build-ladder` compiles `src/ladder/styles.css` (Tailwind v4, a dev dependency, scanning `pages.ts`) into `src/ladder/public/ladder.css`, which is committed so the host needs no build. Rebuild it after changing classes in `pages.ts`. The images in `public/img/` are generated with gpt-image-2 from the art direction's references (hero banner, one header per race named `race-<race id>.webp`, the empty state); a race without its image shows the banner.

**Running it locally**: `cd server && npm run seed-ladder && npm start`, then open `http://127.0.0.1:8787/ladder`. On the host, Nginx needs a `location /ladder` proxied like `/api/`, and `LADDER_DATA` should point outside the deployed folder (for example `/var/lib/canopy-matchhost/ladder`); neither is set up yet.

## Feedback

[D59](decisions.md). `POST /api/feedback` takes a report from the game (`crates/net/src/feedback.rs`). It is a lobby route of its own, not part of the frozen match protocol (D10).

```json
{
  "reportId": "ab86c065a3eb3a1f50f258f74d7d50ae",
  "installId": "c4abfda0917cd190841b6904dc69c00a",
  "category": "bug",
  "description": "The menu music stops after a match",
  "meta": { "version": "0.3.7", "os": "macOS 15.4 (aarch64)", "gpu": "Apple M1 (Metal)", "preset": "High",
            "screen": "match", "map": "echo-isles", "mode": "skirmish", "tick": 2400 },
  "screenshot": { "type": "image/jpeg", "data": "<base64>", "label": "Screenshot of the game" },
  "replay": { "type": "application/vnd.canopy.replay", "data": "<base64>", "name": "Echo Isles 2026-10-17 14-03 Victory.utcr" },
  "save": { "type": "application/gzip", "data": "<base64>", "name": "Echo Isles 2026-10-17 14-03.utcsave" }
}
```

- **Checks**: `installId` and `reportId` (optional) are 8–64 letters, digits or dashes; `category` is `bug`, `idea`, `balance` or `other`; the description is 1–5,000 characters after trimming; the metadata strings are bounded (version 40, OS 120, GPU 160 characters); `map`, `mode`, `tick`, `preset` and `screen` are optional, and fields a later game adds are ignored. The screenshot is optional: JPEG, PNG or WebP (checked by its first bytes), at most 2 MB decoded. A report from a match may also carry `replay`, the match so far (type `application/vnd.canopy.replay`, starting with `UTCR`, at most 1 MB decoded), and `save`, the current moment (type `application/gzip`, starting with the gzip bytes `1f 8b`, at most 4 MB decoded); both are optional, and an invalid one refuses the report with 400. Their `name` (optional, at most 120 characters after trimming) is the file name they are uploaded under, with anything but letters, digits, spaces, dots, dashes, underscores and parentheses turned into `-` and the extension (`.utcr`, `.utcsave`) added when missing; without one they are `replay.utcr` and `save.utcsave`. A body over 10 MB is refused with 413 (Nginx allows 11 MB).
- **Answers**: `202 {"id": "20261017-618472b4"}` once the report is in the spool; a resend with the same `reportId` gets the same answer and is filed once. `400 {"error": "bad_body", "message": "…"}` for a report the server will never take, `429 {"error": "rate_limited"}` past 5 reports in 10 minutes from one address or one install (100 from everyone), `503 {"error": "busy"}` while 500 reports wait. The game keeps the report and tries again later on anything but a 400 or 413 (a server without the route answers 404, which is retried too).
- **Screenshots, replays and saves** wait on the host in `FEEDBACK_MEDIA_DIR` (default `feedback-media` in the working directory; on the host `/var/lib/canopy-matchhost/feedback-media`, private to the service, which systemd creates) under random names until their report is filed, then are deleted; nothing on the host serves them. A daily sweep deletes any such file no waiting report names (left by a crash).
- **Issues**: the server files each report in the background as an issue in the Linear team `LINEAR_TEAM_ID` with the API key `LINEAR_API_KEY` (GraphQL at `api.linear.app`). It first uploads the screenshot, the replay and the save through Linear's file upload (`fileUpload`, then a PUT to the address Linear gives; the replay as `application/octet-stream`, the save as `application/gzip`, both under their file names); then creates the issue: title `[category] ` and the description's first 70 characters, description the player's text, the screenshot embedded, the links `[Replay of the match](…)` and `[Save of this moment](…)` each on a line of its own followed by the file name, then a table of the metadata; labels **Feedback** and the category (**Bug**, **Idea**, **Balance**) where the team or workspace has labels of those names (matched without case; the server does not create labels). Should Linear refuse the labels, the issue is filed without them. `@` in player text is broken up so reports mention nobody.
- **Spool**: reports wait in `FEEDBACK_SPOOL` (JSON lines; default `feedback-spool.jsonl` in the working directory) until Linear has taken them, with the stored files' names (and, once each is uploaded, its Linear address, so a retry does not upload it again) rather than their bytes; they are retried at start and every five minutes. Without the key or the team, reports wait there and the server logs it once. The log names reports by id and gives the files' sizes; the key and the files are never logged.

### Setting up feedback on the host

1. In Linear: a team for feedback (for example **Feedback**, with triage on), and labels **Feedback**, **Bug**, **Idea** and **Balance** in it or in the workspace. Find the team's id (Linear's command menu → "Copy model UUID" on the team, or the API).
2. Create an API key (Linear → Settings → Security & access → Personal API keys) restricted, if your plan offers it, to creating issues in that team only.
3. On the host, write the environment file, readable by root only (systemd reads it before dropping to `ubuntu`):

   ```sh
   sudo install -d -m 755 /etc/canopy-matchhost
   sudo install -m 600 /dev/null /etc/canopy-matchhost/feedback.env
   sudo nano /etc/canopy-matchhost/feedback.env
   ```

   ```sh
   LINEAR_API_KEY=lin_api_…
   LINEAR_TEAM_ID=<the team's id>
   ```

   The service file sets the rest (`FEEDBACK_SPOOL`, `FEEDBACK_MEDIA_DIR`); the file is optional, and without it reports wait in the spool.
4. `just server-deploy` from the Mac. It installs the service file (which reads the environment file and has systemd create `/var/lib/canopy-matchhost`) and the Nginx configuration (an 11 MB body limit for `/api/feedback`).
5. Check: `sudo journalctl -u canopy-matchhost -n 20` shows no "no LINEAR_API_KEY" line after a report; send one from the game and the issue appears in the team with its screenshot. Reports sent before the key existed are filed within five minutes of the restart.

After changing the key, `sudo systemctl restart canopy-matchhost`.

## Deployment

The server runs on a London EC2 host behind Nginx: AWS account **533797168831**, region **eu-west-2**, instance **i-00283091b192feab9** (t3.medium, x86_64), security group **sg-0ae99a69cf8d1d325**, public address **18.134.138.1**. Check `aws sts get-caller-identity` before any infrastructure change.

```bash
just server-deploy             # refuses while a match is being played (FORCE=1 overrides)
just server-deploy rollback    # back to the previous server
```

The deploy needs the SSH key at `~/.config/under-the-canopy/matchhost.pem` (or `MATCHHOST_KEY`); never commit it. It copies `server/` to `/home/ubuntu/canopy-matchhost` (the feedback spool, `/home/ubuntu/canopy-matchhost/feedback-spool.jsonl`, and the files waiting in `/var/lib/canopy-matchhost` stay), runs `npm ci --omit=dev` there, installs `deploy/canopy-matchhost.service` and `deploy/nginx.conf`, stops the old `settlers-matchhost` service (the old repository's server, still installed in `/home/ubuntu/settlers-match` for rollback) and starts the new one, then checks health on the host and over HTTPS. Both services bind 127.0.0.1:8787, so only one runs.

Changing the address the game connects to (`crates/net`, D32) needs new game builds; deploying the server alone does not change what released games use.

## HTTPS and WSS

Lobby `https://18.134.138.1/api/…`, matches `wss://18.134.138.1/match/:id?token=…`. Clients have no plaintext fallback; macOS transport security stays on, so never add ATS exceptions to fix networking.

Nginx terminates TLS and proxies only `/api/` and `/match/` to 127.0.0.1:8787; the loopback connection never leaves the host. Port 80 serves ACME validation and redirects everything else to HTTPS. The security group allows 80 and 443, not 8787; SSH access is separate. HTTPS access logging is off because WebSocket URLs contain room tokens.

The certificate is a publicly trusted Let's Encrypt **shortlived** IP certificate (about six days), never self-signed; private keys stay under `/etc/letsencrypt` on the host. Certbot comes from the official snap (at least 5.4 for IP certificates with the webroot authenticator), and its `snap.certbot.renew.timer` must stay enabled: it remembers the webroot, profile and deploy hook and reloads Nginx after renewal.

On a new host: allow inbound TCP 80 and 443, create `/var/www/canopy-acme`, configure an HTTP-only Nginx server for the address serving `/.well-known/acme-challenge/` from it, and only then issue the certificate (first with `--dry-run`):

```sh
sudo /snap/bin/certbot certonly --non-interactive --agree-tos \
  --register-unsafely-without-email \
  --preferred-profile shortlived --webroot \
  --webroot-path /var/www/canopy-acme --ip-address 18.134.138.1 \
  --cert-name canopy-matchhost \
  --deploy-hook 'nginx -t && systemctl reload nginx'
```

Then deploy (which installs `deploy/nginx.conf` as `/etc/nginx/sites-available/canopy-matchhost`), verify HTTPS and WSS, and remove any public 8787 ingress. Check the renewal path:

```sh
sudo /snap/bin/certbot renew --cert-name canopy-matchhost --dry-run --run-deploy-hooks
systemctl list-timers snap.certbot.renew.timer
sudo openssl x509 -in /etc/letsencrypt/live/canopy-matchhost/fullchain.pem -noout -dates -ext subjectAltName
curl --fail https://18.134.138.1/api/health
```

Never use `curl -k`, disable certificate checks or log WebSocket tokens when verifying. Test a real lobby from the game and a second client, not only HTTP from the command line. If the public address changes, update the client address, the Nginx config and the certificate, and ship new game builds.
