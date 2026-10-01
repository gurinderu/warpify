# Reality — what a claim is checked against
The graph models the work, the repo is part of its embodiment; this file names the third thing — what the work becomes when it runs, and how to look at it. A claim is decided by its canonical carrier, never the source meant to produce it; tests are a rung of evidence, not a carrier.

| Claim class | Canonical carrier | How to observe | Who can |
|---|---|---|---|
| Plugin/CLI behaviour (tabs and clients listed, client bound to a tab, `watch` stream) | the release `.wasm` loaded into a live zellij 0.45.1 session, driven by the built `warpify` binary | the agent hands over exact steps (build commands, how to load the plugin, which `warpify` calls to run, what to expect); the owner runs them and reports what they saw | user |
| Wire format between CLI and plugin | `warpify-proto` serde output | `cargo test -p warpify-proto` — exact-JSON assertions | agent |
| `install`/`uninstall` file effects (wasm placed, `permissions.kdl` merged, `config.kdl` edited or snippet printed, clean revert) | the files the built `warpify` writes under a sandboxed HOME | `T=$(mktemp -d)`; run `warpify install zellij --wasm <local build>` then `uninstall zellij` with `HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, `ZELLIJ_CONFIG_DIR` all inside `$T`; inspect the files. Never against the real home. That zellij then loads the plugin without a prompt is the behaviour row above | agent |
| home-manager module effects (link to the store wasm at the install path, `load_plugins` in the generated `config.kdl`, grant on activation, warnings) | the generated home-manager configuration (home files, activation script, `config.warnings`); after `home-manager switch`, the real files | agent: `nix flake check` (its `home-manager-module` check builds a minimal configuration and asserts the files) — on Linux locally, on macOS by the `nix-darwin` CI job; the real switch and its files: owner | agent / user |
| Code compiles and is lint-clean | build artefacts from `scripts/gate.sh` | `scripts/gate.sh` (inside `nix develop`) and the `ci` check on the PR | agent |

**Ceiling**: the agent cannot observe live zellij behaviour itself — a behavioural claim stays unverified until the owner reports the observation; never close it from tests or source.

**The table grows through use.** A session taught something the table lacks (an unnamed carrier, an observation reachable or not — then into *Ceiling*, a wrong command here) — write the row then, before closing the work that taught it. Only what observation needs goes here; dated measurements and history go to graph nodes.
