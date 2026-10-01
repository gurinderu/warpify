//! Installing the warpify plugin into zellij: where things go, what to do (a pure `Plan`), and
//! doing it. All file edits are atomic and format-preserving; nothing outside the paths handed
//! in is touched (graph @nick/warpify, node #17).

mod checksum;
mod config;
mod exec;
mod fsutil;
mod kdl_edit;
mod paths;
mod permissions;
mod plan;

pub use checksum::{parse_sha256_line, verify_sha256};
pub use config::{edit_install, edit_uninstall};
pub use exec::{entry_for, execute, Fetcher, UreqFetcher};
pub use paths::{EnvVars, Paths};
pub use permissions::{merge_permissions, remove_permissions};
pub use plan::{
    plan_install, plan_uninstall, probe_config, release_url, Action, ConfigAccess, Plan, Source,
};

use std::fmt;

/// What went wrong, as one sentence for the user.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;
