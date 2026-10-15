/**
 * MatchHost HTTP + WebSocket (docs/server.md). Mailbox only: it relays commands and never
 * runs the game. Production binds 127.0.0.1:8787 behind Nginx HTTPS/WSS on port 443.
 */
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { WebSocketServer } from "ws";
import { MatchHost } from "./host";
import type { CreateRoom, JoinRoom } from "./protocol";

const PORT = Number(process.env.PORT ?? 8787);
const BIND = process.env.BIND ?? "127.0.0.1";
const VERSION = "0.1.0";
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "content-type, authorization",
  "access-control-allow-methods": "GET, POST, OPTIONS",
};

const host = new MatchHost();
const probes=setInterval(()=>host.pulse(),1000);
probes.unref();

const server = createServer((req, res) => {
  void handle(req, res);
});
server.on('close',()=>clearInterval(probes));

const wss = new WebSocketServer({ noServer: true });

server.on("upgrade", (req, socket, head) => {
  const url = new URL(req.url ?? "", "http://match");
  const m = /^\/match\/([^/]+)$/.exec(url.pathname);
  const token = url.searchParams.get("token") ?? "";
  const room = m ? host.get(decodeURIComponent(m[1]!)) : undefined;
  if (!room || !token) {
    socket.destroy();
    return;
  }
  wss.handleUpgrade(req, socket, head, (ws) => {
    const bound = room.bind(token, (msg) => {
      if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(msg));
    });
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
    ws.on("close", () => room.unbind(token));
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
      send(res, 200, { ok: true, version: VERSION });
      return;
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
      if (!body?.mapId || !body?.guestName) {
        send(res, 400, { error: "bad_body" });
        return;
      }
      send(res, 200, host.create({ ...body, mapRevision: body.mapRevision ?? body.mapId, name: body.name ?? "room" }));
      return;
    }
    const join = /^\/api\/rooms\/([^/]+)\/join$/.exec(path);
    if (req.method === "POST" && join) {
      const room = host.get(decodeURIComponent(join[1]!));
      if (!room) {
        send(res, 404, { error: "not_found" });
        return;
      }
      const body = (await readJson(req)) as JoinRoom;
      const out = room.join(body.guestName ?? "guest", body.role === "spectator" ? "spectator" : "player");
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

function bearer(req: IncomingMessage): string | null {
  const h = req.headers.authorization;
  if (!h?.startsWith("Bearer ")) return null;
  return h.slice(7);
}

function send(res: ServerResponse, status: number, body: unknown): void {
  const data = JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", ...CORS });
  res.end(data);
}

function readJson(req: IncomingMessage): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
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
