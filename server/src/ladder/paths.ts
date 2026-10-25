/** Where the ladder keeps its data (docs/server.md#ladder). */
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type LadderPaths = {
  /** The folder holding the database, replays and media. */
  data: string;
  /** The SQLite file. */
  db: string;
  /** Online matches' replays (`.utcr`), by the file name a match row names. */
  replays: string;
  /** Hero portraits and map previews copied from the game's content. */
  media: string;
  /** The site's own files: the built CSS, images, the sample replay. */
  site: string;
};

/** `LADDER_DATA` (default `ladder-data` in the working directory) holds everything;
 * `LADDER_DB` may put the database elsewhere. */
export function ladderPaths(env: NodeJS.ProcessEnv = process.env): LadderPaths {
  const data = resolve(env.LADDER_DATA ?? "ladder-data");
  return {
    data,
    db: resolve(env.LADDER_DB ?? join(data, "ladder.sqlite")),
    replays: join(data, "replays"),
    media: join(data, "media"),
    site: fileURLToPath(new URL("./public", import.meta.url)),
  };
}
