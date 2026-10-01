//! Diagnostics setup shared by the plugin and the CLI: `tracing` events go to stderr.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

/// Installs a subscriber writing to stderr, without timestamps (zellij's log and the terminal add
/// their own context). `default_filter` is an `EnvFilter` directive list used when `RUST_LOG` is
/// unset or invalid. Does nothing if a subscriber is already installed. No background threads, so
/// it works on `wasm32-wasip1`.
pub fn init(default_filter: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true)
        .without_time()
        .try_init();
}

/// Like [`init`], but appends to the file at `path` (its directory is created), without ANSI and
/// with timestamps: the file has no other context. For processes with no terminal.
///
/// # Errors
/// Creating the directory or opening the file failed; no subscriber is installed then.
pub fn init_file(default_filter: &str, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_target(true)
        .try_init();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_file_creates_the_directory_and_opens_for_append() {
        let dir = std::env::temp_dir().join(format!("warpify-telemetry-{}", std::process::id()));
        let path = dir.join("a/b/log");
        init_file("info", &path).unwrap();
        init_file("info", &path).unwrap();
        assert!(path.is_file());
        fs::remove_dir_all(dir).unwrap();
    }
}
