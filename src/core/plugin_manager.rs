use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use aerofi_plugin_api::{AerofiPlugin, PluginResults};

/// Wraps a single loaded `.dylib` plugin.
pub struct LoadedPlugin {
    /// Keep the library loaded as long as the plugin lives.
    _lib: Library,
    plugin: AerofiPlugin,
    /// Serializes every call into the plugin. The C ABI makes no thread-
    /// safety guarantee, and `query`/`parse_results`/`free_results`/`activate`
    /// all read or mutate plugin-owned state and memory — so two overlapping
    /// calls (e.g. a background `query` racing a main-thread `activate`, or two
    /// debounced queries) would be a data race in native code. Holding this
    /// lock across a full query-consume-free cycle (or a single activate) is
    /// what makes concurrent use sound. The type itself is `Send`+`Sync` via
    /// its fields (fn pointers, `Library`, strings), so no manual impl is
    /// needed; the lock is the actual synchronization.
    lock: Mutex<()>,
    pub name: String,
    pub prefix: String,
}

/// RAII wrapper around a plugin's query results that frees the plugin-
/// allocated memory on drop. `free_results` is the only way to release the
/// memory a plugin's `query` returned, so without this guard a panic between
/// `query` and `free_results` would leak it (on the plugin side).
struct OwnedResults<'a> {
    plugin: &'a LoadedPlugin,
    results: PluginResults,
}

impl Drop for OwnedResults<'_> {
    fn drop(&mut self) {
        self.plugin.free_results(self.results);
    }
}

impl LoadedPlugin {
    /// Load a plugin from a `.dylib` file.
    #[cfg_attr(test, allow(dead_code))] // only used by `load_all`, which is a no-op under cfg(test)
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let lib =
            unsafe { Library::new(path) }.map_err(|e| format!("Failed to load library: {e}"))?;

        let init_sym: Symbol<unsafe extern "C" fn() -> *const AerofiPlugin> =
            unsafe { lib.get(b"aerofi_plugin_init\0") }
                .map_err(|e| format!("Missing aerofi_plugin_init: {e}"))?;

        let plugin_ptr = unsafe { init_sym() };
        if plugin_ptr.is_null() {
            return Err("aerofi_plugin_init returned null".into());
        }

        // We do a shallow copy of the struct, which is safe since it only contains function pointers and scalars.
        let plugin = unsafe { std::ptr::read(plugin_ptr) };

        // If the API version mismatches, the struct layout may differ, so the
        // function pointers (including `destroy`) can't be trusted — just
        // unload the library (dropping `lib`) without calling into it.
        if plugin.api_version != 1 {
            return Err(format!(
                "Unsupported plugin API version: {}",
                plugin.api_version
            ));
        }

        // `init` failed: the struct is valid (version matched), so call
        // `destroy` to release anything the plugin set up before failing,
        // then unload the library (dropping `lib`).
        if !unsafe { (plugin.init)() } {
            unsafe { (plugin.destroy)() };
            return Err("Plugin initialization failed".into());
        }

        let metadata = unsafe { (plugin.get_metadata)() };
        let name = if metadata.name.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(metadata.name) }
                .to_string_lossy()
                .into_owned()
        };

        let prefix = if metadata.prefix.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(metadata.prefix) }
                .to_string_lossy()
                .into_owned()
        };

        Ok(Self {
            _lib: lib,
            plugin,
            lock: Mutex::new(()),
            name,
            prefix,
        })
    }

    /// Acquire the plugin lock, recovering if a previous holder poisoned it
    /// (a call cannot panic across the C ABI, so this is a defensive no-op in
    /// practice; it just avoids a second panic if it ever did).
    fn acquire(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Run a query and fully consume its results under the plugin lock.
    ///
    /// The query, the copy of every returned string, and the release of the
    /// plugin's result buffer all happen as one critical section, so the
    /// plugin is never re-entered while its result memory is live and two
    /// callers can never race it. Returns owned targets that are safe to use
    /// after the lock is dropped.
    pub fn query_parsed(&self, query: &str) -> Vec<crate::core::item::Target> {
        let _guard = self.acquire();
        let results = self.query(query);
        // Frees the plugin's result memory on drop — including if parsing
        // panics — so a mid-parse panic can't leak the plugin's allocation.
        let owned = OwnedResults {
            plugin: self,
            results,
        };
        self.parse_results(&owned.results)
    }

    /// Run a query on the plugin. The caller must hold the lock (see
    /// [`Self::query_parsed`]) and must eventually free the results.
    fn query(&self, query: &str) -> PluginResults {
        let c_query = CString::new(query).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { (self.plugin.query)(c_query.as_ptr()) }
    }

    /// Free the memory allocated by the plugin for the query results. Must be
    /// called with the lock held, on the same results a locked `query` returned.
    fn free_results(&self, results: PluginResults) {
        unsafe { (self.plugin.free_results)(results) }
    }

    /// Activate a specific item. Returns true if the launcher should be closed.
    ///
    /// Runs under the plugin lock so it cannot interleave with an in-flight
    /// query on the same plugin.
    pub fn activate(&self, id: &str, action_code: u32) -> bool {
        let _guard = self.acquire();
        let c_id = CString::new(id).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { (self.plugin.activate)(c_id.as_ptr(), action_code) }
    }

    /// Convert C ABI results to `Vec<Target>`, tagged with this plugin's name.
    /// Reads plugin-owned memory, so the caller must hold the lock.
    fn parse_results(&self, results: &PluginResults) -> Vec<crate::core::item::Target> {
        use aerofi_plugin_api::PluginItem;
        let items: &[PluginItem] = if results.count == 0 || results.items.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(results.items, results.count) }
        };
        items
            .iter()
            .map(|item| {
                let cstr = |ptr: *const std::ffi::c_char| -> Option<String> {
                    if ptr.is_null() {
                        None
                    } else {
                        Some(
                            unsafe { std::ffi::CStr::from_ptr(ptr) }
                                .to_string_lossy()
                                .into_owned(),
                        )
                    }
                };
                crate::core::item::Target::PluginItem {
                    name: gpui::SharedString::from(cstr(item.title).unwrap_or_default()),
                    subtitle: cstr(item.subtitle).map(gpui::SharedString::from),
                    icon: cstr(item.icon).map(gpui::SharedString::from),
                    plugin_id: gpui::SharedString::from(cstr(item.id).unwrap_or_default()),
                    plugin_name: gpui::SharedString::from(self.name.clone()),
                }
            })
            .collect()
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        unsafe { (self.plugin.destroy)() }
    }
}

/// Process-lifetime cache of loaded plugins, keyed by dylib path.
///
/// Each dylib is `dlopen`ed and `init`ed exactly once per process. On macOS,
/// re-`dlopen`ing an already-loaded path returns the *same* image, so a naive
/// reload (e.g. `Cmd+R`) would call `init` a second time and — once the stale
/// handles finally drop, possibly while a fresh instance is still active — run
/// `destroy` out from under it. Caching the `Arc<LoadedPlugin>` here means
/// reloads simply re-share the same handles, and `destroy` (via `Drop`) only
/// runs when the last reference goes away at process exit: the only safe time
/// to tear down native code.
#[cfg_attr(test, allow(dead_code))] // `load_all` is a no-op under cfg(test)
static PLUGIN_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<LoadedPlugin>>>> = OnceLock::new();

#[cfg_attr(test, allow(dead_code))]
fn plugin_cache() -> &'static Mutex<HashMap<PathBuf, Arc<LoadedPlugin>>> {
    PLUGIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Look up a plugin in the process-lifetime cache, loading it on first use.
///
/// Returns the shared handle (an `Arc` clone) or `None` if the dylib could
/// not be loaded. Safe to call repeatedly for the same path; only the first
/// call loads and inits the plugin.
#[cfg_attr(test, allow(dead_code))]
fn cached_load(path: &Path) -> Option<Arc<LoadedPlugin>> {
    let mut cache = plugin_cache().lock().unwrap();
    if let Some(existing) = cache.get(path) {
        return Some(existing.clone());
    }
    let plugin = match LoadedPlugin::load(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("aerofi: failed to load plugin from {}: {e}", path.display());
            return None;
        }
    };
    println!(
        "aerofi: loaded plugin '{}' (prefix: '{}') from {}",
        plugin.name,
        plugin.prefix,
        path.display()
    );
    let plugin = Arc::new(plugin);
    cache.insert(path.to_path_buf(), plugin.clone());
    Some(plugin)
}

/// Manages all loaded plugins.
pub struct PluginManager {
    pub plugins: Vec<Arc<LoadedPlugin>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Scan the plugins directory and load all `.dylib` files.
    pub fn load_all() -> Self {
        #[cfg(test)]
        return Self::new();

        #[cfg(not(test))]
        {
            let mut manager = Self::new();
            let mut candidate_dirs = Vec::new();

            if let Some(home) = dirs::home_dir() {
                candidate_dirs.push(home.join(".config").join("aerofi").join("plugins"));
            }
            if let Some(config_dir) = dirs::config_dir() {
                let p = config_dir.join("aerofi").join("plugins");
                if !candidate_dirs.contains(&p) {
                    candidate_dirs.push(p);
                }
            }

            let mut loaded_names = std::collections::HashSet::new();
            for dir in candidate_dirs {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("dylib")
                            && let Some(plugin) = cached_load(&path)
                            && loaded_names.insert(plugin.name.clone())
                        {
                            manager.plugins.push(plugin);
                        }
                    }
                }
            }

            manager
        }
    }

    /// Find a plugin by its prefix. If a query matches the prefix, returns the plugin and the remainder of the query.
    pub fn match_prefix<'a>(&self, query: &'a str) -> Option<(Arc<LoadedPlugin>, &'a str)> {
        for plugin in &self.plugins {
            if !plugin.prefix.is_empty() && query.starts_with(&plugin.prefix) {
                return Some((plugin.clone(), &query[plugin.prefix.len()..]));
            }
        }
        None
    }
}
