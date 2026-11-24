// Thin C++ shim over libtorrent for the built-in downloader. Exposed to Rust through
// the `cxx` bridge in src/clients/download/ffi.rs; everything here is a direct call
// into lt::session, which is itself thread-safe (calls are posted to its network thread).
#pragma once

#include <cstdint>
#include <memory>
#include <mutex>

#include <libtorrent/session.hpp>

#include "rust/cxx.h"

namespace sectator {

struct SessionConfig;
struct TorrentStatus;
struct FileEntry;
struct ResumeBlob;

class Session {
public:
    explicit Session(lt::session_params params);

    void apply(const SessionConfig& cfg) const;

    // Each add returns the torrent's lowercase hex infohash.
    rust::String add_magnet(rust::Str uri, rust::Str save_path, bool paused) const;
    rust::String add_torrent(rust::Slice<const std::uint8_t> data, rust::Str save_path, bool paused, bool seed_mode) const;
    rust::String add_resume(rust::Slice<const std::uint8_t> data) const;

    rust::Vec<TorrentStatus> list() const;
    bool pause(rust::Str hash) const;
    bool resume(rust::Str hash) const;
    bool remove(rust::Str hash, bool delete_files) const;
    bool recheck(rust::Str hash) const;

    rust::Vec<FileEntry> files(rust::Str hash) const;
    bool set_priorities(rust::Str hash, rust::Slice<const std::uint8_t> priorities) const;

    // Ask torrents for fast-resume data (all of them, or only those that changed)
    // and wait up to `timeout_ms` for the answers.
    rust::Vec<ResumeBlob> save_resume(bool all, std::uint32_t timeout_ms) const;

private:
    mutable lt::session ses_;
    mutable std::mutex alerts_;
};

std::unique_ptr<Session> new_session(const SessionConfig& cfg);

// Build a .torrent (bencoded) for one local file or folder; used by tests to seed.
rust::Vec<std::uint8_t> make_torrent(rust::Str path, std::uint32_t piece_size);

} // namespace sectator
