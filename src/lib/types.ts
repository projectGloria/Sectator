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
  | { event: "done"; searchId: number; total: number; elapsedMs: number; cancelled: boolean; origin: Origin };

/** Where results came from: live sources, the result cache, or the local index after every source failed. */
export type Origin = "network" | "cache" | "offline";

export const seedersOf = (r: TorrentResult) => r.live?.seeders ?? r.seeders;
export const leechersOf = (r: TorrentResult) => r.live?.leechers ?? r.leechers;

// ---- settings & library (src-tauri/src/storage, clients) ----

export type ProxyOverride = { mode: "inherit" } | { mode: "direct" } | { mode: "custom"; url: string };

export interface IndexerConfig {
  id: string;
  name: string;
  kind: string;
  enabled: boolean;
  baseUrls: string[];
  proxy: ProxyOverride;
  timeoutMs: number | null;
  apiKey: string | null;
}

export interface NetworkSettings {
  userAgent: string;
  proxy: string | null;
  dohUrl: string | null;
  clearnetTimeoutMs: number;
  torTimeoutMs: number;
}

export type ClientKind = "qbittorrent" | "transmission";

export interface ClientConfig {
  id: string;
  kind: ClientKind;
  name: string;
  url: string;
  username: string | null;
  password: string | null;
  savePath: string | null;
  category: string | null;
  paused: boolean;
}

export interface AppSettings {
  net: NetworkSettings;
  indexers: IndexerConfig[];
  cacheTtlSecs: number;
  scrapeTop: number;
  clients: ClientConfig[];
  defaultClient: string | null;
  download: DownloadSettings;
}

// ---- built-in downloader (src-tauri/src/clients/download) ----

export interface DownloadSettings {
  /** Save folder; null = the user's Downloads folder. */
  dir: string | null;
  listenPort: number;
  maxDownloadKib: number;
  maxUploadKib: number;
  /** Stop seeding at this upload ratio; 0 = no limit. */
  seedRatio: number;
}

export type DownloadState = "checking" | "metadata" | "downloading" | "seeding" | "paused" | "completed" | "error";

export interface Download {
  infoHash: string;
  name: string;
  savePath: string;
  state: DownloadState;
  progress: number;
  size: number;
  done: number;
  downRate: number;
  upRate: number;
  seeds: number;
  peers: number;
  swarmSeeds: number;
  swarmPeers: number;
  uploaded: number;
  downloaded: number;
  ratio: number;
  etaSecs: number | null;
  error: string | null;
  addedAt: number;
  completedAt: number | null;
}

export interface DownloadFile {
  index: number;
  path: string;
  size: number;
  done: number;
  wanted: boolean;
}

export interface HistoryEntry {
  query: string;
  lastUsed: number;
  uses: number;
}

export interface Preset {
  name: string;
  query: string;
}

export interface Bookmark {
  infoHash: string;
  title: string;
  magnet: string;
  size: number;
  createdAt: number;
}

export interface IndexerHealth {
  indexerId: string;
  searches: number;
  failures: number;
  avgLatencyMs: number | null;
  lastOkAt: number | null;
  lastError: string | null;
}
