// Lobby seats: the host opens, closes or hands seats to the computer; players pick team
// and colour; the match takes only seats someone plays, and never waits for a computer.
import assert from "node:assert/strict";
import { test } from "node:test";
import { MatchHost } from "../src/host";
import type { ClientMsg, ServerMsg } from "../src/protocol";

const draft = { name: "room", mapId: "map", mapRevision: "1", slotCount: 4, guestName: "Ann" };

/** A room with its host connected; `inbox` collects what each token receives. */
function lobby() {
  const host = new MatchHost(undefined);
  const created = host.create(draft);
  const match = host.get(created.room.id)!;
  const inbox = new Map<string, ServerMsg[]>();
  const connect = (token: string) => {
    inbox.set(token, []);
    match.bind(token, (msg) => inbox.get(token)!.push(msg));
  };
  connect(created.token);
  const say = (token: string, msg: ClientMsg) => match.ingest(token, msg);
  const errors = (token: string) => inbox.get(token)!.filter((m) => m.type === "error").map((m) => (m as { code: string }).code);
  return { match, hostToken: created.token, connect, say, errors };
}

test("the host sets seats; a joining player takes the first open one", () => {
  const { match, hostToken, say } = lobby();
  say(hostToken, { type: "setSeat", player: 1, seat: "ai" });
  say(hostToken, { type: "setSeat", player: 2, seat: "closed" });
  const guest = match.join("Bob", "player");
  assert.ok("token" in guest);
  assert.equal(guest.you.player, 3);
  assert.deepEqual(
    match.view().slots.map((s) => [s.kind, s.name]),
    [["human", "Ann"], ["ai", "Computer"], ["closed", null], ["human", "Bob"]],
  );
  assert.deepEqual(match.join("Cid", "player"), { error: "full" });
});

test("only seats someone plays enter the match, with their team and colour", () => {
  const { match, hostToken, say } = lobby();
  say(hostToken, { type: "setSeat", player: 2, seat: "ai" });
  say(hostToken, { type: "selectTeam", team: 1, player: 2 });
  say(hostToken, { type: "selectTeam", team: 1 });
  const result = match.start(hostToken);
  assert.ok("config" in result);
  assert.deepEqual(
    result.config.slots.map((s) => [s.player, s.kind, s.team]),
    [[0, "human", 1], [2, "ai", 1]],
  );
  assert.deepEqual(result.config.colors, [0, 2]);
});

test("players change only their own seat; the host also changes computers'", () => {
  const { match, hostToken, connect, say, errors } = lobby();
  say(hostToken, { type: "setSeat", player: 1, seat: "ai" });
  const guest = match.join("Bob", "player");
  assert.ok("token" in guest);
  connect(guest.token);
  say(guest.token, { type: "selectTeam", team: 5, player: 0 });
  say(guest.token, { type: "selectTeam", team: 5, player: 1 });
  say(guest.token, { type: "setSeat", player: 3, seat: "closed" });
  assert.equal(guest.you.player, 2);
  say(guest.token, { type: "selectTeam", team: 6 });
  say(hostToken, { type: "selectTeam", team: 7, player: 1 });
  const teams = match.view().slots.map((s) => s.team);
  assert.deepEqual(teams, [1, 7, 6, 4]);
  assert.equal(match.view().slots[3]!.kind, "open");
  // The host cannot take a joined player's seat away.
  say(hostToken, { type: "setSeat", player: 2, seat: "closed" });
  assert.deepEqual(errors(hostToken), ["INVALID_SEAT"]);
});

test("colours stay unique among the seats taking part", () => {
  const { match, hostToken, say, errors } = lobby();
  say(hostToken, { type: "selectColor", color: 3 });
  // Seat 3 would keep its own colour 3, which the host now has: it gets the first free one.
  say(hostToken, { type: "setSeat", player: 3, seat: "ai" });
  assert.deepEqual(match.view().slots.map((s) => s.color), [3, 1, 2, 0]);
  say(hostToken, { type: "selectColor", color: 0 });
  assert.deepEqual(errors(hostToken), ["INVALID_COLOR"]);
  say(hostToken, { type: "selectColor", color: 7, player: 3 });
  assert.equal(match.view().slots[3]!.color, 7);
});

test("the relay commits ticks without waiting for computer seats", () => {
  const { match, hostToken, say } = lobby();
  say(hostToken, { type: "setSeat", player: 1, seat: "ai" });
  const commits: number[] = [];
  match.bind(hostToken, (msg) => {
    if (msg.type === "commit") commits.push(msg.tick);
  });
  const started = match.start(hostToken);
  assert.ok("config" in started);
  say(hostToken, { type: "ready", content: { abi: "1", sha256: "a".repeat(64) }, build: "b" });
  say(hostToken, { type: "turn", through: 3, bundles: [] });
  assert.deepEqual(commits, [1, 2, 3]);
});

test("pings with a position pass the action check", () => {
  const { match, hostToken, say } = lobby();
  const commits: unknown[][] = [];
  match.bind(hostToken, (msg) => {
    if (msg.type === "commit") commits.push(msg.slots.flatMap((s) => s.actions));
  });
  match.start(hostToken);
  say(hostToken, { type: "ready", content: { abi: "1", sha256: "a".repeat(64) }, build: "b" });
  const ping = { type: "ping", position: { x: 12, y: 30 } } as const;
  say(hostToken, { type: "turn", through: 2, bundles: [{ tick: 2, actions: [ping] }] });
  assert.deepEqual(commits, [[], [ping]]);
});
