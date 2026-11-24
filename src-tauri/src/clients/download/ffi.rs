//! `cxx` bridge to the libtorrent shim in `cpp/lt_bridge.{h,cpp}`.

#[cxx::bridge(namespace = "sectator")]
pub mod bridge {
    struct SessionConfig {
        listen_port: u16,
        user_agent: String,
        /// Bytes per second; 0 = unlimited.
        download_rate_limit: i32,
        upload_rate_limit: i32,
    }

    #[derive(Debug, Clone)]
    struct TorrentStatus {
        info_hash: String,
        name: String,
        save_path: String,
        /// `lt::torrent_status::state_t`.
        state: u8,
        paused: bool,
        has_metadata: bool,
        progress: f32,
        total_wanted: i64,
        total_wanted_done: i64,
        download_rate: i32,
        upload_rate: i32,
        num_seeds: i32,
        num_peers: i32,
        list_seeds: i32,
        list_peers: i32,
        all_time_upload: i64,
        all_time_download: i64,
        added_time: i64,
        completed_time: i64,
        error: String,
    }

    #[derive(Debug, Clone)]
    struct FileEntry {
        index: i32,
        path: String,
        size: i64,
        done: i64,
        /// 0 = skip, 1–7 = low…top (4 is libtorrent's default).
        priority: u8,
    }

    struct ResumeBlob {
        info_hash: String,
        data: Vec<u8>,
    }

    unsafe extern "C++" {
        include!("sectator/cpp/lt_bridge.h");

        type Session;

        fn new_session(cfg: &SessionConfig) -> Result<UniquePtr<Session>>;
        fn apply(self: &Session, cfg: &SessionConfig);
        fn add_magnet(self: &Session, uri: &str, save_path: &str, paused: bool) -> Result<String>;
        fn add_torrent(self: &Session, data: &[u8], save_path: &str, paused: bool, seed_mode: bool) -> Result<String>;
        fn add_resume(self: &Session, data: &[u8]) -> Result<String>;
        fn list(self: &Session) -> Vec<TorrentStatus>;
        fn pause(self: &Session, hash: &str) -> bool;
        fn resume(self: &Session, hash: &str) -> bool;
        fn remove(self: &Session, hash: &str, delete_files: bool) -> bool;
        fn recheck(self: &Session, hash: &str) -> bool;
        fn files(self: &Session, hash: &str) -> Vec<FileEntry>;
        fn set_priorities(self: &Session, hash: &str, priorities: &[u8]) -> bool;
        fn save_resume(self: &Session, all: bool, timeout_ms: u32) -> Vec<ResumeBlob>;
        fn make_torrent(path: &str, piece_size: u32) -> Result<Vec<u8>>;
    }
}

// SAFETY: every `Session` method forwards to `lt::session`, whose public API is
// thread-safe (calls are posted to libtorrent's own network thread); the one
// stateful path, alert popping in `save_resume`, is guarded by a C++ mutex.
unsafe impl Send for bridge::Session {}
unsafe impl Sync for bridge::Session {}
