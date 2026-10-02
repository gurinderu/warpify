//! Carrying a `Plan` out. Each action returns one line saying what happened.

use std::path::Path;

use crate::config::{edit_install, edit_uninstall, manual_advice, mark_created, only_ours_left};
use crate::fsutil::{backup_once, backup_path, read_optional, remove_if_exists, write_atomic};
use crate::permissions::{merge_permissions, remove_permissions};
use crate::plan::{load_entry, Action, Plan};
use crate::{verify_sha256, Error, Result};
use warpify_proto::PERMISSIONS;

/// Fetches a URL's body; behind a trait so tests need no network.
pub trait Fetcher {
    /// # Errors
    /// On any transport or HTTP failure.
    fn get(&self, url: &str) -> Result<Vec<u8>>;
}

/// The real thing: blocking HTTPS through `ureq` (rustls).
pub struct UreqFetcher;

const MAX_BYTES: u64 = 64 * 1024 * 1024;

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>> {
        let mut response = ureq::get(url)
            .call()
            .map_err(|e| Error::new(format!("can't download {url}: {e}")))?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .map_err(|e| Error::new(format!("can't read {url}: {e}")))
    }
}

/// Runs the actions in order, stopping at the first failure.
///
/// # Errors
/// On the first action that fails; earlier ones stay done.
pub fn execute(plan: &Plan, fetcher: &dyn Fetcher) -> Result<Vec<String>> {
    plan.actions.iter().map(|a| run(a, fetcher)).collect()
}

fn run(action: &Action, fetcher: &dyn Fetcher) -> Result<String> {
    tracing::debug!(%action, "running");
    match action {
        Action::FetchWasm { url, sha_url, dest } => {
            let sha = String::from_utf8(fetcher.get(sha_url)?)
                .map_err(|_| Error::new(format!("{sha_url} is not text")))?;
            let bytes = fetcher.get(url)?;
            verify_sha256(&bytes, &sha)?;
            write_atomic(dest, &bytes)?;
            Ok(format!(
                "downloaded and verified the plugin to {}",
                dest.display()
            ))
        }
        Action::CopyWasm { from, dest } => {
            let bytes = std::fs::read(from)
                .map_err(|e| Error::new(format!("can't read {}: {e}", from.display())))?;
            write_atomic(dest, &bytes)?;
            Ok(format!("copied the plugin to {}", dest.display()))
        }
        Action::MergePermissions { file, wasm } => edit(file, "permissions", |t| {
            merge_permissions(t, wasm, PERMISSIONS)
        }),
        Action::RemovePermissions { file, wasm } => remove_permissions_file(file, wasm),
        Action::AddLoadPlugin {
            file,
            entry,
            options,
        } => edit_config(file, true, |t| edit_install(t, entry, options)),
        Action::RemoveLoadPlugin { file, entry } => {
            edit_config(file, false, |t| edit_uninstall(t, entry))
        }
        Action::RemoveWasm { dest } => Ok(if remove_if_exists(dest)? {
            format!("removed {}", dest.display())
        } else {
            format!("{} was not there", dest.display())
        }),
        Action::Manual {
            file,
            entry,
            options,
            why,
            adding,
            seen,
        } => Ok(manual_advice(file, seen, entry, options, *adding, why)),
    }
}

/// Grants the plugin at `wasm` the permissions warpify needs in zellij's `permissions` file,
/// touching nothing else (for setups that place the plugin and config themselves, e.g. nix).
///
/// # Errors
/// When `wasm` is not an absolute UTF-8 path, or the file can't be read, parsed or written.
pub fn grant_permissions(permissions: &Path, wasm: &Path) -> Result<String> {
    if !wasm.is_absolute() {
        return Err(Error::new("the plugin path must be absolute"));
    }
    edit(permissions, "permissions", |t| {
        merge_permissions(t, wasm, PERMISSIONS)
    })
}

/// Reads `file` (missing counts as empty), applies `change`, writes when it says so.
fn edit(
    file: &Path,
    what: &str,
    change: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<String> {
    let Some(new) = change(&read_optional(file)?.unwrap_or_default())? else {
        return Ok(format!("{what} in {} already up to date", file.display()));
    };
    write_atomic(file, new.as_bytes())?;
    Ok(format!("updated {what} in {}", file.display()))
}

/// Drops our grants; deletes the file when that leaves it with nothing but whitespace.
fn remove_permissions_file(file: &Path, wasm: &Path) -> Result<String> {
    let Some(new) = remove_permissions(&read_optional(file)?.unwrap_or_default(), wasm)? else {
        return Ok(format!(
            "permissions in {} already up to date",
            file.display()
        ));
    };
    if new.trim().is_empty() {
        remove_if_exists(file)?;
        return Ok(format!(
            "removed {} (the plugin's grants were all it held)",
            file.display()
        ));
    }
    write_atomic(file, new.as_bytes())?;
    Ok(format!("updated permissions in {}", file.display()))
}

/// Like `edit`, for the zellij config. Install backs up an existing file once, and starts a file
/// it creates with a marker line. Uninstall never makes a backup and deletes the file only when
/// that marker is there and nothing else is left once our entry is gone; an existing
/// `.warpify.bak` is left alone and said so.
fn edit_config(
    file: &Path,
    installing: bool,
    change: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<String> {
    let current = read_optional(file)?;
    let Some(mut new) = change(current.as_deref().unwrap_or_default())? else {
        return Ok(format!("config {} already up to date", file.display()));
    };
    if installing {
        if current.is_none() {
            new = mark_created(&new);
        }
        let backup = backup_once(file)?;
        write_atomic(file, new.as_bytes())?;
        return Ok(match (backup, current) {
            (Some(b), _) => format!(
                "updated config {} (original saved as {})",
                file.display(),
                b.display()
            ),
            (None, None) => format!("created config {}", file.display()),
            (None, Some(_)) => format!("updated config {}", file.display()),
        });
    }
    let bak = backup_path(file);
    let kept = if bak.exists() {
        format!(
            "; the one-time backup {} stays, delete it if you don't need it",
            bak.display()
        )
    } else {
        String::new()
    };
    if only_ours_left(current.as_deref().unwrap_or_default(), &new) {
        remove_if_exists(file)?;
        return Ok(format!(
            "removed config {} (warpify created it and nothing else is left in it{kept})",
            file.display()
        ));
    }
    write_atomic(file, new.as_bytes())?;
    Ok(format!(
        "updated config {} (removed warpify's entry{kept})",
        file.display()
    ))
}

/// The entry a plan would put into `load_plugins`, for callers that print it.
///
/// # Errors
/// When the path is not valid UTF-8.
pub fn entry_for(wasm: &Path) -> Result<String> {
    load_entry(wasm)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::plan::{plan_install, plan_uninstall, ConfigAccess, PluginOptions, Seen, Source};
    use crate::Paths;

    struct Fake(HashMap<String, Vec<u8>>, RefCell<Vec<String>>);
    impl Fetcher for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>> {
            self.1.borrow_mut().push(url.to_owned());
            self.0.get(url).cloned().ok_or_else(|| Error::new("404"))
        }
    }

    fn sandbox() -> (tempfile::TempDir, Paths) {
        let t = tempfile::tempdir().unwrap();
        let p = Paths {
            wasm: t.path().join("data/warpify").join(crate::PLUGIN_FILE),
            config: t.path().join("cfg/config.kdl"),
            permissions: t.path().join("cache/zellij/permissions.kdl"),
        };
        (t, p)
    }

    fn fake(wasm: &[u8], sha: &str) -> Fake {
        let mut m = HashMap::new();
        m.insert(
            crate::release_url("1.2.3", crate::PLUGIN_FILE),
            wasm.to_vec(),
        );
        m.insert(
            crate::release_url("1.2.3", &format!("{}.sha256", crate::PLUGIN_FILE)),
            sha.as_bytes().to_vec(),
        );
        Fake(m, RefCell::default())
    }

    #[test]
    fn grant_writes_only_the_permissions_file() {
        let (t, p) = sandbox();
        let wasm = Path::new("/s/warpify/warpify-zellij.wasm");
        let line = grant_permissions(&p.permissions, wasm).unwrap();
        assert!(line.starts_with("updated permissions in "), "{line}");
        let text = std::fs::read_to_string(&p.permissions).unwrap();
        assert!(
            text.contains("\"/s/warpify/warpify-zellij.wasm\" {"),
            "{text}"
        );
        assert!(PERMISSIONS.iter().all(|perm| text.contains(perm)));
        assert!(grant_permissions(&p.permissions, wasm)
            .unwrap()
            .contains("already up to date"));
        assert!(!p.config.exists() && !p.wasm.exists());
        assert!(grant_permissions(&p.permissions, Path::new("rel.wasm")).is_err());
        std::fs::write(&p.permissions, "\"/x\" {").unwrap();
        assert!(grant_permissions(&p.permissions, wasm).is_err());
        drop(t);
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn download_verifies_before_writing() {
        let (_t, p) = sandbox();
        let rel = Source::Release {
            version: "1.2.3".into(),
        };
        let plan =
            plan_install(&p, &rel, PluginOptions::default(), &ConfigAccess::Editable).unwrap();
        let bad = fake(b"abd", ABC);
        assert!(execute(&plan, &bad)
            .unwrap_err()
            .to_string()
            .contains("mismatch"));
        assert!(!p.wasm.exists());
        let good = fake(b"abc", &format!("{ABC}  warpify-zellij.wasm\n"));
        execute(&plan, &good).unwrap();
        assert_eq!(std::fs::read(&p.wasm).unwrap(), b"abc");
    }

    #[test]
    fn install_twice_changes_nothing_the_second_time_then_uninstall_cleans() {
        let (t, p) = sandbox();
        let local = t.path().join("local.wasm");
        std::fs::write(&local, b"wasm").unwrap();
        std::fs::create_dir_all(p.config.parent().unwrap()).unwrap();
        std::fs::write(&p.config, "theme \"x\"\n").unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let plan = plan_install(
            &p,
            &Source::Local(local),
            PluginOptions::default(),
            &ConfigAccess::Editable,
        )
        .unwrap();
        let first = execute(&plan, &none).unwrap();
        assert!(first[2].contains("original saved as"), "{first:?}");
        let (cfg1, perm1) = (read(&p.config), read(&p.permissions));
        let second = execute(&plan, &none).unwrap();
        assert!(
            second[1].contains("already up to date") && second[2].contains("already up to date")
        );
        assert_eq!((read(&p.config), read(&p.permissions)), (cfg1, perm1));
        assert_eq!(
            read(&p.config.with_file_name("config.kdl.warpify.bak")),
            "theme \"x\"\n"
        );

        let out = execute(&plan_uninstall(&p, &ConfigAccess::Editable).unwrap(), &none).unwrap();
        assert!(
            out[0].contains("removed warpify's entry") && out[0].contains("backup"),
            "{out:?}"
        );
        assert!(
            out[1].starts_with("removed") && out[2].starts_with("removed"),
            "{out:?}"
        );
        assert_eq!(read(&p.config), "theme \"x\"\n");
        assert!(!p.permissions.exists());
        assert!(!p.wasm.exists());
    }

    #[test]
    fn missing_config_in_a_writable_dir_is_created_with_just_the_block() {
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        let plan = plan_install(
            &p,
            &Source::Local(local),
            PluginOptions::default(),
            &ConfigAccess::Editable,
        )
        .unwrap();
        execute(&plan, &Fake(HashMap::new(), RefCell::default())).unwrap();
        let entry = entry_for(&p.wasm).unwrap();
        assert_eq!(
            read(&p.config),
            format!("// created by warpify install\nload_plugins {{\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"{entry}\"\n}}\n")
        );
        assert!(!crate::fsutil::backup_path(&p.config).exists());
    }

    #[test]
    fn install_twice_then_uninstall_deletes_what_the_installer_created() {
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let plan = plan_install(
            &p,
            &Source::Local(local),
            PluginOptions::default(),
            &ConfigAccess::Editable,
        )
        .unwrap();
        let first = execute(&plan, &none).unwrap();
        assert!(first[2].starts_with("created config"), "{first:?}");
        assert!(read(&p.config).starts_with("// created by warpify install\n"));
        execute(&plan, &none).unwrap();
        let out = execute(&plan_uninstall(&p, &ConfigAccess::Editable).unwrap(), &none).unwrap();
        assert!(out[0].starts_with("removed config"), "{out:?}");
        assert!(!out[0].contains("backup"), "{out:?}");
        assert!(!p.config.exists() && !p.permissions.exists() && !p.wasm.exists());
        assert!(!crate::fsutil::backup_path(&p.config).exists());
    }

    #[test]
    fn an_existing_file_is_never_deleted_without_our_marker() {
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        std::fs::create_dir_all(p.config.parent().unwrap()).unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let plan = plan_install(
            &p,
            &Source::Local(local),
            PluginOptions::default(),
            &ConfigAccess::Editable,
        )
        .unwrap();
        let bak = crate::fsutil::backup_path(&p.config);
        for user in ["", "a 1\n"] {
            std::fs::write(&p.config, user).unwrap();
            execute(&plan, &none).unwrap();
            assert!(bak.exists());
            std::fs::remove_file(&bak).unwrap(); // no backup, still the user's file
            execute(&plan_uninstall(&p, &ConfigAccess::Editable).unwrap(), &none).unwrap();
            assert_eq!(read(&p.config), user);
        }
    }

    #[test]
    fn user_file_with_our_entry_and_no_backup_loses_only_our_line() {
        let (t, p) = sandbox();
        std::fs::create_dir_all(p.config.parent().unwrap()).unwrap();
        let entry = entry_for(&p.wasm).unwrap();
        let before = format!("a 1\nload_plugins {{\n    \"zellij:link\"\n    \"{entry}\"\n}}\n");
        std::fs::write(&p.config, &before).unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let out = execute(&plan_uninstall(&p, &ConfigAccess::Editable).unwrap(), &none).unwrap();
        assert!(out[0].starts_with("updated config"), "{out:?}");
        assert_eq!(
            read(&p.config),
            "a 1\nload_plugins {\n    \"zellij:link\"\n}\n"
        );
        assert!(!crate::fsutil::backup_path(&p.config).exists());
        drop(t);
    }

    #[test]
    fn uninstall_keeps_permissions_other_plugins_still_need() {
        let (_t, p) = sandbox();
        std::fs::create_dir_all(p.permissions.parent().unwrap()).unwrap();
        let other = "\"/x/other.wasm\" {\n    ReadCliPipes\n}\n";
        std::fs::write(&p.permissions, other).unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let plan = plan_uninstall(&p, &ConfigAccess::Editable).unwrap();
        execute(&plan, &none).unwrap();
        assert_eq!(read(&p.permissions), other);
    }

    fn manual(why: &str, seen: Seen) -> ConfigAccess {
        ConfigAccess::Manual {
            why: why.into(),
            seen,
        }
    }

    #[test]
    fn manual_access_touches_no_config() {
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        let access = manual("nix", Seen::Missing);
        let plan =
            plan_install(&p, &Source::Local(local), PluginOptions::default(), &access).unwrap();
        let lines = execute(&plan, &Fake(HashMap::new(), RefCell::default())).unwrap();
        assert!(
            lines[2].contains("\"zellij:link\"\n    \"file:"),
            "{lines:?}"
        );
        assert!(!p.config.exists());
    }

    #[test]
    fn manual_access_advises_by_what_the_config_holds() {
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        let none = Fake(HashMap::new(), RefCell::default());
        let entry = entry_for(&p.wasm).unwrap();
        let held = "load_plugins {\n    \"zellij:link\"\n}\n";
        let add = execute(
            &plan_install(
                &p,
                &Source::Local(local),
                PluginOptions::default(),
                &manual("nix", Seen::Text(held.into())),
            )
            .unwrap(),
            &none,
        )
        .unwrap()
        .remove(2);
        assert!(
            add.starts_with(&format!(
                "\"{entry}\"\nadd this line inside your existing load_plugins block"
            )),
            "{add}"
        );
        assert!(!add.contains("zellij:link"), "{add}");
        let ours = format!("load_plugins {{\n    \"zellij:link\"\n    \"{entry}\"\n}}\n");
        let rm = execute(
            &plan_uninstall(&p, &manual("nix", Seen::Text(ours))).unwrap(),
            &none,
        )
        .unwrap()
        .remove(0);
        assert!(
            rm.starts_with(&format!(
                "remove this line from your load_plugins block: \"{entry}\""
            )),
            "{rm}"
        );
        assert!(!p.config.exists());
    }

    #[test]
    fn manual_uninstall_keeps_the_wasm_while_our_entry_is_in_the_config() {
        let (_t, p) = sandbox();
        let none = Fake(HashMap::new(), RefCell::default());
        std::fs::create_dir_all(p.config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(p.wasm.parent().unwrap()).unwrap();
        std::fs::write(&p.wasm, b"w").unwrap();
        let entry = entry_for(&p.wasm).unwrap();
        let with = format!("load_plugins {{\n    \"{entry}\"\n}}\n");
        std::fs::write(&p.config, &with).unwrap();
        std::fs::set_permissions(
            &p.config,
            std::os::unix::fs::PermissionsExt::from_mode(0o444),
        )
        .unwrap();
        let access = crate::probe_config(&p.config);
        assert!(matches!(access, ConfigAccess::Manual { .. }));
        let plan = plan_uninstall(&p, &access).unwrap();
        assert!(!plan
            .actions
            .iter()
            .any(|a| matches!(a, Action::RemoveWasm { .. })));
        let out = execute(&plan, &none).unwrap();
        assert!(out[0].starts_with("remove this line"), "{out:?}");
        assert!(
            out[0].contains("run `warpify uninstall zellij` again"),
            "{out:?}"
        );
        assert!(p.wasm.exists());
        assert_eq!(read(&p.config), with);
        // The user removes the line by hand; the second uninstall deletes the plugin file.
        std::fs::set_permissions(
            &p.config,
            std::os::unix::fs::PermissionsExt::from_mode(0o644),
        )
        .unwrap();
        std::fs::write(&p.config, "load_plugins {\n    \"zellij:link\"\n}\n").unwrap();
        std::fs::set_permissions(
            &p.config,
            std::os::unix::fs::PermissionsExt::from_mode(0o444),
        )
        .unwrap();
        let access = crate::probe_config(&p.config);
        let out = execute(&plan_uninstall(&p, &access).unwrap(), &none).unwrap();
        assert!(
            !out.iter().any(|l| l.contains("remove this line")),
            "{out:?}"
        );
        assert!(!p.wasm.exists());
    }

    #[test]
    fn unreadable_config_is_decided_in_the_plan_and_install_completes() {
        use std::os::unix::fs::PermissionsExt;
        let (t, p) = sandbox();
        let local = t.path().join("l.wasm");
        std::fs::write(&local, b"w").unwrap();
        std::fs::create_dir_all(p.config.parent().unwrap()).unwrap();
        std::fs::write(&p.config, "a 1\n").unwrap();
        std::fs::set_permissions(&p.config, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&p.config).is_ok() {
            return; // root reads anything
        }
        let access = crate::probe_config(&p.config);
        let ConfigAccess::Manual { seen, .. } = &access else {
            panic!("{access:?}")
        };
        assert!(matches!(seen, Seen::Unreadable(_)), "{seen:?}");
        let none = Fake(HashMap::new(), RefCell::default());
        let plan =
            plan_install(&p, &Source::Local(local), PluginOptions::default(), &access).unwrap();
        let out = execute(&plan, &none).unwrap();
        assert!(p.wasm.exists() && p.permissions.exists());
        let cfg = p.config.display().to_string();
        assert!(
            out[2].contains("add this block")
                && out[2].contains(&format!("couldn't read {cfg}: "))
                && out[2].contains("showing the full block"),
            "{out:?}"
        );
        // Uninstall can't tell whether the line is there: advice, and the plugin file stays.
        let plan = plan_uninstall(&p, &access).unwrap();
        let out = execute(&plan, &none).unwrap();
        assert!(out[0].contains("couldn't read"), "{out:?}");
        assert!(p.wasm.exists());
        std::fs::set_permissions(&p.config, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }
}
