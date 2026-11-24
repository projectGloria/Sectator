//! Built-in downloader: a libtorrent session (through the `cxx` shim in `ffi`) plus
//! fast-resume persistence, so Sectator can fetch torrents without an external client.
//!
//! The session starts lazily on the first download (or at launch when saved downloads
//! exist), so users who only search never open a listening port.

pub mod ffi;
pub mod icon;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use cxx::UniquePtr;
use serde::{Deserialize, Serialize};

use ffi::bridge as lt;

use crate::parser::kind::{classify, Content};

const RESUME_EXT: &str = "fastresume";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    /// Folder new downloads are saved to; `None` = the user's Downloads folder.
    pub dir: Option<String>,
    /// TCP/UDP port for incoming peers (forward it on the router for better speeds).
    pub listen_port: u16,
    /// KiB/s; 0 = unlimited.
    pub max_download_kib: u32,
    pub max_upload_kib: u32,
    /// Stop seeding a finished download at this upload ratio; 0 = seed until paused.
    pub seed_ratio: f64,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self { dir: None, listen_port: 50413, max_download_kib: 0, max_upload_kib: 0, seed_ratio: 2.0 }
    }
}

impl DownloadSettings {
    pub fn sanitized(mut self) -> Self {
        self.dir = self.dir.take().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
        if self.listen_port != 0 && self.listen_port < 1024 {
            self.listen_port = DownloadSettings::default().listen_port;
        }
        self.max_download_kib = self.max_download_kib.min(2_000_000);
        self.max_upload_kib = self.max_upload_kib.min(2_000_000);
        self.seed_ratio = if self.seed_ratio.is_finite() { self.seed_ratio.clamp(0.0, 100.0) } else { 0.0 };
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Checking,
    /// Magnet added; fetching the torrent's metadata from peers.
    Metadata,
    Downloading,
    Seeding,
    Paused,
    /// Finished and stopped (by the user or the seed-ratio limit).
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub info_hash: String,
    pub name: String,
    pub save_path: String,
    pub state: DownloadState,
    /// 0.0–1.0 of the wanted (non-skipped) files.
    pub progress: f64,
    pub size: i64,
    pub done: i64,
    pub down_rate: i64,
    pub up_rate: i64,
    pub seeds: i32,
    pub peers: i32,
    pub swarm_seeds: i32,
    pub swarm_peers: i32,
    pub uploaded: i64,
    pub downloaded: i64,
    pub ratio: f64,
    pub eta_secs: Option<i64>,
    pub error: Option<String>,
    pub added_at: i64,
    pub completed_at: Option<i64>,
    pub sequential: bool,
    /// What the content is (`kind`) and its main extension (`mainExt`), for the UI icon.
    #[serde(flatten)]
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFile {
    pub index: i32,
    pub path: String,
    pub size: i64,
    pub done: i64,
    pub wanted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStart {
    pub info_hash: String,
    pub already_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPreview {
    pub info_hash: String,
    pub name: String,
    pub save_path: String,
    pub has_metadata: bool,
    pub sequential: bool,
    pub seeds: i32,
    pub peers: i32,
    pub swarm_seeds: i32,
    pub swarm_peers: i32,
    pub total_size: i64,
    pub files: Vec<DownloadFile>,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("not a magnet link")]
    NotMagnet,
    #[error("no such download")]
    Unknown,
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<cxx::Exception> for DownloadError {
    fn from(e: cxx::Exception) -> Self {
        DownloadError::Engine(e.what().to_string())
    }
}

pub type Result<T> = std::result::Result<T, DownloadError>;

// lt::torrent_status::state_t
const CHECKING_FILES: u8 = 1;
const DOWNLOADING_METADATA: u8 = 2;
const DOWNLOADING: u8 = 3;
const CHECKING_RESUME: u8 = 7;

impl From<&lt::TorrentStatus> for Download {
    fn from(s: &lt::TorrentStatus) -> Self {
        let complete = s.has_metadata && s.total_wanted > 0 && s.total_wanted_done >= s.total_wanted;
        let state = if !s.error.is_empty() {
            DownloadState::Error
        } else if s.state == CHECKING_FILES || s.state == CHECKING_RESUME {
            DownloadState::Checking
        } else if s.paused {
            if complete { DownloadState::Completed } else { DownloadState::Paused }
        } else if s.state == DOWNLOADING_METADATA {
            DownloadState::Metadata
        } else if s.state == DOWNLOADING && !complete {
            DownloadState::Downloading
        } else {
            DownloadState::Seeding
        };
        let left = (s.total_wanted - s.total_wanted_done).max(0);
        let eta_secs = (state == DownloadState::Downloading && s.download_rate > 0).then(|| left / i64::from(s.download_rate));
        // Ratio against what we hold, so a seed added from local files still counts.
        let base = s.all_time_download.max(s.total_wanted_done);
        Download {
            info_hash: s.info_hash.clone(),
            name: if s.name.is_empty() { s.info_hash.clone() } else { s.name.clone() },
            save_path: s.save_path.clone(),
            state,
            progress: f64::from(s.progress.clamp(0.0, 1.0)),
            size: s.total_wanted,
            done: s.total_wanted_done,
            down_rate: i64::from(s.download_rate),
            up_rate: i64::from(s.upload_rate),
            seeds: s.num_seeds,
            peers: (s.num_peers - s.num_seeds).max(0),
            swarm_seeds: s.list_seeds,
            swarm_peers: s.list_peers,
            uploaded: s.all_time_upload,
            downloaded: s.all_time_download,
            ratio: if base > 0 { s.all_time_upload as f64 / base as f64 } else { 0.0 },
            eta_secs,
            error: Some(s.error.clone()).filter(|e| !e.is_empty()),
            added_at: s.added_time,
            completed_at: Some(s.completed_time).filter(|t| *t > 0),
            sequential: s.sequential,
            content: classify(&s.name, []),
        }
    }
}

/// Compute libtorrent file priorities (0 = skip, 1–7 = low…top) for sequential mode.
/// Gives the first incomplete wanted file top priority (7), the next one high (6) for
/// seamless handoff, and the remaining wanted files low (1), so multi-file torrents
/// (e.g. season packs) download strictly in file order while preserving `total_wanted`.
/// When `sequential` is false, restores all wanted files to normal priority (4).
fn sequential_priorities(files: &[lt::FileEntry], sequential: bool) -> Option<Vec<u8>> {
    let count = files.iter().map(|f| f.index + 1).max().unwrap_or(0).max(0) as usize;
    if count == 0 {
        return None;
    }
    let mut current = vec![0u8; count];
    for f in files {
        current[f.index as usize] = f.priority;
    }
    let mut next = current.clone();
    if sequential {
        let wanted_count = files.iter().filter(|f| f.priority > 0).count();
        if wanted_count > 1 {
            let mut rank = 0u8;
            for f in files {
                if f.priority == 0 {
                    continue;
                }
                let p = if f.done < f.size {
                    rank = rank.saturating_add(1);
                    match rank {
                        1 => 7,
                        2 => 6,
                        _ => 1,
                    }
                } else {
                    4
                };
                next[f.index as usize] = p;
            }
        }
    } else {
        for f in files {
            if f.priority > 0 {
                next[f.index as usize] = 4;
            }
        }
    }
    (next != current).then_some(next)
}

/// A seeding download that has given back `limit`× what it holds (0 = no limit).
pub fn over_ratio(d: &Download, limit: f64) -> bool {
    limit > 0.0 && d.state == DownloadState::Seeding && d.ratio >= limit
}

pub struct Downloader {
    /// Where `<infohash>.fastresume` files live.
    state_dir: PathBuf,
    /// Fallback save folder when the settings name none.
    default_dir: PathBuf,
    user_agent: String,
    settings: Mutex<DownloadSettings>,
    session: OnceLock<UniquePtr<lt::Session>>,
    init: Mutex<()>,
    /// Removed this run: never write their resume data back.
    removed: Mutex<HashSet<String>>,
    /// Resumed by hand after the ratio limit stopped them.
    ratio_exempt: Mutex<HashSet<String>>,
    /// Content kind per hash, worked out once the file list is known.
    kinds: Mutex<HashMap<String, Content>>,
    /// Magnets staged for the download confirmation popup (hidden from `list` and `persist`).
    staging: Mutex<HashSet<String>>,
}

impl Downloader {
    pub fn new(state_dir: PathBuf, default_dir: PathBuf, settings: DownloadSettings, user_agent: String) -> Self {
        Self {
            state_dir,
            default_dir,
            user_agent,
            settings: Mutex::new(settings.sanitized()),
            session: OnceLock::new(),
            init: Mutex::new(()),
            removed: Mutex::new(HashSet::new()),
            ratio_exempt: Mutex::new(HashSet::new()),
            kinds: Mutex::new(HashMap::new()),
            staging: Mutex::new(HashSet::new()),
        }
    }

    fn config(&self) -> lt::SessionConfig {
        let s = self.settings.lock().unwrap();
        let rate = |kib: u32| i32::try_from(u64::from(kib) * 1024).unwrap_or(i32::MAX);
        lt::SessionConfig {
            listen_port: s.listen_port,
            user_agent: self.user_agent.clone(),
            download_rate_limit: rate(s.max_download_kib),
            upload_rate_limit: rate(s.max_upload_kib),
        }
    }

    fn resume_files(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.state_dir) else { return Vec::new() };
        rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == RESUME_EXT)).collect()
    }

    pub fn has_saved(&self) -> bool {
        !self.resume_files().is_empty()
    }

    pub fn started(&self) -> bool {
        self.session.get().is_some()
    }

    /// The running session, starting it (and restoring saved downloads) on first use.
    fn session(&self) -> Result<&lt::Session> {
        if let Some(s) = self.session.get() {
            return Ok(s);
        }
        let _g = self.init.lock().unwrap();
        if let Some(s) = self.session.get() {
            return Ok(s);
        }
        let ses = lt::new_session(&self.config())?;
        for path in self.resume_files() {
            match std::fs::read(&path).map_err(DownloadError::from).and_then(|b| Ok(ses.add_resume(&b)?)) {
                Ok(_) => {}
                Err(e) => eprintln!("skipping saved download {}: {e}", path.display()),
            }
        }
        let _ = self.session.set(ses);
        Ok(self.session.get().expect("just set"))
    }

    pub fn start(&self) -> Result<()> {
        self.session().map(|_| ())
    }

    pub fn settings(&self) -> DownloadSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn apply_settings(&self, s: DownloadSettings) {
        *self.settings.lock().unwrap() = s.sanitized();
        if let Some(ses) = self.session.get() {
            ses.apply(&self.config());
        }
    }

    fn save_dir(&self, save_path: Option<&str>) -> Result<PathBuf> {
        let dir = save_path
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| self.settings.lock().unwrap().dir.clone().map(PathBuf::from))
            .unwrap_or_else(|| self.default_dir.clone());
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn added(&self, hash: &str) {
        self.removed.lock().unwrap().remove(hash);
        self.staging.lock().unwrap().remove(hash);
    }

    /// Start downloading a magnet into `save_path` (else the configured folder).
    /// Adding one that is already present returns its hash and changes nothing.
    pub fn add_magnet(&self, magnet: &str, save_path: Option<&str>, paused: bool) -> Result<String> {
        if !magnet.starts_with("magnet:?") || magnet.chars().any(char::is_control) {
            return Err(DownloadError::NotMagnet);
        }
        let dir = self.save_dir(save_path)?;
        let hash = self.session()?.add_magnet(magnet, &dir.to_string_lossy(), paused)?;
        self.added(&hash);
        Ok(hash)
    }

    /// Stage a magnet to fetch its metadata without downloading file data.
    /// If the torrent is already an active (non-staged) download, leaves it untouched and sets `already_exists`.
    pub fn preview_start(&self, magnet: &str, save_path: Option<&str>) -> Result<PreviewStart> {
        if !magnet.starts_with("magnet:?") || magnet.chars().any(char::is_control) {
            return Err(DownloadError::NotMagnet);
        }
        let ses = self.session()?;
        let existing: HashSet<String> = {
            let staged = self.staging.lock().unwrap();
            ses.list()
                .into_iter()
                .map(|s| s.info_hash)
                .filter(|h| !staged.contains(h))
                .collect()
        };
        let dir = self.save_dir(save_path)?;
        let hash = ses.add_magnet_preview(magnet, &dir.to_string_lossy())?;
        if existing.contains(&hash) {
            return Ok(PreviewStart { info_hash: hash, already_exists: true });
        }
        self.removed.lock().unwrap().remove(&hash);
        self.staging.lock().unwrap().insert(hash.clone());
        Ok(PreviewStart { info_hash: hash, already_exists: false })
    }

    /// Inspect metadata and file list of a staged (or existing) torrent.
    pub fn preview_status(&self, hash: &str) -> Result<DownloadPreview> {
        let ses = self.running()?;
        let st = ses.list().into_iter().find(|s| s.info_hash == hash).ok_or(DownloadError::Unknown)?;
        let files: Vec<DownloadFile> = if st.has_metadata {
            ses.files(hash)
                .into_iter()
                .map(|f| DownloadFile { index: f.index, path: f.path, size: f.size, done: f.done, wanted: f.priority > 0 })
                .collect()
        } else {
            Vec::new()
        };
        let total_size = files.iter().map(|f| f.size).sum();
        Ok(DownloadPreview {
            info_hash: st.info_hash,
            name: if st.name.is_empty() { hash.to_string() } else { st.name },
            save_path: st.save_path,
            has_metadata: st.has_metadata,
            sequential: st.sequential,
            seeds: st.num_seeds,
            peers: (st.num_peers - st.num_seeds).max(0),
            swarm_seeds: st.list_seeds,
            swarm_peers: st.list_peers,
            total_size,
            files,
            error: Some(st.error).filter(|e| !e.is_empty()),
        })
    }

    /// Promote a staged magnet into a real download (or update an existing one) with
    /// the chosen file selection, sequential mode, and optional save folder.
    pub fn preview_confirm(&self, hash: &str, save_path: Option<&str>, wanted: Option<&[(i32, bool)]>, sequential: bool) -> Result<()> {
        let ses = self.running()?;
        let current = ses.list().into_iter().find(|s| s.info_hash == hash).ok_or(DownloadError::Unknown)?;
        if let Some(sp) = save_path.map(str::trim).filter(|p| !p.is_empty()) {
            let target = self.save_dir(Some(sp))?;
            if Path::new(&current.save_path) != target {
                Self::check(ses.move_storage(hash, &target.to_string_lossy()))?;
            }
        }
        // Set sequential mode BEFORE enabling file priorities so libtorrent's piece
        // picker starts in sequential order from the very first piece request.
        Self::check(ses.set_sequential(hash, sequential))?;
        Self::check(ses.enable_default_download(hash))?;
        if let Some(files) = wanted {
            if current.has_metadata {
                self.set_wanted(hash, files)?;
            }
        }
        if current.has_metadata {
            self.apply_sequential_priorities(hash, sequential);
        }
        Self::check(ses.resume(hash))?;
        self.staging.lock().unwrap().remove(hash);
        self.removed.lock().unwrap().remove(hash);
        Ok(())
    }

    /// Cancel a staged preview, removing the torrent only if it was staged by `preview_start`.
    pub fn preview_cancel(&self, hash: &str) -> Result<()> {
        if !self.staging.lock().unwrap().remove(hash) {
            return Ok(());
        }
        if let Ok(ses) = self.running() {
            let _ = ses.remove(hash, false);
        }
        self.removed.lock().unwrap().insert(hash.to_string());
        self.kinds.lock().unwrap().remove(hash);
        Ok(())
    }

    /// Toggle sequential piece downloading on an active torrent.
    pub fn set_sequential(&self, hash: &str, sequential: bool) -> Result<()> {
        Self::check(self.running()?.set_sequential(hash, sequential))?;
        self.apply_sequential_priorities(hash, sequential);
        Ok(())
    }

    fn apply_sequential_priorities(&self, hash: &str, sequential: bool) {
        let Ok(ses) = self.running() else { return };
        let files = ses.files(hash);
        if let Some(prio) = sequential_priorities(&files, sequential) {
            let _ = ses.set_priorities(hash, &prio);
        }
    }

    /// Keep file priorities ordered for every active sequential download so multi-file
    /// torrents advance strictly from file 1 to file N as each file completes.
    pub fn enforce_sequential(&self) {
        let Some(ses) = self.session.get().and_then(|s| s.as_ref()) else { return };
        let staged = self.staging.lock().unwrap().clone();
        for s in ses.list() {
            if s.sequential && s.has_metadata && !staged.contains(&s.info_hash) {
                let files = ses.files(&s.info_hash);
                if let Some(prio) = sequential_priorities(&files, true) {
                    let _ = ses.set_priorities(&s.info_hash, &prio);
                }
            }
        }
    }

    /// Add a `.torrent`; `seed` trusts files already in `save_path` without rechecking.
    pub fn add_torrent_file(&self, data: &[u8], save_path: Option<&str>, paused: bool, seed: bool) -> Result<String> {
        let dir = self.save_dir(save_path)?;
        let hash = self.session()?.add_torrent(data, &dir.to_string_lossy(), paused, seed)?;
        self.added(&hash);
        Ok(hash)
    }

    pub fn list(&self) -> Vec<Download> {
        let Some(ses) = self.session.get().and_then(|s| s.as_ref()) else { return Vec::new() };
        let staged = self.staging.lock().unwrap().clone();
        let mut kinds = self.kinds.lock().unwrap();
        ses.list()
            .iter()
            .filter(|s| !staged.contains(&s.info_hash))
            .map(|s| {
                let mut d = Download::from(s);
                if let Some(c) = kinds.get(&d.info_hash) {
                    d.content = c.clone();
                } else if s.has_metadata {
                    let files = ses.files(&d.info_hash);
                    if !files.is_empty() {
                        d.content = classify(&d.name, files.iter().map(|f| (f.path.as_str(), f.size)));
                        kinds.insert(d.info_hash.clone(), d.content.clone());
                    }
                }
                d
            })
            .collect()
    }

    pub fn get(&self, hash: &str) -> Option<Download> {
        self.list().into_iter().find(|d| d.info_hash == hash)
    }

    fn running(&self) -> Result<&lt::Session> {
        self.session.get().and_then(|s| s.as_ref()).ok_or(DownloadError::Unknown)
    }

    fn check(found: bool) -> Result<()> {
        if found { Ok(()) } else { Err(DownloadError::Unknown) }
    }

    pub fn pause(&self, hash: &str) -> Result<()> {
        Self::check(self.running()?.pause(hash))
    }

    pub fn resume(&self, hash: &str) -> Result<()> {
        let ses = self.running()?;
        Self::check(ses.resume(hash))?;
        if self.get(hash).is_some_and(|d| d.progress >= 1.0) {
            self.ratio_exempt.lock().unwrap().insert(hash.to_string());
        }
        Ok(())
    }

    pub fn recheck(&self, hash: &str) -> Result<()> {
        Self::check(self.running()?.recheck(hash))
    }

    /// Drop a download (and its data when `delete_files`), forgetting its resume file.
    pub fn remove(&self, hash: &str, delete_files: bool) -> Result<()> {
        Self::check(self.running()?.remove(hash, delete_files))?;
        self.removed.lock().unwrap().insert(hash.to_string());
        self.staging.lock().unwrap().remove(hash);
        self.ratio_exempt.lock().unwrap().remove(hash);
        self.kinds.lock().unwrap().remove(hash);
        match std::fs::remove_file(self.resume_path(hash)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub fn files(&self, hash: &str) -> Result<Vec<DownloadFile>> {
        Ok(self
            .running()?
            .files(hash)
            .into_iter()
            .map(|f| DownloadFile { index: f.index, path: f.path, size: f.size, done: f.done, wanted: f.priority > 0 })
            .collect())
    }

    /// Choose which files to fetch, by position in `files()` order of indexes.
    pub fn set_wanted(&self, hash: &str, wanted: &[(i32, bool)]) -> Result<()> {
        let ses = self.running()?;
        let files = ses.files(hash);
        let count = files.iter().map(|f| f.index + 1).max().unwrap_or(0).max(0) as usize;
        if count == 0 {
            return Err(DownloadError::Engine("the file list isn't known yet".into()));
        }
        let mut prio = vec![0u8; count];
        for f in &files {
            prio[f.index as usize] = f.priority;
        }
        for &(i, w) in wanted {
            if let Some(p) = usize::try_from(i).ok().and_then(|i| prio.get_mut(i)) {
                *p = if w { 4 } else { 0 };
            }
        }
        Self::check(ses.set_priorities(hash, &prio))?;
        if ses.list().iter().any(|s| s.info_hash == hash && s.sequential) {
            self.apply_sequential_priorities(hash, true);
        }
        Ok(())
    }

    /// Where a download's data lives on disk (its folder or single file).
    pub fn content_path(&self, hash: &str) -> Option<PathBuf> {
        let d = self.get(hash)?;
        let p = Path::new(&d.save_path).join(&d.name);
        Some(if p.exists() { p } else { PathBuf::from(d.save_path) })
    }

    /// The exact on-disk path for one file inside a download (by libtorrent file index).
    pub fn file_path(&self, hash: &str, file_index: i32) -> Option<PathBuf> {
        let d = self.get(hash)?;
        let f = self.files(hash).ok()?.into_iter().find(|f| f.index == file_index)?;
        let p = Path::new(&d.save_path).join(&f.path);
        p.exists().then_some(p)
    }

    /// Pause seeding downloads that reached the ratio limit; returns their hashes.
    pub fn enforce_ratio(&self) -> Vec<String> {
        let limit = self.settings.lock().unwrap().seed_ratio;
        let exempt = self.ratio_exempt.lock().unwrap().clone();
        let mut stopped = Vec::new();
        for d in self.list() {
            if over_ratio(&d, limit) && !exempt.contains(&d.info_hash) && self.pause(&d.info_hash).is_ok() {
                stopped.push(d.info_hash);
            }
        }
        stopped
    }

    fn resume_path(&self, hash: &str) -> PathBuf {
        self.state_dir.join(format!("{hash}.{RESUME_EXT}"))
    }

    /// Write fast-resume files for every download (`all`) or only changed ones.
    /// Returns how many were written.
    pub fn persist(&self, all: bool, timeout_ms: u32) -> Result<usize> {
        let Some(ses) = self.session.get() else { return Ok(0) };
        let blobs = ses.save_resume(all, timeout_ms);
        if blobs.is_empty() {
            return Ok(0);
        }
        std::fs::create_dir_all(&self.state_dir)?;
        let removed = self.removed.lock().unwrap().clone();
        let staged = self.staging.lock().unwrap().clone();
        let mut n = 0;
        for b in blobs
            .iter()
            .filter(|b| !removed.contains(&b.info_hash) && !staged.contains(&b.info_hash) && b.info_hash.len() == 40)
        {
            let path = self.resume_path(&b.info_hash);
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &b.data)?;
            std::fs::rename(&tmp, &path)?;
            n += 1;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn status() -> lt::TorrentStatus {
        lt::TorrentStatus {
            info_hash: "ab".repeat(20),
            name: "Fargo".into(),
            save_path: "C:\\Users\\you\\Downloads".into(),
            state: DOWNLOADING,
            paused: false,
            sequential: false,
            has_metadata: true,
            progress: 0.5,
            total_wanted: 1_000,
            total_wanted_done: 500,
            download_rate: 100,
            upload_rate: 10,
            num_seeds: 3,
            num_peers: 5,
            list_seeds: 40,
            list_peers: 60,
            all_time_upload: 250,
            all_time_download: 500,
            added_time: 1_700_000_000,
            completed_time: 0,
            error: String::new(),
        }
    }

    #[test]
    fn status_mapping() {
        let d = Download::from(&status());
        assert_eq!(d.state, DownloadState::Downloading);
        assert_eq!((d.seeds, d.peers), (3, 2));
        assert_eq!(d.eta_secs, Some(5));
        assert!((d.ratio - 0.5).abs() < 1e-9);
        assert_eq!(d.completed_at, None);

        let paused = Download::from(&lt::TorrentStatus { paused: true, ..status() });
        assert_eq!((paused.state, paused.eta_secs), (DownloadState::Paused, None));

        let done = lt::TorrentStatus { state: 5, total_wanted_done: 1_000, progress: 1.0, all_time_upload: 2_000, completed_time: 5, ..status() };
        let d = Download::from(&done);
        assert_eq!(d.state, DownloadState::Seeding);
        assert!(over_ratio(&d, 2.0) && !over_ratio(&d, 0.0) && !over_ratio(&d, 2.5));
        assert_eq!(Download::from(&lt::TorrentStatus { paused: true, ..done.clone() }).state, DownloadState::Completed);

        let meta = Download::from(&lt::TorrentStatus { state: DOWNLOADING_METADATA, has_metadata: false, name: String::new(), ..status() });
        assert_eq!((meta.state, meta.name.len()), (DownloadState::Metadata, 40));
        let err = Download::from(&lt::TorrentStatus { error: "disk full".into(), ..status() });
        assert_eq!((err.state, err.error.as_deref()), (DownloadState::Error, Some("disk full")));
        assert_eq!(Download::from(&lt::TorrentStatus { state: CHECKING_RESUME, ..status() }).state, DownloadState::Checking);
    }

    #[test]
    fn sequential_priorities_order_and_restore() {
        let files = vec![
            lt::FileEntry { index: 0, path: "ep01.mkv".into(), size: 100, done: 100, priority: 4 },
            lt::FileEntry { index: 1, path: "ep02.mkv".into(), size: 100, done: 20, priority: 4 },
            lt::FileEntry { index: 2, path: "ep03.mkv".into(), size: 100, done: 0, priority: 0 },
            lt::FileEntry { index: 3, path: "ep04.mkv".into(), size: 100, done: 0, priority: 4 },
            lt::FileEntry { index: 4, path: "ep05.mkv".into(), size: 100, done: 0, priority: 4 },
        ];
        // ep01 is done (4), ep02 is first incomplete wanted (7), ep03 is skipped (0), ep04 is next (6), ep05 is queued (1).
        assert_eq!(sequential_priorities(&files, true), Some(vec![4, 7, 0, 6, 1]));

        let ordered: Vec<_> = files
            .iter()
            .zip([4, 7, 0, 6, 1])
            .map(|(f, p)| lt::FileEntry { priority: p, ..f.clone() })
            .collect();
        // Already ordered -> no redundant update.
        assert_eq!(sequential_priorities(&ordered, true), None);
        // Disabling sequential restores all wanted files to 4 while keeping skipped at 0.
        assert_eq!(sequential_priorities(&ordered, false), Some(vec![4, 4, 0, 4, 4]));
    }

    #[test]
    fn settings_sanitize() {
        let s = DownloadSettings { dir: Some("  ".into()), listen_port: 80, seed_ratio: f64::NAN, max_upload_kib: u32::MAX, ..Default::default() }.sanitized();
        assert_eq!(s.dir, None);
        assert_eq!(s.listen_port, DownloadSettings::default().listen_port);
        assert_eq!((s.seed_ratio, s.max_upload_kib), (0.0, 2_000_000));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("sectator-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn wait_for(dl: &Downloader, hash: &str, what: impl Fn(&Download) -> bool) -> Download {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(d) = dl.get(hash).filter(|d| what(d)) {
                return d;
            }
            assert!(Instant::now() < until, "timed out waiting on {hash}: {:?}", dl.get(hash));
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn downloader(root: &Path) -> Downloader {
        let s = DownloadSettings { listen_port: 0, ..Default::default() };
        Downloader::new(root.join("state"), root.join("dl"), s, "Sectator-test".into())
    }

    /// Real libtorrent session: seed a freshly made torrent, park a magnet, control
    /// both, and restore them from fast-resume files in a second session.
    #[test]
    fn libtorrent_session_round_trip() {
        let root = temp_dir("dl");
        let data_dir = root.join("seed");
        std::fs::create_dir_all(&data_dir).unwrap();
        let file = data_dir.join("sample.bin");
        std::fs::write(&file, (0..300_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect::<Vec<u8>>()).unwrap();
        let torrent = lt::make_torrent(&file.to_string_lossy(), 0).unwrap();

        let dl = downloader(&root);
        assert!(!dl.started() && dl.list().is_empty(), "nothing starts until used");
        let seed = dl.add_torrent_file(&torrent, Some(&data_dir.to_string_lossy()), false, true).unwrap();
        assert_eq!(seed.len(), 40);
        let d = wait_for(&dl, &seed, |d| d.state == DownloadState::Seeding);
        assert_eq!((d.name.as_str(), d.size, d.progress), ("sample.bin", 300_000, 1.0));
        assert_eq!(dl.content_path(&seed).unwrap(), file);

        let files = dl.files(&seed).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!((files[0].path.as_str(), files[0].size, files[0].wanted), ("sample.bin", 300_000, true));
        assert_eq!(dl.file_path(&seed, files[0].index).unwrap(), file);

        // Sequential download flag toggle.
        assert!(!dl.get(&seed).unwrap().sequential);
        dl.set_sequential(&seed, true).unwrap();
        let d = wait_for(&dl, &seed, |d| d.sequential);
        assert!(d.sequential);
        dl.set_sequential(&seed, false).unwrap();
        wait_for(&dl, &seed, |d| !d.sequential);

        // Staged preview stays hidden from `list()` and `persist()`, and `preview_cancel` removes it.
        let staged_hash = "fedcba9876543210fedcba9876543210fedcba98";
        let staged_magnet = format!("magnet:?xt=urn:btih:{staged_hash}&dn=Staged");
        let ps = dl.preview_start(&staged_magnet, None).unwrap();
        assert_eq!((ps.info_hash.as_str(), ps.already_exists), (staged_hash, false));
        assert!(dl.get(staged_hash).is_none(), "staged preview is hidden from list()");
        let prev = dl.preview_status(staged_hash).unwrap();
        assert_eq!((prev.name.as_str(), prev.has_metadata), ("Staged", false));
        dl.preview_cancel(staged_hash).unwrap();
        assert!(matches!(dl.preview_status(staged_hash), Err(DownloadError::Unknown)));

        // Previewing and confirming promotes the torrent with sequential enabled.
        let ps2 = dl.preview_start(&staged_magnet, None).unwrap();
        assert!(!ps2.already_exists);
        dl.preview_confirm(staged_hash, None, None, true).unwrap();
        let promoted = wait_for(&dl, staged_hash, |d| d.sequential);
        assert!(promoted.sequential);
        dl.remove(staged_hash, false).unwrap();

        let magnet_hash = "0123456789abcdef0123456789abcdef01234567";
        let magnet = format!("magnet:?xt=urn:btih:{magnet_hash}&dn=Parked");
        assert_eq!(dl.add_magnet(&magnet, None, true).unwrap(), magnet_hash);
        assert!(root.join("dl").is_dir(), "default folder is created");
        assert!(matches!(dl.add_magnet("https://example.com/x.torrent", None, false), Err(DownloadError::NotMagnet)));
        let parked = wait_for(&dl, magnet_hash, |d| d.state == DownloadState::Paused);
        assert_eq!(parked.name, "Parked");

        dl.pause(&seed).unwrap();
        wait_for(&dl, &seed, |d| d.state == DownloadState::Completed);
        dl.resume(&seed).unwrap();
        wait_for(&dl, &seed, |d| d.state == DownloadState::Seeding);
        assert!(matches!(dl.pause(&"f".repeat(40)), Err(DownloadError::Unknown)));

        assert_eq!(dl.persist(true, 5_000).unwrap(), 2);
        assert_eq!(dl.resume_files().len(), 2);
        drop(dl);

        let again = downloader(&root);
        assert!(again.has_saved());
        again.start().unwrap();
        wait_for(&again, &seed, |d| d.state == DownloadState::Seeding && d.progress >= 1.0);
        wait_for(&again, magnet_hash, |d| d.state == DownloadState::Paused);

        again.remove(magnet_hash, false).unwrap();
        assert_eq!(again.resume_files().len(), 1, "removing forgets the resume file");
        again.persist(true, 5_000).unwrap();
        assert_eq!(again.resume_files().len(), 1, "a removed download is never written back");
        again.remove(&seed, false).unwrap();
        assert!(file.exists(), "remove without delete keeps the data");
        drop(again);
        let _ = std::fs::remove_dir_all(&root);
    }
}
