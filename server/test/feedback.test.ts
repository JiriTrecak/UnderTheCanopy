// Player feedback: reports are checked, rate limited and kept in the spool with their
// screenshots, replays and saves until Linear takes them; the files are uploaded to Linear
// and embedded or linked in the issue, then deleted from the host.
import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Feedback, fileName, MAX_DESCRIPTION, MAX_REPLAY, MAX_SAVE, ORPHAN_MS, RATE_LIMIT, RATE_WINDOW_MS } from "../src/feedback";

const image = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4]);
const jpeg = image.toString("base64");
const ASSET = "https://uploads.linear.app/team/asset-1";
/** Where the fake Linear keeps an uploaded file other than the screenshot. */
const assetOf = (filename: string) => `https://uploads.linear.app/team/${encodeURIComponent(filename)}`;
const replayBytes = Buffer.concat([Buffer.from("UTCR"), Buffer.from([1, 0, 0, 0, 9, 9])]);
const saveBytes = Buffer.from([0x1f, 0x8b, 0x08, 0, 1, 2, 3]);
const REPLAY_NAME = "Echo Isles 2026-10-17 14-03 Victory.utcr";
const SAVE_NAME = "Echo Isles 2026-10-17 14-03.utcsave";
const replay = (extra: Record<string, unknown> = {}) => ({ type: "application/vnd.canopy.replay", data: replayBytes.toString("base64"), name: REPLAY_NAME, ...extra });
const save = (extra: Record<string, unknown> = {}) => ({ type: "application/gzip", data: saveBytes.toString("base64"), name: SAVE_NAME, ...extra });

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

/** A recorded request: a GraphQL operation (its name) or a file's upload. */
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
    // The screenshot is kept at ASSET, other files at an address of their name.
    const name = String(variables.filename);
    const shot = name.startsWith("screenshot.");
    const uploadFile = {
      uploadUrl: shot ? "https://upload.example/put" : `https://upload.example/put/${encodeURIComponent(name)}`,
      assetUrl: shot ? ASSET : assetOf(name),
      headers: [{ key: "x-amz-acl", value: "private" }],
    };
    const data =
      op === "Upload"
        ? { fileUpload: { success: true, uploadFile } }
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

test("a report from a match carries a replay and a save; both are uploaded and linked in the issue", async () => {
  const { feedback, api, spool, media, logs } = setup();
  const reply = feedback.submit(report({ replay: replay(), save: save() }), "1.2.3.4");
  assert.equal(reply.status, 202);
  const stored = readdirSync(media).map((f) => readFileSync(join(media, f)));
  assert.equal(stored.length, 3);
  assert.ok(stored.some((b) => b.equals(replayBytes)) && stored.some((b) => b.equals(saveBytes)));
  const [waiting] = spooled(spool);
  assert.equal(waiting.replay.name, REPLAY_NAME);
  assert.equal(waiting.report.replay, undefined, "the spool keeps the stored file's name, not the bytes");
  assert.equal(waiting.report.save, undefined);
  await feedback.settled();
  assert.deepEqual(api.ops(), ["Upload", "put", "Upload", "put", "Upload", "put", "Labels", "File"]);
  const uploads = api.calls.filter((c) => c.op === "Upload").map((c) => c.variables);
  assert.deepEqual(uploads.slice(1), [
    { contentType: "application/octet-stream", filename: REPLAY_NAME, size: replayBytes.length },
    { contentType: "application/gzip", filename: SAVE_NAME, size: saveBytes.length },
  ]);
  const puts = api.calls.filter((c) => c.op === "put");
  assert.deepEqual(puts.map((c) => c.bytes), [image.length, replayBytes.length, saveBytes.length]);
  assert.equal(puts[1]!.headers["content-type"], "application/octet-stream");
  assert.equal(puts[2]!.headers["content-type"], "application/gzip");
  const body = String(api.calls.at(-1)!.variables.input.description);
  const shot = body.indexOf(`![Screenshot of the game](${ASSET})`);
  const replayLink = body.indexOf(`[Replay of the match](${assetOf(REPLAY_NAME)})`);
  const saveLink = body.indexOf(`[Save of this moment](${assetOf(SAVE_NAME)})`);
  const table = body.indexOf("| | |");
  assert.ok(shot > 0 && shot < replayLink && replayLink < saveLink && saveLink < table, body);
  assert.ok(body.includes(`\n[Replay of the match](${assetOf(REPLAY_NAME)}): \`${REPLAY_NAME}\`\n`), "on a line of its own");
  assert.equal(feedback.waiting, 0);
  assert.deepEqual(readdirSync(media), [], "all three files are deleted once filed");
  assert.ok(logs.some((l) => l.includes(`replay ${replayBytes.length} bytes`) && l.includes(`save ${saveBytes.length} bytes`)));
});

test("replays and saves are checked: type, first bytes, size and name", () => {
  const { feedback, api, media } = setup();
  const refused = (r: unknown, what: string) => {
    const reply = feedback.submit(r, "1.2.3.4");
    assert.equal(reply.status, 400, what);
    assert.equal(reply.body.error, "bad_body", what);
    assert.ok(String(reply.body.message).length > 0, what);
  };
  refused(report({ replay: replay({ data: saveBytes.toString("base64") }) }), "a replay without UTCR");
  refused(report({ replay: replay({ data: "" }) }), "an empty replay");
  const big = Buffer.concat([Buffer.from("UTCR"), Buffer.alloc(MAX_REPLAY - 3)]);
  refused(report({ replay: replay({ data: big.toString("base64") }) }), "a replay over 1 MB");
  refused(report({ save: save({ data: replayBytes.toString("base64") }) }), "a save without the gzip magic");
  const bigSave = Buffer.concat([saveBytes, Buffer.alloc(MAX_SAVE)]);
  refused(report({ save: save({ data: bigSave.toString("base64") }) }), "a save over 4 MB");
  refused(report({ replay: replay({ type: "application/octet-stream" }) }), "a replay of the wrong type");
  refused(report({ save: save({ type: "application/zip" }) }), "a save of the wrong type");
  refused(report({ save: save({ name: "x".repeat(121) }) }), "a name over 120 characters");
  refused(report({ replay: replay(), save: save({ data: jpeg }) }), "one bad attachment refuses the report");
  assert.equal(api.calls.length, 0);
  assert.throws(() => readdirSync(media), "nothing stored");
});

test("attachment names are made safe and end in their extension", () => {
  assert.equal(fileName("replay", undefined), "replay.utcr");
  assert.equal(fileName("save", "   "), "save.utcsave");
  assert.equal(fileName("replay", REPLAY_NAME), REPLAY_NAME);
  assert.equal(fileName("save", "Echo/Isles: (1) ä_b"), "Echo-Isles- (1) -_b.utcsave");
  assert.equal(fileName("replay", "../../etc/passwd"), "..-..-etc-passwd.utcr");
  assert.equal(fileName("save", "Moment.UTCSAVE"), "Moment.UTCSAVE");
});

test("an attachment without a name is uploaded under the default name", async () => {
  const { feedback, api } = setup();
  const sent = report({ screenshot: undefined, replay: replay({ name: undefined }), save: save({ name: "  moment  " }) });
  assert.equal(feedback.submit(sent, "1.2.3.4").status, 202);
  await feedback.settled();
  const names = api.calls.filter((c) => c.op === "Upload").map((c) => c.variables.filename);
  assert.deepEqual(names, ["replay.utcr", "moment.utcsave"]);
  assert.ok(String(api.calls.at(-1)!.variables.input.description).includes(`[Replay of the match](${assetOf("replay.utcr")})`));
});

test("a report without attachments files as before, linking nothing", async () => {
  const { feedback, api } = setup();
  assert.equal(feedback.submit(report(), "1.2.3.4").status, 202);
  await feedback.settled();
  assert.deepEqual(api.ops(), ["Upload", "put", "Labels", "File"]);
  const body = String(api.calls.at(-1)!.variables.input.description);
  assert.ok(!body.includes("Replay of the match") && !body.includes("Save of this moment"));
});

test("a failed save upload is retried later; what was uploaded is not uploaded again", async () => {
  let down = true;
  const { feedback, api, spool, media } = setup({ fail: (c) => down && c.op === "put" && String(c.variables.url).includes("utcsave") });
  feedback.submit(report({ replay: replay(), save: save() }), "1.2.3.4");
  await feedback.settled();
  assert.equal(feedback.waiting, 1);
  assert.equal(readdirSync(media).length, 3, "the files wait for the next try");
  assert.ok(!api.ops().includes("File"));
  const [waiting] = spooled(spool);
  assert.equal(waiting.asset, ASSET);
  assert.equal(waiting.replay.asset, assetOf(REPLAY_NAME), "the spool remembers the uploaded replay");
  assert.equal(waiting.save.asset, undefined);
  down = false;
  const restarted = new Feedback({ apiKey: "k", teamId: "team-1", spool, mediaDir: media, fetch: api.fetch, log: () => {} });
  await restarted.retry();
  assert.equal(restarted.waiting, 0);
  const uploaded = api.calls.filter((c) => c.op === "Upload").map((c) => c.variables.filename);
  assert.deepEqual(uploaded, ["screenshot.jpg", REPLAY_NAME, SAVE_NAME, SAVE_NAME], "only the save again");
  const body = String(api.calls.at(-1)!.variables.input.description);
  assert.ok(body.includes(`[Replay of the match](${assetOf(REPLAY_NAME)})`) && body.includes(`[Save of this moment](${assetOf(SAVE_NAME)})`));
  assert.deepEqual(readdirSync(media), []);
});

test("stray replays and saves are swept; a waiting report's stay", () => {
  const { feedback, clock, media } = setup({ configured: false });
  feedback.submit(report({ screenshot: undefined, replay: replay(), save: save() }), "1.2.3.4");
  const waiting = readdirSync(media);
  assert.equal(waiting.length, 2);
  const strays = ["BBBBBBBBBBBBBBBBBBBBBBBB.utcr", "CCCCCCCCCCCCCCCCCCCCCCCC.utcsave"];
  for (const name of strays) writeFileSync(join(media, name), saveBytes);
  const old = new Date(clock.t - ORPHAN_MS - 1000);
  for (const name of [...waiting, ...strays]) utimesSync(join(media, name), old, old);
  assert.equal(feedback.sweep(), 2);
  assert.deepEqual(readdirSync(media).sort(), waiting.sort());
});
