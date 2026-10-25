// Protocol 2 (docs/server.md#protocol-2): version and player id, spectator places and moves,
// kicks, computer difficulty, the commit log with rejoining and the observer delay, waiting,
// dropping, away and back, pausing and chat.
import assert from "node:assert/strict";
import { test } from "node:test";
import { MatchHost } from "../src/host";
import { admit, type Action, type ClientMsg, type ServerMsg } from "../src/protocol";

const draft = { name: "room", mapId: "map", mapRevision: "1", slotCount: 4, guestName: "Ann", playerId: "Ann1" };
const content = { type: "ready", content: { abi: "1", sha256: "a".repeat(64) }, build: "b" } as const;
const move = (x: number): Action => ({ type: "move", actors: [1], destination: { x, y: 1 } });

/** A room on a fake clock and fake timers; `inbox` collects what each token receives. */
function room(slotCount = 4) {
  const clock = { t: 0 };
  const timers: { at: number; fn: () => void; live: boolean }[] = [];
  const schedule = (fn: () => void, ms: number) => {
    const timer = { at: clock.t + ms, fn, live: true };
    timers.push(timer);
    return () => { timer.live = false; };
  };
  const host = new MatchHost(() => clock.t, { schedule, log: () => {} });
  const created = host.create({ ...draft, slotCount });
  const match = host.get(created.room.id)!;
  const inbox = new Map<string, ServerMsg[]>();
  const closed = new Set<string>();
  const connect = (token: string, since?: number) => {
    inbox.set(token, []);
    const send = (msg: ServerMsg) => inbox.get(token)!.push(msg);
    const bound = match.bind(token, send, { since, close: () => { closed.add(token); match.unbind(token, send); } });
    return bound;
  };
  const disconnect = (token: string) => match.unbind(token);
  connect(created.token);
  const say = (token: string, msg: ClientMsg) => match.ingest(token, msg);
  const got = <T extends ServerMsg["type"]>(token: string, type: T) =>
    (inbox.get(token) ?? []).filter((m): m is Extract<ServerMsg, { type: T }> => m.type === type);
  const errors = (token: string) => got(token, "error").map((m) => m.code);
  const clear = (token: string) => inbox.set(token, []);
  /** Advances the clock a second at a time: due timers run, then the server's pulse. */
  const wait = (ms: number) => {
    for (let left = ms; left > 0; left -= 1000) {
      clock.t += Math.min(1000, left);
      for (const timer of timers) if (timer.live && timer.at <= clock.t) { timer.live = false; timer.fn(); }
      host.pulse();
    }
  };
  const join = (name: string, role: "player" | "spectator" = "player") => {
    const out = match.join(name, role, `${name}1`);
    assert.ok("token" in out, JSON.stringify(out));
    connect(out.token);
    return out.token;
  };
  const turn = (token: string, through: number, bundles: { tick: number; actions: Action[] }[] = []) =>
    say(token, { type: "turn", through, bundles });
  return { clock, host, match, hostToken: created.token, inbox, closed, connect, disconnect, say, got, errors, clear, wait, join, turn };
}

/** A match of the host and `guests` more players, every one ready and past `go`. */
function playing(guests = 1) {
  const r = room();
  const tokens = [r.hostToken, ...Array.from({ length: guests }, (_, i) => r.join(["Bob", "Cid", "Dee"][i]!))];
  assert.ok("config" in r.match.start(r.hostToken));
  for (const t of tokens) r.say(t, content);
  assert.equal(r.got(r.hostToken, "go").length, 1);
  return { ...r, tokens };
}

const ticks = (msgs: { tick: number }[]) => msgs.map((m) => m.tick);
const slotOf = (commit: { slots: { player: number; actions: unknown[] }[] }, player: number) =>
  commit.slots.find((s) => s.player === player)!.actions;

test("create and join need protocol 2 and a well-formed player id", () => {
  assert.deepEqual(admit({ guestName: "Ann" }), {
    status: 426,
    body: { error: "update_required", message: "Update Under the Canopy to play online." },
  });
  assert.equal(admit({ protocol: 1, playerId: "Ann1" })?.status, 426);
  for (const playerId of [undefined, "", "a".repeat(33), "Jiri Trecak", "jiří"])
    assert.deepEqual(admit({ protocol: 2, playerId }), { status: 400, body: { error: "bad_player_id" } });
  for (const playerId of ["Jiri1", "a", "A.b_c-9", "x".repeat(32)]) assert.equal(admit({ protocol: 2, playerId }), null);
});

test("human seats show their player id; spectators are listed by public id, never by token", () => {
  const r = room();
  r.join("Bob");
  const eve = r.join("Eve", "spectator");
  const view = r.match.view();
  assert.deepEqual(view.slots.slice(0, 2).map((s) => [s.name, s.playerId]), [["Ann", "Ann1"], ["Bob", "Bob1"]]);
  assert.equal(view.spectators, 1);
  assert.equal(view.observers.length, 1);
  const observer = view.observers[0]!;
  assert.equal(observer.name, "Eve");
  assert.equal(observer.playerId, "Eve1");
  assert.ok(observer.id.length <= 8);
  assert.ok(!JSON.stringify(view).includes(eve));
});

test("spectator places: 2 by default, set by the host from 0 to 4 and never below those present", () => {
  const r = room();
  assert.equal(r.match.view().spectatorPlaces, 2);
  r.join("Eve", "spectator");
  r.join("Fay", "spectator");
  assert.deepEqual(r.match.join("Gus", "spectator", "Gus1"), { error: "full" });
  r.say(r.hostToken, { type: "setSpectatorPlaces", count: 1 });
  r.say(r.hostToken, { type: "setSpectatorPlaces", count: 5 });
  assert.deepEqual(r.errors(r.hostToken), ["INVALID_SPECTATOR_PLACES", "INVALID_SPECTATOR_PLACES"]);
  r.say(r.hostToken, { type: "setSpectatorPlaces", count: 3 });
  assert.equal(r.match.view().spectatorPlaces, 3);
  r.join("Gus", "spectator");
  // Only the host sets them.
  const bob = r.join("Bob");
  r.say(bob, { type: "setSpectatorPlaces", count: 4 });
  assert.equal(r.match.view().spectatorPlaces, 3);
  // A match under way takes any spectator.
  r.say(r.hostToken, { type: "setSpectatorPlaces", count: 3 });
  r.match.start(r.hostToken);
  r.join("Hal", "spectator");
  assert.equal(r.match.view().spectators, 4);
});

test("anyone moves to an open seat or a free spectator place while the lobby waits", () => {
  const r = room();
  r.say(r.hostToken, { type: "setSeat", player: 2, seat: "closed" });
  const bob = r.join("Bob");
  const eve = r.join("Eve", "spectator");
  r.say(bob, { type: "takeSeat", player: 3 });
  r.say(eve, { type: "takeSeat", player: 2 });
  r.say(eve, { type: "takeSeat", player: 3 });
  assert.deepEqual(r.errors(eve), ["INVALID_SEAT", "INVALID_SEAT"]);
  r.say(eve, { type: "takeSeat", player: 1 });
  assert.deepEqual(r.match.view().slots.map((s) => s.name), ["Ann", "Eve", null, "Bob"]);
  assert.deepEqual(r.got(eve, "welcome").at(-1)!.you, { role: "player", player: 1, name: "Eve" });
  assert.equal(r.match.you(eve)?.player, 1);
  // The host may spectate too, but a match needs a seated human.
  r.say(r.hostToken, { type: "spectate" });
  r.say(bob, { type: "spectate" });
  r.say(eve, { type: "spectate" });
  assert.deepEqual(r.errors(eve), ["INVALID_SEAT", "INVALID_SEAT", "SPECTATORS_FULL"]);
  assert.deepEqual(r.match.view().observers.map((o) => o.name), ["Ann", "Bob"]);
  assert.deepEqual(r.got(r.hostToken, "welcome").at(-1)!.you, { role: "spectator", name: "Ann" });
  r.say(eve, { type: "spectate" });
  assert.deepEqual(r.errors(eve).at(-1), "SPECTATORS_FULL");
  r.say(eve, { type: "takeSeat", player: 0 });
  r.say(r.hostToken, { type: "takeSeat", player: 1 });
  const started = r.match.start(r.hostToken);
  assert.ok("config" in started);
  assert.deepEqual(started.config.slots.map((s) => [s.player, s.name]), [[0, "Eve"], [1, "Ann"]]);
});

test("a match needs at least one seated human", () => {
  const r = room();
  r.say(r.hostToken, { type: "spectate" });
  assert.deepEqual(r.match.start(r.hostToken), { error: "empty" });
});

test("the host kicks a player or a spectator: kicked, closed, token void", () => {
  const r = room();
  const bob = r.join("Bob");
  const eve = r.join("Eve", "spectator");
  r.say(bob, { type: "kick", player: 0 });
  assert.equal(r.match.view().slots[0]!.name, "Ann");
  r.say(r.hostToken, { type: "kick", player: 0 });
  r.say(r.hostToken, { type: "kick", observer: "nobody" });
  assert.deepEqual(r.errors(r.hostToken), ["INVALID_KICK", "INVALID_KICK"]);
  r.say(r.hostToken, { type: "kick", player: 1 });
  r.say(r.hostToken, { type: "kick", observer: r.match.view().observers[0]!.id });
  for (const t of [bob, eve]) {
    assert.equal(r.inbox.get(t)!.at(-1)!.type, "kicked");
    assert.ok(r.closed.has(t));
    assert.deepEqual(r.match.bind(t, () => {}), { error: "bad_token" });
  }
  const view = r.match.view();
  assert.equal(view.slots[1]!.kind, "open");
  assert.deepEqual(view.observers, []);
});

test("computer seats take a difficulty; Normal is no difficulty; the slots carry it", () => {
  const r = room();
  r.say(r.hostToken, { type: "setSeat", player: 1, seat: "ai", difficulty: "hard" });
  r.say(r.hostToken, { type: "setSeat", player: 2, seat: "ai", difficulty: "easy" });
  r.say(r.hostToken, { type: "setSeat", player: 3, seat: "ai" });
  r.say(r.hostToken, { type: "selectRace", race: "beetles", player: 2 });
  r.say(r.hostToken, { type: "setSeat", player: 2, seat: "ai", difficulty: "normal" });
  r.say(r.hostToken, { type: "setSeat", player: 3, seat: "ai", difficulty: "brutal" as "hard" });
  assert.deepEqual(r.errors(r.hostToken), ["INVALID_SEAT"]);
  const seats = r.match.view().slots;
  assert.deepEqual(seats.map((s) => s.difficulty), [undefined, "hard", undefined, undefined]);
  assert.ok(!("difficulty" in seats[2]!));
  // Only the difficulty changed: the computer keeps its race.
  assert.equal(seats[2]!.race, "beetles");
  const started = r.match.start(r.hostToken);
  assert.ok("config" in started);
  assert.equal(started.config.v, 2);
  assert.deepEqual(started.config.slots.map((s) => s.difficulty), [undefined, "hard", undefined, undefined]);
});

test("the observer delay is 0, 1, 2 or 3 minutes and reaches the config in beats", () => {
  const r = room();
  assert.equal(r.match.view().observerDelay, 0);
  r.say(r.hostToken, { type: "setObserverDelay", seconds: 90 });
  assert.deepEqual(r.errors(r.hostToken), ["INVALID_OBSERVER_DELAY"]);
  r.say(r.hostToken, { type: "setObserverDelay", seconds: 120 });
  assert.equal(r.match.view().observerDelay, 120);
  const started = r.match.start(r.hostToken);
  assert.ok("config" in started);
  assert.equal(started.config.observerDelay, 120 * 40);
});

test("a member binding during a match gets start, then the commits holding actions after since", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.turn(ann, 10, [{ tick: 3, actions: [move(3)] }, { tick: 7, actions: [move(7)] }]);
  r.turn(bob, 10, [{ tick: 5, actions: [move(5)] }]);
  assert.deepEqual(ticks(r.got(ann, "commit")), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  r.disconnect(bob);
  r.connect(bob, 4);
  const inbox = r.inbox.get(bob)!.filter((m) => m.type !== "latencyProbe").map((m) => m.type);
  assert.deepEqual(inbox, ["welcome", "start", "history"]);
  const history = r.got(bob, "history")[0]!;
  assert.equal(history.through, 10);
  assert.deepEqual(ticks(history.commits), [5, 7]);
  assert.deepEqual(slotOf(history.commits[1]!, 0), [move(7)]);
  r.connect(bob);
  assert.deepEqual(ticks(r.got(bob, "history")[0]!.commits), [3, 5, 7]);
});

test("spectators stay the observer delay behind, and get the rest before the match ends", () => {
  const r = room();
  r.say(r.hostToken, { type: "setObserverDelay", seconds: 60 });
  r.say(r.hostToken, { type: "setSeat", player: 1, seat: "ai" });
  const eve = r.join("Eve", "spectator");
  r.match.start(r.hostToken);
  r.say(r.hostToken, content);
  const delay = 60 * 40;
  r.turn(r.hostToken, delay + 100, [{ tick: 50, actions: [move(50)] }, { tick: delay + 50, actions: [move(9)] }]);
  assert.equal(r.got(r.hostToken, "commit").length, delay + 100);
  const seen = r.got(eve, "commit");
  assert.deepEqual(ticks(seen), Array.from({ length: 100 }, (_, i) => i + 1));
  assert.deepEqual(slotOf(seen[49]!, 0), [move(50)]);
  // A spectator joining now replays as far as the delay allows.
  const fay = r.join("Fay", "spectator");
  const history = r.got(fay, "history")[0]!;
  assert.equal(history.through, 100);
  assert.deepEqual(ticks(history.commits), [50]);
  r.turn(r.hostToken, delay + 101);
  assert.deepEqual(ticks(r.got(fay, "commit")), [101]);
  r.say(r.hostToken, { type: "ended", outcome: { winner: 0, defeated: [1] }, tick: delay + 101, checksum: 1 });
  for (const t of [eve, fay]) {
    const msgs = r.inbox.get(t)!;
    assert.equal(msgs.at(-1)!.type, "ended");
    assert.equal(r.got(t, "commit").at(-1)!.tick, delay + 101);
  }
  assert.deepEqual(slotOf(r.got(eve, "commit")[delay + 49]!, 0), [move(9)]);
});

test("a closed socket no longer drops the seat; everyone hears whom the beats wait for", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.turn(bob, 2);
  r.turn(ann, 2);
  r.disconnect(bob);
  r.turn(ann, 10);
  assert.deepEqual(ticks(r.got(ann, "commit")), [1, 2]);
  r.wait(4000);
  assert.deepEqual(r.got(ann, "waiting"), []);
  r.wait(1000);
  assert.deepEqual(r.got(ann, "waiting"), [{ type: "waiting", players: [1], seconds: 5 }]);
  r.wait(2000);
  assert.deepEqual(r.got(ann, "waiting").map((w) => w.seconds), [5, 6, 7]);
  // Bob is back: the beats flow and the notice clears.
  r.connect(bob, 2);
  r.say(bob, content);
  assert.deepEqual(r.got(bob, "go"), [{ type: "go", tick: 3 }]);
  r.turn(bob, 10);
  assert.equal(r.got(ann, "commit").length, 10);
  assert.deepEqual(r.got(ann, "waiting").at(-1), { type: "waiting", players: [], seconds: 0 });
  r.wait(4000);
  assert.equal(r.got(ann, "waiting").length, 4);
  // Nobody sends any more: after 5 s the beats wait for both.
  r.wait(1000);
  assert.deepEqual(r.got(ann, "waiting").at(-1), { type: "waiting", players: [0, 1], seconds: 5 });
});

test("after 30 s of waiting the others may drop a seat; its slot of the next commit says away", () => {
  const r = playing(2);
  const [ann, bob, cid] = r.tokens as [string, string, string];
  r.turn(ann, 5);
  r.turn(cid, 5);
  // Bob is still connected, but sends nothing.
  r.wait(29_000);
  r.say(ann, { type: "dropPlayer", player: 1 });
  assert.deepEqual(r.errors(ann), ["DROP_REFUSED"]);
  r.wait(1000);
  r.say(ann, { type: "dropPlayer", player: 2 });
  assert.deepEqual(r.errors(ann), ["DROP_REFUSED", "DROP_REFUSED"]);
  r.say(cid, { type: "dropPlayer", player: 1 });
  const commits = r.got(ann, "commit");
  assert.deepEqual(ticks(commits), [1, 2, 3, 4, 5]);
  assert.deepEqual(slotOf(commits[0]!, 1), [{ type: "away" }]);
  assert.deepEqual(slotOf(commits[1]!, 1), []);
  assert.deepEqual(r.inbox.get(bob)!.at(-1), { type: "dropped" });
  assert.ok(r.closed.has(bob));
  assert.equal(r.match.view().slots[1]!.away, true);
  assert.deepEqual(r.got(ann, "waiting").at(-1), { type: "waiting", players: [], seconds: 0 });
  // A dropped seat holds nothing back.
  r.turn(ann, 8);
  r.turn(cid, 8);
  assert.equal(r.got(ann, "commit").length, 8);
});

test("leaving during a match drops the seat at once and voids the token", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.turn(ann, 3);
  r.match.leave(bob);
  assert.deepEqual(r.inbox.get(bob)!.at(-1), { type: "dropped" });
  assert.deepEqual(r.match.bind(bob, () => {}), { error: "bad_token" });
  const commits = r.got(ann, "commit");
  assert.deepEqual(ticks(commits), [1, 2, 3]);
  assert.deepEqual(slotOf(commits[0]!, 1), [{ type: "away" }]);
  const seat = r.match.view().slots[1]!;
  assert.deepEqual([seat.name, seat.kind, seat.playerId, seat.away], ["Bob", "human", "Bob1", true]);
});

test("a dropped player rejoins: catches up, readies, and comes back at the next beat", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.turn(ann, 2, [{ tick: 2, actions: [move(2)] }]);
  r.turn(bob, 2);
  r.disconnect(bob);
  r.wait(30_000);
  r.say(ann, { type: "dropPlayer", player: 1 });
  r.turn(ann, 6);
  assert.equal(r.got(ann, "commit").length, 6);
  // Bob comes back with what he had.
  r.connect(bob, 2);
  assert.deepEqual(ticks(r.got(bob, "history")[0]!.commits), [3]);
  assert.equal(r.got(bob, "history")[0]!.through, 6);
  r.say(bob, content);
  assert.deepEqual(r.got(bob, "go"), [{ type: "go", tick: 7 }]);
  assert.equal(r.match.view().slots[1]!.away, undefined);
  // The beats wait for him again, from the last commit.
  r.turn(ann, 9);
  assert.equal(r.got(ann, "commit").length, 6);
  r.turn(bob, 9, [{ tick: 8, actions: [move(8)] }]);
  const commits = r.got(ann, "commit");
  assert.deepEqual(ticks(commits).slice(6), [7, 8, 9]);
  assert.deepEqual(slotOf(commits[6]!, 1), [{ type: "back" }]);
  assert.deepEqual(slotOf(commits[7]!, 1), [move(8)]);
  assert.deepEqual(ticks(r.got(bob, "commit")), [7, 8, 9]);
});

test("away and back are the server's: a turn holding one is refused whole", () => {
  const r = playing(0);
  for (const type of ["away", "back"] as const) {
    r.turn(r.hostToken, 3, [{ tick: 1, actions: [move(1)] }, { tick: 2, actions: [{ type }] }]);
  }
  assert.deepEqual(r.got(r.hostToken, "commit"), []);
  r.turn(r.hostToken, 3, [{ tick: 1, actions: [move(1)] }]);
  assert.deepEqual(ticks(r.got(r.hostToken, "commit")), [1, 2, 3]);
});

test("pause: three a player; the pauser resumes at once, others after 30 s; then a 3 s countdown", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.say(ann, { type: "pause" });
  assert.deepEqual(r.got(bob, "paused"), [{ type: "paused", player: 0, name: "Ann", pausesLeft: 2 }]);
  r.say(bob, { type: "pause" });
  assert.deepEqual(r.errors(bob), ["PAUSE_REFUSED"]);
  r.turn(ann, 4);
  r.turn(bob, 4);
  assert.deepEqual(r.got(ann, "commit"), []);
  // Nobody is waited for while paused.
  r.wait(10_000);
  assert.deepEqual(r.got(ann, "waiting"), []);
  r.say(bob, { type: "resume" });
  assert.deepEqual(r.errors(bob), ["PAUSE_REFUSED", "RESUME_REFUSED"]);
  r.wait(20_000);
  r.say(bob, { type: "resume" });
  assert.deepEqual(r.got(ann, "resuming"), [{ type: "resuming", seconds: 3 }]);
  r.wait(2000);
  assert.deepEqual(r.got(ann, "resumed"), []);
  assert.deepEqual(r.got(ann, "commit"), []);
  r.wait(1000);
  assert.deepEqual(r.got(ann, "resumed"), [{ type: "resumed" }]);
  assert.deepEqual(ticks(r.got(ann, "commit")), [1, 2, 3, 4]);
  // The pauser resumes at once; after three pauses there are none left.
  for (const left of [1, 0]) {
    r.say(ann, { type: "pause" });
    assert.equal(r.got(bob, "paused").at(-1)!.pausesLeft, left);
    r.say(ann, { type: "resume" });
    r.wait(3000);
  }
  r.say(ann, { type: "pause" });
  assert.deepEqual(r.errors(ann), ["PAUSE_REFUSED"]);
  r.say(bob, { type: "pause" });
  assert.equal(r.got(ann, "paused").at(-1)!.pausesLeft, 2);
});

test("the waiting clock stands still while paused", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  r.turn(ann, 5);
  r.wait(3000);
  r.say(ann, { type: "pause" });
  r.wait(60_000);
  assert.deepEqual(r.got(ann, "waiting"), []);
  r.say(ann, { type: "resume" });
  r.wait(3000);
  assert.deepEqual(r.got(ann, "waiting"), []);
  r.wait(2000);
  assert.deepEqual(r.got(ann, "waiting"), [{ type: "waiting", players: [1], seconds: 5 }]);
  r.say(ann, { type: "dropPlayer", player: 1 });
  assert.deepEqual(r.errors(ann), ["DROP_REFUSED"]);
  assert.ok(!r.closed.has(bob));
});

test("a spectator's chat reaches spectators only; a player's reaches everyone", () => {
  const r = playing(1);
  const [ann, bob] = r.tokens as [string, string];
  const eve = r.join("Eve", "spectator");
  const fay = r.join("Fay", "spectator");
  r.say(eve, { type: "chat", text: "gg" });
  r.say(bob, { type: "chat", text: "hi" });
  const lines = (t: string) => r.got(t, "chat").map((c) => c.message.text);
  assert.deepEqual(lines(ann), ["hi"]);
  assert.deepEqual(lines(bob), ["hi"]);
  assert.deepEqual(lines(eve), ["gg", "hi"]);
  assert.deepEqual(lines(fay), ["gg", "hi"]);
  assert.deepEqual(r.got(fay, "chat")[0]!.message, { name: "Eve", player: null, text: "gg" });
});
