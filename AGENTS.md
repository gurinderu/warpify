# warpify
A zellij plugin plus the `warpify` CLI: see a session's tabs and clients from outside and bind a client to a tab over `zellij pipe`.

## Cover
Slot, value, source. Source is `derived`, `agreed: <who>` or `<not agreed — case №1>` (the alignment case on the repo contour).

| Slot | Value | Source |
|---|---|---|
| Nature | research — gate stays strict; no coverage threshold, no e2e suite required | agreed: Nick |
| Graph | `@nick/warpify` (r303) — every session starts here | derived |
| Focus contour | #2 «🌀 warpify — плагин zellij и CLI управления клиентами по вкладкам» | derived |
| Repository | `github.com/gurinderu/warpify` — the `repository` attr of #2, from origin | agreed: Nick |
| Agent role | #3 «🛠 Делатель warpify» — adhikarin, steward of #2; inbox `iskron_orient(focus="3")` | derived |
| Owner role | #1 «👑 Владелец warpify» — svatantra, the `posed_to` address outside the mandate | derived |
| Stack | Rust stable (`rust-toolchain.toml`, target `wasm32-wasip1`), edition 2021, serde/serde_json, `zellij-tile =0.45.1` | derived |
| Gate | `scripts/gate.sh` — fmt check, clippy pedantic `-D warnings` (host + `wasm32-wasip1`), tests, release plugin build | agreed: Nick |
| Consumers | the owner only, in their own zellij sessions; breakage shows up there | agreed: Nick |
| Cost of breakage | low: the owner's own sessions; nothing shipped to others | agreed: Nick |
| Reality | `REALITY.md` at the root — read on occasion (section «Reality» below); behaviour is checked by the owner by hand | agreed: Nick |
| Layout | code map: «Project structure» section until a component gets its README — whoever first touches a component moves its map there; gotchas: graph nodes on #2 | derived |
| Cross-project memory | personal realm `@nick/mind`; no global instructions file; never the memory directory | derived |
| Language | talk to the user in Russian; everything written into the repo (code, comments, docs, commits, PRs) in English | agreed: Nick |
| Feedback reflection | no | agreed: Nick |
| Workflow-suite interop | none — no coercive workflow suite installed | derived |
| Alignment | empty — all slots agreed (case №1 on #2); a new cover question is a word in a case on #2, not a node | derived |

## Persistence rules
State lives in the **repo** or in the **graph** — nowhere else. The harness's built-in memory (memory directory, conversation summaries, `/tmp`, machine-local files) is **forbidden entirely, not by category**. The work ledger is case lines; while no case exists — one file in the session temp dir (the `iskron` door's cross-cutting norms): a reason to open a case, it dies with the session — the only exception.
- **Repo**: code, configs, rituals (how to act here), branch state.
- **Graph**: decisions, substantive questions (vimarshas), plans (map of transformations), lessons, gotchas. Don't retell the graph in the repo — link it.
- **Case** (journal on a graph node): one-off tasks, one-off questions, work progress, shift handover — never copied into nodes.
- **Fetch state, don't recall.** A "we decided…" with no source — read the graph or repo before acting.
- **External design/spec files are drafts for intake**: the graph holds decisions.
- **A node isn't written until its puller is named** — which kriya breaks if it vanishes? Three shelters: prose in this file; a sinn phenomenon for what acts; a lone `context` arrow. The door enters the graph where the doer acts — upadhi on a kriya.
- **Whose fact is it?** — always ask, above the harness memory instruction; before finishing, check every durable fact from the conversation is persisted. Ritual, command, order of this repo → this file; code fact, servers, deploy, dated debts (date in node `attrs`) → this repo's graph; meaning of code (decisions, grounds, links) → graph always; file navigation → per the «Layout» slot; decision → node at once; substantive question, commitment → vimarsha; one-off task and progress → case; work state → node modes. The user's own off-project facts (machines, deadlines, people, cross-project lessons) → `@nick/mind` (**minding**) the moment you learn them; about another project → that project's graph. "How to talk to me" preference → `@nick/mind`; one affecting process and result → this file or the project graph. Project rules never go into another graph.
- **Only committed worktree files (AGENTS.md, the hooks file) and the project graph configure agent behaviour** — never a global instructions file (`~/.claude/CLAUDE.md`, `~/.config/opencode/AGENTS.md`…), harness memory, user hooks. Installing the delivery (bridge, plugin) is delivery, not configuration.
- The memory directory is **evacuated and frozen**: `MEMORY.md` is a one-line prohibition stub pointing here; the memory-guard `PreToolUse` hook blocks writes there (exit 2). Evacuate before freezing; nothing is deleted unread.

## Session lifecycle
One line per rule; the full norm of case work and the ledger is in the `iskron` skill.
- **Graph is the work, git is how we got here.** SHAs, branches, PR numbers, "merged" never go into the graph (bodies, names, attrs); provenance goes into a write's `reasoning`. Case journals may carry them freely.
- **Session start** — the «Start» section of the `iskron` skill, before acting; it ends at readiness (graph and role named, greeting delivered), not at reading. Standing only on watch (the word "вахта"/"watch", `start`, a seat address from the window, a frame), via one `iskron_stand`. `start <graph> <role> <case №N>` enters that case; first word is a retelling of the assignment. A subagent has its own satellite bridge (`iskron_stand` with `satellite_of`, `join`, `leave` on it); on the launcher's bridge it does not start — a subagent without its own bridge doesn't write to the graph and says so in the first line of its result. Addresses come from the cover; the owner is their role's seq, not `me`.
- **Start of work: graph, then design, then code.** (1) graph reconnaissance (`entry`): what's recorded about the change site, open vimarshas, decided and rejected, recorded external surfaces; (2) integration field (`integrity`, section below); (3) design (`design`), then code. Skip only on an explicit "just work" or another named protocol, with the reconnaissance debt carried to reconcile; the human's silence is not permission. Working past the graph is an agent's worst failure.
- **A decision goes into the graph the moment it's made**, wherever it came from (chat, socket, agent agreement): epistemic no higher than `anumita`, ontic `anagata`, volitive `chanda`/`adhimoksha`; who decided and what counts as done. A changed situation — likewise at once.
- **A task is described before starting — as what it is.** One-off — a word in a case: assignment, retelling, a `поручение: …` line at the assigner, closed by the doer under the same key per outcome; arriving without a case — open one (`open_room` on the subject node or `iskron_case(action="talk", about=<subject>)`) before the first outside change. Into the graph — the transitions it changes (in design modes; large ones as a transformation) and decisions. Vimarsha — only a substantive question or commitment; kriya — only a repeatable transition: ask what it eats and produces next run — no answer, it's a task.
- **Work runs through the case.** Ledger line: one key per topic; `done` — one action; `note` — what's wrong, with `ok` only the observation ceiling; unobserved is not `ok`; work that changed what the graph describes, on the event (merge, rollout, decision, case close) — `partial` «граф догнать — на ткаче», same key `ok` with three parts: landed, moved, released, each at least "zero, because…"; done and open diverge — done `ok` on its topic, the remainder under its own key `partial` «on whom, waiting for what». Waiting inside a case — `partial`; across cases and on substance — vimarsha `posed_to`. Word to an agent — in the case; the channel only for a human without a seat in the case. Refusing an assignment — a word and a `bad` line «отказ: reason» under the same key; withdrawal — the withdrawer's line; refusing a vimarsha — by editing it. Leaving or handing over — close or hand over open lines; handover is a transformation seed and a word in the case, leadership by agreement (`architect` skill).
- **Merge → graph.** A push that opened or updated a PR shipped nothing. The sequence hangs on the merge event, not on a lull. All acts mandatory — except work by reference from an agent (assignment by references, report in the case): there weaving, axis close and reconcile belong to the assigner, the doer owes the transformation seed and delivery modes (`vahta` skill):
  - **Weave** (`weaving`): what shipped goes into the target system (architecture, API, delivery, experience, integration) as nodes and edges, not a paragraph; repo mechanics stay in git. Zero nodes and arrows after a substantive wave — say plainly why.
  - **Advance the map**: open work — `anga` to the transformation; one `genre=hint` seed per transformation — only what matters after the session, live cases one line each.
  - **Flip modes** (`anagata→vartamana`, `kalpita→pratyakshita`) across the designed contour — after evidence on the carrier (`REALITY.md`, **reality-audit**).
  - **Close by axis** (`inquiry`): `addressed_by` to the carrying node; `visarjana` when the answer stands as a node, the repo shows it and reality shows how far it reaches (where it can't — the user's word); otherwise present to the owner; other ends — keep, supersede, crystallize. Same move — the `posed_to` inbox (don't judge others by age; a change at the anchor wakes them), case lines `ok` per observed, `propose_close` with evidence.
  - **Reconcile** (`reconcile`): nodes against code, code against graph, the discarded recorded and referenceable; remainder as vimarshas.
  - **Vocabulary pass**: borrowed words (ticket, backlog, sprint, epic, story, done, blocker, committed) in landing text and nodes — name them to the human and ask what the project calls them; don't substitute yourself.
- **Design isn't ready until decisions, risks and lifecycle are in the graph** — whatever skill elicited it; another suite's design/spec file is taken in the same session. Without the owner: decisions and risks now, the transformation with its telos for confirmation.
- **The main session orchestrates; subagents do the work.** It holds the dialogue with the owner, decisions, writing the design into the graph, briefs and acceptance. Implementation of a settled design goes to `worker` (self-contained brief: design nodes, files, gate, return contract); graph-writing design to `designer`; reconnaissance to `reader`/`searcher`; checks to cold `reviewer`/`verifier`. Writing code in the main session only when the owner asks for it.
- **Execution suites run execution** (plan, TDD, debugging, review); the graph carries memory and design. Execution decisions and risks go to the graph before the session ends.
- **A claim you made is not a claim you accept.** Behavioural claims are closed by a cold `verifier`: brief — the claim, carrier and falsifier from `REALITY.md`; wait for the verdict. No such role — observe the carrier yourself, never the source.
- **Hook merging**: entries from different suites in the hooks file coexist — add alongside, never overwrite others.
- Hooks in `.claude/settings.json` — session start, push, merge, memory-guard — are wired, one line each. Merge done on the GitHub web UI with trunk pulled by `git merge --ff-only` fires no hook — run the post-merge acts yourself.
- **Keep this file honest.** The contract number is the first word of the `iskronify` skill description, present in every session's context: compare it with the stamp at the bottom, loading nothing. Higher than the stamp, or the sources moved after its date (`git log -1 --format=%cd -- <those files>`) — offer an `iskronify` run as the first move (launch is the human's or a case's word; silence — offer again; on watch without a window, after asking colleagues, the run is done per the «Who runs it» rule of `iskronify`: the `designer` role, without subagents or the role — yourself). A line here disagrees with the skill — say so aloud (in the case to the assigner, otherwise to the human): stamp lower — the skill is right; equal — a template defect, feedback to the skills-delivery steward (`feedback` skill).
- **Keep the toolchain fresh**: updates are on by default, take them as the channel delivers. A channel without auto-update (unpacked copy) — check the version before the session or move.

### Stage self-check
Gate green and a coherent stage done (PR opened or updated, or about to touch nodes beyond the initial ones) — reread the branch diff against trunk: bugs, fragile spots, weak error handling, DRY/SOLID violations, missing or useless tests, files over 150 lines and god-units. Fix in the same branch and push, or say plainly nothing surfaced; don't invent findings. Per stage, not only at the end.

### Cold stage review
- **Self-check doesn't replace cold review** — both, in this order: you see your own work as you intended it.
- After self-check of an open PR or a large stage — review by the `reviewer` role, **in a separate worktree**, where the spawning tool gives isolation (Claude Code — `isolation`). The harness can't — say there was no cold review. Only push has a watchdog: a stage without a push is yours to hold.
- Reviewer's field: the whole branch diff against trunk; the repo itself; the focus holon and its steward role; references to graph nodes in the diff (not a retelling). It runs `integrity` read-only and returns, with findings, an integration report: affected, relays, open questions, neighbours' readiness, whom to wake (`standing`); unknown — `unknown`.
- `NEEDS_CONTEXT` is a graph defect: design further, weave, pose vimarshas, repeat the review.
- A finding you disagree with — reject with a recorded "why".
- A subagent helping a case is launched with the line `start <graph> <role> <case №N>` (`vahta` skill).

### Branch discipline
One branch until its merge — follow-ups go into it. After merge: `git checkout main && git pull`; delete the merged branch (`git branch -d`) and others already in `main`; weave what shipped into the graph; next branch from fresh `origin/main`; confirm cleanup before the next task.

## Working principles
1. **Think before code.** Name assumptions; when unsure, ask *what exactly* is unclear. **Ask the human in text** (conversation, case, or their standing bridge while they have no seat in the case); never a multiple-choice tool — it substitutes an answer for the question. Push back on a simpler path or a false premise. Touch the live system before trusting a type, name, or doc. Outside the mandate — a vimarsha `posed_to` the owner role.
2. **Simplicity first.** Minimum for the task; no speculative features, abstractions for one-offs, handling of the impossible. Validate at the boundaries.
3. **Stay inside the repo boundary.** Edits only in the working directory; outside it — reading carriers, the session temp dir and the delivery home. Another contour — a vimarsha on its node (`anga` to the transformation) and a word to its steward in a case (`iskron_case(action="talk", about=<subject>)`).
4. **A second implementation is an event to report.** Derive both sites via `integrity`, name them to the human, propose reunification or a named fork; a new consumer gets its edges in the same move.
5. **Surgical changes.** Touch what the task needs; don't reformat or refactor neighbouring code; keep the style; the linter is authoritative; delete only what your change made dead, flag the rest.
6. **Goal-driven execution.** A bug — a failing test before the patch. Multi-step — `step → check` pairs. Runtime — in the real environment. Falsifier before looking; observe the carrier (`REALITY.md`), not the source.
7. **Read before answering an open question.** Discuss, think through, design, "what do you think" — from what's recorded, not training data: the graph several ways (`entry` skill). Search output is a lead, not data: structure and links only via `iskron_look`, `iskron_orient` and lenses; hand wide reconnaissance to a subagent.
8. **Think in the graph, speak the project's language.** Graph vocabulary (kriya, phenomenon, contour, role, vimarsha, modes) is for reasoning; to the human — project words, until they use them first. About work — no ticket, task, sprint, backlog, story, done: question, change, what's open, what it unblocks.

## Integration field — from the graph only
- Traversal root — the focus holon and steward role from the cover. Don't keep or ask for a list of shared surfaces and consumers here — what the graph doesn't model goes into «External surfaces».
- For each change name the nodes whose embodiment is in the diff and run `integrity`: phenomenon — `iskron_orient(lens="trace")` both ways; kriya — the `next` thread and `ahara`/`utpatti`/`upadhi` relays; leaving into another holon — to its steward role.
- A dependency the traversal didn't find is a model defect: design further (`design`), weave (`weaving`), waiting — a `posed_to` vimarsha and a word in the case. A public handle with no trace in the graph is either unneeded or a graph debt. A new consumer is in when code and edges appeared in one move.

## External surfaces — what you use and don't own
Foreign API, SDK, CLI, protocol, schema: memory of them is indistinguishable from knowledge, and a wrong name spreads on a live call. Here: `zellij-tile` and the `zellij pipe` CLI.
- **Before work, pin the touched part of the surface** as a graph node, with its version.
- **Pratyaksha before shabda**: observation by your own hands (a call, `--help`, the installed package's types) beats docs, docs beat memory, memory is not a source. `pratyakshita` — only the observed.
- **Weave the link**: the surface node is `upadhi` (or `ahara`/`utpatti`) to the kriya acting through it.
- **Keep in step**: a divergence or new version — edit the node in the same move, lower the epistemic if not observed.
- Source at a surface carries `(graph @nick/warpify, node #N)` — and you read that node before working.

## Reality — what a claim is checked against
The carrier table is in `REALITY.md` at the root, read on occasion. Before saying a behaviour "works", flipping a mode or closing a question — read your claim class's row and observe its carrier; the row goes whole into the `verifier` and `reviewer` brief. The first row is plugin/CLI behaviour in a live zellij session — observed by the owner by hand: hand them exact steps and wait for their report. Ceiling is there too. Learned a carrier the table lacks — add the row then.

## Graph ↔ repo: what lives where
| Concern | Repo | Graph |
|---|---|---|
| Code, configs, lockfiles | ✓ | |
| Commands, conventions, stack | ✓ (AGENTS.md) | |
| Reality carriers | ✓ (REALITY.md) | |
| Gotchas | | ✓ node; this file references it |
| Branch state, what's in flight | git + PR body (from case lines) | ✓ (`genre=hint` transformation seed) |
| Methodology, ontology | | ✓ |
| Decisions | | ✓ (as a node, at once) |
| Substantive questions, commitments | | ✓ (vimarshas) |
| Plans | | ✓ (map of transformations) |
| One-off task, progress, shift handover | | ✓ (case journal) |
| Commit history, PRs, SHAs | git | (never in the graph) |

**No `HANDOVER.md`**: branch state has homes — `git branch`/`log` and the open PR body (from case lines), node modes, the transformation seed; a hand-written file is the only one of them that lies silently. Forge unreachable — progress is read from modes and the seed.

## Commands
| What | Command |
|---|---|
| gate (the only call before push; CI runs it) | `scripts/gate.sh` |
| build (default members: all but the plugin) | `cargo build` |
| build plugin | `cargo build -p warpify-plugin --target wasm32-wasip1 --release` |
| test | `cargo test` |
| release | merge the release PR that release-please keeps open on main (conventional commits drive the version; the release gets warpify.wasm + .sha256) |
| lint | `cargo clippy --workspace --exclude warpify-plugin --all-targets -- -D warnings` (plugin: `-p warpify-plugin --target wasm32-wasip1`) |
| format | `cargo fmt --all` |

The plugin lands at `target/wasm32-wasip1/release/warpify.wasm`; the CLI at `target/debug/warpify` (or `release`).
- Toolchain and C linker come from the flake devShell: `nix develop` (or direnv with `.envrc`), then cargo / `scripts/gate.sh` as usual; outside it native builds fail for lack of `cc` (graph @nick/warpify, node #4).
- Pre-commit hook lives in `.githooks/`; enable per clone with `git config core.hooksPath .githooks`.

## Project structure
- `flake.nix` / `.envrc` — devShell: Rust from `rust-toolchain.toml` via rust-overlay, plus the C linker.
- Binary crates in `bin/` stay thin (wiring, args, I/O); logic lives in library crates in `crates/`, unit-tested on the host.
- `Cargo.toml` — workspace; members `crates/proto`, `crates/session`, `crates/client`, `crates/telemetry`, `crates/install`, `bin/plugin`, `bin/cli`; default members exclude the plugin (it targets wasm). Release profile is size-optimized (`opt-level = "s"`, LTO, strip).
- `crates/proto` — `warpify-proto`: wire types shared by plugin and CLI (`Request`, `Target`, `Event`, `State`); pipe name `warpify`, NDJSON replies, heartbeat on `watch`.
- `crates/session` — `warpify-session`: the plugin's logic without zellij types, host-tested: `Snapshot` of tabs and clients taken from `TabUpdate` (non-mirrored sessions only), the wire `State` built from it, leader choice, which instance handles a request, and the binding state machine (target → step, pin pull-back, detach once the bound tab is gone, forget-on-departure; frozen instances of departed clients stay silent; graph @nick/warpify, node #9).
- `crates/client` — `warpify-client`: the CLI side of the pipe: runs `zellij pipe`, reads NDJSON replies, liveness timeout and the "is it loaded?" errors; fire-and-forget `send`; `bind` confirmation and `attach`'s wait for the new client (`bind.rs`; node #16).
- `crates/telemetry` — `warpify-telemetry`: `init(default_filter)` installs the `tracing` subscriber on stderr (no timestamps, `RUST_LOG` overrides); no threads, builds for `wasm32-wasip1`.
- `crates/install` — `warpify-install`: `warpify install|uninstall zellij` logic, host-tested: paths, sha256-verified download (`ureq`), a pure `Plan` then execution, format-preserving `kdl` edits of zellij's `config.kdl` (`load_plugins`) and `permissions.kdl` (grants from `warpify_proto::PERMISSIONS`), "manual" snippet for nix-store/read-only configs (graph @nick/warpify, node #17).
- `.github/workflows/release-please.yml`, `release-please-config.json`, `.release-please-manifest.json` — release-please opens and updates the release PR on `main`; merging it tags `vX.Y.Z`, and the workflow's `assets` job builds the wasm plugin and attaches `warpify.wasm` and `warpify.wasm.sha256` to the GitHub release.
- `bin/plugin` — `warpify-plugin`, bin `warpify` (`src/main.rs`): the zellij plugin, built for `wasm32-wasip1`, pinned to `zellij-tile =0.45.1`. zellij runs one instance per client and fans each pipe message to all of them. Designed so that session-wide replies come from the lowest client's instance and per-client moves from that client's own instance (graph @nick/warpify, nodes #9, #10) — not yet observed in a live session.
- `bin/cli` — `warpify-cli`, bin `warpify`: the host CLI (clap, printing) over `warpify-client` (`state`, `watch`, `bind`, `attach`, `install`/`uninstall`, hidden `__bind-new` helper; global `-s/--session <name>` targets a named session, default the current one — `attach` defaults to `warpify`).

## Code conventions
- **Meaning lives in the graph, code references it**: a comment carrying rationale, discarded alternatives or integration design is a node; in code — "(graph @nick/warpify, node #N)", also for the discarded ("not cached: #N"). Mechanics — words in place. After referencing, check the node says it; diverged — fix the node.
- **Clippy pedantic is on workspace-wide** (`[workspace.lints.clippy]`); every member crate carries `[lints] workspace = true`.
- **Wire format is a contract**: any change to `warpify-proto` types changes plugin and CLI in the same change, and the serde-format tests in `crates/proto` assert the exact JSON.
- **Output vs diagnostics**: program output (the CLI's answer) is plain writes to stdout; diagnostics go through `tracing` (stderr, `RUST_LOG`/`-v`), never `println!`/`eprintln!` — except the CLI's final error line.
- **Gotchas don't live here**: a graph node on #2; here and in code — a reference.

## What to update when
- `AGENTS.md` — **what can be learned from a graph node isn't written here**; it holds what's needed before an agent reaches the graph (commands, the way into orientation, invariants outside the linter, stop-forks), and changes when that changes. Pruning prose means moving it into carrying nodes, not deleting.
- `REALITY.md` — when a claim class's carrier appears, changes or turns out unreachable; dated measurements go to the graph.
- A component README — in the same commit that moves, renames or creates its files.
- Project graph — on every merge («Session lifecycle»).

## Git workflow
- **Conventional commits** (`feat:`/`fix:`/`chore:`/`refactor:`/`docs:`/`test:`); branches `feat/…`, `fix/…`, `chore/…`; PR titles in the same format.
- **No AI attribution** — no co-author trailer, no "Generated with …" line in commits, PR titles or bodies.
- **Gate — one call**: `scripts/gate.sh`; call it by name, never assemble the steps by hand; CI (`.github/workflows/ci.yml`) runs `scripts/gate.sh` inside `nix develop`, like locally.
- **Pre-commit hook** (`.githooks/pre-commit`) runs fmt check and host clippy; not enabled — gate before push.
- **Branch doesn't live without a PR**: pushed a branch — open a PR in the same move (draft if unfinished). Forge: GitHub, CLI `gh` (`gh pr checks <n> --watch`).
- **Definition of done**: PR into `main` merged with the `ci` check green (`gh pr checks <n> --watch`).
- **Releases**: never tag by hand; release-please opens the release PR, merging it publishes the release. Remove `release-as` from release-please-config.json after the first release.
- **Never** `--no-verify`, `--force`, `--no-gpg-sign`, `git reset --hard` without an explicit instruction.

*(iskronify: contract 18, stamp 2026-10-01 — offer a re-run when the installed iskronify's description names a higher contract or when the sources this file was derived from moved after this date.)*
