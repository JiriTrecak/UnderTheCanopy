// Player feedback: reports are checked, rate limited and kept in the spool with their
// screenshots until Linear takes them; the screenshot is uploaded to Linear and embedded in
// the issue, then deleted from the host.
import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Feedback, MAX_DESCRIPTION, ORPHAN_MS, RATE_LIMIT, RATE_WINDOW_MS } from "../src/feedback";

const image = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4]);
const jpeg = image.toString("base64");
const ASSET = "https://uploads.linear.app/team/asset-1";

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

/** A recorded request: a GraphQL operation (its name) or the screenshot's upload. */
type Call = { op: string; variables: Record<string, any>; auth?: string; headers: Record<string, string>; bytes?: number };

type Fail = (call: Call) => boolean;

/** A fake Linear: answers its GraphQL operations and the upload, fails where `fail` says. */
function linear(fail: Fail = () => false, labels = [{ id: "l-feedback", name: "Feedback", team: null }, { id: "l-bug", name: "bug", team: { id: "team-1" } }, { id: "l-other-team", name: "Idea", team: { id: "team-9" } }]) {
  const calls: Call[] = [];
  const fetch = (async (url: string, init: RequestInit) => {
    const headers = init.headers as Record<string, string>;
    if (url !== "https://api.linear.app/graphql") {
      const call = { op: "put", variables: { url }, headers, bytes: (init.body as Buffer).length };
      calls.push(call);
      return new Response(null, { status: fail(call) ? 500 : 200 });
    }
    const { query, variables } = JSON.parse(String(init.body));
    const op = /(?:mutation|query) (\w+)/.exec(query)![1]!;
    const call = { op, variables, auth: headers.authorization, headers };
    calls.push(call);
    if (fail(call)) return new Response(JSON.stringify({ errors: [{ message: "refused" }] }), { status: 200 });
    const data =
      op === "Upload"
        ? { fileUpload: { success: true, uploadFile: { uploadUrl: "https://upload.example/put", assetUrl: ASSET, headers: [{ key: "x-amz-acl", value: "private" }] } } }
        : op === "Labels"
          ? { issueLabels: { nodes: labels } }
          : { issueCreate: { success: true, issue: { identifier: "FB-7" } } };
    return new Response(JSON.stringify({ data }), { status: 200 });
  }) as typeof globalThis.fetch;
  return { calls, fetch, ops: () => calls.map((c) => c.op) };
}

function setup(options: { configured?: boolean; fail?: Fail } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "canopy-feedback-"));
  const spool = join(dir, "spool.jsonl");
  const media = join(dir, "media");
  const clock = { t: Date.UTC(2026, 9, 17, 12) };
  const api = linear(options.fail);
  const logs: string[] = [];
  const configured = options.configured ?? true;
  const feedback = new Feedback({
    apiKey: configured ? "lin_api_secret" : undefined,
    teamId: "team-1",
    spool,
    mediaDir: media,
    fetch: api.fetch,
    now: () => clock.t,
    log: (m) => logs.push(m),
  });
  return { feedback, api, clock, spool, media, logs };
}

const spooled = (path: string) => readFileSync(path, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));

test("a report becomes a Linear issue showing its uploaded screenshot, then leaves the host", async () => {
  const { feedback, api, spool, media, logs } = setup();
  const reply = feedback.submit(report(), "1.2.3.4");
  assert.equal(reply.status, 202);
  assert.match(String(reply.body.id), /^20261017-[0-9a-f]{8}$/);
  const files = readdirSync(media);
  assert.equal(files.length, 1);
  assert.deepEqual(readFileSync(join(media, files[0]!)), image);
  await feedback.settled();
  assert.deepEqual(api.ops(), ["Upload", "put", "Labels", "File"]);
  const [upload, put, , file] = api.calls;
  assert.deepEqual(upload!.variables, { contentType: "image/jpeg", filename: "screenshot.jpg", size: image.length });
  assert.equal(put!.bytes, image.length);
  assert.equal(put!.headers["x-amz-acl"], "private", "Linear's upload headers are sent");
  assert.equal(put!.headers["content-type"], "image/jpeg");
  const input = file!.variables.input;
  assert.equal(input.teamId, "team-1");
  assert.equal(input.title, "[bug] The beetle walked through the wall | twice @​someone");
  assert.deepEqual(input.labelIds, ["l-feedback", "l-bug"], "the team's and the workspace's labels, by name");
  const body = String(input.description);
  assert.ok(body.startsWith("The beetle walked through the wall | twice @​someone"));
  assert.ok(body.includes(`![Screenshot of the game](${ASSET})`));
  assert.ok(body.includes("| GPU | Apple M1 (Metal) |"));
  assert.ok(body.includes("| Tick | 2400 (1:00) |"));
  assert.equal(file!.auth, "lin_api_secret");
  assert.equal(feedback.waiting, 0);
  assert.deepEqual(spooled(spool), []);
  assert.deepEqual(readdirSync(media), [], "the screenshot is deleted once filed");
  assert.ok(logs.some((l) => l.includes("filed as FB-7")));
  assert.ok(!logs.join("\n").includes("lin_api_secret"));
  assert.ok(!logs.join("\n").includes(jpeg));
});

test("reports are checked: description, category, metadata and screenshot", () => {
  const { feedback, api, media } = setup();
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
  assert.equal(api.calls.length, 0);
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
  const { feedback, api } = setup();
  const first = feedback.submit(report(), "1.2.3.4");
  const again = feedback.submit(report(), "1.2.3.4");
  assert.deepEqual(again, first);
  await feedback.settled();
  assert.equal(api.ops().filter((op) => op === "File").length, 1);
});

test("reports wait in the spool while Linear fails; a retry after a restart files them, uploading once", async () => {
  let down = true;
  // The upload goes through, then filing fails.
  const { feedback, api, spool, media } = setup({ fail: (c) => down && c.op === "File" });
  const reply = feedback.submit(report(), "1.2.3.4");
  assert.equal(reply.status, 202);
  await feedback.settled();
  assert.equal(feedback.waiting, 1);
  const [waiting] = spooled(spool);
  const [file] = readdirSync(media);
  assert.equal(waiting.id, reply.body.id);
  assert.equal(waiting.media.file, file, "the spool remembers the stored screenshot");
  assert.equal(waiting.asset, ASSET, "and where Linear keeps it");
  assert.equal(waiting.report.screenshot, undefined, "but not the image itself");
  // A restarted server reads the spool and files it once Linear is back.
  down = false;
  const restarted = new Feedback({ apiKey: "k", teamId: "team-1", spool, mediaDir: media, fetch: api.fetch, log: () => {} });
  assert.equal(restarted.waiting, 1);
  await restarted.retry();
  assert.equal(restarted.waiting, 0);
  assert.equal(api.ops().filter((op) => op === "Upload").length, 1, "not uploaded again");
  assert.ok(String(api.calls.at(-1)!.variables.input.description).includes(ASSET));
  assert.deepEqual(readdirSync(media), []);
  assert.deepEqual(spooled(spool), []);
  // The resent report id is still known after the restart's load.
  assert.equal(restarted.submit(report(), "9.9.9.9").status, 202);
});

test("a failed upload keeps the report and its screenshot for the next try", async () => {
  const { feedback, api, media } = setup({ fail: (c) => c.op === "put" });
  feedback.submit(report(), "1.2.3.4");
  await feedback.settled();
  assert.equal(feedback.waiting, 1);
  assert.equal(readdirSync(media).length, 1);
  assert.ok(!api.ops().includes("File"));
});

test("an issue Linear refuses with its labels is filed without them", async () => {
  const { feedback, api } = setup({ fail: (c) => c.op === "File" && Boolean(c.variables.input.labelIds) });
  feedback.submit(report({ screenshot: undefined }), "1.2.3.4");
  await feedback.settled();
  const files = api.calls.filter((c) => c.op === "File");
  assert.equal(files.length, 2);
  assert.equal(files[1]!.variables.input.labelIds, undefined);
  assert.equal(feedback.waiting, 0);
});

test("without a Linear key reports are kept in the spool", async () => {
  const { feedback, api, spool, logs } = setup({ configured: false });
  assert.equal(feedback.configured, false);
  assert.equal(feedback.submit(report(), "1.2.3.4").status, 202);
  await feedback.retry();
  assert.equal(api.calls.length, 0);
  assert.equal(spooled(spool).length, 1);
  assert.ok(logs.some((l) => l.includes("wait in the spool")));
});

test("stray screenshots are swept; waiting reports' screenshots and other files stay", () => {
  const { feedback, clock, media } = setup({ configured: false });
  feedback.submit(report(), "1.2.3.4");
  const [waiting] = readdirSync(media);
  const stray = "AAAAAAAAAAAAAAAAAAAAAAAA.jpg";
  writeFileSync(join(media, stray), image);
  writeFileSync(join(media, "notes.txt"), "kept");
  const old = new Date(clock.t - ORPHAN_MS - 1000);
  for (const name of [waiting!, stray, "notes.txt"]) utimesSync(join(media, name), old, old);
  assert.equal(feedback.sweep(), 1);
  assert.deepEqual(readdirSync(media).sort(), ["notes.txt", waiting!].sort());
});
