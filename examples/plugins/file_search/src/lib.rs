use aerofi_plugin_api::{AerofiPlugin, PluginItem, PluginMetadata, PluginResults};
use std::ffi::{c_char, CStr, CString};
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
        description: CString::new("Fast macOS Spotlight file search").unwrap().into_raw(),
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
            title: CString::new("Search files with Spotlight…").unwrap().into_raw(),
            subtitle: CString::new("Type filename to search (e.g. f notes)").unwrap().into_raw(),
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

    // Query Spotlight using `mdfind`
    let output = match Command::new("mdfind")
        .arg("-name")
        .arg(q_trimmed)
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => return PluginResults { items: ptr::null(), count: 0 },
    };

    let mut plugin_items = Vec::new();

    // Take top 25 matches
    for line in output.lines().take(25) {
        let path = Path::new(line);
        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        let title_c = match CString::new(filename) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let subtitle_c = match CString::new(line) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let id_c = match CString::new(line) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let icon_str = if path.is_dir() { "📁" } else { line };
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

    let count = plugin_items.len();
    if count == 0 {
        return PluginResults {
            items: ptr::null(),
            count: 0,
        };
    }

    let boxed_slice = plugin_items.into_boxed_slice();
    let items_ptr = Box::into_raw(boxed_slice) as *const PluginItem;

    PluginResults {
        items: items_ptr,
        count,
    }
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
        let _ = Command::new("open").arg("-R").arg(path_str.as_ref()).spawn();
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

    let slice = unsafe { std::slice::from_raw_parts_mut(results.items as *mut PluginItem, results.count) };
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
