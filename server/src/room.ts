import { validAction } from "./protocol";
/**
 * In-process MatchHost room: collect `through` from every playing slot, emit `commit`.
 * MemoryChannel is this Room with no listen port. Node later binds the same class to WS.
 */
import type {
  Action,
  Bundle,
  MatchConfig,
  PipelineSnap,
  ServerMsg,
} from "./protocol";

export class Room {
  private readonly through = new Map<number, number>();
  private readonly held = new Map<number, Map<number, Action[]>>();
  /** Last committed tick (0 before the first `commit`). */
  get tick(): number {
    return this.committed;
  }
  private committed = 0;
  private readonly dropped = new Set<number>();
  private readonly listeners: Array<(msg: ServerMsg) => void> = [];

  /** Seats that send commands; computers run inside every player's game and never do. */
  private readonly humans: MatchConfig["slots"];

  constructor(readonly config: MatchConfig,private acceptingCommits=true) {
    this.humans = config.slots.filter((s) => s.kind === "human");
    for (const slot of this.humans) {
      this.through.set(slot.player, 0);
      this.held.set(slot.player, new Map());
    }
  }

  subscribe(fn: (msg: ServerMsg) => void): () => void {
    this.listeners.push(fn);
    return () => {
      const i = this.listeners.indexOf(fn);
      if (i >= 0) this.listeners.splice(i, 1);
    };
  }

  /** Freeze mailbox for a save. `commits` / `sentThrough` come from Lockstep. */
  snapshot(): Omit<PipelineSnap, "commits" | "sentThrough"> {
    const through: PipelineSnap["through"] = [];
    for (const [player, value] of this.through)
      through.push({ player, through: value });
    const held: PipelineSnap["held"] = [];
    for (const [player, map] of this.held) {
      for (const [tick, actions] of map)
        held.push({ player, tick, actions: actions.slice() });
    }
    return { committed: this.committed, through, held };
  }

  /** Resume after load. Does not re-emit already-committed ticks. */
  resume(snap: Pick<PipelineSnap, "committed" | "through" | "held">): void {
    this.committed = snap.committed;
    this.through.clear();
    for (const slot of this.humans) this.through.set(slot.player, 0);
    for (const t of snap.through) this.through.set(t.player, t.through);
    this.held.clear();
    for (const slot of this.humans) this.held.set(slot.player, new Map());
    for (const h of snap.held) {
      const map = this.held.get(h.player) ?? new Map();
      this.held.set(h.player, map);
      map.set(h.tick, h.actions.slice());
    }
    this.dropped.clear();
  }

  /** Slot confirms it will send no more actions with tick <= `through`. Bundles may be for through+D. */
  confirm(player: number, through: number, bundles: readonly Bundle[]): void {
    if (!this.through.has(player)) return;
    if (
      !Number.isSafeInteger(through) ||
      through < 0 ||
      through > this.committed + 4096 ||
      !Array.isArray(bundles) ||
      bundles.length > 64
    )
      return;
    if (
      !bundles.every(
        (b) =>
          b &&
          Number.isSafeInteger(b.tick) &&
          b.tick > 0 &&
          b.tick <= through + this.config.delay + 1 &&
          Array.isArray(b.actions) &&
          b.actions.length <= 64 &&
          b.actions.every(validAction),
      )
    )
      return;
    if (through <= (this.through.get(player) ?? 0)) return;
    this.through.set(player, through);
    const held = this.held.get(player);
    if (!held) return;
    for (const b of bundles) {
      if (b.tick <= this.committed) continue;
      if (!held.has(b.tick)) held.set(b.tick, structuredClone(b.actions));
    }
    this.flush();
  }

  /** Slot gone: do not wait on their `through`. Still listed in `commit` (empty). */
  drop(player: number): void {
    if (!this.through.has(player)) return;
    this.dropped.add(player);
    this.flush();
  }

  releaseContentBarrier(){this.acceptingCommits=true;this.flush();}

  private flush(): void {
    if(!this.acceptingCommits)return;
    while (this.ready(this.committed + 1)) {
      const tick = this.committed + 1;
      const slots = this.config.slots
        .map((s) => s.player)
        .sort((a, b) => a - b)
        .map((player) => ({
          player,
          actions: this.held.get(player)?.get(tick) ?? [],
        }));
      for (const held of this.held.values()) held.delete(tick);
      this.committed = tick;
      const msg: ServerMsg = { type: "commit", tick, slots };
      for (const fn of this.listeners) fn(msg);
    }
  }

  private ready(tick: number): boolean {
    for (const slot of this.humans) {
      if (this.dropped.has(slot.player)) continue;
      if ((this.through.get(slot.player) ?? 0) < tick) return false;
    }
    return this.humans.some((s) => !this.dropped.has(s.player));
  }
}
