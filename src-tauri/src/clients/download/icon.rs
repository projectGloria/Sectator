//! The Windows shell icon for a file type: the icon Explorer shows for `.mkv` etc., i.e.
//! the user's default app for it. Returned as PNG bytes and cached per extension.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

static CACHE: Lazy<Mutex<HashMap<String, Option<Vec<u8>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// PNG of the shell icon for files with extension `ext` (no dot), or `None` when Windows
/// has none for it (or off Windows).
pub fn for_extension(ext: &str) -> Option<Vec<u8>> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    if ext.is_empty() || ext.len() > 16 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if let Some(hit) = CACHE.lock().unwrap().get(&ext) {
        return hit.clone();
    }
    let png = shell_icon_rgba(&ext).and_then(|(w, h, rgba)| encode_png(w, h, &rgba));
    CACHE.lock().unwrap().insert(ext, png.clone());
    png
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

/// Turn top-down BGRA rows into RGBA. Icons without an alpha channel (all zero) take
/// their transparency from the AND mask instead (`mask` black = opaque).
fn bgra_to_rgba(mut px: Vec<u8>, mask: Option<&[u8]>) -> Vec<u8> {
    let no_alpha = px.chunks_exact(4).all(|p| p[3] == 0);
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        p.swap(0, 2);
        if no_alpha {
            let opaque = mask.and_then(|m| m.get(i * 4)).is_none_or(|&b| b == 0);
            p[3] = if opaque { 255 } else { 0 };
        }
    }
    px
}

#[cfg(windows)]
fn shell_icon_rgba(ext: &str) -> Option<(u32, u32, Vec<u8>)> {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    // Top-down 32-bit pixels of a GDI bitmap.
    unsafe fn pixels(hdc: windows_sys::Win32::Graphics::Gdi::HDC, bmp: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut bi: BITMAPINFO = unsafe { zeroed() };
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { zeroed() }
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let rows = unsafe { GetDIBits(hdc, bmp, 0, h as u32, buf.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS) };
        (rows == h).then_some(buf)
    }

    let wide: Vec<u16> = format!("file.{ext}").encode_utf16().chain([0]).collect();
    unsafe {
        // The shell wants COM on the calling thread; balance only our own init.
        let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as _) >= 0;
        let mut info: SHFILEINFOW = zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            FILE_ATTRIBUTE_NORMAL,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        );
        let result = (ok != 0 && !info.hIcon.is_null()).then(|| {
            let mut ii: ICONINFO = zeroed();
            if GetIconInfo(info.hIcon, &mut ii) == 0 {
                return None;
            }
            let mut out = None;
            let mut bm: BITMAP = zeroed();
            if !ii.hbmColor.is_null() && GetObjectW(ii.hbmColor, size_of::<BITMAP>() as i32, (&mut bm as *mut BITMAP).cast()) != 0 {
                let (w, h) = (bm.bmWidth, bm.bmHeight);
                let hdc = CreateCompatibleDC(null_mut());
                if let Some(color) = pixels(hdc, ii.hbmColor, w, h) {
                    let mask = if ii.hbmMask.is_null() { None } else { pixels(hdc, ii.hbmMask, w, h) };
                    out = Some((w as u32, h as u32, bgra_to_rgba(color, mask.as_deref())));
                }
                DeleteDC(hdc);
            }
            if !ii.hbmColor.is_null() {
                DeleteObject(ii.hbmColor);
            }
            if !ii.hbmMask.is_null() {
                DeleteObject(ii.hbmMask);
            }
            DestroyIcon(info.hIcon);
            out
        });
        if com {
            CoUninitialize();
        }
        result.flatten()
    }
}

#[cfg(not(windows))]
fn shell_icon_rgba(_ext: &str) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgra_and_falls_back_to_mask() {
        // Has alpha: channels swap, alpha kept.
        assert_eq!(bgra_to_rgba(vec![1, 2, 3, 200], None), vec![3, 2, 1, 200]);
        // No alpha at all: mask black = opaque, white = transparent.
        let px = vec![1, 2, 3, 0, 4, 5, 6, 0];
        let mask = [0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(bgra_to_rgba(px, Some(&mask)), vec![3, 2, 1, 255, 6, 5, 4, 0]);
    }

    #[test]
    fn rejects_odd_extensions() {
        assert_eq!(for_extension(""), None);
        assert_eq!(for_extension("../evil"), None);
        assert_eq!(for_extension("averyveryverylongextension"), None);
    }

    #[cfg(windows)]
    #[test]
    fn real_shell_icon_is_a_png() {
        // Every Windows install has an icon for .txt.
        let png = for_extension("txt").expect("shell icon for .txt");
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(for_extension(".TXT").as_deref(), Some(&png[..]), "cached and normalised");
    }
}
