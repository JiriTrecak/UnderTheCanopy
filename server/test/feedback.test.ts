// Player feedback: reports are checked, rate limited, their screenshots stored on the host
// and linked from a GitHub issue, and they wait in the spool while GitHub cannot take them.
import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Feedback, MAX_DESCRIPTION, MEDIA_DAYS, RATE_LIMIT, RATE_WINDOW_MS } from "../src/feedback";

const image = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4]);
const jpeg = image.toString("base64");

function report(extra: Record<string, unknown> = {}) {
  return {
    reportId: "r-0000-0001",
    installId: "install-1234",
    category: "bug",
    description: "  The beetle walked through the wall | twice @someone  ",
    meta: { version: "0.3.7", os: "macOS 26.0", gpu: "Apple M1 (Metal)", preset: "High", screen: "match", map: "twilight-ruins", mode: "skirmish", tick: 2400 },
    screenshot: { type: "image/jpeg", data: jpeg, label: "Screenshot of the game" },
    ...extra,
  };
}

type Call = { method: string; url: string; body: Record<string, unknown>; auth: string };

/** A fake GitHub: answers with `status(call)`, records every call. */
function github(status: (call: Call) => number = () => 201) {
  const calls: Call[] = [];
  const fetch = (async (url: string, init: RequestInit) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: JSON.parse(String(init.body)),
      auth: String((init.headers as Record<string, string>).authorization),
    };
    calls.push(call);
    return new Response(JSON.stringify({ number: 7 }), { status: status(call) });
  }) as typeof globalThis.fetch;
  return { calls, fetch };
}

function setup(options: { repo?: string; status?: (call: Call) => number } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "canopy-feedback-"));
  const spool = join(dir, "spool.jsonl");
  const media = join(dir, "media");
  const clock = { t: Date.UTC(2026, 9, 17, 12) };
  const gh = github(options.status);
  const logs: string[] = [];
  const feedback = new Feedback({
    repo: "repo" in options ? options.repo : "JiriTrecak/UnderTheCanopy",
    token: "secret-token",
    spool,
    mediaDir: media,
    publicBase: "https://18.134.138.1/",
    fetch: gh.fetch,
    now: () => clock.t,
    log: (m) => logs.push(m),
  });
  return { feedback, gh, clock, spool, media, logs };
}

const spooled = (path: string) => readFileSync(path, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));

test("a report becomes an issue showing its screenshot, stored on the host", async () => {
  const { feedback, gh, spool, media, logs } = setup();
  const reply = feedback.submit(report(), "1.2.3.4");
  assert.equal(reply.status, 202);
  assert.match(String(reply.body.id), /^20261017-[0-9a-f]{8}$/);
  const files = readdirSync(media);
  assert.equal(files.length, 1);
  assert.match(files[0]!, /^[A-Za-z0-9_-]{24}\.jpg$/, "an unguessable name");
  assert.deepEqual(readFileSync(join(media, files[0]!)), image);
  await feedback.settled();
  assert.equal(gh.calls.length, 1, "only the issue: nothing is committed");
  const [issue] = gh.calls;
  assert.equal(issue!.url, "https://api.github.com/repos/JiriTrecak/UnderTheCanopy/issues");
  assert.equal(issue!.body.title, "[bug] The beetle walked through the wall | twice @​someone");
  assert.deepEqual(issue!.body.labels, ["feedback", "bug", "v0.3.7"]);
  const body = String(issue!.body.body);
  assert.ok(body.startsWith("The beetle walked through the wall | twice @​someone"));
  assert.ok(body.includes(`![Screenshot of the game](https://18.134.138.1/feedback-media/${files[0]})`));
  assert.ok(body.includes("| GPU | Apple M1 (Metal) |"));
  assert.ok(body.includes("| Tick | 2400 (1:00) |"));
  assert.equal(issue!.auth, "Bearer secret-token");
  assert.equal(feedback.waiting, 0);
  assert.deepEqual(spooled(spool), []);
  assert.ok(!logs.join("\n").includes("secret-token"));
  assert.ok(!logs.join("\n").includes(jpeg));
});

test("reports are checked: description, category, metadata and screenshot", () => {
  const { feedback, gh, media } = setup();
  const bad = (r: unknown) => feedback.submit(r, "1.2.3.4").status;
  assert.equal(bad(report({ description: "   " })), 400);
  assert.equal(bad(report({ description: "x".repeat(MAX_DESCRIPTION + 1) })), 400);
  assert.equal(bad(report({ category: "rant" })), 400);
  assert.equal(bad(report({ installId: "" })), 400);
  assert.equal(bad(report({ meta: { version: "0.3.7", os: "x".repeat(500), gpu: "g" } })), 400);
  assert.equal(bad(report({ screenshot: { type: "image/gif", data: jpeg } })), 400);
  assert.equal(bad(report({ screenshot: { type: "image/png", data: jpeg } })), 400, "a JPEG claiming to be a PNG");
  assert.equal(bad(report({ screenshot: { type: "image/jpeg", data: "A".repeat(3 * 1024 * 1024) } })), 400);
  assert.equal(bad("hello"), 400);
  assert.equal(gh.calls.length, 0);
  assert.throws(() => readdirSync(media), "nothing stored");
  // Without a screenshot, and with fields a later game adds, it is fine.
  assert.equal(bad(report({ screenshot: undefined, reportId: undefined, extra: 1 })), 202);
});

test("each address and each install may send a few reports in ten minutes", async () => {
  const { feedback, clock } = setup();
  const send = (ip: string, install: string, n: number) =>
    feedback.submit(report({ reportId: `r-${ip.replaceAll(".", "")}-${install}-${n}`, installId: install }), ip).status;
  for (let i = 0; i < RATE_LIMIT; i++) assert.equal(send("1.1.1.1", "install-aaaa", i), 202);
  assert.equal(send("1.1.1.1", "install-bbbb", 9), 429, "same address");
  assert.equal(send("2.2.2.2", "install-aaaa", 9), 429, "same install");
  assert.equal(send("2.2.2.2", "install-bbbb", 9), 202, "someone else");
  clock.t += RATE_WINDOW_MS;
  feedback.prune();
  assert.equal(send("1.1.1.1", "install-aaaa", 10), 202);
  await feedback.settled();
});

test("a resent report is recognised and filed once", async () => {
  const { feedback, gh, media } = setup();
  const first = feedback.submit(report(), "1.2.3.4");
  const again = feedback.submit(report(), "1.2.3.4");
  assert.deepEqual(again, first);
  await feedback.settled();
  assert.equal(gh.calls.length, 1);
  assert.equal(readdirSync(media).length, 1);
});

test("reports wait in the spool while GitHub fails, and a retry files them with the stored screenshot", async () => {
  let down = true;
  const { feedback, gh, spool, media } = setup({ status: () => (down ? 502 : 201) });
  const reply = feedback.submit(report(), "1.2.3.4");
  assert.equal(reply.status, 202);
  await feedback.settled();
  assert.equal(feedback.waiting, 1);
  const [waiting] = spooled(spool);
  const [file] = readdirSync(media);
  assert.equal(waiting.id, reply.body.id);
  assert.equal(waiting.media.file, file, "the spool remembers the stored screenshot");
  assert.equal(waiting.report.screenshot, undefined, "and does not keep the image itself");
  // A restarted server reads the spool and files it once GitHub is back.
  down = false;
  const restarted = new Feedback({
    repo: "JiriTrecak/UnderTheCanopy",
    token: "t",
    spool,
    mediaDir: media,
    publicBase: "https://18.134.138.1",
    fetch: gh.fetch,
    log: () => {},
  });
  assert.equal(restarted.waiting, 1);
  await restarted.retry();
  assert.equal(restarted.waiting, 0);
  assert.ok(String(gh.calls.at(-1)!.body.body).includes(`https://18.134.138.1/feedback-media/${file}`));
  assert.deepEqual(readdirSync(media), [file]);
  assert.deepEqual(spooled(spool), []);
  // The resent report id is still known after the restart's load.
  assert.equal(restarted.submit(report(), "9.9.9.9").status, 202);
});

test("an issue GitHub refuses with its labels is filed without them", async () => {
  const { feedback, gh } = setup({ status: (c) => (c.body.labels ? 422 : 201) });
  feedback.submit(report(), "1.2.3.4");
  await feedback.settled();
  assert.equal(gh.calls.length, 2);
  assert.equal(gh.calls[1]!.body.labels, undefined);
  assert.equal(feedback.waiting, 0);
});

test("without a repository reports are kept in the spool", async () => {
  const { feedback, gh, spool, logs } = setup({ repo: undefined });
  assert.equal(feedback.configured, false);
  assert.equal(feedback.submit(report(), "1.2.3.4").status, 202);
  await feedback.retry();
  assert.equal(gh.calls.length, 0);
  assert.equal(spooled(spool).length, 1);
  assert.ok(logs.some((l) => l.includes("wait in the spool")));
});

test("screenshots older than 180 days are swept; other files stay", () => {
  const { feedback, clock, media } = setup();
  feedback.submit(report(), "1.2.3.4");
  feedback.submit(report({ reportId: "r-0000-0002" }), "1.2.3.4");
  const [old, recent] = readdirSync(media);
  const day = 24 * 60 * 60 * 1000;
  const at = (ms: number) => new Date(ms);
  utimesSync(join(media, old!), at(clock.t - (MEDIA_DAYS + 1) * day), at(clock.t - (MEDIA_DAYS + 1) * day));
  utimesSync(join(media, recent!), at(clock.t - 10 * day), at(clock.t - 10 * day));
  writeFileSync(join(media, "notes.txt"), "kept");
  utimesSync(join(media, "notes.txt"), at(0), at(0));
  assert.equal(feedback.sweep(), 1);
  assert.deepEqual(readdirSync(media).sort(), ["notes.txt", recent!].sort());
});
