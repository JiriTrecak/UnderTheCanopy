// Rooms nobody can come back to are discarded, and only open lobbies are listed.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ABANDONED_AFTER_MS, MatchHost } from "../src/host";
import type { ServerMsg } from "../src/protocol";

const draft = { name: "room", mapId: "map", mapRevision: "1", slotCount: 2, guestName: "Ann" };

function host() {
  const clock = { t: 0 };
  return { clock, host: new MatchHost(undefined, () => clock.t) };
}

/** Advances the clock and runs the server's once-a-second pulse. */
function wait(h: ReturnType<typeof host>, ms: number) {
  h.clock.t += ms;
  h.host.pulse();
}

const sink = (_: ServerMsg) => {};

test("a lobby whose host never connects or closed the game goes after the grace period", () => {
  const h = host();
  const { room } = h.host.create(draft);
  wait(h, ABANDONED_AFTER_MS - 1000);
  assert.ok(h.host.get(room.id));
  wait(h, 1000);
  assert.equal(h.host.get(room.id), undefined);
});

test("a connected lobby stays, and is listed until it starts", () => {
  const h = host();
  const { token, room } = h.host.create(draft);
  h.host.get(room.id)!.bind(token, sink);
  wait(h, ABANDONED_AFTER_MS * 3);
  assert.deepEqual(h.host.list().map((r) => r.id), [room.id]);
  h.host.get(room.id)!.start(token);
  assert.deepEqual(h.host.list(), []);
  assert.ok(h.host.get(room.id));
});

test("a match every player left waits for them to reconnect, then goes", () => {
  const h = host();
  const { token, room } = h.host.create(draft);
  const match = h.host.get(room.id)!;
  const guest = match.join("Bob", "player");
  assert.ok("token" in guest);
  match.bind(token, sink);
  match.bind(guest.token, sink);
  match.start(token);
  wait(h, 1000);
  match.unbind(token);
  wait(h, ABANDONED_AFTER_MS);
  assert.ok(h.host.get(room.id), "one player is still connected");
  match.unbind(guest.token);
  wait(h, 1000);
  match.bind(guest.token, sink);
  wait(h, ABANDONED_AFTER_MS);
  assert.ok(h.host.get(room.id), "the reconnect reset the wait");
  match.unbind(guest.token);
  wait(h, 1000);
  wait(h, ABANDONED_AFTER_MS);
  assert.equal(h.host.get(room.id), undefined);
});

test("a finished match goes as soon as the last player disconnects", () => {
  const h = host();
  const { token, room } = h.host.create(draft);
  const match = h.host.get(room.id)!;
  match.bind(token, sink);
  match.start(token);
  match.shutdown({ winner: 0, defeated: [] });
  wait(h, 1000);
  assert.equal(h.host.get(room.id), undefined);
});
