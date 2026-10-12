use std::sync::Arc;

use gpui::Context;

use crate::core::search::SearchIndex;
use crate::core::widget::WidgetRegistry;

use super::state::Launcher;
use super::types::{LauncherState, MdStyled};

impl Launcher {
    /// Re-read `config.toml`, rescan the targets and rebuild the search
    /// index (aliases, `max_results`, sources, ignored apps, script dirs).
    ///
    /// `pub(crate)` so global Builtin bindings (`"ctrl+alt+r" =
    /// "Reload Configuration"`) can drive it through the view from
    /// `ui::window`.
    pub(crate) fn reload(&mut self) {
        let config = crate::core::config::AppConfig::load();
        let theme_name = &config.theme;
        let theme = crate::core::theme::load_theme(theme_name);
        let mut targets = crate::core::scanner::scan_all(&config);
        crate::sys::icons::extract_all(&mut targets);
        self.app_config = config;
        self.base_count = targets.len();
        self.all = targets;
        // Reconcile inline-script daemons with the new target set (scripts
        // added/removed/re-intervalled by the reload).
        crate::core::scheduler::reconcile_daemons(&self.all);
        // Re-point the source watcher at the (possibly new) dirs —
        // `[scripts]`, `[apps]`, and `[sources]` may all have changed.
        crate::core::fs_watcher::reconcile(&self.app_config);
        self.search = SearchIndex::new(&self.app_config.aliases, &self.app_config.pinned);
        self.plugin_manager = crate::core::plugin_manager::PluginManager::load_all();
        // Re-resolve the active source filter against the reloaded
        // Re-resolve the active mode against the reloaded config and
        // plugin set. A [modes] filter survives if it still exists; a
        // plugin mode ends with the query clear below (its session needs
        // a query) — silently, since the name was never invalid.
        if let Some(name) = self.mode_name.clone() {
            let name = name.to_string();
            if self.plugin_manager.find_by_name(&name).is_some() {
                self.mode_name = None;
                self.mode_mask = None;
            } else {
                let mask = self
                    .app_config
                    .modes
                    .get(&name)
                    .and_then(|v| Self::parse_mode_list(v));
                match mask {
                    Some(m) => self.mode_mask = Some(m),
                    None => {
                        eprintln!(
                            "aerofi: warning: mode {name:?} is gone or source-less \
after reload; showing all sources"
                        );
                        self.mode_name = None;
                        self.mode_mask = None;
                    }
                }
            }
        }
        self.widget_registry = WidgetRegistry::from_theme(&theme.widgets);
        self.image_dims = super::helpers::collect_image_dims(&theme);
        self.button_hotkeys = self.widget_registry.button_hotkeys();
        self.theme = Arc::new(theme);

        self.pango_cache.borrow_mut().clear();
        self.sticky_metatags = None;
        crate::sys::gpu::trim_gpu_memory();

        // Start reveal dance so the UI doesn't stutter or show half-rendered
        // frames while transitioning to the new theme's dimensions.
        crate::sys::appkit::set_window_alpha(0.0);
        self.pending_reveal = true;
        self.reveal_wait_frames = 0;
        crate::sys::appkit::arm_reveal_safety_net(120);

        self.font_fallbacks = super::helpers::font_fallback_families(&self.theme.font.fallback);
        let (root_font, inputbar_font, element_font_val, mono_font) =
            Self::build_fonts(self.theme.as_ref(), &self.font_fallbacks);
        self.root_font = root_font;
        self.inputbar_font = inputbar_font;
        self.element_font_val = element_font_val;
        self.mono_font = mono_font;
        // Markdown styling is theme-derived (link color, code background,
        // mono family), so rebuild it for any blocks still on screen.
        self.full_output_styled = self
            .full_output_blocks
            .iter()
            .map(|b| self.build_md_styled(b))
            .collect();
        if let LauncherState::RofiMode {
            preview_blocks: Some(blocks),
            ..
        } = &self.state
        {
            let styled: Vec<Option<MdStyled>> =
                blocks.iter().map(|b| self.build_md_styled(b)).collect();
            if let LauncherState::RofiMode { preview_styled, .. } = &mut self.state {
                *preview_styled = Some(styled);
            }
        }
        crate::sys::appkit::set_corner_radius(self.theme.window.corner_radius);
        // Defer centering to the next render() call so it fires *after*
        // window.resize() applies the new theme dimensions.
        self.needs_center = true;
        self.query.clear();
        self.refilter(None);
        self.selected = 0;
        println!(
            "aerofi: configuration reloaded (theme: {})",
            self.app_config.theme
        );
    }

    /// Rescan only the script targets after a script-folder change on disk
    /// (the file watcher).
    ///
    /// The lightweight counterpart of [`Self::reload`]: re-reads the script
    /// folders, splices the result into the base list (apps and builtins
    /// stay put), and reconciles the inline-script daemons. It does not
    /// re-read the config, rebuild the theme/fonts, or re-extract icons
    /// (scripts resolve theirs at parse time).
    pub(crate) fn rescan_scripts(&mut self) {
        let new_scripts = crate::core::scanner::scan_scripts(&self.app_config);
        let script_count = new_scripts.len();
        self.base_count =
            crate::core::scanner::replace_scripts(&mut self.all, self.base_count, new_scripts);
        // New/re-intervalled inline scripts get their daemons immediately.
        crate::core::scheduler::reconcile_daemons(&self.all);
        self.refilter(None);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        println!(
            "aerofi: rescanned script folders ({} script(s))",
            script_count
        );
    }

    /// Rescan only the application targets after an app-dir change on
    /// disk (the fs watcher).
    ///
    /// Like [`Self::rescan_scripts`], this splices the freshly scanned
    /// apps into the base list (scripts and builtins stay put). Icon
    /// extraction is disk-cached: an already-cached app costs one
    /// metadata check, only genuinely new bundles hit AppKit.
    pub(crate) fn rescan_apps(&mut self, cx: Option<&mut Context<Self>>) {
        if !self.app_config.sources.apps {
            return;
        }
        let new_apps = crate::core::scanner::scan_applications(&self.app_config);
        let app_count = new_apps.len();
        self.base_count =
            crate::core::scanner::replace_apps(&mut self.all, self.base_count, new_apps);
        // Icon extraction is disk-cached: an already-cached app costs one
        // metadata check, only genuinely new bundles hit AppKit. The
        // CPU-heavy downsample of new bundles must not block the main
        // thread, so with a context we route through the async job
        // pipeline; on_show (no context) falls back to the blocking
        // variant — that path only runs when the window is re-shown, and
        // by then new bundles are rare.
        let jobs = crate::sys::icons::prepare_icon_jobs(&mut self.all);
        if !jobs.is_empty() {
            match cx {
                Some(cx) => self.start_icon_jobs(jobs, cx),
                None => {
                    let results = crate::sys::icons::process_icon_jobs(jobs);
                    crate::sys::icons::apply_icon_results(&mut self.all, results);
                }
            }
        }
        self.refilter(None);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        println!("aerofi: rescanned application folders ({app_count} app(s))");
    }

    /// Rescan the plugin directories after a change on disk (the fs
    /// watcher).
    ///
    /// `load_all` reuses the per-path plugin cache, so an unchanged
    /// `.dylib` keeps the same `LoadedPlugin` (and any live session
    /// referencing it keeps working); only added/removed plugins change
    /// the set. A plugin-mode that lost its plugin falls back to all
    /// sources.
    pub(crate) fn rescan_plugins(&mut self) {
        self.plugin_manager = crate::core::plugin_manager::PluginManager::load_all();
        if let Some(name) = self.mode_name.clone() {
            let name = name.to_string();
            let survives = self.app_config.modes.contains_key(&name)
                || self.plugin_manager.find_by_name(&name).is_some();
            if !survives {
                eprintln!(
                    "aerofi: warning: plugin {name:?} was removed; \
showing all sources"
                );
                self.mode_name = None;
                self.mode_mask = None;
            }
        }
        self.refilter(None);
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        println!(
            "aerofi: rescanned plugin directories ({} plugin(s))",
            self.plugin_manager.plugins.len()
        );
    }

    /// Start background icon processing for the given jobs (collected on
    /// the main thread by [`crate::sys::icons::prepare_icon_jobs`]). The
    /// CPU-heavy downsample runs on a worker thread; results are applied to
    /// `all` when they arrive, so the first frame never waits on it.
    pub fn start_icon_jobs(
        &mut self,
        jobs: Vec<crate::sys::icons::IconJob>,
        cx: &mut Context<Self>,
    ) {
        if jobs.is_empty() {
            return;
        }
        let (tx, rx) = futures::channel::oneshot::channel();
        if let Err(e) = std::thread::Builder::new()
            .name("aerofi-icon-extract".into())
            .spawn(move || {
                let _ = tx.send(crate::sys::icons::process_icon_jobs(jobs));
            })
        {
            eprintln!("aerofi: failed to spawn icon worker thread: {e}");
            return;
        }
        self.icon_task = Some(
            cx.spawn(|view: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                // Clone into an owned handle so the future is 'static.
                let mut cx = cx.clone();
                async move {
                    let Ok(results) = rx.await else {
                        return;
                    };
                    let _ = view.update(&mut cx, |this, cx| {
                        crate::sys::icons::apply_icon_results(&mut this.all, results);
                        cx.notify();
                    });
                }
            }),
        );
    }
}
