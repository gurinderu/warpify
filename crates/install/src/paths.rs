use std::path::PathBuf;

use crate::{Error, Result};

/// The environment variables path resolution reads; passed in so it stays pure.
#[derive(Debug, Default, Clone)]
pub struct EnvVars {
    pub home: Option<String>,
    pub xdg_data_home: Option<String>,
    pub xdg_config_home: Option<String>,
    pub xdg_cache_home: Option<String>,
    pub zellij_config_file: Option<String>,
    pub zellij_config_dir: Option<String>,
}

impl EnvVars {
    #[must_use]
    pub fn from_process() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self {
            home: get("HOME"),
            xdg_data_home: get("XDG_DATA_HOME"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
            xdg_cache_home: get("XDG_CACHE_HOME"),
            zellij_config_file: get("ZELLIJ_CONFIG_FILE"),
            zellij_config_dir: get("ZELLIJ_CONFIG_DIR"),
        }
    }
}

/// An empty variable counts as unset (XDG base-dir rule).
fn set(v: Option<&String>) -> Option<&str> {
    v.map(String::as_str).filter(|s| !s.is_empty())
}

/// Where the plugin, zellij's config and zellij's permission cache live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub wasm: PathBuf,
    pub config: PathBuf,
    pub permissions: PathBuf,
}

impl Paths {
    /// # Errors
    /// When a location needs `HOME` and it is not set.
    pub fn resolve(env: &EnvVars) -> Result<Self> {
        let home = |rest: &str| {
            set(env.home.as_ref())
                .map(|h| PathBuf::from(h).join(rest))
                .ok_or_else(|| Error::new("HOME is not set"))
        };
        let wasm = match set(env.xdg_data_home.as_ref()) {
            Some(d) => PathBuf::from(d).join("warpify/warpify.wasm"),
            None => home(".local/share/warpify/warpify.wasm")?,
        };
        let config = if let Some(f) = set(env.zellij_config_file.as_ref()) {
            PathBuf::from(f)
        } else if let Some(d) = set(env.zellij_config_dir.as_ref()) {
            PathBuf::from(d).join("config.kdl")
        } else if let Some(d) = set(env.xdg_config_home.as_ref()) {
            PathBuf::from(d).join("zellij/config.kdl")
        } else {
            home(".config/zellij/config.kdl")?
        };
        let permissions = match set(env.xdg_cache_home.as_ref()) {
            Some(d) => PathBuf::from(d).join("zellij/permissions.kdl"),
            None => home(".cache/zellij/permissions.kdl")?,
        };
        Ok(Self {
            wasm,
            config,
            permissions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(f: impl FnOnce(&mut EnvVars)) -> EnvVars {
        let mut e = EnvVars {
            home: Some("/h".into()),
            ..EnvVars::default()
        };
        f(&mut e);
        e
    }

    #[test]
    fn defaults_come_from_home() {
        let p = Paths::resolve(&env(|_| {})).unwrap();
        assert_eq!(
            p.wasm,
            PathBuf::from("/h/.local/share/warpify/warpify.wasm")
        );
        assert_eq!(p.config, PathBuf::from("/h/.config/zellij/config.kdl"));
        assert_eq!(
            p.permissions,
            PathBuf::from("/h/.cache/zellij/permissions.kdl")
        );
    }

    #[test]
    fn xdg_dirs_win_over_home_and_empty_counts_as_unset() {
        let p = Paths::resolve(&env(|e| {
            e.xdg_data_home = Some("/d".into());
            e.xdg_config_home = Some("/c".into());
            e.xdg_cache_home = Some(String::new());
        }))
        .unwrap();
        assert_eq!(p.wasm, PathBuf::from("/d/warpify/warpify.wasm"));
        assert_eq!(p.config, PathBuf::from("/c/zellij/config.kdl"));
        assert_eq!(
            p.permissions,
            PathBuf::from("/h/.cache/zellij/permissions.kdl")
        );
    }

    #[test]
    fn zellij_config_file_beats_dir_beats_xdg() {
        let p = Paths::resolve(&env(|e| {
            e.zellij_config_dir = Some("/zd".into());
            e.xdg_config_home = Some("/c".into());
        }))
        .unwrap();
        assert_eq!(p.config, PathBuf::from("/zd/config.kdl"));
        let p = Paths::resolve(&env(|e| {
            e.zellij_config_file = Some("/f/my.kdl".into());
            e.zellij_config_dir = Some("/zd".into());
        }))
        .unwrap();
        assert_eq!(p.config, PathBuf::from("/f/my.kdl"));
    }

    #[test]
    fn missing_home_is_an_error_only_when_needed() {
        let only = EnvVars {
            xdg_data_home: Some("/d".into()),
            xdg_cache_home: Some("/k".into()),
            zellij_config_file: Some("/f".into()),
            ..EnvVars::default()
        };
        assert!(Paths::resolve(&only).is_ok());
        assert!(Paths::resolve(&EnvVars::default()).is_err());
    }
}
