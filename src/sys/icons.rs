//! Persistent cache for application icons extracted via AppKit.
//!
//! Icons are written as `.tiff` files under `~/.cache/aerofi/icons/`.
//! GPUI's `img()` element handles decoding the TIFF data at render time.
//! The cache persists across restarts, so AppKit is only called once per
//! application.
//!
//! User image files (script `@aerofi.icon` paths and plugin item icons)
//! are additionally downscaled to bounded-size thumbnails under
//! `~/.cache/aerofi/thumbs/` so the renderer never decodes — and the
//! GPU asset cache never retains — a full-resolution photo.

use crate::core::item::Target;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Cache format version — bump to invalidate stale icons after size or
/// format changes.  Stored as `VERSION` inside the icon directory.
const CACHE_VERSION: u32 = 4; // v1 = 128×128, v2 = 64×64, v3 = 128×128, v4 = 96×96 (Lanczos3)

/// Keep at most this many app icons on disk; the oldest are pruned so the
/// cache can't grow without bound as apps are installed and removed.
const ICON_MAX_FILES: usize = 1024;

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
    prune_icons();
    Some(path)
}

/// Drop the oldest app icons when the cache directory exceeds
/// [`ICON_MAX_FILES`] entries. Mirrors [`prune_thumbs`], but filters to
/// `.tiff` files so the `VERSION` marker is never removed. A pruned icon is
/// simply re-extracted on the next scan (a cache miss), so this is safe.
fn prune_icons() {
    let dir = icon_dir();
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("tiff"))
            .filter_map(|e| {
                let t = e.metadata().ok()?.modified().ok()?;
                Some((t, e.path()))
            })
            .collect(),
        Err(_) => return,
    };
    if entries.len() <= ICON_MAX_FILES {
        return;
    }
    entries.sort_by_key(|e| e.0);
    for (_, p) in entries.iter().take(entries.len() - ICON_MAX_FILES) {
        let _ = fs::remove_file(p);
    }
}

/// One uncached app awaiting icon processing: its identity (for applying
/// the result back) and the raw AppKit TIFF.
pub struct IconJob {
    /// App path (identifier) to match against when applying results.
    pub target: std::sync::Arc<std::path::Path>,
    /// Application name, used to derive the cache file name.
    pub name: String,
    /// Raw multi-resolution TIFF fetched from AppKit on the main thread.
    pub raw_tiff: Vec<u8>,
}

/// Main-thread phase: apply disk-cached icons in place and collect the
/// uncached apps' raw TIFFs (fast AppKit fetch) for background
/// processing. Must be called from the main thread after the Objective-C
/// run loop has started (i.e. inside `on_finish_launching`).
pub fn prepare_icon_jobs(targets: &mut [Target]) -> Vec<IconJob> {
    let dir = icon_dir();
    let mut jobs = Vec::new();
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
            *icon_path = Some(std::sync::Arc::from(cached));
            continue;
        }
        if let Some(raw_tiff) = crate::sys::appkit::raw_icon_for_app_bundle(path) {
            jobs.push(IconJob {
                target: std::sync::Arc::clone(path),
                name: name.to_string(),
                raw_tiff,
            });
        }
    }
    jobs
}

/// Worker-thread phase: downsample every job's raw TIFF and write it to
/// the cache. Returns `(app path, cached icon path)` per job.
pub fn process_icon_jobs(
    jobs: Vec<IconJob>,
) -> Vec<(
    std::sync::Arc<std::path::Path>,
    Option<std::sync::Arc<std::path::Path>>,
)> {
    jobs.into_iter()
        .map(|job| {
            let icon = crate::sys::appkit::process_icon_tiff(&job.raw_tiff)
                .and_then(|tiff| cache_icon(&job.name, &tiff).map(std::sync::Arc::from));
            (job.target, icon)
        })
        .collect()
}

/// Main-thread phase: apply worker results to the target list in place.
pub fn apply_icon_results(
    targets: &mut [Target],
    results: Vec<(
        std::sync::Arc<std::path::Path>,
        Option<std::sync::Arc<std::path::Path>>,
    )>,
) {
    for (path, icon) in results {
        let id = path.to_str().unwrap_or("");
        for target in targets.iter_mut() {
            if target.identifier() != id {
                continue;
            }
            if let Target::App { icon_path, .. } = target
                && icon_path.is_none()
            {
                *icon_path = icon;
            }
            break;
        }
    }
}

/// Extract and cache icons for every `Target::App` in the list, mutating
/// the `icon_path` field in-place. The AppKit fetch runs on the main
/// thread; the CPU-heavy downsample runs on a worker thread that this call
/// joins, so the main thread does no image processing.
pub fn extract_all(targets: &mut [Target]) {
    let jobs = prepare_icon_jobs(targets);
    if jobs.is_empty() {
        return;
    }
    let results = std::thread::Builder::new()
        .name("aerofi-icon-extract".into())
        .spawn(move || process_icon_jobs(jobs))
        .ok()
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    apply_icon_results(targets, results);
}

// ---------------------------------------------------------------------------
// Thumbnails for user image files
// ---------------------------------------------------------------------------

/// Longest side of a generated thumbnail, in pixels.
const THUMB_MAX_DIM: u32 = 128;
/// Source images larger than this are not downscaled (would require a very
/// large transient decode buffer); callers fall back to a text glyph.
const THUMB_MAX_SOURCE_PIXELS: u64 = 64 * 1024 * 1024;
/// Keep at most this many thumbnails on disk; the oldest are pruned.
const THUMB_MAX_FILES: usize = 1024;

/// Lazily-created thumbnail cache directory: `~/.cache/aerofi/thumbs/`.
fn thumb_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("aerofi")
            .join("thumbs");
        let _ = fs::create_dir_all(&dir);
        dir
    })
}

/// FNV-1a 64-bit — stable across runs and processes, unlike the standard
/// library's per-process keyed hasher. Used only as a cache key.
fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Expand a leading `~` to the user's home directory (standalone copy so
/// this module stays independent of the UI helpers).
fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_string()
}

/// Return the path of a cached, downscaled (≤ 128 px) TIFF thumbnail for
/// the image file at `image_path`, generating it on first use.
///
/// The key includes the file's modification time, so edited files are
/// re-thumbscaled while untouched files hit the disk cache (metadata only,
/// no decode). Returns `None` when the file is missing, undecodable, or
/// larger than [`THUMB_MAX_SOURCE_PIXELS`].
pub fn thumbnail_for(image_path: &Path) -> Option<PathBuf> {
    let meta = fs::metadata(image_path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    let key = format!(
        "{:016x}-{:?}",
        fnv1a(&image_path.to_string_lossy()),
        mtime.as_nanos()
    );
    let path = thumb_dir().join(format!("{}.tiff", key));
    if path.exists() {
        return Some(path);
    }

    let reader = image::ImageReader::open(image_path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w as u64 * h as u64 > THUMB_MAX_SOURCE_PIXELS {
        return None;
    }
    // Same pipeline as the app icons: full decode, then a Lanczos3
    // downscale to the bounded size. The full-size buffer is transient —
    // only the small TIFF is persisted and rendered.
    let img = image::ImageReader::open(image_path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumb = img.resize(
        THUMB_MAX_DIM,
        THUMB_MAX_DIM,
        image::imageops::FilterType::Lanczos3,
    );
    let mut bytes = Vec::new();
    thumb
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Tiff,
        )
        .ok()?;
    fs::write(&path, &bytes).ok()?;
    prune_thumbs();
    Some(path)
}

/// Drop the oldest thumbnails when the cache directory exceeds
/// [`THUMB_MAX_FILES`] entries.
fn prune_thumbs() {
    let dir = thumb_dir();
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let t = e.metadata().ok()?.modified().ok()?;
                Some((t, e.path()))
            })
            .collect(),
        Err(_) => return,
    };
    if entries.len() <= THUMB_MAX_FILES {
        return;
    }
    entries.sort_by_key(|e| e.0);
    for (_, p) in entries.iter().take(entries.len() - THUMB_MAX_FILES) {
        let _ = fs::remove_file(p);
    }
}

/// Replace every plugin item icon that is a full-resolution image path
/// with its cached thumbnail path, so rendering never decodes the original
/// file. Items whose thumbnail cannot be produced get a generic glyph.
/// Call this on a worker thread (first use decodes the source image).
pub fn thumbnail_image_icons(items: &mut [Target]) {
    for item in items.iter_mut() {
        let Target::PluginItem { icon, .. } = item else {
            continue;
        };
        let Some(icon) = icon else {
            continue;
        };
        let s = icon.as_ref();
        if !s.starts_with('/') && !s.starts_with('~') {
            continue;
        }
        let resolved = expand_home(s);
        match thumbnail_for(Path::new(&resolved)) {
            Some(path) => *icon = gpui::SharedString::from(path.to_string_lossy()),
            None => *icon = gpui::SharedString::from("🖼️"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_image(path: &Path, w: u32, h: u32) {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgba([x as u8, y as u8, 128, 255]);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn thumbnail_is_downscaled_and_cached() {
        let dir = std::env::temp_dir().join(format!("aerofi-thumb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("big.png");
        write_test_image(&src, 400, 300);

        let thumb = thumbnail_for(&src).expect("thumbnail created");
        assert!(thumb.exists());
        let loaded = image::open(&thumb).unwrap();
        assert!(loaded.width() <= THUMB_MAX_DIM);
        assert!(loaded.height() <= THUMB_MAX_DIM);

        // Second call hits the disk cache (same path, no re-encode).
        let again = thumbnail_for(&src).unwrap();
        assert_eq!(again, thumb);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnail_missing_file_is_none() {
        assert!(thumbnail_for(Path::new("/nonexistent/aerofi-test.png")).is_none());
    }

    #[test]
    fn plugin_icons_replaced_with_thumbnails() {
        let dir = std::env::temp_dir().join(format!("aerofi-thumb-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.png");
        write_test_image(&src, 200, 200);

        let mut items = vec![
            Target::PluginItem {
                name: "photo.png".into(),
                subtitle: None,
                icon: Some(src.to_string_lossy().into_owned().into()),
                plugin_id: "a".into(),
                plugin_name: "file-search".into(),
            },
            Target::PluginItem {
                name: "notes.txt".into(),
                subtitle: None,
                icon: Some("📝".into()),
                plugin_id: "b".into(),
                plugin_name: "file-search".into(),
            },
            Target::PluginItem {
                name: "gone.png".into(),
                subtitle: None,
                icon: Some("/nonexistent/gone.png".into()),
                plugin_id: "c".into(),
                plugin_name: "file-search".into(),
            },
        ];

        thumbnail_image_icons(&mut items);

        assert!(items[0].icon().unwrap().ends_with(".tiff"));
        assert_eq!(items[1].icon().unwrap(), "📝");
        assert_eq!(items[2].icon().unwrap(), "🖼️");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
