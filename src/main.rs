//! aerofi: a lightweight, keyboard- and mouse-driven script launcher.
//!
//! Composition root only: scans the scripts folder, opens the GPUI window,
//! wires keystrokes and the global hotkey. Everything else lives in
//! `core/` (types & business logic), `ui/` (rendering) and
//! `sys/` (macOS system calls). See ARCHITECTURE.md.

mod core;
mod sys;
mod ui;

use gpui::App;
use gpui_platform::application;

fn main() {
    // Index all targets (applications + scripts).
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

    // Global target shortcuts (ADR 0002): resolve the configured combos to
    // keycodes and the named targets. Unknown combos/targets are skipped
    // with a warning and never block startup.
    let mut globals = Vec::new();
    for (combo, name) in &app_config.bindings.global {
        match sys::carbon::parse_combo(combo) {
            Some((keycode, modifiers)) => {
                if let Some(target) = targets.iter().find(|t| t.name() == name).cloned() {
                    // Skip pipe-mode scripts: they copy to clipboard, no global hotkey needed.
                    if matches!(
                        target,
                        crate::core::item::Target::Script {
                            mode: crate::core::item::ScriptMode::Pipe,
                            ..
                        }
                    ) {
                        eprintln!(
                            "aerofi: warning: global shortcut {combo:?}: skipping pipe-mode script {name:?}"
                        );
                        continue;
                    }
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

    // Safety net for exits that never return from `application().run`
    // (e.g. a `process::exit` deeper in the stack): libc's `exit()` runs
    // atexit handlers. Idempotent with the explicit call after `run`.
    extern "C" fn quit_cleanup() {
        core::executor::kill_all_scripts();
    }
    unsafe { libc::atexit(quit_cleanup) };

    application().run(move |cx: &mut App| {
        // Run as a background accessory (no Dock icon). GPUI's
        // applicationDidFinishLaunching just forced the Regular policy, and
        // we are still inside it, so the Dock icon never appears.
        sys::appkit::hide_from_dock();
        // Extract native app icons now that the Objective-C run loop is
        // active and MainThreadMarker is available.
        sys::icons::extract_all(&mut targets);
        // Register bundled fonts (~/.config/aerofi/fonts/) before the first
        // render so a theme's [font] family can reference them.
        let bundled = sys::fonts::load_custom_fonts(cx);
        if bundled > 0 {
            println!("aerofi: registered {bundled} bundled font(s)");
        }
        let theme = core::theme::load_theme(&app_config.theme);
        let toggle_hotkey = app_config.bindings.toggle.clone();
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
        cx.intercept_keystrokes(move |event, _window, cx| {
            if !ui::window::is_visible() {
                return;
            }
            view_clone.update(cx, |launcher, cx| {
                let action = launcher.handle_keystroke(&event.keystroke, Some(cx));
                cx.notify();
                launcher.perform_action(action, false, cx);
            });
        })
        .detach();

        // Global hotkeys: toggle_hotkey toggles the launcher; configured
        // `[bindings.global]` run their targets directly.
        if let Err(e) = sys::carbon::install(&toggle_hotkey, globals) {
            eprintln!("aerofi: failed to register global hotkeys: {e}");
        }

        // Start hidden: drop textures and yield focus back to the terminal.
        view.update(cx, |launcher, cx| {
            launcher.on_hide();
            cx.notify();
        });
        ui::window::hide();
    });
    // The app has terminated: take down every background script's process
    // group so nothing aerofi spawned survives the process as an orphan.
    core::executor::kill_all_scripts();
}
