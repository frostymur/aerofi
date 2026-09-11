//! Spawning script execution and routing its output to the right view.
//!
//! Shared by both input paths: the keystroke observer in `main.rs` and the
//! mouse listeners on the launcher's elements. Callers hide the launcher
//! window for `silent` scripts themselves (they hold the view).

use gpui::{App, AppContext, AsyncApp, Entity};

use crate::core::item::{ScriptMode, Target};
use crate::core::theme::ThemeConfig;
use crate::ui::launcher::Launcher;

/// Spawn the given script with `args` and route the output by mode:
/// clipboard for `pipe`, the full-output page for `fullOutput`, the toast
/// window for `compact`/`silent`, and the row subtitle for `inline`.
pub fn execute_script(
    cx: &mut App,
    view: Entity<Launcher>,
    theme: ThemeConfig,
    target: Target,
    args: Vec<String>,
) {
    let Target::Script {
        mode, path, name, ..
    } = target
    else {
        return;
    };
    let title = name.to_string();
    let executor = cx.background_executor().clone();
    let cx_async = cx.to_async();

    let toast_view = if mode == ScriptMode::Compact {
        Some(crate::ui::toast_window::open_toast_window(cx, theme.clone(), title.clone()))
    } else {
        None
    };

    // The closure captures `path` by clone so the async block can own it.
    let path2 = path.clone();
    cx.spawn(move |_: &mut AsyncApp| async move {
        let result = executor
            .spawn(async move {
                let mut cmd = crate::core::executor::script_command(&*path2);
                cmd.args(args);
                cmd.output()
            })
            .await;

        cx_async.update(|cx| {
            match mode {
                // pipe: copy stdout to clipboard, hide.
                ScriptMode::Pipe => {
                    if let Ok(out) = result {
                        let text = String::from_utf8_lossy(&out.stdout).to_string();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }
                    view.update(cx, |launcher, _| launcher.on_hide());
                    crate::ui::window::hide();
                }
                // fullOutput: update the launcher view to show full page output.
                ScriptMode::FullOutput => {
                    let text = match result {
                        Ok(out) => {
                            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            if !stderr.trim().is_empty() && stdout.trim().is_empty() {
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
                        launcher.set_full_output(title.clone(), text);
                        cx.notify();
                    });
                }
                // compact/silent: update the floating toast window.
                ScriptMode::Compact | ScriptMode::Silent => {
                    let mut has_output = true;
                    let (text, is_error) = match result {
                        Ok(out) => {
                            let is_err = !out.status.success();
                            let src = if is_err { &out.stderr } else { &out.stdout };
                            let raw = String::from_utf8_lossy(src);
                            // Show the last non-empty line (Raycast compact behaviour).
                            if let Some(l) = raw.lines().rfind(|l| !l.trim().is_empty()) {
                                (l.to_string(), is_err)
                            } else {
                                has_output = false;
                                (if is_err { "Script failed.".to_string() } else { "Done.".to_string() }, is_err)
                            }
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    };

                    let handle_toast = |cx: &mut App, win_handle: gpui::AnyWindowHandle, toast: Entity<crate::ui::toast_window::ToastWindow>| {
                        let mut cx_async = cx.to_async();
                        toast.update(cx, |t, cx| {
                            t.set_done(text.clone(), is_error);
                            cx.notify();
                            cx.spawn(move |_, _: &mut AsyncApp| async move {
                                cx_async
                                    .background_executor()
                                    .timer(std::time::Duration::from_secs(3))
                                    .await;
                                let _ = cx_async.update_window(win_handle, |_, window, _| {
                                    window.remove_window()
                                });
                            })
                            .detach();
                        });
                    };

                    if mode == ScriptMode::Compact {
                        if let Some((win_handle, toast)) = toast_view {
                            if !has_output && !is_error {
                                let mut cx_async = cx.to_async();
                                let _ = cx_async.update_window(win_handle, |_, window, _| {
                                    window.remove_window()
                                });
                            } else {
                                handle_toast(cx, win_handle, toast);
                            }
                        }
                    } else if mode == ScriptMode::Silent {
                        if has_output || is_error {
                            let (win_handle, toast) = crate::ui::toast_window::open_toast_window(cx, theme.clone(), title.clone());
                            handle_toast(cx, win_handle, toast);
                        }
                    }
                }
                // inline: update the subtitle in the list row.
                ScriptMode::Inline => {
                    let output = match result {
                        Ok(out) => {
                            let raw = String::from_utf8_lossy(&out.stdout);
                            raw.lines()
                                .rfind(|l| !l.trim().is_empty())
                                .map(|s| gpui::SharedString::from(s.to_string()))
                        }
                        Err(_) => None,
                    };
                    view.update(cx, |launcher, cx| {
                        launcher.apply_inline_output(&path, output);
                        cx.notify();
                    });
                }
                // gui: interactive sessions are managed by the Launcher
                // state machine directly (start_gui_session). This mode
                // should never reach execute_script.
                ScriptMode::Gui => {}
            }
        });
    })
    .detach();
}
