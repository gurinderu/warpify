//! `warpify install|uninstall <integration>`: wiring over `warpify-install`.

use std::io::{self, Write};
use std::path::PathBuf;

use warpify_install::{
    execute, plan_install, plan_uninstall, probe_config, Dirs, EnvVars, Paths, Plan, Source,
    UreqFetcher,
};

use crate::{Integration, Outcome};

const INSTALLED: &str =
    "new zellij sessions will load the plugin; restart running sessions to pick it up";
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
    finish(&plan_install(&paths, &source, &access)?, dry_run, INSTALLED)
}

pub fn uninstall(integration: Integration, dry_run: bool) -> Outcome {
    let Integration::Zellij = integration;
    let paths = Paths::resolve(&EnvVars::from_process(), &Dirs::from_system()?)?;
    let access = probe_config(&paths.config);
    finish(&plan_uninstall(&paths, &access)?, dry_run, UNINSTALLED)
}

fn finish(plan: &Plan, dry_run: bool, closing: &str) -> Outcome {
    let mut out = io::stdout().lock();
    if dry_run {
        for action in &plan.actions {
            writeln!(out, "would {action}")?;
        }
        return Ok(());
    }
    for line in execute(plan, &UreqFetcher)? {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "{closing}")?;
    Ok(())
}
