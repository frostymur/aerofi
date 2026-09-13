use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aerofi_plugin_api::{AerofiPlugin, PluginResults};

/// Wraps a single loaded `.dylib` plugin.
#[allow(dead_code)]
pub struct LoadedPlugin {
    /// Keep the library loaded as long as the plugin lives.
    _lib: Library,
    plugin: AerofiPlugin,
    pub name: String,
    pub description: String,
    pub prefix: String,
    pub path: PathBuf,
}

impl LoadedPlugin {
    /// Load a plugin from a `.dylib` file.
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

        if plugin.api_version != 1 {
            return Err(format!(
                "Unsupported plugin API version: {}",
                plugin.api_version
            ));
        }

        if !unsafe { (plugin.init)() } {
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

        let description = if metadata.description.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(metadata.description) }
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
            name,
            description,
            prefix,
            path: path.to_path_buf(),
        })
    }

    /// Run a query on the plugin. The caller must ensure that the results are eventually freed by calling `free_results`.
    pub fn query(&self, query: &str) -> PluginResults {
        let c_query = CString::new(query).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { (self.plugin.query)(c_query.as_ptr()) }
    }

    /// Free the memory allocated by the plugin for the query results.
    pub fn free_results(&self, results: PluginResults) {
        unsafe { (self.plugin.free_results)(results) }
    }

    /// Activate a specific item.
    /// Returns true if the launcher should be closed.
    pub fn activate(&self, id: &str, action_code: u32) -> bool {
        let c_id = CString::new(id).unwrap_or_else(|_| CString::new("").unwrap());
        unsafe { (self.plugin.activate)(c_id.as_ptr(), action_code) }
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        unsafe { (self.plugin.destroy)() }
    }
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
        let mut manager = Self::new();

        #[cfg(test)]
        return manager;

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
                    if path.extension().and_then(|e| e.to_str()) == Some("dylib") {
                        match LoadedPlugin::load(&path) {
                            Ok(plugin) => {
                                if loaded_names.insert(plugin.name.clone()) {
                                    println!(
                                        "aerofi: loaded plugin '{}' (prefix: '{}') from {}",
                                        plugin.name,
                                        plugin.prefix,
                                        path.display()
                                    );
                                    manager.plugins.push(Arc::new(plugin));
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "aerofi: failed to load plugin from {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        manager
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
