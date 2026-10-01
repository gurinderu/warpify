//! `warpify install|uninstall <integration>`: wiring over `warpify-install`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use warpify_install::{
    execute, grant_permissions as grant, plan_install, plan_uninstall, probe_config, Action, Dirs,
    EnvVars, Paths, Plan, Source, UreqFetcher,
};

use crate::{Integration, Outcome};

const INSTALLED: &str =
    "new zellij sessions will load the plugin; restart running sessions to pick it up";
const INSTALLED_BY_HAND: &str = "after you add it, new zellij sessions will load the plugin; restart running sessions to pick it up";
const UNINSTALL_PENDING: &str = "the plugin file stays and new zellij sessions still load it until you remove that line and run the uninstall again";
const UNINSTALLED: &str =
    "new zellij sessions won't load the plugin; running sessions keep it until restarted";

pub fn install(integration: Integration, wasm: Option<PathBuf>, dry_run: bool) -> Outcome {
    let Integration::Zellij = integration;
    let paths = Paths::resolve(&EnvVars::from_process(), &Dirs::from_system()?)?;
    let source = match wasm {
        Some(from) => Source::Local(std::fs::canonicalize(from)?),
        None => Source::Release {
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    };
    let access = probe_config(&paths.config);
    let plan = plan_install(&paths, &source, &access)?;
    finish(&plan, dry_run, closing(&plan, true))
}

/// Hidden: only the permission grant for a plugin placed elsewhere (nix); one line out.
pub fn grant_permissions(integration: Integration, wasm: &Path) -> Outcome {
    let Integration::Zellij = integration;
    let paths = Paths::resolve(&EnvVars::from_process(), &Dirs::from_system()?)?;
    writeln!(io::stdout().lock(), "{}", grant(&paths.permissions, wasm)?)?;
    Ok(())
}

pub fn uninstall(integration: Integration, dry_run: bool) -> Outcome {
    let Integration::Zellij = integration;
    let paths = Paths::resolve(&EnvVars::from_process(), &Dirs::from_system()?)?;
    let access = probe_config(&paths.config);
    let plan = plan_uninstall(&paths, &access)?;
    finish(&plan, dry_run, closing(&plan, false))
}

/// The last line says what the user will see, which depends on what happened: a config we
/// edited, or one they must edit by hand (then uninstall may have kept the plugin file).
fn closing(plan: &Plan, installing: bool) -> &'static str {
    let by_hand = plan
        .actions
        .iter()
        .any(|a| matches!(a, Action::Manual { .. }));
    match (installing, by_hand) {
        (true, false) => INSTALLED,
        (true, true) => INSTALLED_BY_HAND,
        (false, true) => UNINSTALL_PENDING,
        (false, false) => UNINSTALLED,
    }
}

fn finish(plan: &Plan, dry_run: bool, closing: &str) -> Outcome {
    let mut out = io::stdout().lock();
    if dry_run {
        for action in &plan.actions {
            writeln!(out, "would {action}")?;
        }
        return Ok(());
    }
    for line in execute(plan, &UreqFetcher)?
        .iter()
        .filter(|l| !l.is_empty())
    {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "{closing}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use warpify_install::{ConfigAccess, Seen};

    fn paths() -> Paths {
        Paths {
            wasm: "/d/warpify/warpify-zellij.wasm".into(),
            config: "/c/config.kdl".into(),
            permissions: "/k/permissions.kdl".into(),
        }
    }

    fn manual(seen: Seen) -> ConfigAccess {
        ConfigAccess::Manual {
            why: "nix".into(),
            seen,
        }
    }

    #[test]
    fn the_summary_follows_what_happened() {
        let src = Source::Local("/w.wasm".into());
        let line = |a: &ConfigAccess| closing(&plan_install(&paths(), &src, a).unwrap(), true);
        assert_eq!(line(&ConfigAccess::Editable), INSTALLED);
        let by_hand = line(&manual(Seen::Missing));
        assert_eq!(by_hand, INSTALLED_BY_HAND);
        assert!(by_hand.starts_with("after you add it, new zellij sessions"));

        let ours = "load_plugins {\n    \"file:/d/warpify/warpify-zellij.wasm\"\n}\n";
        let line = |a: &ConfigAccess| closing(&plan_uninstall(&paths(), a).unwrap(), false);
        assert_eq!(line(&ConfigAccess::Editable), UNINSTALLED);
        assert_eq!(line(&manual(Seen::Text(ours.into()))), UNINSTALL_PENDING);
        assert_eq!(
            line(&manual(Seen::Unreadable("denied".into()))),
            UNINSTALL_PENDING
        );
        // Our line is not in the config: nothing left to do by hand, the plugin file goes.
        assert_eq!(line(&manual(Seen::Text("a 1\n".into()))), UNINSTALLED);
        assert_eq!(line(&manual(Seen::Missing)), UNINSTALLED);
    }
}
