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
    // Scan the scripts folder and print the parsed metadata so the parser can
    // be verified at startup.
    let scripts_dir = std::path::Path::new("examples/scripts");
    let scripts = core::scanner::scan_scripts(scripts_dir);
    println!(
        "aerofi: parsed {} script(s) from {}",
        scripts.len(),
        scripts_dir.display()
    );
    for (i, item) in scripts.iter().enumerate() {
        println!(
            "  {}. {} | mode={} | icon={:?} | path={}",
            i + 1,
            item.name,
            item.mode.as_str(),
            item.icon,
            item.path.display()
        );
    }

    application().run(|cx: &mut App| {
        let config = common::config::Config::default();
        let view = ui::window::create_launcher_window(cx, scripts, config);

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
