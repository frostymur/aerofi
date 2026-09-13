use aerofi_plugin_api::{AerofiPlugin, PluginItem, PluginMetadata, PluginResults};
use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::process::Command;
use std::ptr;

static PLUGIN: AerofiPlugin = AerofiPlugin {
    api_version: 1,
    init,
    get_metadata,
    query,
    activate,
    free_results,
    destroy,
};

/// Returns the static pointer to the `AerofiPlugin` C ABI interface.
///
/// # Safety
/// This function is safe to call across FFI boundaries. The returned pointer
/// references a static, immutable `AerofiPlugin` struct valid for the lifetime of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aerofi_plugin_init() -> *const AerofiPlugin {
    &PLUGIN
}

unsafe extern "C" fn init() -> bool {
    true
}

unsafe extern "C" fn get_metadata() -> PluginMetadata {
    PluginMetadata {
        name: CString::new("file-search").unwrap().into_raw(),
        description: CString::new("Fast macOS Spotlight file search")
            .unwrap()
            .into_raw(),
        prefix: CString::new("f ").unwrap().into_raw(),
    }
}

unsafe extern "C" fn query(query_ptr: *const c_char) -> PluginResults {
    if query_ptr.is_null() {
        return PluginResults {
            items: ptr::null(),
            count: 0,
        };
    }

    let q = unsafe { CStr::from_ptr(query_ptr) }.to_string_lossy();
    let q_trimmed = q.trim();

    if q_trimmed.is_empty() {
        let item = PluginItem {
            id: CString::new("").unwrap().into_raw(),
            title: CString::new("Search files…").unwrap().into_raw(),
            subtitle: CString::new("Type filename to search (e.g. f notes)")
                .unwrap()
                .into_raw(),
            icon: CString::new("📁").unwrap().into_raw(),
        };
        let items = vec![item];
        let count = items.len();
        let boxed_slice = items.into_boxed_slice();
        let items_ptr = Box::into_raw(boxed_slice) as *const PluginItem;
        return PluginResults {
            items: items_ptr,
            count,
        };
    }

    let matching_lines = search_files(q_trimmed);

    let mut plugin_items = Vec::new();

    if matching_lines.is_empty() {
        let item = PluginItem {
            id: CString::new("").unwrap().into_raw(),
            title: CString::new(format!("No files found matching '{}'", q_trimmed))
                .unwrap()
                .into_raw(),
            subtitle: CString::new("Try another search term").unwrap().into_raw(),
            icon: CString::new("🔍").unwrap().into_raw(),
        };
        plugin_items.push(item);
    } else {
        // Take top 25 matches
        for line in matching_lines.into_iter().take(25) {
            let path = Path::new(&line);
            let filename = match path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            let title_c = match CString::new(filename) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let subtitle_c = match CString::new(line.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let id_c = match CString::new(line.as_str()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let icon_str = if path.is_dir() {
                "📁".to_string()
            } else {
                match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "tiff" => line.clone(),
                    "rs" | "py" | "js" | "ts" | "go" | "c" | "cpp" | "sh" | "zsh" | "rb" => {
                        "💻".to_string()
                    }
                    "md" | "txt" | "pdf" | "doc" | "docx" | "rtf" => "📝".to_string(),
                    "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" => "📦".to_string(),
                    "mp3" | "wav" | "flac" | "mp4" | "mov" | "mkv" | "avi" => "🎬".to_string(),
                    _ => "📄".to_string(),
                }
            };

            let icon_c = match CString::new(icon_str) {
                Ok(c) => c,
                Err(_) => continue,
            };

            plugin_items.push(PluginItem {
                id: id_c.into_raw(),
                title: title_c.into_raw(),
                subtitle: subtitle_c.into_raw(),
                icon: icon_c.into_raw(),
            });
        }
    }

    let count = plugin_items.len();
    let boxed_slice = plugin_items.into_boxed_slice();
    let items_ptr = Box::into_raw(boxed_slice) as *const PluginItem;

    PluginResults {
        items: items_ptr,
        count,
    }
}

/// Search for files using Spotlight `mdfind` first, with fallbacks to `fd` and `find`.
fn search_files(query: &str) -> Vec<String> {
    // 1. Try Spotlight `mdfind`
    if let Ok(out) = Command::new("mdfind").arg("-name").arg(query).output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<String> = s
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            return lines;
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users".into());

    // 2. Try `fd` if available (fast multithreaded directory search)
    let fd_bin = if Path::new("/opt/homebrew/bin/fd").exists() {
        "/opt/homebrew/bin/fd"
    } else if Path::new("/usr/local/bin/fd").exists() {
        "/usr/local/bin/fd"
    } else {
        "fd"
    };

    if let Ok(out) = Command::new(fd_bin)
        .arg("-i")
        .arg(query)
        .arg(&home)
        .arg("-E")
        .arg("Library")
        .arg("-E")
        .arg(".cache")
        .arg("-E")
        .arg(".git")
        .arg("-E")
        .arg("node_modules")
        .arg("-E")
        .arg("target")
        .arg("-E")
        .arg("OrbStack")
        .arg("-E")
        .arg(".Trash")
        .arg("--max-results")
        .arg("25")
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<String> = s
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            return lines;
        }
    }

    // 3. Fallback: search key user directories with `find`
    let subdirs = ["Desktop", "Documents", "Downloads", "projects", "Developer"];
    let search_dirs: Vec<std::path::PathBuf> = subdirs
        .iter()
        .map(|sub| Path::new(&home).join(sub))
        .filter(|p| p.exists())
        .collect();

    if !search_dirs.is_empty() {
        let mut cmd = Command::new("find");
        for d in &search_dirs {
            cmd.arg(d);
        }
        cmd.arg("-maxdepth").arg("4");
        cmd.arg("-iname").arg(format!("*{query}*"));
        if let Ok(out) = cmd.output()
            && out.status.success()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .take(25)
                .collect();
            if !lines.is_empty() {
                return lines;
            }
        }
    }

    Vec::new()
}

unsafe extern "C" fn activate(id_ptr: *const c_char, action_code: u32) -> bool {
    if id_ptr.is_null() {
        return true;
    }

    let path_str = unsafe { CStr::from_ptr(id_ptr) }.to_string_lossy();
    if path_str.is_empty() {
        return false;
    }

    if action_code == 1 {
        // Reveal in Finder if action_code is 1 (e.g. Shift+Enter / Alt+Enter)
        let _ = Command::new("open")
            .arg("-R")
            .arg(path_str.as_ref())
            .spawn();
    } else {
        // Open file normally
        let _ = Command::new("open").arg(path_str.as_ref()).spawn();
    }

    true
}

unsafe extern "C" fn free_results(results: PluginResults) {
    if results.items.is_null() || results.count == 0 {
        return;
    }

    let slice =
        unsafe { std::slice::from_raw_parts_mut(results.items as *mut PluginItem, results.count) };
    for item in &mut *slice {
        if !item.id.is_null() {
            drop(unsafe { CString::from_raw(item.id as *mut c_char) });
        }
        if !item.title.is_null() {
            drop(unsafe { CString::from_raw(item.title as *mut c_char) });
        }
        if !item.subtitle.is_null() {
            drop(unsafe { CString::from_raw(item.subtitle as *mut c_char) });
        }
        if !item.icon.is_null() {
            drop(unsafe { CString::from_raw(item.icon as *mut c_char) });
        }
    }

    drop(unsafe { Box::from_raw(slice as *mut [PluginItem]) });
}

unsafe extern "C" fn destroy() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_search_empty_query() {
        let q = CString::new("").unwrap();
        let res = unsafe { query(q.as_ptr()) };
        assert_eq!(res.count, 1);
        assert!(!res.items.is_null());
        unsafe { free_results(res) };
    }

    #[test]
    fn test_file_search_finds_files() {
        let q = CString::new("aerofi").unwrap();
        let res = unsafe { query(q.as_ptr()) };
        assert!(res.count >= 1);
        assert!(!res.items.is_null());
        unsafe { free_results(res) };
    }
}
