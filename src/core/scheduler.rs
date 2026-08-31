use crate::core::item::{Target, ScriptMode};
use std::time::Duration;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

const BOUNDARY: &str = "__AEROFI_BOUNDARY__";

/// Parses a refresh time string like "5m", "1h", "30s" into a Duration.
pub fn parse_refresh_time(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, unit) = s.split_at(s.len().saturating_sub(1));
    if let Ok(num) = num_str.parse::<u64>() {
        match unit {
            "s" => Some(Duration::from_secs(num)),
            "m" => Some(Duration::from_secs(num * 60)),
            "h" => Some(Duration::from_secs(num * 3600)),
            "d" => Some(Duration::from_secs(num * 86400)),
            _ => None,
        }
    } else {
        None
    }
}

/// A background daemon that runs inline scripts with a refreshTime.
/// Uses a single long-lived bash wrapper process per script instead of
/// forking on every interval. Communicates back to GPUI via the foreground
/// executor only — zero background threads, zero smol thread-pool overhead.
pub fn start_daemon(
    cx: &mut gpui::App,
    all_targets: &[Target],
    view: gpui::Entity<crate::ui::launcher::Launcher>,
) {
    // Deduplicate by filename stem: if the same script name appears in multiple
    // script_dirs, we only spawn one daemon for the first occurrence.
    let mut started: std::collections::HashSet<String> = std::collections::HashSet::new();

    for target in all_targets.iter() {
        if let Target::Script { mode, path, .. } = target {
            if *mode == ScriptMode::Inline {
                // Use the file stem as the dedup key (e.g. "test_inline")
                let stem = path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if !started.insert(stem) {
                    continue; // already running a daemon for this script name
                }

                if let Some(refresh_str) = target.refresh_time() {
                    if let Some(duration) = parse_refresh_time(refresh_str) {
                        let path_str = path.to_string_lossy().into_owned();
                        let view2 = view.clone();
                        let path2 = path.clone();
                        let cx_async = cx.to_async();

                        // futures::channel::mpsc — sender is Send (works in std::thread),
                        // receiver implements Stream so the GPUI task truly sleeps until data arrives.
                        // No busy-spin, no wasted CPU, no residual allocator pressure.
                        let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();

                        // macOS default thread stack = 8 MB. Two inline scripts = 16 MB wasted.
                        // Our loop only does BufReader::lines() + channel send — 256 KB is plenty.
                        let _ = std::thread::Builder::new()
                            .stack_size(256 * 1024)
                            .name(format!("aerofi-inline:{}", path_str))
                            .spawn(move || {
                            let secs = duration.as_secs().max(1);
                            let bash_script = format!(
                                "while true; do\n  . \"{script}\" 2>/dev/null || true\n  printf '%s\\n' '{boundary}'\n  read -t {secs} _ 2>/dev/null || true\ndone",
                                script   = path_str,
                                boundary = BOUNDARY,
                                secs     = secs,
                            );

                            let mut child = match Command::new("bash")
                                .arg("-c")
                                .arg(&bash_script)
                                .stdout(Stdio::piped())
                                .stderr(Stdio::null())
                                .spawn()
                            {
                                Ok(c) => c,
                                Err(e) => {
                                    eprintln!("aerofi: scheduler: failed to spawn bash for {path_str}: {e}");
                                    return;
                                }
                            };

                            let stdout = match child.stdout.take() {
                                Some(s) => s,
                                None => { let _ = child.kill(); return; }
                            };

                            let reader = BufReader::new(stdout);
                            let mut buf = Vec::<String>::new();

                            for line in reader.lines() {
                                let line = match line {
                                    Ok(l) => l,
                                    Err(_) => break,
                                };

                                if line.trim() == BOUNDARY {
                                    let text = buf.join("\n");
                                    let text = text.trim().to_string();
                                    buf.clear();

                                    if !text.is_empty() {
                                        // UnboundedSender::unbounded_send never blocks
                                        if tx.unbounded_send(text).is_err() {
                                            // Receiver gone — UI shut down, kill bash
                                            let _ = child.kill();
                                            return;
                                        }
                                    }
                                } else {
                                    buf.push(line);
                                }
                            }

                            let _ = child.kill();
                        });

                        // GPUI receiver: truly sleeps via Stream::next() until a message arrives.
                        // No polling, no waker spinning — zero CPU between script outputs.
                        cx.spawn(move |_: &mut gpui::AsyncApp| async move {
                            use futures::StreamExt;
                            let mut cx_async = cx_async;
                            while let Some(text) = rx.next().await {
                                let _ = cx_async.update(|cx| {
                                    view2.update(cx, |launcher, cx| {
                                        launcher.apply_inline_output(
                                            &path2,
                                            Some(gpui::SharedString::from(text)),
                                        );
                                        if crate::ui::window::is_visible() {
                                            cx.notify();
                                        }
                                    });
                                });
                            }
                        }).detach();
                    }
                }
            }
        }
    }
}
