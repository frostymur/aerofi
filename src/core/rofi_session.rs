//! Async I/O wrapper for a running Rofi-mode script process.
//!
//! A `RofiSession` owns a child process whose stdout emits
//! [`RofiBurst`](crate::core::rofi_protocol::RofiBurst) frames and whose
//! stdin receives selected row text.  The child stays alive across
//! multiple interaction rounds (wizard steps).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::core::rofi_protocol::{RofiBurst, RofiRow};

/// A live Rofi script session backed by a child process.
pub struct RofiSession {
    child: Child,
    stdin: ChildStdin,
    /// Receiver for lines produced by the background stdout reader thread.
    line_rx: mpsc::Receiver<LineEvent>,
}

/// Events from the stdout reader thread.
enum LineEvent {
    /// A line of text was read from stdout.
    Line(String),
    /// The stdout stream reached EOF (process exited or closed stdout).
    Eof,
    /// An I/O error occurred while reading.
    Error(String),
}

/// Maximum bytes a single stdout line may occupy before the reader stops.
/// A protocol row is short; a multi-MiB "line" is a pathological script that
/// would otherwise allocate a giant `String` and flood the channel.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Maximum total bytes accumulated for a single burst. Bounds the in-memory
/// `lines` buffer so a script that streams without flushing cannot OOM the
/// launcher; once crossed the burst is returned early.
const MAX_BURST_BYTES: usize = 4 * 1024 * 1024;

/// Depth of the stdout line channel. A bounded channel gives backpressure:
/// when the consumer isn't draining, the reader thread blocks (and the
/// child's pipe fills) instead of the channel growing without bound.
const MAX_CHANNEL_LINES: usize = 4096;

/// Check if a stdout line represents an explicit frame flush command.
fn is_flush_line(line: &str) -> bool {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    trimmed == "\0flush" || trimmed.starts_with("\0flush\x1f")
}

impl RofiSession {
    /// Spawn a Rofi-mode script and return the session handle.
    ///
    /// The script is started with `stdin = piped`, `stdout = piped`,
    /// `stderr = piped` (stderr is discarded for now — a future version
    /// may route it to the error toast).
    pub fn spawn(
        path: &Path,
        args: Vec<String>,
        envs: std::collections::HashMap<String, String>,
    ) -> std::io::Result<Self> {
        let mut cmd = crate::core::executor::script_command(path);
        cmd.args(args);
        cmd.envs(envs);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let (tx, rx) = mpsc::sync_channel(MAX_CHANNEL_LINES);
        // Background thread reads stdout line by line and sends events.
        // The reader loop only does BufReader::lines() + a channel send —
        // the default 8 MB stack reservation is unnecessary.
        let spawned = std::thread::Builder::new()
            .name("aerofi-rofi-stdout".to_string())
            .stack_size(256 * 1024)
            .spawn(move || {
                Self::stdout_reader_thread(stdout, tx);
            });
        if let Err(e) = spawned {
            // Couldn't start the stdout reader (e.g. OOM). Kill the child so
            // it doesn't run untracked and unkillable, then report the
            // failure instead of panicking out of the UI path.
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other(format!(
                "failed to spawn stdout reader thread: {e}"
            )));
        }

        let session = Self {
            child,
            stdin,
            line_rx: rx,
        };
        // Rofi sessions are killed on hide, but a quit that skips the
        // window still needs the script tree cleaned up.
        crate::core::executor::register_script(session.child.id());
        Ok(session)
    }

    /// The background thread that reads stdout lines and sends them
    /// to the main thread via the channel.
    ///
    /// The channel is bounded, so `tx.send` blocks (backpressure) when the
    /// consumer isn't draining — this is what keeps memory bounded even if a
    /// script streams faster than we read.
    fn stdout_reader_thread(stdout: ChildStdout, tx: mpsc::SyncSender<LineEvent>) {
        let reader = BufReader::new(stdout);
        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    if line.len() > MAX_LINE_BYTES {
                        // A pathologically long "line" — stop reading rather
                        // than allocate a giant buffer. The consumer observes
                        // the channel close and treats the session as exited.
                        return;
                    }
                    if tx.send(LineEvent::Line(line)).is_err() {
                        // Receiver dropped — session was killed.
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(LineEvent::Error(e.to_string()));
                    return;
                }
            }
        }
        let _ = tx.send(LineEvent::Eof);
    }

    /// Read one "burst" of output: all lines until the script blocks
    /// (no new line within `timeout` after the last received line) or
    /// until EOF.
    ///
    /// Read one "burst" of output: all lines until the script blocks,
    /// emits an explicit `\0flush` marker, or reaches EOF.
    ///
    /// When `\0flush` is encountered, the burst completes immediately with
    /// zero artificial timeout delay.
    pub fn read_burst(&self, timeout: Duration) -> ReadResult {
        let mut lines = Vec::new();
        let mut burst_bytes = 0usize;

        // First line: block for up to `timeout`.
        match self.line_rx.recv_timeout(timeout) {
            Ok(LineEvent::Line(line)) => {
                let is_flush = is_flush_line(&line);
                burst_bytes += line.len();
                lines.push(line);
                if is_flush {
                    return ReadResult::Burst(RofiBurst::from_lines(&lines));
                }
            }
            Ok(LineEvent::Eof) => return ReadResult::Exited,
            Ok(LineEvent::Error(e)) => return ReadResult::Error(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return ReadResult::Burst(RofiBurst::default());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return ReadResult::Exited,
        }

        // Subsequent lines: if a flush line is received, return immediately.
        // Otherwise use a short inter-line timeout to detect the end of the burst.
        let inter_line = Duration::from_millis(50);
        loop {
            match self.line_rx.recv_timeout(inter_line) {
                Ok(LineEvent::Line(line)) => {
                    let is_flush = is_flush_line(&line);
                    burst_bytes += line.len();
                    lines.push(line);
                    // A well-behaved script flushes each frame; if it keeps
                    // streaming past the cap, return what we have so the
                    // in-memory burst stays bounded.
                    if is_flush || burst_bytes > MAX_BURST_BYTES {
                        break;
                    }
                }
                Ok(LineEvent::Eof) => {
                    // Process the lines we have, then signal exit.
                    let burst = RofiBurst::from_lines(&lines);
                    return if burst.rows.is_empty() && burst.commands.is_empty() {
                        ReadResult::Exited
                    } else {
                        // Return the burst; next call will get Exited.
                        ReadResult::BurstThenExit(burst)
                    };
                }
                Ok(LineEvent::Error(_)) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        ReadResult::Burst(RofiBurst::from_lines(&lines))
    }

    /// Write a structured Rofi event to the script's stdin.
    pub fn send_event(
        &mut self,
        event: &crate::core::rofi_protocol::RofiEvent,
    ) -> std::io::Result<()> {
        writeln!(self.stdin, "{}", event.to_event_line())?;
        self.stdin.flush()
    }

    /// Write a live-search query change to the script's stdin.
    pub fn send_query_change(&mut self, query: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "\0change\x1f{query}")?;
        self.stdin.flush()
    }

    /// The child's pid (== its process group id, see
    /// `executor::script_command`).
    pub fn child_pid(&self) -> u32 {
        self.child.id()
    }

    /// Kill the script's entire process group (SIGKILL).
    ///
    /// Scripts run in their own process group (see
    /// `executor::script_command`), so the negative-pid signal reaches the
    /// script and every process it spawned — a plain `child.kill()` would
    /// orphan grandchildren.
    pub fn kill(&mut self) {
        let pid = self.child_pid();
        unsafe { libc::kill(-(pid as libc::c_int), libc::SIGKILL) };
        let _ = self.child.wait();
        crate::core::executor::unregister_script(pid);
    }
}

impl Drop for RofiSession {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Result of reading a burst from the session.
#[derive(Debug)]
pub enum ReadResult {
    /// A burst of commands and rows was received.
    Burst(RofiBurst),
    /// A burst was received but EOF followed immediately.
    BurstThenExit(RofiBurst),
    /// The script exited (EOF on stdout, no data).
    Exited,
    /// An I/O error occurred.
    Error(String),
}

/// Convenience: filter Rofi rows by fuzzy matching on `text` and `meta`.
pub fn filter_rofi_rows(rows: &[RofiRow], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..rows.len()).collect();
    }
    let query_lower = query.to_ascii_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            let text_lower = row.text.to_ascii_lowercase();
            let meta_lower = row
                .meta
                .as_deref()
                .map(|m| m.to_ascii_lowercase())
                .unwrap_or_default();
            text_lower.contains(&query_lower) || meta_lower.contains(&query_lower)
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_rofi_rows_empty_query() {
        let rows = vec![RofiRow::new("A"), RofiRow::new("B")];
        assert_eq!(filter_rofi_rows(&rows, ""), vec![0, 1]);
    }

    #[test]
    fn filter_rofi_rows_by_text() {
        let rows = vec![
            RofiRow::new("Firefox"),
            RofiRow::new("Chrome"),
            RofiRow::new("Safari"),
        ];
        assert_eq!(filter_rofi_rows(&rows, "fire"), vec![0]);
        assert_eq!(filter_rofi_rows(&rows, "chr"), vec![1]);
    }

    #[test]
    fn filter_rofi_rows_by_meta() {
        let mut row_a = RofiRow::new("Item A");
        row_a.meta = Some("secret keyword".into());
        let rows = vec![row_a, RofiRow::new("Item B")];
        assert_eq!(filter_rofi_rows(&rows, "secret"), vec![0]);
    }

    #[test]
    fn filter_rofi_rows_case_insensitive() {
        let rows = vec![RofiRow::new("Hello World")];
        assert_eq!(filter_rofi_rows(&rows, "HELLO"), vec![0]);
        assert_eq!(filter_rofi_rows(&rows, "hello"), vec![0]);
    }

    #[test]
    fn test_flush_line_detection() {
        assert!(is_flush_line("\0flush"));
        assert!(is_flush_line("\0flush\n"));
        assert!(is_flush_line("\0flush\r\n"));
        assert!(is_flush_line("\0flush\x1ftrue"));
        assert!(!is_flush_line("flush"));
        assert!(!is_flush_line("\0prompt\x1fflush"));
    }

    #[test]
    fn session_spawn_and_read() {
        let _lock = crate::core::executor::SCRIPT_PROCESS_LOCK.lock().unwrap();
        // Spawn a simple echo script that outputs protocol lines and exits.
        let dir = std::env::temp_dir();
        let script = dir.join(format!("aerofi_rofi_test_{}.sh", std::process::id()));
        std::fs::write(
            &script,
            "#!/bin/bash\nprintf '\\0prompt\\x1fTest Prompt\\n'\nprintf 'Item One\\0icon\\x1f🔥\\n'\nprintf 'Item Two\\0info\\x1fNew\\n'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let session =
            RofiSession::spawn(&script, vec![], std::collections::HashMap::new()).unwrap();
        let result = session.read_burst(Duration::from_secs(5));

        let burst = match result {
            ReadResult::Burst(b) | ReadResult::BurstThenExit(b) => b,
            other => panic!("Expected Burst, got {:?}", other),
        };

        assert_eq!(burst.commands.len(), 1);
        assert_eq!(
            burst.commands[0],
            crate::core::rofi_protocol::RofiCommand::SetPrompt("Test Prompt".to_string())
        );
        assert_eq!(burst.rows.len(), 2);
        assert_eq!(burst.rows[0].text, "Item One");
        assert_eq!(burst.rows[0].icon, Some("🔥".to_string()));
        assert_eq!(burst.rows[1].text, "Item Two");
        assert_eq!(burst.rows[1].info, Some("New".to_string()));

        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn session_send_event_reaches_script_stdin() {
        let _lock = crate::core::executor::SCRIPT_PROCESS_LOCK.lock().unwrap();
        // Replicates the power-menu flow: emit rows + flush, block on a
        // selection, then act on the selected id. Proves the Rust
        // send_event path delivers the structured event to the script.
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let script = dir.join(format!("aerofi_rofi_send_test_{pid}.sh"));
        let result_file = dir.join(format!("aerofi_rofi_send_test_{pid}.out"));
        let _ = std::fs::remove_file(&result_file);
        std::fs::write(
            &script,
            format!(
                "#!/bin/bash\n\
                 printf '\\0no-custom\\x1ftrue\\n'\n\
                 printf 'Lock\\0icon\\x1fX\\0id\\x1flock\\n'\n\
                 printf 'Sleep\\0icon\\x1fY\\0id\\x1fsleep\\n'\n\
                 printf '\\0flush\\n'\n\
                 IFS= read -r -n 1 _nul\n\
                 IFS= read -r event_line\n\
                 choice=\"\"\n\
                 IFS=$'\\x1f' read -r -a fields <<< \"$event_line\"\n\
                 for kv in \"${{fields[@]}}\"; do\n\
                 case \"$kv\" in id:*) choice=${{kv#id:}} ;; esac\n\
                 done\n\
                 printf '%s' \"$choice\" > \"{out}\"\n",
                out = result_file.display(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let mut session =
            RofiSession::spawn(&script, vec![], std::collections::HashMap::new()).unwrap();
        let result = session.read_burst(Duration::from_secs(5));
        let burst = match result {
            ReadResult::Burst(b) | ReadResult::BurstThenExit(b) => b,
            other => panic!("Expected Burst, got {:?}", other),
        };
        assert_eq!(burst.rows.len(), 2);
        assert_eq!(burst.rows[0].id.as_deref(), Some("lock"));

        // Send a Select for row 0 ("lock") and confirm the script received it.
        let event = crate::core::rofi_protocol::RofiEvent::Select {
            key: "enter".to_string(),
            index: 0,
            id: "lock".to_string(),
            text: "Lock".to_string(),
            retv: 1,
            data: None,
            selected_ids: vec!["lock".to_string()],
            selected_texts: vec!["Lock".to_string()],
        };
        session.send_event(&event).expect("send_event failed");
        // Give the script a moment to read + write the result file.
        std::thread::sleep(Duration::from_millis(300));

        let got = std::fs::read_to_string(&result_file).unwrap_or_default();
        assert_eq!(
            got.trim(),
            "lock",
            "script did not receive/parse the selection event"
        );
        session.kill();
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&result_file);
    }

    #[test]
    fn session_kill_takes_down_grandchildren() {
        let _lock = crate::core::executor::SCRIPT_PROCESS_LOCK.lock().unwrap();
        // A script that spawns a long-lived grandchild and reports its
        // pid, then blocks. Killing the session must take the grandchild
        // down too (process-group kill), not orphan it.
        let dir = std::env::temp_dir();
        let script = dir.join(format!("aerofi_grandchild_test_{}.sh", std::process::id()));
        std::fs::write(&script, "#!/bin/bash\nsleep 3601 &\necho $!\nsleep 3600\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let mut session =
            RofiSession::spawn(&script, vec![], std::collections::HashMap::new()).unwrap();
        let result = session.read_burst(Duration::from_secs(5));
        let burst = match result {
            ReadResult::Burst(b) => b,
            other => panic!("Expected Burst, got {:?}", other),
        };
        let pid: i32 = burst.rows[0].text.trim().parse().unwrap();
        let session_pid = session.child_pid();
        assert!(
            crate::core::executor::script_is_registered(session_pid),
            "session not in the running-scripts registry"
        );
        session.kill();
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !crate::core::executor::script_is_registered(session_pid),
            "session still in the running-scripts registry after kill"
        );

        let pid_str = pid.to_string();
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid_str])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if alive {
            // Don't leave a test orphan behind when we fail.
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid_str])
                .status();
            panic!("grandchild {pid} survived the session kill");
        }
        let _ = std::fs::remove_file(&script);
    }
}
