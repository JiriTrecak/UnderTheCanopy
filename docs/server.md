# Match server

`server/` is the multiplayer match server: the lobby (`/api/…`) and the lockstep relay (`/match/:id`, WebSocket). It also takes players' feedback reports and files them as issues in Linear ([Feedback](#feedback), D59). It never runs the game; it collects each player's commands and sends every tick's commands to all players. Players need the same game version (`ContentIdentity`), which the server checks before a match starts.

It is the old game's TypeScript server moved here in the smallest form (D48): its own code (`index.ts`, `host.ts`, `room.ts`, `latency.ts`) unchanged apart from imports, plus `protocol.ts` and `rules.ts` with the few pieces of the old game it used. The protocol is frozen (D10); `crates/net` is its client.

| File | What |
|---|---|
| `src/index.ts` | HTTP routes, CORS, WebSocket upgrade |
| `src/host.ts` | rooms: lobby, race and hero picks (passed on unchecked), start, saves, restart, latency probes, content agreement, desync hashes, chat |
| `src/room.ts` | the lockstep mailbox: commits a tick once every player has confirmed it |
| `src/latency.ts` | measured round trips and the command delay chosen from them |
| `src/protocol.ts` | wire messages, match config, action shapes, chat clean-up, the parts of a save the host reads |
| `src/feedback.ts` | feedback reports: checks, rate limits, the spool, screenshots uploaded to Linear, Linear issues |
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
  "screenshot": { "type": "image/jpeg", "data": "<base64>", "label": "Screenshot of the game" }
}
```

- **Checks**: `installId` and `reportId` (optional) are 8–64 letters, digits or dashes; `category` is `bug`, `idea`, `balance` or `other`; the description is 1–5,000 characters after trimming; the metadata strings are bounded (version 40, OS 120, GPU 160 characters); `map`, `mode`, `tick`, `preset` and `screen` are optional, and fields a later game adds are ignored. The screenshot is optional: JPEG, PNG or WebP (checked by its first bytes), at most 2 MB decoded. A body over 3.5 MB is refused with 413.
- **Answers**: `202 {"id": "20261017-618472b4"}` once the report is in the spool; a resend with the same `reportId` gets the same answer and is filed once. `400 {"error": "bad_body", "message": "…"}` for a report the server will never take, `429 {"error": "rate_limited"}` past 5 reports in 10 minutes from one address or one install (100 from everyone), `503 {"error": "busy"}` while 500 reports wait. The game keeps the report and tries again later on anything but a 400 or 413 (a server without the route answers 404, which is retried too).
- **Screenshots** wait on the host in `FEEDBACK_MEDIA_DIR` (default `feedback-media` in the working directory; on the host `/var/lib/canopy-matchhost/feedback-media`, private to the service, which systemd creates) under random names until their report is filed, then are deleted; nothing on the host serves them. A daily sweep deletes any screenshot no waiting report names (left by a crash).
- **Issues**: the server files each report in the background as an issue in the Linear team `LINEAR_TEAM_ID` with the API key `LINEAR_API_KEY` (GraphQL at `api.linear.app`). It first uploads the screenshot through Linear's file upload (`fileUpload`, then a PUT to the address Linear gives) and embeds it, so it shows in the issue; then creates the issue: title `[category] ` and the description's first 70 characters, description the player's text, the screenshot, then a table of the metadata; labels **Feedback** and the category (**Bug**, **Idea**, **Balance**) where the team or workspace has labels of those names (matched without case; the server does not create labels). Should Linear refuse the labels, the issue is filed without them. `@` in player text is broken up so reports mention nobody.
- **Spool**: reports wait in `FEEDBACK_SPOOL` (JSON lines; default `feedback-spool.jsonl` in the working directory) until Linear has taken them, with the stored screenshot's name (and, once uploaded, its Linear address, so a retry does not upload it again) rather than the image; they are retried at start and every five minutes. Without the key or the team, reports wait there and the server logs it once. The log names reports by id; the key and screenshots are never logged.

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
4. `just server-deploy` from the Mac. It installs the service file (which reads the environment file and has systemd create `/var/lib/canopy-matchhost`) and the Nginx configuration (a 4 MB body limit for `/api/feedback`).
5. Check: `sudo journalctl -u canopy-matchhost -n 20` shows no "no LINEAR_API_KEY" line after a report; send one from the game and the issue appears in the team with its screenshot. Reports sent before the key existed are filed within five minutes of the restart.

After changing the key, `sudo systemctl restart canopy-matchhost`.

## Deployment

The server runs on a London EC2 host behind Nginx: AWS account **533797168831**, region **eu-west-2**, instance **i-00283091b192feab9** (t3.medium, x86_64), security group **sg-0ae99a69cf8d1d325**, public address **18.134.138.1**. Check `aws sts get-caller-identity` before any infrastructure change.

```bash
just server-deploy             # refuses while a match is being played (FORCE=1 overrides)
just server-deploy rollback    # back to the previous server
```

The deploy needs the SSH key at `~/.config/under-the-canopy/matchhost.pem` (or `MATCHHOST_KEY`); never commit it. It copies `server/` to `/home/ubuntu/canopy-matchhost` (the feedback spool, `/home/ubuntu/canopy-matchhost/feedback-spool.jsonl`, and the screenshots waiting in `/var/lib/canopy-matchhost` stay), runs `npm ci --omit=dev` there, installs `deploy/canopy-matchhost.service` and `deploy/nginx.conf`, stops the old `settlers-matchhost` service (the old repository's server, still installed in `/home/ubuntu/settlers-match` for rollback) and starts the new one, then checks health on the host and over HTTPS. Both services bind 127.0.0.1:8787, so only one runs.

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
