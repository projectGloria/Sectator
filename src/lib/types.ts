// Mirrors of the Rust IPC types (src-tauri/src/models.rs, engine/runner.rs).

export type SourceType = "curated" | "dht" | "torznab";
export type Category = "movies" | "tv" | "software" | "games" | "music" | "books" | "anime" | "other";
export const CATEGORIES: Category[] = ["movies", "tv", "software", "games", "music", "books", "anime", "other"];

export interface TorrentFile {
  path: string;
  size: number;
}

export interface ParsedMeta {
  resolution: string | null;
  source: string | null;
  codec: string | null;
  dynamicRange: string[];
  audio: string[];
  season: number | null;
  episode: number | null;
  complete: boolean;
  year: number | null;
  group: string | null;
  version: string | null;
  os: string | null;
  arch: string | null;
  format: string[];
}

export interface SourceHit {
  indexerId: string;
  indexerName: string;
  sourceType: SourceType;
  detailsUrl: string | null;
  seeders: number | null;
  leechers: number | null;
}

export interface LivePeers {
  seeders: number;
  leechers: number;
  completed: number;
  scrapedAt: number;
  trackersOk: number;
}

export type SpamFlag =
  | "tinyForCategory"
  | "executableInMedia"
  | "deadSwarm"
  | "keywordStuffed"
  | "spoofedSeeders"
  | "noSeedData";

export interface TorrentResult {
  infoHash: string;
  title: string;
  size: number;
  seeders: number | null;
  leechers: number | null;
  completed: number | null;
  live: LivePeers | null;
  uploadedAt: number | null;
  uploader: string | null;
  verifiedUploader: boolean;
  category: Category | null;
  trackers: string[];
  magnet: string;
  files: TorrentFile[];
  sources: SourceHit[];
  meta: ParsedMeta;
  score: number;
  flags: SpamFlag[];
  fileMatch: boolean;
}

export type IndexerState = "searching" | "ok" | "timeout" | "error" | "cancelled" | "skipped";

export interface IndexerStatus {
  indexerId: string;
  name: string;
  sourceType: SourceType;
  state: IndexerState;
  latencyMs: number | null;
  count: number;
  error: string | null;
  proxied: boolean;
}

export type SearchEvent =
  | { event: "started"; searchId: number; query: unknown; indexers: IndexerStatus[] }
  | { event: "status"; searchId: number; status: IndexerStatus }
  | { event: "results"; searchId: number; results: TorrentResult[] }
  | { event: "removed"; searchId: number; hashes: string[] }
  | { event: "scraping"; searchId: number; count: number }
  | { event: "done"; searchId: number; total: number; elapsedMs: number; cancelled: boolean };

export const seedersOf = (r: TorrentResult) => r.live?.seeders ?? r.seeders;
export const leechersOf = (r: TorrentResult) => r.live?.leechers ?? r.leechers;
