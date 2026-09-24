//! Spawning script execution and routing its output to the right view.
//!
//! Shared by both input paths: the keystroke observer in `main.rs` and the
//! mouse listeners on the launcher's elements. Callers hide the launcher
//! window for `silent` scripts themselves (they hold the view).

use gpui::{App, AppContext, AsyncApp, Entity};
use std::sync::Arc;

use crate::core::item::{ScriptMode, Target};
use crate::core::theme::ThemeConfig;
use crate::ui::launcher::Launcher;

/// Spawn the given script with `args` and route the output by mode:
/// clipboard for `pipe`, the full-output page for `fullOutput`, the toast
/// window for `compact`/`silent`, and the row subtitle for `inline`.
pub fn execute_script(
    cx: &mut App,
    view: Entity<Launcher>,
    theme: Arc<ThemeConfig>,
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
        Some(crate::ui::toast_window::open_toast_window(
            cx,
            theme.clone(),
            title.clone(),
        ))
    } else {
        None
    };

    // The closure captures `path` by clone so the async block can own it.
    let path2 = path.clone();
    // Bound the captured output: display modes only need the head, the
    // clipboard a generous amount. Unbounded capture spikes RSS on chatty
    // scripts (the allocator never returns the high-water mark).
    let stdout_cap = if mode == ScriptMode::Pipe {
        crate::core::executor::MAX_CLIPBOARD_OUTPUT
    } else {
        crate::core::executor::MAX_DISPLAY_OUTPUT
    };
    cx.spawn(move |_: &mut AsyncApp| async move {
        let result = executor
            .spawn(async move { crate::core::executor::run_bounded(&path2, &args, stdout_cap) })
            .await;

        cx_async.update(|cx| {
            match mode {
                // pipe: copy stdout to clipboard, hide.
                ScriptMode::Pipe => {
                    if let Ok(out) = result {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(out.stdout));
                    }
                    view.update(cx, |launcher, _| launcher.on_hide());
                    crate::ui::window::hide();
                }
                // fullOutput: update the launcher view to show full page output.
                ScriptMode::FullOutput => {
                    let text = match result {
                        Ok(out) => {
                            if !out.stderr.trim().is_empty() && out.stdout.trim().is_empty() {
                                out.stderr
                            } else if !out.stderr.trim().is_empty() {
                                format!("{}\n\n[stderr]\n{}", out.stdout, out.stderr)
                            } else if out.stdout.trim().is_empty() {
                                "(no output)".to_string()
                            } else {
                                out.stdout
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
                            let is_err = !out.success;
                            // Show the last non-empty line (Raycast compact
                            // behaviour): stderr on failure, stdout
                            // otherwise.
                            let line = if is_err {
                                out.stderr.lines().rfind(|l| !l.trim().is_empty())
                            } else {
                                out.last_line.as_deref()
                            };
                            match line {
                                Some(l) => (l.to_string(), is_err),
                                None => {
                                    has_output = false;
                                    (
                                        if is_err {
                                            "Script failed.".to_string()
                                        } else {
                                            "Done.".to_string()
                                        },
                                        is_err,
                                    )
                                }
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
                    } else if mode == ScriptMode::Silent
                        && (has_output || is_error) {
                            let (win_handle, toast) = crate::ui::toast_window::open_toast_window(cx, theme.clone(), title.clone());
                            handle_toast(cx, win_handle, toast);
                        }
                }
                // inline: update the subtitle in the list row.
                ScriptMode::Inline => {
                    let output = result
                        .ok()
                        .and_then(|out| out.last_line)
                        .map(gpui::SharedString::from);
                    view.update(cx, |launcher, cx| {
                        launcher.apply_inline_output(&path, output);
                        cx.notify();
                    });
                }
                // rofi: interactive sessions are managed by the Launcher
                // state machine directly (start_rofi_session). This mode
                // should never reach execute_script.
                ScriptMode::Rofi => {}
            }
        });
    })
    .detach();
}
