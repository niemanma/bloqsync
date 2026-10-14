//! File+stderr logging, so autostart runs can be diagnosed after a reboot.

/// Rotate the log once it grows past this size, so it cannot fill the disk.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Append a line to `~/.cache/bloqsync/bloqsync.log` (and stderr).
pub(crate) fn log(msg: &str) {
    if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::Path::new(&home).join(".cache/bloqsync");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bloqsync.log");
        // Simple single-backup rotation.
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > MAX_LOG_BYTES {
                let _ = std::fs::rename(&path, dir.join("bloqsync.log.1"));
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{secs}] {msg}");
        }
    }
    eprintln!("{msg}");
}