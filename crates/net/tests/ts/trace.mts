// Replays operations through the old client's outbox (`src/net/lockstep.ts`) and the old
// server's mailbox (`src/net/room.ts`) and prints every message they send, one per line,
// exactly as `JSON.stringify` writes it. Actions arrive as JSON text, so they keep the
// key order the Rust client writes. `tests/old_lockstep.rs` compares them with the
// Rust ports. Run from the old repository:
//   node --import tsx <this file> <old repository> <operations.json>
import {readFileSync} from 'node:fs';
import {join} from 'node:path';

const [oldRepo, file] = process.argv.slice(2);
if (!oldRepo || !file) throw Error('usage: trace.mts <old repository> <operations.json>');
const {Lockstep} = await import(join(oldRepo, 'src/net/lockstep.ts'));
const {Room} = await import(join(oldRepo, 'src/net/room.ts'));
const {outbox, mailbox} = JSON.parse(readFileSync(file, 'utf8'));

const lockstep = new Lockstep({send: (msg: unknown) => console.log(JSON.stringify(msg)), onMessage() {}}, 0, 1);
for (const op of outbox) {
  if (op.op === 'send') lockstep.send(JSON.parse(op.action));
  else lockstep.confirm(op.through, op.bundleTick);
}
console.log('---');
const room = new Room({slots: mailbox.players.map((player: number) => ({player, kind: 'human'})), delay: mailbox.delay});
room.subscribe((msg: unknown) => console.log(JSON.stringify(msg)));
for (const op of mailbox.ops) {
  if (op.op === 'confirm') room.confirm(op.player, op.through, op.bundles.map((b: {tick: number; actions: string[]}) => ({tick: b.tick, actions: b.actions.map(a => JSON.parse(a))})));
  else room.drop(op.player);
}
