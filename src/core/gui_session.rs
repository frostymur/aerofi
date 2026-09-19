//! Async I/O wrapper for a running GUI-mode script process.
//!
//! A `GuiSession` owns a child process whose stdout emits
//! [`GuiBurst`](crate::core::gui_protocol::GuiBurst) frames and whose
//! stdin receives selected row text.  The child stays alive across
//! multiple interaction rounds (wizard steps).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::core::gui_protocol::{GuiBurst, GuiRow};

/// A live GUI script session backed by a child process.
pub struct GuiSession {
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

/// Check if a stdout line represents an explicit frame flush command.
fn is_flush_line(line: &str) -> bool {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    trimmed == "\0flush" || trimmed.starts_with("\0flush\x1f")
}

impl GuiSession {
    /// Spawn a GUI-mode script and return the session handle.
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

        let (tx, rx) = mpsc::channel();
        // Background thread reads stdout line by line and sends events.
        std::thread::Builder::new()
            .name("aerofi-gui-stdout".to_string())
            .spawn(move || {
                Self::stdout_reader_thread(stdout, tx);
            })
            .expect("failed to spawn stdout reader thread");

        let session = Self {
            child,
            stdin,
            line_rx: rx,
        };
        // GUI sessions are killed on hide, but a quit that skips the
        // window still needs the script tree cleaned up.
        crate::core::executor::register_script(session.child.id());
        Ok(session)
    }

    /// The background thread that reads stdout lines and sends them
    /// to the main thread via the channel.
    fn stdout_reader_thread(stdout: ChildStdout, tx: mpsc::Sender<LineEvent>) {
        let reader = BufReader::new(stdout);
        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
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

        // First line: block for up to `timeout`.
        match self.line_rx.recv_timeout(timeout) {
            Ok(LineEvent::Line(line)) => {
                let is_flush = is_flush_line(&line);
                lines.push(line);
                if is_flush {
                    return ReadResult::Burst(GuiBurst::from_lines(&lines));
                }
            }
            Ok(LineEvent::Eof) => return ReadResult::Exited,
            Ok(LineEvent::Error(e)) => return ReadResult::Error(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return ReadResult::Burst(GuiBurst::default());
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
                    lines.push(line);
                    if is_flush {
                        break;
                    }
                }
                Ok(LineEvent::Eof) => {
                    // Process the lines we have, then signal exit.
                    let burst = GuiBurst::from_lines(&lines);
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

        ReadResult::Burst(GuiBurst::from_lines(&lines))
    }

    /// Write a structured GUI event to the script's stdin.
    pub fn send_event(
        &mut self,
        event: &crate::core::gui_protocol::GuiEvent,
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
        let pid = self.child.id();
        unsafe { libc::kill(-(pid as libc::c_int), libc::SIGKILL) };
        let _ = self.child.wait();
        crate::core::executor::unregister_script(pid);
    }
}

impl Drop for GuiSession {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Result of reading a burst from the session.
#[derive(Debug)]
pub enum ReadResult {
    /// A burst of commands and rows was received.
    Burst(GuiBurst),
    /// A burst was received but EOF followed immediately.
    BurstThenExit(GuiBurst),
    /// The script exited (EOF on stdout, no data).
    Exited,
    /// An I/O error occurred.
    Error(String),
}

/// Convenience: filter GUI rows by fuzzy matching on `text` and `meta`.
pub fn filter_gui_rows(rows: &[GuiRow], query: &str) -> Vec<usize> {
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
    fn filter_gui_rows_empty_query() {
        let rows = vec![GuiRow::new("A"), GuiRow::new("B")];
        assert_eq!(filter_gui_rows(&rows, ""), vec![0, 1]);
    }

    #[test]
    fn filter_gui_rows_by_text() {
        let rows = vec![
            GuiRow::new("Firefox"),
            GuiRow::new("Chrome"),
            GuiRow::new("Safari"),
        ];
        assert_eq!(filter_gui_rows(&rows, "fire"), vec![0]);
        assert_eq!(filter_gui_rows(&rows, "chr"), vec![1]);
    }

    #[test]
    fn filter_gui_rows_by_meta() {
        let mut row_a = GuiRow::new("Item A");
        row_a.meta = Some("secret keyword".into());
        let rows = vec![row_a, GuiRow::new("Item B")];
        assert_eq!(filter_gui_rows(&rows, "secret"), vec![0]);
    }

    #[test]
    fn filter_gui_rows_case_insensitive() {
        let rows = vec![GuiRow::new("Hello World")];
        assert_eq!(filter_gui_rows(&rows, "HELLO"), vec![0]);
        assert_eq!(filter_gui_rows(&rows, "hello"), vec![0]);
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
        // Spawn a simple echo script that outputs protocol lines and exits.
        let dir = std::env::temp_dir();
        let script = dir.join(format!("aerofi_gui_test_{}.sh", std::process::id()));
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

        let session = GuiSession::spawn(&script, vec![], std::collections::HashMap::new()).unwrap();
        let result = session.read_burst(Duration::from_secs(5));

        let burst = match result {
            ReadResult::Burst(b) | ReadResult::BurstThenExit(b) => b,
            other => panic!("Expected Burst, got {:?}", other),
        };

        assert_eq!(burst.commands.len(), 1);
        assert_eq!(
            burst.commands[0],
            crate::core::gui_protocol::GuiCommand::SetPrompt("Test Prompt".to_string())
        );
        assert_eq!(burst.rows.len(), 2);
        assert_eq!(burst.rows[0].text, "Item One");
        assert_eq!(burst.rows[0].icon, Some("🔥".to_string()));
        assert_eq!(burst.rows[1].text, "Item Two");
        assert_eq!(burst.rows[1].info, Some("New".to_string()));

        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn session_kill_takes_down_grandchildren() {
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
            GuiSession::spawn(&script, vec![], std::collections::HashMap::new()).unwrap();
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
