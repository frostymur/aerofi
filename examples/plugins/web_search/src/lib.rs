use aerofi_plugin_api::{AerofiPlugin, PluginItem, PluginMetadata, PluginResults};
use std::ffi::{c_char, CStr, CString};
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
        name: CString::new("web-search").unwrap().into_raw(),
        description: CString::new("Quick web search via browser").unwrap().into_raw(),
        prefix: CString::new("g ").unwrap().into_raw(),
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
        return PluginResults {
            items: ptr::null(),
            count: 0,
        };
    }

    let google_title = CString::new(format!("Search Google for '{}'", q_trimmed)).unwrap();
    let google_sub = CString::new("Opens Google Search in your default browser").unwrap();
    let google_id = CString::new(format!("google:{}", q_trimmed)).unwrap();
    let google_icon = CString::new("🔍").unwrap();

    let ddg_title = CString::new(format!("Search DuckDuckGo for '{}'", q_trimmed)).unwrap();
    let ddg_sub = CString::new("Opens DuckDuckGo Search in your default browser").unwrap();
    let ddg_id = CString::new(format!("ddg:{}", q_trimmed)).unwrap();
    let ddg_icon = CString::new("🦆").unwrap();

    let items = vec![
        PluginItem {
            id: google_id.into_raw(),
            title: google_title.into_raw(),
            subtitle: google_sub.into_raw(),
            icon: google_icon.into_raw(),
        },
        PluginItem {
            id: ddg_id.into_raw(),
            title: ddg_title.into_raw(),
            subtitle: ddg_sub.into_raw(),
            icon: ddg_icon.into_raw(),
        },
    ];

    let count = items.len();
    let boxed_slice = items.into_boxed_slice();
    let items_ptr = Box::into_raw(boxed_slice) as *const PluginItem;

    PluginResults {
        items: items_ptr,
        count,
    }
}

unsafe extern "C" fn activate(id_ptr: *const c_char, _action_code: u32) -> bool {
    if id_ptr.is_null() {
        return true;
    }

    let id = unsafe { CStr::from_ptr(id_ptr) }.to_string_lossy();

    let url = if let Some(q) = id.strip_prefix("google:") {
        format!("https://www.google.com/search?q={}", url_encode(q))
    } else if let Some(q) = id.strip_prefix("ddg:") {
        format!("https://duckduckgo.com/?q={}", url_encode(q))
    } else {
        return true;
    };

    let _ = Command::new("open").arg(url).spawn();
    true // Close launcher after opening search
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

fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
