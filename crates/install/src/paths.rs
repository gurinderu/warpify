use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

use crate::plan::{LEGACY_PLUGIN_FILE, PLUGIN_FILE};
use crate::{Error, Result};

/// The environment variables zellij itself reads for its config; passed in so resolution stays
/// pure. An empty variable counts as unset.
#[derive(Debug, Default, Clone)]
pub struct EnvVars {
    pub zellij_config_file: Option<String>,
    pub zellij_config_dir: Option<String>,
}

impl EnvVars {
    #[must_use]
    pub fn from_process() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self {
            zellij_config_file: get("ZELLIJ_CONFIG_FILE"),
            zellij_config_dir: get("ZELLIJ_CONFIG_DIR"),
        }
    }
}

fn set(v: Option<&String>) -> Option<&str> {
    v.map(String::as_str).filter(|s| !s.is_empty())
}

/// zellij's system-wide config dir on unix (`SYSTEM_DEFAULT_CONFIG_DIR` in zellij-utils).
const SYSTEM_CONFIG_DIR: &str = "/etc/zellij";

/// The base directories, already resolved: `from_system` asks the same `directories` calls
/// zellij-utils 0.45.1 makes (`consts.rs` `ZELLIJ_PROJ_DIR`, `home_unix.rs`), tests build one by
/// hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// zellij's config dirs in its own search order (`home.rs` `default_config_dirs`).
    pub config_candidates: Vec<PathBuf>,
    /// `ProjectDirs::from("org", "Zellij Contributors", "Zellij")` cache dir.
    pub zellij_cache: PathBuf,
    /// Where warpify keeps its own data: the user data dir plus `warpify`.
    pub warpify_data: PathBuf,
}

impl Dirs {
    /// # Errors
    /// When the home directory can't be determined.
    pub fn from_system() -> Result<Self> {
        let base = BaseDirs::new().ok_or_else(|| Error::new("HOME is not set"))?;
        let zellij = ProjectDirs::from("org", "Zellij Contributors", "Zellij")
            .ok_or_else(|| Error::new("HOME is not set"))?;
        Ok(Self {
            config_candidates: vec![
                base.home_dir().join(".config/zellij"),
                zellij.config_dir().to_path_buf(),
                PathBuf::from(SYSTEM_CONFIG_DIR),
            ],
            zellij_cache: zellij.cache_dir().to_path_buf(),
            warpify_data: base.data_dir().join("warpify"),
        })
    }

    /// The config dir zellij picks: the first candidate that exists, else the first one (the one
    /// zellij creates, `try_create_home_config_dir`).
    fn config_dir(&self) -> Option<&Path> {
        self.config_candidates
            .iter()
            .find(|d| d.exists())
            .or_else(|| self.config_candidates.first())
            .map(PathBuf::as_path)
    }
}

/// Where the plugin, zellij's config and zellij's permission cache live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub wasm: PathBuf,
    /// Where 0.1.0 put the plugin (`LEGACY_PLUGIN_FILE`); cleaned up on install and uninstall.
    pub legacy_wasm: PathBuf,
    pub config: PathBuf,
    pub permissions: PathBuf,
}

impl Paths {
    /// Config file order, as in zellij: `ZELLIJ_CONFIG_FILE`, `ZELLIJ_CONFIG_DIR/config.kdl`,
    /// then the default config dirs.
    ///
    /// # Errors
    /// When there is no config location at all.
    pub fn resolve(env: &EnvVars, dirs: &Dirs) -> Result<Self> {
        let config = if let Some(f) = set(env.zellij_config_file.as_ref()) {
            PathBuf::from(f)
        } else if let Some(d) = set(env.zellij_config_dir.as_ref()) {
            PathBuf::from(d).join("config.kdl")
        } else {
            dirs.config_dir()
                .ok_or_else(|| Error::new("no zellij config directory"))?
                .join("config.kdl")
        };
        Ok(Self {
            wasm: dirs.warpify_data.join(PLUGIN_FILE),
            legacy_wasm: dirs.warpify_data.join(LEGACY_PLUGIN_FILE),
            config,
            permissions: dirs.zellij_cache.join("permissions.kdl"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(t: &Path) -> Dirs {
        Dirs {
            config_candidates: vec![t.join("home/.config/zellij"), t.join("proj"), t.join("etc")],
            zellij_cache: t.join("cache/zellij"),
            warpify_data: t.join("data/warpify"),
        }
    }

    #[test]
    fn none_existing_picks_the_dir_zellij_creates() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::resolve(&EnvVars::default(), &dirs(t.path())).unwrap();
        assert_eq!(p.config, t.path().join("home/.config/zellij/config.kdl"));
        assert_eq!(p.wasm, t.path().join("data/warpify/warpify-zellij.wasm"));
        assert_eq!(p.legacy_wasm, t.path().join("data/warpify/warpify.wasm"));
        assert_eq!(p.permissions, t.path().join("cache/zellij/permissions.kdl"));
    }

    #[test]
    fn first_existing_dir_wins_in_zellijs_order() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(t.path());
        std::fs::create_dir_all(t.path().join("etc")).unwrap();
        let p = Paths::resolve(&EnvVars::default(), &d).unwrap();
        assert_eq!(p.config, t.path().join("etc/config.kdl"));
        std::fs::create_dir_all(t.path().join("proj")).unwrap();
        let p = Paths::resolve(&EnvVars::default(), &d).unwrap();
        assert_eq!(p.config, t.path().join("proj/config.kdl"));
        std::fs::create_dir_all(t.path().join("home/.config/zellij")).unwrap();
        let p = Paths::resolve(&EnvVars::default(), &d).unwrap();
        assert_eq!(p.config, t.path().join("home/.config/zellij/config.kdl"));
    }

    #[test]
    fn zellij_config_file_beats_dir_beats_defaults_and_empty_is_unset() {
        let t = tempfile::tempdir().unwrap();
        let d = dirs(t.path());
        std::fs::create_dir_all(t.path().join("proj")).unwrap();
        let mut e = EnvVars {
            zellij_config_dir: Some("/zd".into()),
            ..EnvVars::default()
        };
        assert_eq!(
            Paths::resolve(&e, &d).unwrap().config,
            PathBuf::from("/zd/config.kdl")
        );
        e.zellij_config_file = Some("/f/my.kdl".into());
        assert_eq!(
            Paths::resolve(&e, &d).unwrap().config,
            PathBuf::from("/f/my.kdl")
        );
        e.zellij_config_file = Some(String::new());
        e.zellij_config_dir = Some(String::new());
        assert_eq!(
            Paths::resolve(&e, &d).unwrap().config,
            t.path().join("proj/config.kdl")
        );
    }

    #[test]
    fn system_dirs_use_zellijs_project_dirs_call() {
        // Needs HOME; skipped without it (e.g. a bare sandbox).
        let Ok(d) = Dirs::from_system() else { return };
        assert!(d.config_candidates[0].ends_with(".config/zellij"));
        assert_eq!(d.config_candidates.len(), 3);
        assert!(d.warpify_data.ends_with("warpify"));
        assert!(d
            .zellij_cache
            .to_string_lossy()
            .to_lowercase()
            .contains("zellij"));
    }
}
