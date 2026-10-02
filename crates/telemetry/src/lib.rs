//! Diagnostics setup shared by the plugin and the CLI: `tracing` events go to stderr.

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
