//! aerofi: a lightweight, keyboard- and mouse-driven script launcher.
//!
//! Composition root only: scans the scripts folder, opens the GPUI window,
//! wires keystrokes and the global hotkey. Everything else lives in
//! `common/` (types), `core/` (business logic), `ui/` (rendering) and
//! `sys/` (macOS system calls). See ARCHITECTURE.md.

mod common;
mod core;
mod sys;
mod ui;

use gpui::App;
use gpui_platform::application;

fn main() {
    // Index all targets (applications + scripts) and print a summary plus the
    // parsed script metadata, so the parser can be verified at startup.
    let app_config = core::config::AppConfig::load();
    let mut targets = core::scanner::scan_all(&app_config);
    let history = core::history::History::load();
    let app_count = targets
        .iter()
        .filter(|t| matches!(t, core::item::Target::App { .. }))
        .count();
    let script_count = targets
        .iter()
        .filter(|t| matches!(t, core::item::Target::Script { .. }))
        .count();
    let builtin_count = targets
        .iter()
        .filter(|t| matches!(t, core::item::Target::Builtin { .. }))
        .count();
    println!(
        "aerofi: indexed {} target(s) ({} app(s), {} script(s), {} builtin(s))",
        targets.len(),
        app_count,
        script_count,
        builtin_count
    );
    let mut script_i = 0;
    for item in &targets {
        if let core::item::Target::Script {
            name,
            mode,
            icon,
            path,
            ..
        } = item
        {
            script_i += 1;
            println!(
                "  {}. {} | mode={} | icon={:?} | path={}",
                script_i,
                name,
                mode.as_str(),
                icon,
                path.display()
            );
        }
    }

    // Global target shortcuts (ADR 0002): resolve the configured combos to
    // keycodes and the named targets. Unknown combos/targets are skipped
    // with a warning and never block startup.
    let mut globals = Vec::new();
    for (combo, name) in &app_config.global_shortcuts {
        match sys::carbon::parse_combo(combo) {
            Some((keycode, modifiers)) => {
                if let Some(target) = targets.iter().find(|t| t.name() == name).cloned() {
                    globals.push(sys::carbon::GlobalBinding {
                        keycode,
                        modifiers,
                        target,
                        label: combo.clone(),
                    });
                } else {
                    eprintln!(
                        "aerofi: warning: global shortcut {combo:?}: unknown target {name:?}"
                    );
                }
            }
            None => eprintln!(
                "aerofi: warning: global shortcut {combo:?}: unsupported combo \
                 (need cmd/ctrl/opt + a key from a-z 0-9 f1-f12 space/tab/return/arrows)"
            ),
        }
    }

    application().run(move |cx: &mut App| {
        // Run as a background accessory (no Dock icon). GPUI's
        // applicationDidFinishLaunching just forced the Regular policy, and
        // we are still inside it, so the Dock icon never appears.
        sys::appkit::hide_from_dock();
        // Extract native app icons now that the Objective-C run loop is
        // active and MainThreadMarker is available.
        sys::icons::extract_all(&mut targets);
        let theme = core::theme::load_theme(&app_config.theme);
        let view =
            ui::window::create_launcher_window(cx, targets.clone(), theme, app_config, history);

        // start_daemon uses only std::thread + cx.spawn (foreground executor).
        // We intentionally do NOT call cx.background_executor() here: that call
        // initializes smol's global thread pool (~8 threads × 2 MB each = 16-20 MB).
        // The foreground executor runs on the main run-loop with zero extra threads.
        let daemon_view = view.clone();
        crate::core::scheduler::start_daemon(cx, &targets, daemon_view);
        // Route every keystroke into the launcher while the window is visible.
        // `detach()` keeps the observer alive for the app's lifetime without
        // requiring us to hold the `Subscription` handle.
        // Route every keystroke into the launcher while the window is
        // visible; the resulting action's side effects run inside the view
        // update (mouse interactions on the launcher's elements use the
        // same `perform_action` path).
        // `detach()` keeps the observer alive for the app's lifetime without
        // requiring us to hold the `Subscription` handle.
        let view_clone = view.clone();
        cx.observe_keystrokes(move |event, _window, cx| {
            if !ui::window::is_visible() {
                return;
            }
            view_clone.update(cx, |launcher, cx| {
                let action = launcher.handle_keystroke(&event.keystroke);
                cx.notify();
                launcher.perform_action(action, cx);
            });
        })
        .detach();

        // Global hotkeys: Option+Space toggles the launcher; configured
        // `[global_shortcuts]` run their targets directly.
        if let Err(e) = sys::carbon::install(globals) {
            eprintln!("aerofi: failed to register global hotkeys: {e}");
        }

        // Start hidden: drop textures and yield focus back to the terminal.
        view.update(cx, |launcher, cx| {
            launcher.on_hide();
            cx.notify();
        });
        ui::window::hide();
    });
}
