-- Sectator local store. Everything stays on disk; nothing is synced anywhere.
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Cached merged results, keyed by infohash. `payload` is the serialized TorrentResult.
CREATE TABLE IF NOT EXISTS torrents (
    info_hash   TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    size        INTEGER NOT NULL DEFAULT 0,
    seeders     INTEGER,
    leechers    INTEGER,
    uploaded_at INTEGER,
    category    TEXT,
    payload     TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Inner file lists, kept separately so `file:` queries hit an index instead of JSON.
CREATE TABLE IF NOT EXISTS files (
    info_hash TEXT NOT NULL REFERENCES torrents(info_hash) ON DELETE CASCADE,
    path      TEXT NOT NULL,
    size      INTEGER NOT NULL,
    PRIMARY KEY (info_hash, path)
);

-- Contentless-delete FTS5 tables mirror torrents/files for instant local search.
CREATE VIRTUAL TABLE IF NOT EXISTS torrents_fts USING fts5(
    info_hash UNINDEXED, title, tokenize = 'unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    info_hash UNINDEXED, path, tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS torrents_ai AFTER INSERT ON torrents BEGIN
    INSERT INTO torrents_fts(info_hash, title) VALUES (new.info_hash, new.title);
END;
CREATE TRIGGER IF NOT EXISTS torrents_ad AFTER DELETE ON torrents BEGIN
    DELETE FROM torrents_fts WHERE info_hash = old.info_hash;
END;
CREATE TRIGGER IF NOT EXISTS torrents_au AFTER UPDATE OF title ON torrents BEGIN
    UPDATE torrents_fts SET title = new.title WHERE info_hash = old.info_hash;
END;
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(info_hash, path) VALUES (new.info_hash, new.path);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE info_hash = old.info_hash AND path = old.path;
END;

-- One row per executed search; links to the hashes it produced (TTL cache).
CREATE TABLE IF NOT EXISTS searches (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    query_key   TEXT NOT NULL,
    raw_query   TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS searches_key ON searches(query_key, created_at);

CREATE TABLE IF NOT EXISTS search_results (
    search_id INTEGER NOT NULL REFERENCES searches(id) ON DELETE CASCADE,
    info_hash TEXT NOT NULL REFERENCES torrents(info_hash) ON DELETE CASCADE,
    PRIMARY KEY (search_id, info_hash)
);

CREATE TABLE IF NOT EXISTS history (
    raw_query  TEXT PRIMARY KEY,
    last_used  INTEGER NOT NULL,
    uses       INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS presets (
    name       TEXT PRIMARY KEY,
    raw_query  TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS bookmarks (
    info_hash  TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    magnet     TEXT NOT NULL,
    size       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- Rolling per-indexer health for the status strip and circuit breaker.
CREATE TABLE IF NOT EXISTS indexer_stats (
    indexer_id   TEXT PRIMARY KEY,
    searches     INTEGER NOT NULL DEFAULT 0,
    failures     INTEGER NOT NULL DEFAULT 0,
    avg_latency  REAL,
    last_ok_at   INTEGER,
    last_error   TEXT
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
