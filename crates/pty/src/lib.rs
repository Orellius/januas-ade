//! Januas PTY — spawn a shell, read its bytes on a background thread.
//!
//! Purpose: hide `portable-pty` behind a small surface that gives the app a
//! resizable PTY pair and a `Receiver<Vec<u8>>` for shell output. Writes go
//! through `PtyWriter::write` which forwards bytes back into the shell.
//! Why this file (vs inlining in app): isolating the PTY lifecycle keeps the
//! main event loop free of blocking IO and threading concerns.
//! Not responsibilities: VT parsing (terminal crate), input semantics (app).
//! Test strategy: integration tests spawn `/bin/sh -c "echo hi"` and assert
//! "hi" comes back on the channel.

use std::io::{Read as _, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context as _, Result};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};

/// A spawned shell behind a PTY. Holds the master side, the writer, and the
/// receiver end of the byte-output channel.
pub struct PtyShell {
    /// Inbound shell bytes. The reader thread pushes here; consume each frame.
    pub bytes: Receiver<Vec<u8>>,
    /// Writes go through this; cloning is cheap (Arc<Mutex>).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Master PTY kept alive so resize works and the slave side stays attached.
    master: Box<dyn MasterPty + Send>,
}

impl PtyShell {
    /// Spawn the user's `$SHELL` (or `/bin/sh` fallback) inside a PTY of the given size.
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY pair can't be opened, the shell can't be spawned,
    /// or the master reader can't be cloned.
    pub fn spawn(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let cmd = CommandBuilder::new(shell);
        let _child = pair
            .slave
            .spawn_command(cmd)
            .context("spawn_command failed")?;
        // Drop the slave so the shell owns it solely; reading the master EOFs when shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("try_clone_reader failed")?;
        let writer = pair.master.take_writer().context("take_writer failed")?;

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
        thread::Builder::new()
            .name("januas-pty-reader".into())
            .spawn(move || {
                let mut buf = [0_u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .context("spawn pty reader thread failed")?;

        Ok(Self {
            bytes: rx,
            writer: Arc::new(Mutex::new(writer)),
            master: pair.master,
        })
    }

    /// Send bytes into the shell's stdin.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer lock is poisoned or the underlying write fails.
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("pty writer lock poisoned: {e}"))?;
        w.write_all(data).context("pty write failed")?;
        w.flush().context("pty flush failed")?;
        drop(w);
        Ok(())
    }

    /// Inform the shell about a new terminal size.
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY ioctl fails.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("pty resize failed")?;
        Ok(())
    }
}
