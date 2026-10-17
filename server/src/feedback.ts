/**
 * Player feedback (`POST /api/feedback`, docs/server.md, D59): the game sends a report with
 * a screenshot; the server checks it, stores the screenshot on this host under an
 * unguessable name (Nginx serves it at `/feedback-media/`), keeps the report in a spool
 * file and files it as an issue in the game's GitHub repository with the screenshot
 * embedded by its public URL. Reports wait in the spool while GitHub is unreachable or
 * the repository is not configured, and are retried. Screenshots go after 180 days.
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
/** How long screenshots are kept. */
export const MEDIA_DAYS = 180;
/** How often old screenshots are swept. */
export const SWEEP_MS = 24 * 60 * 60 * 1000;
/** The public path Nginx serves the media directory at. */
export const MEDIA_PATH = "/feedback-media/";

const GITHUB = "https://api.github.com";
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
  attempts: number;
};

export type Reply = { status: number; body: Record<string, unknown> };

export type FeedbackOptions = {
  /** `owner/name` of the repository issues go to; without it reports wait in the spool. */
  repo?: string;
  /** A fine-grained token for that repository (Issues: read and write). */
  token?: string;
  /** The spool file (JSON lines). */
  spool: string;
  /** Where screenshots are stored (Nginx serves it at `/feedback-media/`). */
  mediaDir: string;
  /** The public HTTPS origin screenshots are linked from (`https://18.134.138.1`). */
  publicBase?: string;
  fetch?: typeof fetch;
  now?: () => number;
  log?: (message: string) => void;
};

export class Feedback {
  private readonly repo?: string;
  private readonly token?: string;
  private readonly spool: string;
  private readonly mediaDir: string;
  private readonly publicBase?: string;
  private readonly fetch: typeof fetch;
  private readonly now: () => number;
  private readonly log: (message: string) => void;
  private queue: Spooled[] = [];
  private readonly hits = new Map<string, number[]>();
  /** Report ids already taken, with the id they were filed under. */
  private readonly seen = new Map<string, string>();
  private delivering: Promise<void> = Promise.resolve();
  private warned = false;

  constructor(options: FeedbackOptions) {
    const repo = options.repo?.trim();
    this.repo = repo && /^[\w.-]+\/[\w.-]+$/.test(repo) ? repo : undefined;
    this.token = options.token?.trim() || undefined;
    this.spool = options.spool;
    this.mediaDir = options.mediaDir;
    this.publicBase = options.publicBase?.trim().replace(/\/+$/, "") || undefined;
    this.fetch = options.fetch ?? fetch;
    this.now = options.now ?? Date.now;
    this.log = options.log ?? ((m) => console.log(m));
    this.queue = readSpool(this.spool);
    for (const s of this.queue) if (s.report.reportId) this.seen.set(s.report.reportId, s.id);
    if (repo && !this.repo) this.log("feedback: FEEDBACK_GITHUB_REPO is not owner/name");
  }

  get configured(): boolean {
    return Boolean(this.repo && this.token);
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
    // Remember recent report ids only (a resend comes within minutes).
    while (this.seen.size > 2000) this.seen.delete(this.seen.keys().next().value!);
  }

  /** Deletes screenshots older than `MEDIA_DAYS`; returns how many went. */
  sweep(): number {
    const before = this.now() - MEDIA_DAYS * 24 * 60 * 60 * 1000;
    let removed = 0;
    let names: string[];
    try {
      names = readdirSync(this.mediaDir);
    } catch {
      return 0;
    }
    for (const name of names) {
      if (!MEDIA_NAME.test(name)) continue;
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
    if (removed) this.log(`feedback: removed ${removed} screenshots older than ${MEDIA_DAYS} days`);
    return removed;
  }

  /** Writes a screenshot to the media directory; its file name, or none when it fails. */
  private store(image: Buffer, type: ImageType): string | undefined {
    const file = `${randomBytes(18).toString("base64url")}.${IMAGE_TYPES[type]}`;
    try {
      mkdirSync(this.mediaDir, { recursive: true, mode: 0o755 });
      // Readable by Nginx, which serves it; written once, never changed.
      writeFileSync(join(this.mediaDir, file), image, { mode: 0o644, flag: "wx" });
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
      if (!this.warned) this.log("feedback: no FEEDBACK_GITHUB_REPO or FEEDBACK_GITHUB_TOKEN; reports wait in the spool");
      this.warned = true;
      return;
    }
    s.attempts += 1;
    try {
      const fields = { title: issueTitle(s.report), body: issueBody(s, this.publicBase) };
      let issue = await this.createIssue({ ...fields, labels: issueLabels(s.report) });
      // 422: GitHub refused the labels (a token that may not create them); file it without.
      if (issue.status === 422) {
        this.log(`feedback ${s.id}: GitHub refused the labels; filing without them`);
        issue = await this.createIssue(fields);
      }
      if (!issue.ok) throw new Error(`GitHub answered ${issue.status}`);
      const number = ((await issue.json()) as { number?: number }).number;
      this.queue = this.queue.filter((q) => q !== s);
      this.save();
      this.log(`feedback ${s.id}: filed as issue ${number ?? "?"}`);
    } catch (e) {
      this.log(`feedback ${s.id}: not filed yet (attempt ${s.attempts}): ${e instanceof Error ? e.message : "error"}`);
    }
  }

  private createIssue(body: Record<string, unknown>): Promise<Response> {
    return this.fetch(`${GITHUB}/repos/${this.repo}/issues`, {
      method: "POST",
      headers: {
        accept: "application/vnd.github+json",
        authorization: `Bearer ${this.token}`,
        "content-type": "application/json",
        "user-agent": "canopy-matchhost",
        "x-github-api-version": "2022-11-28",
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
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

export function issueLabels(report: Spooled["report"]): string[] {
  const labels = ["feedback", report.category];
  const version = report.meta.version.replace(/[^\w.+-]/g, "").slice(0, 40);
  if (version) labels.push(`v${version}`);
  return labels;
}

export function issueBody(s: Spooled, publicBase?: string): string {
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
  let shot = "";
  if (s.media) {
    const label = cell(s.media.label ?? "Screenshot");
    shot = publicBase
      ? `\n\n![${label}](${publicBase}${MEDIA_PATH}${s.media.file})`
      : `\n\n${label}: \`${MEDIA_PATH}${s.media.file}\` on the match host (FEEDBACK_PUBLIC_BASE is not set)`;
  }
  return `${quiet(r.description)}${shot}\n\n| | |\n|---|---|\n${table}\n`;
}

/** Match time of a tick (40 ticks a second). */
function clock(tick: number): string {
  const seconds = Math.floor(tick / 40);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
