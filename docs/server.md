# Match server

`server/` is the multiplayer match server: the lobby (`/api/…`) and the lockstep relay (`/match/:id`, WebSocket). It never runs the game; it collects each player's commands and sends every tick's commands to all players. Players need the same game version (`ContentIdentity`), which the server checks before a match starts.

It is the old game's TypeScript server moved here in the smallest form (D48): its own code (`index.ts`, `host.ts`, `room.ts`, `latency.ts`) unchanged apart from imports, plus `protocol.ts` and `rules.ts` with the few pieces of the old game it used. The protocol is frozen (D10); `crates/net` is its client.

| File | What |
|---|---|
| `src/index.ts` | HTTP routes, CORS, WebSocket upgrade |
| `src/host.ts` | rooms: lobby, race and hero picks, start, saves, restart, latency probes, content agreement, desync hashes, chat |
| `src/room.ts` | the lockstep mailbox: commits a tick once every player has confirmed it |
| `src/latency.ts` | measured round trips and the command delay chosen from them |
| `src/protocol.ts` | wire messages, match config, action shapes, chat clean-up, the parts of a save the host reads |
| `src/rules.ts` | races and starting heroes from `content/game.json` |
| `deploy/` | deploy script, systemd service, Nginx config |

## Running and testing

```bash
just server          # installs node_modules and serves 127.0.0.1:8787 (PORT, BIND, GAME_JSON override)
```

`CANOPY_MATCH_SERVER=http://127.0.0.1:8787 just play` points the game at it. `npm run check` in `server/` type-checks it and `npm test` runs its tests; `just check` does both once `node_modules` exists. The network tests (`crates/net/tests/match_server.rs`, the game's `screens/net_tests.rs`) start this server and play matches against it. `old_client.rs` and `old_lockstep.rs` also run the old game's TypeScript client against it when the old repository is checked out beside this one.

The server reads `content/game.json` once at start for the races and starting heroes it accepts. A new race or hero needs a redeploy; new action types need `protocol.ts` changed (and the protocol is frozen, D10).

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

## Deployment

The server runs on a London EC2 host behind Nginx: AWS account **533797168831**, region **eu-west-2**, instance **i-00283091b192feab9** (t3.medium, x86_64), security group **sg-0ae99a69cf8d1d325**, public address **18.134.138.1**. Check `aws sts get-caller-identity` before any infrastructure change.

```bash
just server-deploy             # refuses while a match is being played (FORCE=1 overrides)
just server-deploy rollback    # back to the previous server
```

The deploy needs the SSH key at `~/.config/under-the-canopy/matchhost.pem` (or `MATCHHOST_KEY`); never commit it. It copies `server/` and `content/game.json` to `/home/ubuntu/canopy-matchhost`, runs `npm ci --omit=dev` there, installs `deploy/canopy-matchhost.service` and `deploy/nginx.conf`, stops the old `settlers-matchhost` service (the old repository's server, still installed in `/home/ubuntu/settlers-match` for rollback) and starts the new one, then checks health on the host and over HTTPS. Both services bind 127.0.0.1:8787, so only one runs.

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
