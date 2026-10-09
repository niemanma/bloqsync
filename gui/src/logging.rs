//! File+stderr logging, so autostart runs can be diagnosed after a reboot.

/// Append a line to `~/.cache/bloqsync/bloqsync.log` (and stderr).
pub(crate) fn log(msg: &str) {
    if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::Path::new(&home).join(".cache/bloqsync");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("bloqsync.log"))
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