//! Persistent cache for application icons extracted via AppKit.
//!
//! Icons are written as `.tiff` files under `~/.cache/aerofi/icons/`.
//! GPUI's `img()` element handles decoding the TIFF data at render time.
//! The cache persists across restarts, so AppKit is only called once per
//! application.

use crate::core::item::Target;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Cache format version — bump to invalidate stale icons after size or
/// format changes.  Stored as `VERSION` inside the icon directory.
const CACHE_VERSION: u32 = 2; // v1 = 128×128, v2 = 64×64

/// Lazily-created persistent cache directory: `~/.cache/aerofi/icons/`.
fn icon_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("aerofi")
            .join("icons");
        let _ = fs::create_dir_all(&dir);
        invalidate_stale_cache(&dir);
        dir
    })
}

/// If the on-disk cache was written by an older version, wipe it so icons
/// are re-extracted at the current resolution.
fn invalidate_stale_cache(dir: &PathBuf) {
    let version_file = dir.join("VERSION");
    let current = fs::read_to_string(&version_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if current == Some(CACHE_VERSION) {
        return;
    }
    // Remove every .tiff in the directory (ignore errors — files may be
    // in use or permission-denied, which is fine; they'll be overwritten).
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "tiff") {
                let _ = fs::remove_file(&p);
            }
        }
    }
    let _ = fs::write(&version_file, CACHE_VERSION.to_string());
}

/// Sanitise an application name so it is safe as a file name.
fn sanitise_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Persist raw TIFF bytes for the given application and return the path
/// to the cached `.tiff` file. Returns `None` on I/O failure.
pub fn cache_icon(name: &str, tiff_bytes: &[u8]) -> Option<PathBuf> {
    let path = icon_dir().join(format!("{}.tiff", sanitise_name(name)));
    if path.exists() {
        return Some(path);
    }
    fs::write(&path, tiff_bytes).ok()?;
    Some(path)
}

/// Extract and cache icons for every `Target::App` in the list, mutating
/// the `icon_path` field in-place. Must be called from the main thread
/// after the Objective-C run loop has started (i.e. inside
/// `on_finish_launching`).
pub fn extract_all(targets: &mut [Target]) {
    let dir = icon_dir();
    for target in targets.iter_mut() {
        let Target::App {
            name,
            path,
            icon_path,
        } = target
        else {
            continue;
        };
        if icon_path.is_some() {
            continue;
        }
        let cached = dir.join(format!("{}.tiff", sanitise_name(name)));
        if cached.exists() {
            *icon_path = Some(cached);
            continue;
        }
        *icon_path =
            crate::sys::appkit::icon_for_app_bundle(path).and_then(|tiff| cache_icon(name, &tiff));
    }
}
