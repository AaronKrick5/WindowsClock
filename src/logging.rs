//! A small log file to help find the cause of problems.
//! The file has a size limit. When the limit is reached at start, the old
//! file is kept as one backup and a new file is started.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

const MAX_BYTES: u64 = 512 * 1024;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set the log file. Call one time at start.
pub fn init(path: PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    let _ = LOG_PATH.set(path);
}

/// Write one line. Errors are ignored: a log problem must not stop the clock.
pub fn log(timestamp: &str, text: &str) {
    let Some(path) = LOG_PATH.get() else { return };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{timestamp} {text}");
    }
}
