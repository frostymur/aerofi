//! Temp-file cache for application icons extracted via AppKit.
//!
//! Each icon is written as a `.tiff` file under a per-process temp
//! directory (`$TMPDIR/aerofi-icons/`). GPUI's `img()` element handles
//! decoding the TIFF data at render time.

use crate::core::item::Target;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Lazily-created temp directory that lives for the process lifetime.
fn icon_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("aerofi-icons");
        let _ = fs::create_dir_all(&dir);
        dir
    })
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
    fs::write(&path, tiff_bytes).ok()?;
    Some(path)
}

/// Extract and cache icons for every `Target::App` in the list, mutating
/// the `icon_path` field in-place. Must be called from the main thread
/// after the Objective-C run loop has started (i.e. inside
/// `on_finish_launching`).
pub fn extract_all(targets: &mut [Target]) {
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
        *icon_path =
            crate::sys::appkit::icon_for_app_bundle(path).and_then(|tiff| cache_icon(name, &tiff));
    }
}
