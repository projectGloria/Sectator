//! Compiles the libtorrent shim (`cpp/lt_bridge.cpp`) through `cxx-build` and links
//! libtorrent, Boost and OpenSSL statically from the vcpkg manifest install
//! (`vcpkg install --triplet x64-windows-static-md --x-install-root=vcpkg_installed`).

use std::path::{Path, PathBuf};

const TRIPLET: &str = "x64-windows-static-md";

fn vcpkg_root() -> PathBuf {
    println!("cargo:rerun-if-env-changed=SECTATOR_VCPKG_INSTALLED");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = std::env::var_os("SECTATOR_VCPKG_INSTALLED").map(PathBuf::from).unwrap_or_else(|| manifest.join("vcpkg_installed").join(TRIPLET));
    if !root.join("include").join("libtorrent").is_dir() {
        panic!(
            "libtorrent not found under {}.\nBuild it once with vcpkg from src-tauri:\n  vcpkg install --triplet {TRIPLET} --x-install-root=vcpkg_installed\n(or point SECTATOR_VCPKG_INSTALLED at an <install-root>/{TRIPLET} folder)",
            root.display()
        );
    }
    root
}

/// The compile definitions libtorrent's CMake package exports (TORRENT_USE_OPENSSL,
/// BOOST_ASIO_* …): the shim must see the same ones or the ABI won't match.
fn libtorrent_defines(root: &Path) -> Vec<(String, Option<String>)> {
    let share = root.join("share");
    let targets = std::fs::read_dir(&share)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
        .map(|f| f.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("LibtorrentRasterbarTargets.cmake")))
        .unwrap_or_else(|| panic!("LibtorrentRasterbarTargets.cmake not found under {}", share.display()));
    let text = std::fs::read_to_string(&targets).unwrap();
    let mut out = Vec::new();
    let mut in_target = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("set_target_properties(LibtorrentRasterbar::torrent-rasterbar") {
            in_target = true;
        } else if in_target && l.starts_with("INTERFACE_COMPILE_DEFINITIONS") {
            let list = l.split_once('"').and_then(|(_, r)| r.rsplit_once('"')).map(|(v, _)| v).unwrap_or("");
            for def in list.split(';').filter(|d| !d.is_empty() && !d.starts_with("$<")) {
                match def.split_once('=') {
                    Some((k, v)) => out.push((k.to_string(), Some(v.to_string()))),
                    None => out.push((def.to_string(), None)),
                }
            }
            break;
        } else if in_target && l == ")" {
            break;
        }
    }
    println!("cargo:rerun-if-changed={}", targets.display());
    out
}

fn main() {
    let root = vcpkg_root();
    let mut b = cxx_build::bridge("src/clients/download/ffi.rs");
    b.file("cpp/lt_bridge.cpp")
        .include(root.join("include"))
        .include("cpp")
        .std("c++17")
        .define("NDEBUG", None)
        .define("WIN32_LEAN_AND_MEAN", None)
        .define("NOMINMAX", None)
        .flag_if_supported("/EHsc")
        .flag_if_supported("/permissive-")
        .flag_if_supported("/Zc:__cplusplus")
        .flag_if_supported("/bigobj")
        .warnings(false);
    for (k, v) in libtorrent_defines(&root) {
        b.define(&k, v.as_deref());
    }
    b.compile("lt_bridge");

    let lib = root.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    let mut libs: Vec<String> = std::fs::read_dir(&lib)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "lib"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    libs.sort();
    for l in libs {
        println!("cargo:rustc-link-lib=static={l}");
    }
    for sys in ["ws2_32", "iphlpapi", "mswsock", "crypt32", "bcrypt", "advapi32", "user32", "dbghelp"] {
        println!("cargo:rustc-link-lib=dylib={sys}");
    }
    println!("cargo:rerun-if-changed=cpp/lt_bridge.h");
    println!("cargo:rerun-if-changed=cpp/lt_bridge.cpp");
    println!("cargo:rerun-if-changed=src/clients/download/ffi.rs");

    tauri_build::build()
}
