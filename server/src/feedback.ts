/**
 * Player feedback (`POST /api/feedback`, docs/server.md, D59): the game sends a report with
 * a screenshot; the server checks it, keeps the report in a spool file (its screenshot in a
 * private folder beside it) and files it as an issue in the game's Linear team, uploading
 * the screenshot to Linear and embedding it. Reports wait in the spool while Linear is
 * unreachable or not configured, and are retried; a screenshot is deleted from this host
 * once its issue exists.
 *
 * Not part of the frozen match protocol (D10): a separate route old games never call.
 */
import { randomBytes } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";

export const CATEGORIES = ["bug", "idea", "balance", "other"] as const;
export type Category = (typeof CATEGORIES)[number];

export const MAX_DESCRIPTION = 5000;
/** Decoded screenshot bytes. */
export const MAX_SCREENSHOT = 2 * 1024 * 1024;
/** The whole request: the screenshot in base64 plus the text. */
export const MAX_BODY = 3 * 1024 * 1024 + 512 * 1024;
/** Reports per IP address and per install in `RATE_WINDOW_MS`. */
export const RATE_LIMIT = 5;
/** Reports from everyone in `RATE_WINDOW_MS`, so the spool and the disk cannot be flooded. */
export const GLOBAL_LIMIT = 100;
export const RATE_WINDOW_MS = 10 * 60 * 1000;
/** Reports kept waiting at most; beyond it new ones are refused (the game queues them). */
export const MAX_SPOOLED = 500;
/** How often waiting reports are sent again. */
export const RETRY_MS = 5 * 60 * 1000;
/** Screenshots no waiting report names are deleted after this (left by a crash). */
export const ORPHAN_MS = 60 * 60 * 1000;
/** How often stray screenshots are swept. */
export const SWEEP_MS = 24 * 60 * 60 * 1000;

const LINEAR = "https://api.linear.app/graphql";
const TIMEOUT_MS = 20_000;

const IMAGE_TYPES = { "image/jpeg": "jpg", "image/png": "png", "image/webp": "webp" } as const;
type ImageType = keyof typeof IMAGE_TYPES;
/** A stored screenshot's name: 144 random bits in base64url and the extension. */
const MEDIA_NAME = /^[A-Za-z0-9_-]{24}\.(?:jpg|png|webp)$/;

const token = z.string().regex(/^[A-Za-z0-9-]{8,64}$/);
const text = (max: number) => z.string().max(max);
const screenshotSchema = z.object({
  type: z.enum(Object.keys(IMAGE_TYPES) as [ImageType, ...ImageType[]]),
  data: z.string().max(Math.ceil(MAX_SCREENSHOT / 3) * 4 + 4),
  /** What it shows ("Screenshot of the menu"). */
  label: text(80).optional(),
});
const reportSchema = z.object({
  /** The game's own id for the report: a resend after a lost reply is recognised. */
  reportId: token.optional(),
  /** A random id per installation (no personal data). */
  installId: token,
  category: z.enum(CATEGORIES),
  description: z
    .string()
    .max(MAX_DESCRIPTION * 2)
    .transform((s) => s.trim())
    .pipe(z.string().min(1).max(MAX_DESCRIPTION)),
  meta: z.object({
    version: text(40),
    os: text(120),
    gpu: text(160),
    preset: text(40).optional(),
    /** What showed: `menu` or `match`. */
    screen: text(40).optional(),
    map: text(160).optional(),
    mode: text(40).optional(),
    tick: z.number().int().min(0).max(1e9).optional(),
  }),
  screenshot: screenshotSchema.optional(),
});
export type Report = z.infer<typeof reportSchema>;

/** A report waiting to be filed; its screenshot is already stored as `media`. */
type Spooled = {
  id: string;
  receivedAt: string;
  report: Omit<Report, "screenshot">;
  media?: { file: string; label?: string };
  /** The screenshot's address in Linear once uploaded (a retry does not upload it again). */
  asset?: string;
  attempts: number;
};

export type Reply = { status: number; body: Record<string, unknown> };

export type FeedbackOptions = {
  /** A Linear API key that may create issues in the team; without it reports wait. */
  apiKey?: string;
  /** The Linear team issues go to (its id). */
  teamId?: string;
  /** The spool file (JSON lines). */
  spool: string;
  /** Where screenshots wait until their issue exists (private to the service). */
  mediaDir: string;
  fetch?: typeof fetch;
  now?: () => number;
  log?: (message: string) => void;
};

export class Feedback {
  private readonly apiKey?: string;
  private readonly teamId?: string;
  private readonly spool: string;
  private readonly mediaDir: string;
  private readonly fetch: typeof fetch;
  private readonly now: () => number;
  private readonly log: (message: string) => void;
  private queue: Spooled[] = [];
  private readonly hits = new Map<string, number[]>();
  /** Report ids already taken, with the id they were filed under. */
  private readonly seen = new Map<string, string>();
  private delivering: Promise<void> = Promise.resolve();
  private warned = false;
  /** Label ids by lowercase name, once read from Linear. */
  private labelIds?: Map<string, string>;

  constructor(options: FeedbackOptions) {
    this.apiKey = options.apiKey?.trim() || undefined;
    this.teamId = options.teamId?.trim() || undefined;
    this.spool = options.spool;
    this.mediaDir = options.mediaDir;
    this.fetch = options.fetch ?? fetch;
    this.now = options.now ?? Date.now;
    this.log = options.log ?? ((m) => console.log(m));
    this.queue = readSpool(this.spool);
    for (const s of this.queue) if (s.report.reportId) this.seen.set(s.report.reportId, s.id);
  }

  get configured(): boolean {
    return Boolean(this.apiKey && this.teamId);
  }

  /** Reports waiting to be filed. */
  get waiting(): number {
    return this.queue.length;
  }

  /** Checks and takes a report; it is filed in the background. */
  submit(raw: unknown, ip: string): Reply {
    const parsed = reportSchema.safeParse(raw);
    if (!parsed.success) {
      const issue = parsed.error.issues[0];
      const where = issue?.path.join(".") || "body";
      return { status: 400, body: { error: "bad_body", message: `${where}: ${issue?.message ?? "invalid"}` } };
    }
    const { screenshot, ...report } = parsed.data;
    let image: Buffer | undefined;
    if (screenshot) {
      image = Buffer.from(screenshot.data, "base64");
      if (image.length === 0 || image.length > MAX_SCREENSHOT || !looksLike(image, screenshot.type)) {
        return { status: 400, body: { error: "bad_body", message: "screenshot: not a valid image" } };
      }
    }
    const known = report.reportId && this.seen.get(report.reportId);
    if (known) return { status: 202, body: { id: known } };
    const now = this.now();
    const keys = ["all", `ip:${ip}`, `install:${report.installId}`];
    if (!this.allow(keys[0]!, GLOBAL_LIMIT, now) || keys.slice(1).some((k) => !this.allow(k, RATE_LIMIT, now))) {
      return { status: 429, body: { error: "rate_limited", message: "Too many reports; try again later." } };
    }
    if (this.queue.length >= MAX_SPOOLED) {
      return { status: 503, body: { error: "busy", message: "Feedback is not being taken right now." } };
    }
    for (const k of keys) this.hits.get(k)!.push(now);
    const id = newId(now);
    const spooled: Spooled = { id, receivedAt: new Date(now).toISOString(), report, attempts: 0 };
    if (screenshot && image) {
      const file = this.store(image, screenshot.type);
      if (file) spooled.media = { file, label: screenshot.label };
    }
    if (report.reportId) this.seen.set(report.reportId, id);
    this.queue.push(spooled);
    this.save();
    this.log(`feedback ${id}: ${report.category}, ${report.description.length} characters${spooled.media ? `, screenshot ${image!.length} bytes` : ""}`);
    this.schedule(() => this.deliver(spooled));
    return { status: 202, body: { id } };
  }

  /** Files every waiting report (called periodically). */
  retry(): Promise<void> {
    this.schedule(async () => {
      for (const s of [...this.queue]) await this.deliver(s);
    });
    return this.settled();
  }

  /** Resolves once the deliveries started so far have finished. */
  settled(): Promise<void> {
    return this.delivering;
  }

  /** Forgets rate-limit hits older than the window. */
  prune(): void {
    const since = this.now() - RATE_WINDOW_MS;
    for (const [key, times] of this.hits) {
      const kept = times.filter((t) => t > since);
      if (kept.length) this.hits.set(key, kept);
      else this.hits.delete(key);
    }
    // Labels made in Linear meanwhile are found at the next filing.
    this.labelIds = undefined;
    // Remember recent report ids only (a resend comes within minutes).
    while (this.seen.size > 2000) this.seen.delete(this.seen.keys().next().value!);
  }

  /** Deletes screenshots no waiting report names (older than `ORPHAN_MS`); returns how many. */
  sweep(): number {
    const before = this.now() - ORPHAN_MS;
    const named = new Set(this.queue.flatMap((s) => (s.media ? [s.media.file] : [])));
    let removed = 0;
    let names: string[];
    try {
      names = readdirSync(this.mediaDir);
    } catch {
      return 0;
    }
    for (const name of names) {
      if (!MEDIA_NAME.test(name) || named.has(name)) continue;
      const path = join(this.mediaDir, name);
      try {
        if (statSync(path).mtimeMs < before) {
          unlinkSync(path);
          removed += 1;
        }
      } catch {
        // Gone meanwhile.
      }
    }
    if (removed) this.log(`feedback: removed ${removed} stray screenshots`);
    return removed;
  }

  /** Writes a screenshot to the media directory; its file name, or none when it fails. */
  private store(image: Buffer, type: ImageType): string | undefined {
    const file = `${randomBytes(18).toString("base64url")}.${IMAGE_TYPES[type]}`;
    try {
      mkdirSync(this.mediaDir, { recursive: true, mode: 0o700 });
      writeFileSync(join(this.mediaDir, file), image, { mode: 0o600, flag: "wx" });
      return file;
    } catch (e) {
      this.log(`feedback: cannot store a screenshot: ${e instanceof Error ? e.message : "error"}`);
      return undefined;
    }
  }

  private allow(key: string, limit: number, now: number): boolean {
    const since = now - RATE_WINDOW_MS;
    const times = (this.hits.get(key) ?? []).filter((t) => t > since);
    this.hits.set(key, times);
    return times.length < limit;
  }

  /** Deliveries run one after another, so the spool is rewritten in order. */
  private schedule(work: () => Promise<void>): void {
    this.delivering = this.delivering.then(work).catch((e) => {
      this.log(`feedback: ${e instanceof Error ? e.message : "delivery failed"}`);
    });
  }

  private async deliver(s: Spooled): Promise<void> {
    if (!this.queue.includes(s)) return;
    if (!this.configured) {
      if (!this.warned) this.log("feedback: no LINEAR_API_KEY or LINEAR_TEAM_ID; reports wait in the spool");
      this.warned = true;
      return;
    }
    s.attempts += 1;
    try {
      if (s.media && !s.asset) {
        s.asset = await this.upload(s.media.file);
        this.save();
      }
      const input = { teamId: this.teamId, title: issueTitle(s.report), description: issueBody(s) };
      const labelIds = await this.labels(s.report);
      let issue: { identifier?: string } | undefined;
      try {
        issue = await this.createIssue(labelIds.length ? { ...input, labelIds } : input);
      } catch (e) {
        // A label the key may not use: file it without.
        if (!labelIds.length) throw e;
        this.log(`feedback ${s.id}: Linear refused the labels; filing without them`);
        issue = await this.createIssue(input);
      }
      this.queue = this.queue.filter((q) => q !== s);
      this.save();
      if (s.media) this.forget(s.media.file);
      this.log(`feedback ${s.id}: filed as ${issue?.identifier ?? "an issue"}`);
    } catch (e) {
      this.log(`feedback ${s.id}: not filed yet (attempt ${s.attempts}): ${e instanceof Error ? e.message : "error"}`);
    }
  }

  /** One Linear GraphQL request; its data, or an error for any failure. */
  private async graphql<T>(query: string, variables: Record<string, unknown>): Promise<T> {
    const response = await this.fetch(LINEAR, {
      method: "POST",
      headers: { authorization: this.apiKey!, "content-type": "application/json" },
      body: JSON.stringify({ query, variables }),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (!response.ok) throw new Error(`Linear answered ${response.status}`);
    const json = (await response.json()) as { data?: T; errors?: { message?: string }[] };
    if (json.errors?.length || !json.data) throw new Error(`Linear: ${json.errors?.[0]?.message ?? "no data"}`);
    return json.data;
  }

  /** Uploads a stored screenshot to Linear; its asset address. */
  private async upload(file: string): Promise<string> {
    const image = readFileSync(join(this.mediaDir, file));
    const ext = file.slice(file.lastIndexOf(".") + 1);
    const contentType = ext === "png" ? "image/png" : ext === "webp" ? "image/webp" : "image/jpeg";
    const data = await this.graphql<{
      fileUpload: { success: boolean; uploadFile?: { uploadUrl: string; assetUrl: string; headers: { key: string; value: string }[] } };
    }>(
      `mutation Upload($contentType: String!, $filename: String!, $size: Int!) {
        fileUpload(contentType: $contentType, filename: $filename, size: $size) {
          success
          uploadFile { uploadUrl assetUrl headers { key value } }
        }
      }`,
      { contentType, filename: `screenshot.${ext}`, size: image.length },
    );
    const target = data.fileUpload.uploadFile;
    if (!data.fileUpload.success || !target) throw new Error("Linear refused the screenshot");
    const headers: Record<string, string> = { "content-type": contentType, "cache-control": "public, max-age=31536000" };
    for (const h of target.headers) headers[h.key] = h.value;
    const put = await this.fetch(target.uploadUrl, {
      method: "PUT",
      headers,
      body: image,
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (!put.ok) throw new Error(`the screenshot upload answered ${put.status}`);
    return target.assetUrl;
  }

  /** The ids of the report's labels that exist in Linear (none when they cannot be read). */
  private async labels(report: Spooled["report"]): Promise<string[]> {
    if (!this.labelIds) {
      try {
        const data = await this.graphql<{ issueLabels: { nodes: { id: string; name: string; team?: { id: string } | null }[] } }>(
          `query Labels { issueLabels(first: 250) { nodes { id name team { id } } } }`,
          {},
        );
        this.labelIds = new Map(
          data.issueLabels.nodes
            .filter((l) => !l.team || l.team.id === this.teamId)
            .map((l) => [l.name.toLowerCase(), l.id]),
        );
      } catch (e) {
        this.log(`feedback: cannot read Linear's labels: ${e instanceof Error ? e.message : "error"}`);
        return [];
      }
    }
    return issueLabels(report).flatMap((name) => {
      const id = this.labelIds!.get(name.toLowerCase());
      return id ? [id] : [];
    });
  }

  private async createIssue(input: Record<string, unknown>): Promise<{ identifier?: string } | undefined> {
    const data = await this.graphql<{ issueCreate: { success: boolean; issue?: { identifier?: string } } }>(
      `mutation File($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { identifier } } }`,
      { input },
    );
    if (!data.issueCreate.success) throw new Error("Linear did not create the issue");
    return data.issueCreate.issue;
  }

  /** Deletes a screenshot whose issue exists. */
  private forget(file: string): void {
    try {
      unlinkSync(join(this.mediaDir, file));
    } catch {
      // Already gone.
    }
  }

  private save(): void {
    try {
      const temp = `${this.spool}.tmp`;
      writeFileSync(temp, this.queue.map((s) => JSON.stringify(s) + "\n").join(""), { mode: 0o600 });
      renameSync(temp, this.spool);
    } catch (e) {
      this.log(`feedback: cannot write the spool: ${e instanceof Error ? e.message : "error"}`);
    }
  }
}

function readSpool(path: string): Spooled[] {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch {
    return [];
  }
  const out: Spooled[] = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      const s = JSON.parse(line) as Spooled;
      const media = s.media === undefined || MEDIA_NAME.test(s.media.file);
      if (typeof s.id === "string" && media && reportSchema.safeParse(s.report).success) out.push(s);
    } catch {
      // A torn last line from a crash; the rest still counts.
    }
  }
  return out;
}

/** `20261017-1a2b3c4d`: the day it came in and a random part. */
function newId(now: number): string {
  return `${new Date(now).toISOString().slice(0, 10).replaceAll("-", "")}-${randomBytes(4).toString("hex")}`;
}

function looksLike(data: Buffer, type: ImageType): boolean {
  switch (type) {
    case "image/jpeg":
      return data[0] === 0xff && data[1] === 0xd8 && data[2] === 0xff;
    case "image/png":
      return data.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    case "image/webp":
      return data.subarray(0, 4).toString("latin1") === "RIFF" && data.subarray(8, 12).toString("latin1") === "WEBP";
  }
}

/** Player text in an issue never mentions anyone. */
function quiet(s: string): string {
  return s.replaceAll("@", "@​");
}

export function issueTitle(report: Spooled["report"]): string {
  const line = report.description.replace(/\s+/g, " ").trim();
  const short = line.length > 70 ? `${line.slice(0, 70).trimEnd()}…` : line;
  return `[${report.category}] ${quiet(short)}`;
}

/** Label names a report's issue gets where the team has them: Feedback and its category. */
export function issueLabels(report: Spooled["report"]): string[] {
  const category = { bug: "Bug", idea: "Idea", balance: "Balance", other: undefined }[report.category];
  return category ? ["Feedback", category] : ["Feedback"];
}

export function issueBody(s: Spooled): string {
  const r = s.report;
  const cell = (v: string) => quiet(v).replace(/\|/g, "\\|").replace(/\s+/g, " ").trim() || "–";
  const rows: [string, string | undefined][] = [
    ["Category", r.category],
    ["Version", r.meta.version],
    ["OS", r.meta.os],
    ["GPU", r.meta.gpu],
    ["Graphics", r.meta.preset],
    ["Screen", r.meta.screen],
    ["Map", r.meta.map],
    ["Mode", r.meta.mode],
    ["Tick", r.meta.tick === undefined ? undefined : `${r.meta.tick} (${clock(r.meta.tick)})`],
    ["Install", r.installId],
    ["Report", s.id],
    ["Received", s.receivedAt],
  ];
  const table = rows
    .filter(([, v]) => v !== undefined && v !== "")
    .map(([k, v]) => `| ${k} | ${cell(v!)} |`)
    .join("\n");
  const shot = s.asset ? `\n\n![${cell(s.media?.label ?? "Screenshot")}](${s.asset})` : "";
  return `${quiet(r.description)}${shot}\n\n| | |\n|---|---|\n${table}\n`;
}

/** Match time of a tick (40 ticks a second). */
function clock(tick: number): string {
  const seconds = Math.floor(tick / 40);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
