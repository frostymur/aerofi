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

use gpui::{App, AppContext};
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
        let theme_clone = theme.clone();
        let view = ui::window::create_launcher_window(cx, targets.clone(), theme, app_config, history);
        
        // start_daemon uses only std::thread + cx.spawn (foreground executor).
        // We intentionally do NOT call cx.background_executor() here: that call
        // initializes smol's global thread pool (~8 threads × 2 MB each = 16-20 MB).
        // The foreground executor runs on the main run-loop with zero extra threads.
        let daemon_view = view.clone();
        crate::core::scheduler::start_daemon(cx, &targets, daemon_view);
        // Route every keystroke into the launcher while the window is visible.
        // `detach()` keeps the observer alive for the app's lifetime without
        // requiring us to hold the `Subscription` handle.
        let view_clone = view.clone();
        cx.observe_keystrokes(move |event, _window, cx| {
            if !ui::window::is_visible() {
                return;
            }
            let action = view_clone.update(cx, |launcher, cx| {
                let action = launcher.handle_keystroke(&event.keystroke);
                cx.notify();
                action
            });

            match action {
                ui::launcher::LauncherAction::Hide => {
                    view_clone.update(cx, |launcher, _| launcher.on_hide());
                    ui::window::hide();
                }
                ui::launcher::LauncherAction::None => {}
                ui::launcher::LauncherAction::CopyToClipboardAndHide(text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    view_clone.update(cx, |launcher, _| launcher.on_hide());
                    ui::window::hide();
                }
                ui::launcher::LauncherAction::SetFullOutput { title, text } => {
                    view_clone.update(cx, |launcher, cx| {
                        launcher.set_full_output(title, text);
                        cx.notify();
                    });
                }
                ui::launcher::LauncherAction::SetInlineOutput { path, output } => {
                    view_clone.update(cx, |launcher, cx| {
                        launcher.apply_inline_output(&path, output);
                        cx.notify();
                    });
                }
                ui::launcher::LauncherAction::ExecuteScript(target, args) => {
                    if let core::item::Target::Script {
                        mode, path, name, ..
                    } = target
                    {
                        let view = view_clone.clone();
                        let title = name.to_string();
                        let path = path.clone();
                        let executor = cx.background_executor().clone();
                        let cx_async = cx.to_async();

                        let toast_view = if mode == core::item::ScriptMode::Compact
                            || mode == core::item::ScriptMode::Silent
                        {
                            if mode == core::item::ScriptMode::Silent {
                                view.update(cx, |launcher, _| launcher.on_hide());
                                ui::window::hide_launcher_only();
                            }
                            Some(ui::toast_window::open_toast_window(
                                cx,
                                theme_clone.clone(),
                                title.clone(),
                            ))
                        } else {
                            None
                        };

                        // Helper: build a Command that respects the script's shebang.
                        // The closure captures `path` by clone so the async block can own it.
                        let path2 = path.clone();
                        cx.spawn(move |_: &mut gpui::AsyncApp| async move {
                            let result = executor
                                .spawn(async move {
                                    let content = std::fs::read_to_string(&*path2).ok();
                                    let mut cmd = if let Some(ref c) = content {
                                        if let Some(first) = c.lines().next() {
                                            if let Some(shebang) = first.strip_prefix("#!") {
                                                let parts: Vec<&str> =
                                                    shebang.split_whitespace().collect();
                                                if !parts.is_empty() {
                                                    let mut cmd =
                                                        std::process::Command::new(parts[0]);
                                                    cmd.args(&parts[1..]);
                                                    cmd.arg(&*path2);
                                                    cmd
                                                } else {
                                                    std::process::Command::new(&*path2)
                                                }
                                            } else {
                                                std::process::Command::new(&*path2)
                                            }
                                        } else {
                                            std::process::Command::new(&*path2)
                                        }
                                    } else {
                                        std::process::Command::new(&*path2)
                                    };
                                    cmd.args(args);
                                    cmd.output()
                                })
                                .await;

                            cx_async.update(|cx| {
                                match mode {
                                    // pipe: copy stdout to clipboard, hide.
                                    core::item::ScriptMode::Pipe => {
                                        if let Ok(out) = result {
                                            let text =
                                                String::from_utf8_lossy(&out.stdout).to_string();
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                text,
                                            ));
                                        }
                                        view.update(cx, |launcher, _| launcher.on_hide());
                                        ui::window::hide();
                                    }
                                    // fullOutput: update the launcher view to show full page output.
                                    core::item::ScriptMode::FullOutput => {
                                        let text = match result {
                                            Ok(out) => {
                                                let stdout = String::from_utf8_lossy(&out.stdout)
                                                    .to_string();
                                                let stderr = String::from_utf8_lossy(&out.stderr);
                                                if !stderr.trim().is_empty()
                                                    && stdout.trim().is_empty()
                                                {
                                                    stderr.into_owned()
                                                } else if !stderr.trim().is_empty() {
                                                    format!("{}\n\n[stderr]\n{}", stdout, stderr)
                                                } else if stdout.trim().is_empty() {
                                                    "(no output)".to_string()
                                                } else {
                                                    stdout
                                                }
                                            }
                                            Err(e) => format!("Error: {e}"),
                                        };
                                        view.update(cx, |launcher, cx| {
                                            launcher.set_full_output(title, text);
                                            cx.notify();
                                        });
                                    }
                                    // compact/silent: update the floating toast window.
                                    core::item::ScriptMode::Compact
                                    | core::item::ScriptMode::Silent => {
                                        let (text, is_error) = match result {
                                            Ok(out) => {
                                                let is_err = !out.status.success();
                                                let src =
                                                    if is_err { &out.stderr } else { &out.stdout };
                                                let raw = String::from_utf8_lossy(src);
                                                // Show the last non-empty line (Raycast compact behaviour).
                                                let last = raw
                                                    .lines()
                                                    .rfind(|l| !l.trim().is_empty())
                                                    .unwrap_or(if is_err {
                                                        "Script failed."
                                                    } else {
                                                        "Done."
                                                    })
                                                    .to_string();
                                                (last, is_err)
                                            }
                                            Err(e) => (format!("Error: {e}"), true),
                                        };
                                        if let Some((win_handle, toast)) = toast_view {
                                            let mut cx_async = cx.to_async();
                                            toast.update(cx, |t, cx| {
                                                t.set_done(text, is_error);
                                                cx.notify();
                                                cx.spawn(
                                                    move |_, _: &mut gpui::AsyncApp| async move {
                                                        cx_async
                                                            .background_executor()
                                                            .timer(std::time::Duration::from_secs(
                                                                3,
                                                            ))
                                                            .await;
                                                        let _ = cx_async.update_window(
                                                            win_handle,
                                                            |_, window, _| window.remove_window(),
                                                        );
                                                    },
                                                )
                                                .detach();
                                            });
                                        }
                                    }
                                    // inline: update the subtitle in the list row.
                                    core::item::ScriptMode::Inline => {
                                        let output = match result {
                                            Ok(out) => {
                                                let raw = String::from_utf8_lossy(&out.stdout);
                                                raw.lines().rfind(|l| !l.trim().is_empty()).map(
                                                    |s| gpui::SharedString::from(s.to_string()),
                                                )
                                            }
                                            Err(_) => None,
                                        };
                                        view.update(cx, |launcher, cx| {
                                            launcher.apply_inline_output(&path, output);
                                            cx.notify();
                                        });
                                    }
                                }
                            });
                        })
                        .detach();
                    }
                }
            }
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
