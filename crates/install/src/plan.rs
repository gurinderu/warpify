//! What an install or uninstall will do, decided without touching anything.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::edit_install;
use crate::{Error, Paths, Result};

const REPO: &str = "https://github.com/gurinderu/warpify";
const NIX_STORE: &str = "/nix/store";

/// URL of a release asset for the CLI's own version.
#[must_use]
pub fn release_url(version: &str, file: &str) -> String {
    format!("{REPO}/releases/download/v{version}/{file}")
}

/// Where the plugin comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The release matching this CLI version.
    Release { version: String },
    /// A local build (`--wasm`).
    Local(PathBuf),
}

/// Whether warpify may edit the zellij config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAccess {
    Editable,
    /// Managed elsewhere (nix store, read-only): the user does it by hand.
    Manual(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    FetchWasm {
        url: String,
        sha_url: String,
        dest: PathBuf,
    },
    CopyWasm {
        from: PathBuf,
        dest: PathBuf,
    },
    MergePermissions {
        file: PathBuf,
        wasm: PathBuf,
    },
    AddLoadPlugin {
        file: PathBuf,
        entry: String,
    },
    RemoveWasm {
        dest: PathBuf,
    },
    RemovePermissions {
        file: PathBuf,
        wasm: PathBuf,
    },
    RemoveLoadPlugin {
        file: PathBuf,
        entry: String,
    },
    /// The config can't be edited here; `adding` says which way the snippet goes.
    Manual {
        snippet: String,
        why: String,
        adding: bool,
    },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FetchWasm { url, dest, .. } => {
                write!(f, "download {url} (sha256-verified) to {}", dest.display())
            }
            Self::CopyWasm { from, dest } => {
                write!(f, "copy {} to {}", from.display(), dest.display())
            }
            Self::MergePermissions { file, .. } => {
                write!(f, "grant the plugin its permissions in {}", file.display())
            }
            Self::AddLoadPlugin { file, entry } => {
                write!(f, "add \"{entry}\" to load_plugins in {}", file.display())
            }
            Self::RemoveWasm { dest } => write!(f, "remove {}", dest.display()),
            Self::RemovePermissions { file, .. } => {
                write!(f, "drop the plugin's permissions from {}", file.display())
            }
            Self::RemoveLoadPlugin { file, entry } => {
                write!(
                    f,
                    "remove \"{entry}\" from load_plugins in {}",
                    file.display()
                )
            }
            Self::Manual { adding, .. } => {
                let verb = if *adding { "add" } else { "remove" };
                write!(
                    f,
                    "(manual) {verb} the load_plugins entry in your zellij config"
                )
            }
        }
    }
}

/// An ordered list of actions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub actions: Vec<Action>,
}

/// `file:<abs path>` — how `load_plugins` names a local plugin.
///
/// # Errors
/// When the path is not valid UTF-8.
pub fn load_entry(wasm: &Path) -> Result<String> {
    wasm.to_str()
        .map(|p| format!("file:{p}"))
        .ok_or_else(|| Error::new("the plugin path is not valid UTF-8"))
}

/// The block to paste by hand: what `edit_install` would add to a config without one, so the
/// zellij defaults stay (multi-line: zellij's KDL v1 wants a newline before `}`).
fn snippet(entry: &str) -> Result<String> {
    Ok(edit_install("", entry)?
        .unwrap_or_default()
        .trim_end()
        .to_owned())
}

fn config_action(paths: &Paths, access: &ConfigAccess, adding: bool) -> Result<Action> {
    let entry = load_entry(&paths.wasm)?;
    Ok(match access {
        ConfigAccess::Editable if adding => Action::AddLoadPlugin {
            file: paths.config.clone(),
            entry,
        },
        ConfigAccess::Editable => Action::RemoveLoadPlugin {
            file: paths.config.clone(),
            entry,
        },
        ConfigAccess::Manual(why) => Action::Manual {
            snippet: snippet(&entry)?,
            why: why.clone(),
            adding,
        },
    })
}

/// # Errors
/// When the plugin path is not valid UTF-8.
pub fn plan_install(paths: &Paths, source: &Source, access: &ConfigAccess) -> Result<Plan> {
    let wasm = match source {
        Source::Release { version } => Action::FetchWasm {
            url: release_url(version, "warpify.wasm"),
            sha_url: release_url(version, "warpify.wasm.sha256"),
            dest: paths.wasm.clone(),
        },
        Source::Local(from) => Action::CopyWasm {
            from: from.clone(),
            dest: paths.wasm.clone(),
        },
    };
    Ok(Plan {
        actions: vec![
            wasm,
            Action::MergePermissions {
                file: paths.permissions.clone(),
                wasm: paths.wasm.clone(),
            },
            config_action(paths, access, true)?,
        ],
    })
}

/// # Errors
/// When the plugin path is not valid UTF-8.
pub fn plan_uninstall(paths: &Paths, access: &ConfigAccess) -> Result<Plan> {
    Ok(Plan {
        actions: vec![
            config_action(paths, access, false)?,
            Action::RemovePermissions {
                file: paths.permissions.clone(),
                wasm: paths.wasm.clone(),
            },
            Action::RemoveWasm {
                dest: paths.wasm.clone(),
            },
        ],
    })
}

/// Looks at the config file and decides whether we may edit it: not a symlink into the nix
/// store, not read-only, and (when missing) its nearest existing directory is writable.
#[must_use]
pub fn probe_config(config: &Path) -> ConfigAccess {
    let manual = |why: &str| ConfigAccess::Manual(why.to_owned());
    if let Ok(real) = std::fs::canonicalize(config) {
        let linked = std::fs::symlink_metadata(config).is_ok_and(|m| m.file_type().is_symlink());
        if linked && real.starts_with(NIX_STORE) {
            return manual("it is a symlink into /nix/store");
        }
        return match std::fs::metadata(&real) {
            Ok(m) if m.permissions().readonly() => manual("it is read-only"),
            _ => ConfigAccess::Editable,
        };
    }
    let dir = config
        .parent()
        .and_then(|d| d.ancestors().find(|a| a.exists()));
    match dir.map(std::fs::metadata) {
        Some(Ok(m)) if !m.permissions().readonly() => ConfigAccess::Editable,
        _ => manual("its directory is not writable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths {
            wasm: "/d/warpify/warpify.wasm".into(),
            config: "/c/config.kdl".into(),
            permissions: "/k/permissions.kdl".into(),
        }
    }

    #[test]
    fn release_urls_follow_the_version() {
        assert_eq!(
            release_url("0.1.0", "warpify.wasm.sha256"),
            "https://github.com/gurinderu/warpify/releases/download/v0.1.0/warpify.wasm.sha256"
        );
    }

    #[test]
    fn install_plan_downloads_or_copies_then_grants_then_edits() {
        let rel = Source::Release {
            version: "0.1.0".into(),
        };
        let plan = plan_install(&paths(), &rel, &ConfigAccess::Editable).unwrap();
        assert!(matches!(plan.actions[0], Action::FetchWasm { .. }));
        assert!(matches!(plan.actions[1], Action::MergePermissions { .. }));
        assert!(matches!(plan.actions[2], Action::AddLoadPlugin { .. }));
        let local = Source::Local("/w.wasm".into());
        let plan = plan_install(&paths(), &local, &ConfigAccess::Editable).unwrap();
        assert!(matches!(plan.actions[0], Action::CopyWasm { .. }));
    }

    #[test]
    fn manual_access_yields_the_exact_snippet() {
        let plan = plan_install(
            &paths(),
            &Source::Local("/w.wasm".into()),
            &ConfigAccess::Manual("nix".into()),
        )
        .unwrap();
        let Action::Manual {
            snippet, adding, ..
        } = &plan.actions[2]
        else {
            panic!("not manual")
        };
        assert!(adding);
        assert_eq!(
            snippet,
            "load_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify.wasm\"\n}"
        );
    }

    #[test]
    fn uninstall_plan_reverses() {
        let plan = plan_uninstall(&paths(), &ConfigAccess::Editable).unwrap();
        assert!(matches!(plan.actions[0], Action::RemoveLoadPlugin { .. }));
        assert!(matches!(plan.actions[2], Action::RemoveWasm { .. }));
    }

    #[test]
    fn symlink_into_the_nix_store_is_manual() {
        let Some(target) = std::fs::read_dir("/nix/store")
            .ok()
            .and_then(|mut d| d.find_map(|e| e.ok().filter(|e| e.path().is_file())))
        else {
            return; // no nix store on this machine
        };
        let t = tempfile::tempdir().unwrap();
        let link = t.path().join("config.kdl");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();
        let ConfigAccess::Manual(why) = probe_config(&link) else {
            panic!("editable")
        };
        assert!(why.contains("/nix/store"), "{why}");
    }

    #[test]
    fn probe_sees_missing_writable_readonly_and_unwritable_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_config(&t.path().join("new/config.kdl")),
            ConfigAccess::Editable
        );
        let f = t.path().join("config.kdl");
        std::fs::write(&f, "").unwrap();
        assert_eq!(probe_config(&f), ConfigAccess::Editable);
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(matches!(probe_config(&f), ConfigAccess::Manual(_)));
        let ro = t.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(matches!(
            probe_config(&ro.join("config.kdl")),
            ConfigAccess::Manual(_)
        ));
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
