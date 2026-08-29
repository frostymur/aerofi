//! aerofi: a lightweight, keyboard-driven script launcher.
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
    let targets = core::scanner::scan_all(&app_config);
    let app_count = targets
        .iter()
        .filter(|t| matches!(t, core::item::Target::App { .. }))
        .count();
    println!(
        "aerofi: indexed {} target(s) ({} app(s), {} script(s))",
        targets.len(),
        app_count,
        targets.len() - app_count
    );
    let mut script_i = 0;
    for item in &targets {
        if let core::item::Target::Script {
            name,
            mode,
            icon,
            path,
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

    application().run(|cx: &mut App| {
        let config = common::config::Config::default();
        let view = ui::window::create_launcher_window(cx, targets, config, app_config);
        // Route every keystroke into the launcher while the window is visible.
        // `detach()` keeps the observer alive for the app's lifetime without
        // requiring us to hold the `Subscription` handle.
        cx.observe_keystrokes(move |event, _window, cx| {
            if !ui::window::is_visible() {
                return;
            }
            let should_hide = view.update(cx, |launcher, cx| {
                let hide = launcher.handle_keystroke(&event.keystroke);
                cx.notify();
                hide
            });
            if should_hide {
                ui::window::hide();
            }
        })
        .detach();

        // Global hotkey: Option+Space toggles the launcher.
        if let Err(e) = sys::carbon::install() {
            eprintln!("aerofi: failed to register global hotkey: {e}");
        }

        // Start hidden: yield focus back to the terminal.
        ui::window::hide();
    });
}
