/**
 * MatchHost HTTP + WebSocket (docs/server.md). Mailbox only: it relays commands and never
 * runs the game. Production binds 127.0.0.1:8787 behind Nginx HTTPS/WSS on port 443.
 */
import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { resolve as resolvePath } from "node:path";
import { WebSocketServer } from "ws";
import { Feedback, MAX_BODY, RETRY_MS, SWEEP_MS } from "./feedback";
import { MatchHost } from "./host";
import { PROTOCOL, admit, type CreateRoom, type JoinRoom } from "./protocol";
import { Queue, queueRoute } from "./queue";

const PORT = Number(process.env.PORT ?? 8787);
const BIND = process.env.BIND ?? "127.0.0.1";
// The game's version: releases set it in package.json with the app's (docs/releases.md).
const VERSION: string = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "content-type, authorization",
  "access-control-allow-methods": "GET, POST, DELETE, OPTIONS",
};

// The ladder website (docs/server.md#ladder). Loaded on its own so that a Node without
// node:sqlite still runs matches; the ladder then answers 503.
const ladder = await import("./ladder/routes")
  .then((m) => m.createLadder())
  .catch((e: unknown) => {
    console.error(`ladder disabled: ${e instanceof Error ? e.message : e}`);
    return null;
  });

const host = new MatchHost();
// The 1v1 queues (docs/server.md#queues-and-matchmaking) rate and record through the ladder.
const queue = ladder ? new Queue({ db: ladder.db, host }) : null;
const probes=setInterval(()=>{host.pulse();queue?.pulse();},1000);
probes.unref();

// Player feedback becomes issues in the game's Linear team (docs/server.md, D59).
const feedback = new Feedback({
  apiKey: process.env.LINEAR_API_KEY,
  teamId: process.env.LINEAR_TEAM_ID,
  spool: resolvePath(process.env.FEEDBACK_SPOOL ?? "feedback-spool.jsonl"),
  mediaDir: resolvePath(process.env.FEEDBACK_MEDIA_DIR ?? "feedback-media"),
});
const feedbackRetry = setInterval(() => {
  feedback.prune();
  void feedback.retry();
}, RETRY_MS);
feedbackRetry.unref();
const feedbackSweep = setInterval(() => feedback.sweep(), SWEEP_MS);
feedbackSweep.unref();
void feedback.retry();
feedback.sweep();

const server = createServer((req, res) => {
  void handle(req, res);
});
server.on('close',()=>clearInterval(probes));

const wss = new WebSocketServer({ noServer: true });

server.on("upgrade", (req, socket, head) => {
  const url = new URL(req.url ?? "", "http://match");
  const m = /^\/match\/([^/]+)$/.exec(url.pathname);
  const token = url.searchParams.get("token") ?? "";
  // The last commit the client has; history starts after it.
  const since = Number(url.searchParams.get("since") ?? 0);
  const room = m ? host.get(decodeURIComponent(m[1]!)) : undefined;
  if (!room || !token) {
    socket.destroy();
    return;
  }
  wss.handleUpgrade(req, socket, head, (ws) => {
    const send = (msg: unknown) => {
      if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(msg));
    };
    const bound = room.bind(token, send, { since: Number.isSafeInteger(since) && since > 0 ? since : 0, close: () => ws.close() });
    if ("error" in bound) {
      ws.close();
      return;
    }
    ws.on("message", (raw) => {
      try {
        room.ingest(token, JSON.parse(String(raw)));
        if (room.view().state === "ended") host.discard(room.id);
      } catch {
        ws.send(JSON.stringify({ type: "error", code: "bad_json", message: "bad json" }));
      }
    });
    ws.on("close", () => room.unbind(token, send));
  });
});

server.listen(PORT, BIND, () => {
  console.log(`matchhost ${VERSION} ${BIND}:${PORT}`);
});

async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
  if (req.method === "OPTIONS") {
    res.writeHead(204, CORS);
    res.end();
    return;
  }
  const url = new URL(req.url ?? "/", "http://match");
  const path = url.pathname;
  try {
    if (req.method === "GET" && path === "/api/health") {
      send(res, 200, { ok: true, version: VERSION, protocol: PROTOCOL });
      return;
    }
    if (req.method === "POST" && path === "/api/feedback") {
      const body = await readJson(req, MAX_BODY).catch((e: unknown) => e);
      if (body instanceof TooLarge) {
        send(res, 413, { error: "too_large" });
        return;
      }
      if (body instanceof Error) {
        send(res, 400, { error: "bad_json" });
        return;
      }
      const reply = feedback.submit(body, clientAddress(req));
      send(res, reply.status, reply.body);
      return;
    }
    if (path === "/api/queue" || path.startsWith("/api/queue/")) {
      const body = req.method === "POST" ? await readJson(req, 64 * 1024).catch((e: unknown) => (e instanceof Error ? e : new Error("bad json"))) : undefined;
      const reply = queueRoute(queue, req.method ?? "GET", path, body);
      if (reply) {
        send(res, reply.status, reply.body);
        return;
      }
    }
    if (req.method === "GET" && path === "/api/rooms") {
      send(res, 200, host.list());
      return;
    }
    if (req.method === "POST" && path === "/api/rooms/clear") {
      if (!fromThisMachine(req)) {
        send(res, 403, { error: "forbidden" });
        return;
      }
      const deleted = host.discardAll();
      // Existing sockets retain room references; close them after the ended event.
      for (const ws of wss.clients) ws.close(1000, "Server sessions cleared");
      send(res, 200, { deleted });
      return;
    }
    const one = /^\/api\/rooms\/([^/]+)$/.exec(path);
    if (req.method === "GET" && one) {
      const room = host.get(decodeURIComponent(one[1]!));
      if (!room) {
        send(res, 404, { error: "not_found" });
        return;
      }
      send(res, 200, room.view());
      return;
    }
    if (req.method === "POST" && path === "/api/rooms") {
      const body = (await readJson(req)) as CreateRoom;
      const refused = admit(body);
      if (refused) {
        send(res, refused.status, refused.body);
        return;
      }
      if (!body?.mapId || !body?.guestName) {
        send(res, 400, { error: "bad_body" });
        return;
      }
      send(res, 200, host.create({ ...body, mapRevision: body.mapRevision ?? body.mapId, name: body.name ?? "room" }));
      return;
    }
    const join = /^\/api\/rooms\/([^/]+)\/join$/.exec(path);
    if (req.method === "POST" && join) {
      const body = (await readJson(req)) as JoinRoom;
      // An outdated game is told to update before anything else.
      const refused = admit(body);
      if (refused) {
        send(res, refused.status, refused.body);
        return;
      }
      const room = host.get(decodeURIComponent(join[1]!));
      if (!room) {
        send(res, 404, { error: "not_found" });
        return;
      }
      const out = room.join(body.guestName ?? "guest", body.role === "spectator" ? "spectator" : "player", body.playerId);
      if ("error" in out) {
        send(res, 400, out);
        return;
      }
      send(res, 200, { token: out.token, room: room.view(), you: out.you });
      return;
    }
    const start = /^\/api\/rooms\/([^/]+)\/start$/.exec(path);
    if (req.method === "POST" && start) {
      const room = host.get(decodeURIComponent(start[1]!));
      const token = bearer(req);
      if (!room || !token) {
        send(res, 404, { error: "not_found" });
        return;
      }
      const out = room.start(token);
      if ("error" in out) {
        send(res, 400, out);
        return;
      }
      send(res, 200, { ok: true, config: out.config });
      return;
    }
    const load = /^\/api\/rooms\/([^/]+)\/load$/.exec(path);
    if (req.method === "POST" && load) {
      const room = host.get(decodeURIComponent(load[1]!));
      const token = bearer(req);
      if (!room || !token) {
        send(res, 404, { error: "not_found" });
        return;
      }
      const body = await readJson(req);
      const out = room.load(token, body);
      if ("error" in out) {
        send(res, 400, out);
        return;
      }
      send(res, 200, { ok: true, config: out.config });
      return;
    }
    const restart = /^\/api\/rooms\/([^/]+)\/restart$/.exec(path);
    if (req.method === "POST" && restart) {
      const room = host.get(decodeURIComponent(restart[1]!));
      const token = bearer(req);
      if (!room || !token) {
        send(res, 404, { error: "not_found" });
        return;
      }
      const out = room.restart(token);
      if ("error" in out) {
        send(res, 400, out);
        return;
      }
      send(res, 200, { ok: true, config: out.config });
      return;
    }
    const leave = /^\/api\/rooms\/([^/]+)\/leave$/.exec(path);
    if (req.method === "POST" && leave) {
      const room = host.get(decodeURIComponent(leave[1]!));
      const token = bearer(req);
      if (room && token) room.leave(token);
      send(res, 200, { ok: true });
      return;
    }
    const end = /^\/api\/rooms\/([^/]+)\/end$/.exec(path);
    if (req.method === "POST" && end) {
      const room = host.get(decodeURIComponent(end[1]!));
      const token = bearer(req);
      if (!room || !token || !room.you(token)) {
        send(res, 404, { error: "not_found" });
        return;
      }
      host.discard(room.id);
      send(res, 200, { ok: true });
      return;
    }
    if (path === "/ladder" || path.startsWith("/ladder/")) {
      if (ladder) await ladder.handle(req, res, url);
      else send(res, 503, { error: "ladder_unavailable" });
      return;
    }
    send(res, 404, { error: "not_found" });
  } catch (err) {
    send(res, 500, { error: err instanceof Error ? err.message : "error" });
  }
}

/** Maintenance is only for someone on the server itself: Nginx forwards the internet with
 * `X-Forwarded-For`, so a proxied request never counts as local. */
function fromThisMachine(req: IncomingMessage): boolean {
  const address = req.socket.remoteAddress ?? "";
  const loopback = address === "127.0.0.1" || address === "::1" || address === "::ffff:127.0.0.1";
  return loopback && !req.headers["x-forwarded-for"] && !req.headers["x-real-ip"];
}

/** The caller's address: Nginx passes it in `X-Forwarded-For` to the loopback service. */
function clientAddress(req: IncomingMessage): string {
  const address = req.socket.remoteAddress ?? "";
  const forwarded = req.headers["x-forwarded-for"];
  const loopback = address === "127.0.0.1" || address === "::1" || address === "::ffff:127.0.0.1";
  if (loopback && typeof forwarded === "string" && forwarded) return forwarded.split(",")[0]!.trim();
  return address;
}

function bearer(req: IncomingMessage): string | null {
  const h = req.headers.authorization;
  if (!h?.startsWith("Bearer ")) return null;
  return h.slice(7);
}

function send(res: ServerResponse, status: number, body: unknown): void {
  if (status === 204) {
    res.writeHead(204, CORS);
    res.end();
    return;
  }
  const data = JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", ...CORS });
  res.end(data);
}

class TooLarge extends Error {}

/** The request's JSON body; a body over `limit` bytes is refused with `TooLarge`. */
function readJson(req: IncomingMessage, limit = Infinity): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on("data", (c: Buffer) => {
      size += c.length;
      if (size > limit) {
        chunks.length = 0;
        reject(new TooLarge("too large"));
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => {
      if (size > limit) return;
      if (chunks.length === 0) {
        resolve({});
        return;
      }
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch (e) {
        reject(e);
      }
    });
    req.on("error", reject);
  });
}
