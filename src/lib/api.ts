// Typed wrappers over the Rust IPC commands (src-tauri/src/commands/mod.rs).
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, Bookmark, ClientConfig, Download, DownloadFile, HistoryEntry, IndexerHealth, Preset, TorrentResult } from "./types";

export const api = {
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  indexerHealth: () => invoke<IndexerHealth[]>("indexer_health"),

  history: (limit = 100) => invoke<HistoryEntry[]>("history_list", { limit }),
  deleteHistory: (query?: string) => invoke<void>("history_delete", { query: query ?? null }),
  presets: () => invoke<Preset[]>("preset_list"),
  savePreset: (name: string, query: string) => invoke<void>("preset_save", { name, query }),
  deletePreset: (name: string) => invoke<void>("preset_delete", { name }),

  bookmarks: () => invoke<Bookmark[]>("bookmark_list"),
  addBookmark: (result: TorrentResult) => invoke<void>("bookmark_add", { result }),
  removeBookmark: (infoHash: string) => invoke<void>("bookmark_remove", { infoHash }),

  cacheStats: () => invoke<{ torrents: number; searches: number }>("cache_stats"),
  clearCache: () => invoke<void>("cache_clear"),

  openMagnet: (magnet: string) => invoke<void>("open_magnet", { magnet }),
  pushToClient: (magnet: string, clientId?: string) => invoke<"added" | "duplicate">("push_to_client", { magnet, clientId: clientId ?? null }),
  testClient: (config: ClientConfig) => invoke<string>("test_client", { config }),

  download: (magnet: string, savePath?: string) => invoke<string>("download_add", { magnet, savePath: savePath ?? null }),
  downloads: () => invoke<Download[]>("download_list"),
  pauseDownload: (infoHash: string) => invoke<void>("download_pause", { infoHash }),
  resumeDownload: (infoHash: string) => invoke<void>("download_resume", { infoHash }),
  recheckDownload: (infoHash: string) => invoke<void>("download_recheck", { infoHash }),
  removeDownload: (infoHash: string, deleteFiles: boolean) => invoke<void>("download_remove", { infoHash, deleteFiles }),
  downloadFiles: (infoHash: string) => invoke<DownloadFile[]>("download_files", { infoHash }),
  setWanted: (infoHash: string, files: [number, boolean][]) => invoke<void>("download_set_wanted", { infoHash, files }),
  revealDownload: (infoHash: string) => invoke<void>("download_reveal", { infoHash }),
};
