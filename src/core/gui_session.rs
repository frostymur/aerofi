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

impl GuiSession {
    /// Spawn a GUI-mode script and return the session handle.
    ///
    /// The script is started with `stdin = piped`, `stdout = piped`,
    /// `stderr = piped` (stderr is discarded for now — a future version
    /// may route it to the error toast).
    pub fn spawn(path: &Path, args: Vec<String>) -> std::io::Result<Self> {
        let mut cmd = crate::core::executor::script_command(path);
        cmd.args(args);
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

        Ok(Self {
            child,
            stdin,
            line_rx: rx,
        })
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
    /// The initial call should use a longer timeout (e.g. 5 s) to give
    /// the script time to start; subsequent calls (after writing a
    /// selection) can use a shorter timeout (e.g. 500 ms).
    pub fn read_burst(&self, timeout: Duration) -> ReadResult {
        let mut lines = Vec::new();

        // First line: block for up to `timeout`.
        match self.line_rx.recv_timeout(timeout) {
            Ok(LineEvent::Line(line)) => lines.push(line),
            Ok(LineEvent::Eof) => return ReadResult::Exited,
            Ok(LineEvent::Error(e)) => return ReadResult::Error(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return ReadResult::Burst(GuiBurst::default());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return ReadResult::Exited,
        }

        // Subsequent lines: use a short inter-line timeout to detect the
        // end of the burst.
        let inter_line = Duration::from_millis(50);
        loop {
            match self.line_rx.recv_timeout(inter_line) {
                Ok(LineEvent::Line(line)) => lines.push(line),
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

    /// Write the selected row's display text to the script's stdin.
    pub fn send_selection(&mut self, text: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "{text}")?;
        self.stdin.flush()
    }

    /// Kill the child process (SIGTERM).
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Check whether the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the child to exit and return the exit code.
    pub fn wait_exit_code(&mut self) -> Option<i32> {
        self.child.wait().ok().and_then(|s| s.code())
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
        let rows = vec![
            GuiRow {
                text: "A".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
            GuiRow {
                text: "B".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
        ];
        assert_eq!(filter_gui_rows(&rows, ""), vec![0, 1]);
    }

    #[test]
    fn filter_gui_rows_by_text() {
        let rows = vec![
            GuiRow {
                text: "Firefox".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
            GuiRow {
                text: "Chrome".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
            GuiRow {
                text: "Safari".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
        ];
        assert_eq!(filter_gui_rows(&rows, "fire"), vec![0]);
        assert_eq!(filter_gui_rows(&rows, "chr"), vec![1]);
    }

    #[test]
    fn filter_gui_rows_by_meta() {
        let rows = vec![
            GuiRow {
                text: "Item A".into(),
                icon: None,
                info: None,
                meta: Some("secret keyword".into()),
                nonselectable: false,
            },
            GuiRow {
                text: "Item B".into(),
                icon: None,
                info: None,
                meta: None,
                nonselectable: false,
            },
        ];
        assert_eq!(filter_gui_rows(&rows, "secret"), vec![0]);
    }

    #[test]
    fn filter_gui_rows_case_insensitive() {
        let rows = vec![GuiRow {
            text: "Hello World".into(),
            icon: None,
            info: None,
            meta: None,
            nonselectable: false,
        }];
        assert_eq!(filter_gui_rows(&rows, "HELLO"), vec![0]);
        assert_eq!(filter_gui_rows(&rows, "hello"), vec![0]);
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

        let session = GuiSession::spawn(&script, vec![]).unwrap();
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
}
