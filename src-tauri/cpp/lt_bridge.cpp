// Same path the generated bridge header uses, so #pragma once sees one file.
#include "sectator/cpp/lt_bridge.h"

#include <chrono>
#include <sstream>
#include <string>
#include <vector>

#include <libtorrent/add_torrent_params.hpp>
#include <libtorrent/alert_types.hpp>
#include <libtorrent/bencode.hpp>
#include <libtorrent/create_torrent.hpp>
#include <libtorrent/error_code.hpp>
#include <libtorrent/load_torrent.hpp>
#include <libtorrent/magnet_uri.hpp>
#include <libtorrent/read_resume_data.hpp>
#include <libtorrent/settings_pack.hpp>
#include <libtorrent/torrent_handle.hpp>
#include <libtorrent/torrent_info.hpp>
#include <libtorrent/torrent_status.hpp>
#include <libtorrent/write_resume_data.hpp>

#include "sectator/src/clients/download/ffi.rs.h"

namespace sectator {

namespace {

std::string to_std(rust::Str s) { return std::string(s.data(), s.size()); }

rust::String lossy(const std::string& s) { return rust::String::lossy(s.data(), s.size()); }

std::string hex_of(const lt::sha1_hash& h) {
    static const char digits[] = "0123456789abcdef";
    std::string out;
    out.reserve(40);
    for (std::uint8_t b : h) {
        out.push_back(digits[b >> 4]);
        out.push_back(digits[b & 15]);
    }
    return out;
}

// The v1 (SHA-1) infohash the indexers report; v2-only torrents fall back to the truncated v2 hash.
std::string hash_of(const lt::info_hash_t& ih) { return hex_of(ih.has_v1() ? ih.v1 : ih.get_best()); }

int nibble(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

bool parse_hash(rust::Str s, lt::sha1_hash& out) {
    if (s.size() != 40) return false;
    for (std::size_t i = 0; i < 20; ++i) {
        int hi = nibble(s.data()[2 * i]), lo = nibble(s.data()[2 * i + 1]);
        if (hi < 0 || lo < 0) return false;
        out[static_cast<int>(i)] = static_cast<std::uint8_t>(hi * 16 + lo);
    }
    return true;
}

lt::torrent_handle find(lt::session& ses, rust::Str hash) {
    lt::sha1_hash h;
    if (!parse_hash(hash, h)) return {};
    return ses.find_torrent(h);
}

lt::settings_pack pack_for(const SessionConfig& cfg) {
    lt::settings_pack p;
    p.set_int(lt::settings_pack::alert_mask,
              lt::alert_category::status | lt::alert_category::error | lt::alert_category::storage);
    std::string port = std::to_string(cfg.listen_port);
    p.set_str(lt::settings_pack::listen_interfaces, "0.0.0.0:" + port + ",[::]:" + port);
    p.set_str(lt::settings_pack::user_agent, to_std(cfg.user_agent));
    p.set_int(lt::settings_pack::download_rate_limit, cfg.download_rate_limit);
    p.set_int(lt::settings_pack::upload_rate_limit, cfg.upload_rate_limit);
    p.set_bool(lt::settings_pack::enable_dht, true);
    p.set_bool(lt::settings_pack::enable_lsd, true);
    p.set_bool(lt::settings_pack::enable_upnp, true);
    p.set_bool(lt::settings_pack::enable_natpmp, true);
    return p;
}

// Torrents are not auto-managed: everything added runs until the user pauses it,
// rather than waiting in libtorrent's download queue.
void own(lt::add_torrent_params& p, const std::string& save_path, bool paused) {
    if (!save_path.empty()) p.save_path = save_path;
    p.flags &= ~lt::torrent_flags::auto_managed;
    p.flags &= ~lt::torrent_flags::duplicate_is_error;
    if (paused) p.flags |= lt::torrent_flags::paused;
    else p.flags &= ~lt::torrent_flags::paused;
}

rust::String add(lt::session& ses, lt::add_torrent_params& p) {
    lt::error_code ec;
    lt::torrent_handle h = ses.add_torrent(std::move(p), ec);
    if (ec) throw std::runtime_error(ec.message());
    return rust::String(hash_of(h.info_hashes()));
}

} // namespace

Session::Session(lt::session_params params) : ses_(std::move(params)) {}

std::unique_ptr<Session> new_session(const SessionConfig& cfg) {
    lt::session_params params(pack_for(cfg));
    return std::make_unique<Session>(std::move(params));
}

void Session::apply(const SessionConfig& cfg) const { ses_.apply_settings(pack_for(cfg)); }

rust::String Session::add_magnet(rust::Str uri, rust::Str save_path, bool paused) const {
    lt::error_code ec;
    lt::add_torrent_params p = lt::parse_magnet_uri(to_std(uri), ec);
    if (ec) throw std::runtime_error("invalid magnet link: " + ec.message());
    own(p, to_std(save_path), paused);
    return add(ses_, p);
}

rust::String Session::add_magnet_preview(rust::Str uri, rust::Str save_path) const {
    lt::error_code ec;
    lt::add_torrent_params p = lt::parse_magnet_uri(to_std(uri), ec);
    if (ec) throw std::runtime_error("invalid magnet link: " + ec.message());
    own(p, to_std(save_path), false);
    p.flags |= lt::torrent_flags::default_dont_download;
    return add(ses_, p);
}

rust::String Session::add_torrent(rust::Slice<const std::uint8_t> data, rust::Str save_path, bool paused, bool seed_mode) const {
    lt::error_code ec;
    lt::add_torrent_params p = lt::load_torrent_buffer(
        lt::span<const char>(reinterpret_cast<const char*>(data.data()), static_cast<std::ptrdiff_t>(data.size())), ec,
        lt::load_torrent_limits{});
    if (ec) throw std::runtime_error("invalid torrent file: " + ec.message());
    own(p, to_std(save_path), paused);
    if (seed_mode) p.flags |= lt::torrent_flags::seed_mode;
    return add(ses_, p);
}

rust::String Session::add_resume(rust::Slice<const std::uint8_t> data) const {
    lt::error_code ec;
    lt::add_torrent_params p = lt::read_resume_data(
        lt::span<const char>(reinterpret_cast<const char*>(data.data()), static_cast<std::ptrdiff_t>(data.size())), ec);
    if (ec) throw std::runtime_error("unreadable resume data: " + ec.message());
    p.flags &= ~lt::torrent_flags::auto_managed;
    return add(ses_, p);
}

rust::Vec<TorrentStatus> Session::list() const {
    rust::Vec<TorrentStatus> out;
    for (const lt::torrent_status& st : ses_.get_torrent_status([](const lt::torrent_status&) { return true; },
                                                                      lt::torrent_handle::query_name | lt::torrent_handle::query_save_path)) {
        TorrentStatus t;
        t.info_hash = rust::String(hash_of(st.info_hashes));
        t.name = lossy(st.name);
        t.save_path = lossy(st.save_path);
        t.state = static_cast<std::uint8_t>(st.state);
        t.paused = static_cast<bool>(st.flags & lt::torrent_flags::paused);
        t.sequential = static_cast<bool>(st.flags & lt::torrent_flags::sequential_download);
        t.has_metadata = st.has_metadata;
        t.progress = st.progress;
        t.total_wanted = st.total_wanted;
        t.total_wanted_done = st.total_wanted_done;
        t.download_rate = st.download_payload_rate;
        t.upload_rate = st.upload_payload_rate;
        t.num_seeds = st.num_seeds;
        t.num_peers = st.num_peers;
        t.list_seeds = st.list_seeds;
        t.list_peers = st.list_peers;
        t.all_time_upload = st.all_time_upload;
        t.all_time_download = st.all_time_download;
        t.added_time = static_cast<std::int64_t>(st.added_time);
        t.completed_time = static_cast<std::int64_t>(st.completed_time);
        t.error = st.errc ? lossy(st.errc.message()) : rust::String();
        out.push_back(std::move(t));
    }
    return out;
}

bool Session::pause(rust::Str hash) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    h.unset_flags(lt::torrent_flags::auto_managed);
    h.pause(lt::torrent_handle::graceful_pause);
    return true;
}

bool Session::resume(rust::Str hash) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    h.clear_error();
    h.resume();
    return true;
}

bool Session::remove(rust::Str hash, bool delete_files) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    ses_.remove_torrent(h, delete_files ? lt::session::delete_files : lt::remove_flags_t{});
    return true;
}

bool Session::recheck(rust::Str hash) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    h.force_recheck();
    return true;
}

bool Session::set_sequential(rust::Str hash, bool sequential) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    if (sequential) h.set_flags(lt::torrent_flags::sequential_download);
    else h.unset_flags(lt::torrent_flags::sequential_download);
    return true;
}

bool Session::enable_default_download(rust::Str hash) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    h.unset_flags(lt::torrent_flags::default_dont_download);
    return true;
}

bool Session::move_storage(rust::Str hash, rust::Str save_path) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return false;
    h.move_storage(to_std(save_path));
    return true;
}

rust::Vec<FileEntry> Session::files(rust::Str hash) const {
    rust::Vec<FileEntry> out;
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid()) return out;
    std::shared_ptr<const lt::torrent_info> ti = h.torrent_file();
    if (!ti) return out;
    const lt::file_storage& fs = ti->layout();
    std::vector<std::int64_t> done = h.file_progress(lt::torrent_handle::piece_granularity);
    std::vector<lt::download_priority_t> prio = h.get_file_priorities();
    for (lt::file_index_t i : fs.file_range()) {
        if (fs.pad_file_at(i)) continue;
        int k = static_cast<int>(i);
        FileEntry f;
        f.index = k;
        f.path = lossy(fs.file_path(i));
        f.size = fs.file_size(i);
        f.done = k < static_cast<int>(done.size()) ? done[k] : 0;
        f.priority = k < static_cast<int>(prio.size()) ? static_cast<std::uint8_t>(prio[k]) : 4;
        out.push_back(std::move(f));
    }
    return out;
}

bool Session::set_priorities(rust::Str hash, rust::Slice<const std::uint8_t> priorities) const {
    lt::torrent_handle h = find(ses_, hash);
    if (!h.is_valid() || !h.torrent_file()) return false;
    std::vector<lt::download_priority_t> prio = h.get_file_priorities();
    for (std::size_t i = 0; i < priorities.size() && i < prio.size(); ++i) {
        prio[i] = lt::download_priority_t{static_cast<std::uint8_t>(priorities[i] > 7 ? 7 : priorities[i])};
    }
    h.prioritize_files(prio);
    return true;
}

rust::Vec<ResumeBlob> Session::save_resume(bool all, std::uint32_t timeout_ms) const {
    std::lock_guard<std::mutex> lock(alerts_);
    rust::Vec<ResumeBlob> out;
    int outstanding = 0;
    for (const lt::torrent_handle& h : ses_.get_torrents()) {
        if (!h.is_valid() || (!all && !h.need_save_resume_data())) continue;
        h.save_resume_data(lt::torrent_handle::save_info_dict);
        ++outstanding;
    }
    auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::vector<lt::alert*> alerts;
    while (outstanding > 0) {
        auto left = deadline - std::chrono::steady_clock::now();
        if (left <= std::chrono::steady_clock::duration::zero()) break;
        if (!ses_.wait_for_alert(std::chrono::duration_cast<lt::time_duration>(left))) continue;
        ses_.pop_alerts(&alerts);
        for (lt::alert* a : alerts) {
            if (auto* r = lt::alert_cast<lt::save_resume_data_alert>(a)) {
                --outstanding;
                std::vector<char> buf = lt::write_resume_data_buf(r->params);
                ResumeBlob b;
                b.info_hash = rust::String(hash_of(r->params.info_hashes));
                b.data.reserve(buf.size());
                for (char c : buf) b.data.push_back(static_cast<std::uint8_t>(c));
                out.push_back(std::move(b));
            } else if (lt::alert_cast<lt::save_resume_data_failed_alert>(a)) {
                --outstanding;
            }
        }
    }
    return out;
}

rust::Vec<std::uint8_t> make_torrent(rust::Str path, std::uint32_t piece_size) {
    std::string root = to_std(path);
    std::vector<lt::create_file_entry> entries = lt::list_files(root);
    if (entries.empty()) throw std::runtime_error("nothing to add at " + root);
    lt::create_torrent t(std::move(entries), static_cast<int>(piece_size));
    std::string parent = root.substr(0, root.find_last_of("\\/"));
    lt::error_code ec;
    lt::set_piece_hashes(t, parent, ec);
    if (ec) throw std::runtime_error("hashing failed: " + ec.message());
    std::vector<char> buf = t.generate_buf();
    rust::Vec<std::uint8_t> out;
    out.reserve(buf.size());
    for (char c : buf) out.push_back(static_cast<std::uint8_t>(c));
    return out;
}

} // namespace sectator
