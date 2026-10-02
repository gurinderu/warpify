//! zellij's `config.kdl`: add or drop our entry in the top-level `load_plugins` block.

use std::path::Path;

use kdl::{KdlDocument, KdlNode};

use crate::kdl_edit::{add_child, append_node, has_child, parse, remove_child, set_options};
use crate::plan::{PluginOptions, Seen};
use crate::Result;

const BLOCK: &str = "load_plugins";
/// The background plugins zellij 0.45.1 loads by default (`default.kdl`). A `load_plugins` block
/// REPLACES them (zellij-utils `kdl/mod.rs`), so a block we create has to carry them.
const ZELLIJ_DEFAULTS: &[&str] = &["zellij:link"];
const DEFAULTS_NOTE: &str = "zellij defaults kept: load_plugins replaces them";

/// A fresh `load_plugins` block holding zellij's defaults under a note.
fn defaults_block() -> KdlNode {
    let defaults = ZELLIJ_DEFAULTS.join("\"\n    \"");
    let text = format!("{BLOCK} {{\n    // {DEFAULTS_NOTE}\n    \"{defaults}\"\n}}\n");
    let mut doc: KdlDocument = text.parse().expect("the defaults block is valid KDL");
    doc.nodes_mut().remove(0)
}
const WHAT: &str = "the zellij config";
/// First line of a `config.kdl` the installer created; uninstall deletes only files that have it.
const CREATED_MARKER: &str = "// created by warpify install";

/// `text` of a config file warpify is creating: the marker line on top.
pub(crate) fn mark_created(text: &str) -> String {
    format!("{CREATED_MARKER}\n{text}")
}

/// Whether uninstall may delete the file: `before` starts with our marker and `after` (the
/// text with our entry removed) holds nothing but the marker, our defaults note and whitespace.
pub(crate) fn only_ours_left(before: &str, after: &str) -> bool {
    let note = format!("// {DEFAULTS_NOTE}");
    before.lines().next() == Some(CREATED_MARKER)
        && after
            .lines()
            .map(str::trim)
            .all(|l| l.is_empty() || l == CREATED_MARKER || l == note)
}

/// Whether `entry` is in the top-level `load_plugins` block of `text` (plain text search when it
/// does not parse, so a broken file still counts as holding our line).
pub(crate) fn lists_entry(text: &str, entry: &str) -> bool {
    parse(text, WHAT).map_or_else(
        |_| text.contains(entry),
        |doc| doc.get(BLOCK).is_some_and(|b| has_child(b, entry)),
    )
}

/// What to tell a user whose config warpify must not edit: `seen` is what the probe found in
/// `file`, `why` is why it is not editable. Adding to an existing `load_plugins` block is one
/// line; without a block (or without a readable file) it is the whole block, zellij's defaults
/// included. Removing says to run uninstall again, because the plugin file stays meanwhile.
pub(crate) fn manual_advice(
    file: &Path,
    seen: &Seen,
    entry: &str,
    options: &PluginOptions,
    adding: bool,
    why: &str,
) -> String {
    let line = options.snippet(entry);
    let tail = format!("(it's managed outside warpify: {why})");
    let again = "then run `warpify uninstall zellij` again to delete the plugin file";
    if !adding {
        return match seen {
            Seen::Unreadable(e) => format!(
                "if {line} is in your load_plugins block, remove it, {again} (couldn't read {}: {e})",
                file.display()
            ),
            _ => format!("remove this line from your load_plugins block: {line}, {again} {tail}"),
        };
    }
    let full_block = |note: &str| {
        let block = edit_install("", entry, options)
            .ok()
            .flatten()
            .unwrap_or_default();
        format!(
            "{}\nadd this block to your zellij config (load_plugins replaces zellij's defaults, so they are in it) {note}",
            block.trim_end()
        )
    };
    let doc = match seen {
        Seen::Unreadable(e) => {
            return full_block(&format!(
                "(couldn't read {}: {e}; showing the full block)",
                file.display()
            ))
        }
        Seen::Missing => KdlDocument::new(),
        Seen::Text(t) => match parse(t, WHAT) {
            Ok(doc) => doc,
            Err(e) => {
                return format!("{line}\nadd this line inside your load_plugins block ({e}) {tail}")
            }
        },
    };
    match doc.get(BLOCK) {
        Some(block) if has_child(block, entry) => {
            format!("{line}\nalready in your load_plugins block, nothing to add {tail}")
        }
        Some(_) => {
            format!("{line}\nadd this line inside your existing load_plugins block {tail}")
        }
        None => full_block(&tail),
    }
}

/// Adds `entry` (e.g. `file:/abs/warpify-zellij.wasm`) to `load_plugins` with `options` as its
/// children, or brings an existing entry's options in line with `options`. A block that exists
/// only gets our child; when there is none it is created at the end with zellij's default entries
/// too: new text, `None` when the entry is already there as wanted.
///
/// # Errors
/// When `text` is not valid KDL.
pub fn edit_install(text: &str, entry: &str, options: &PluginOptions) -> Result<Option<String>> {
    let mut doc = parse(text, WHAT)?;
    let pairs = options.pairs();
    let owned = PluginOptions::KEYS;
    if let Some(block) = doc.get_mut(BLOCK) {
        if has_child(block, entry) {
            return Ok(set_options(block, entry, owned, &pairs).then(|| doc.to_string()));
        }
        add_child(block, entry);
        set_options(block, entry, owned, &pairs);
    } else {
        let mut block = defaults_block();
        add_child(&mut block, entry);
        set_options(&mut block, entry, owned, &pairs);
        append_node(&mut doc, block);
    }
    Ok(Some(doc.to_string()))
}

/// Drops `entry` from `load_plugins`, and the block itself when that leaves it empty or holding
/// just the defaults we added: new text,
/// `None` when the entry was not there.
///
/// # Errors
/// When `text` is not valid KDL.
pub fn edit_uninstall(text: &str, entry: &str) -> Result<Option<String>> {
    let mut doc = parse(text, WHAT)?;
    let Some(block) = doc.get_mut(BLOCK) else {
        return Ok(None);
    };
    if !remove_child(block, entry) {
        return Ok(None);
    }
    if is_ours_alone(block) {
        drop_block(&mut doc);
    }
    Ok(Some(doc.to_string()))
}

/// Removes the `load_plugins` node, handing the text that preceded it (comments the user had
/// after their last node, which install moved into the node's leading) to what follows. Last in
/// the file, the one newline install put between that text and the block goes too.
fn drop_block(doc: &mut KdlDocument) {
    let Some(at) = doc.nodes().iter().position(|n| n.name().value() == BLOCK) else {
        return;
    };
    let lead = doc
        .nodes_mut()
        .remove(at)
        .leading()
        .unwrap_or_default()
        .to_owned();
    if let Some(next) = doc.nodes_mut().get_mut(at) {
        let joined = format!("{lead}{}", next.leading().unwrap_or_default());
        next.set_leading(joined);
    } else {
        let keep = lead.strip_suffix('\n').unwrap_or(&lead);
        let joined = format!("{keep}{}", doc.trailing().unwrap_or_default());
        doc.set_trailing(joined);
    }
}

/// The block is empty, or is what `defaults_block` made (our note, only default entries).
fn is_ours_alone(block: &KdlNode) -> bool {
    let Some(children) = block.children() else {
        return true;
    };
    let nodes = children.nodes();
    nodes.is_empty()
        || (block.to_string().contains(DEFAULTS_NOTE)
            && nodes
                .iter()
                .all(|n| ZELLIJ_DEFAULTS.contains(&n.name().value())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: &str = "file:/d/warpify/warpify-zellij.wasm";
    const NONE: PluginOptions = PluginOptions {
        new_tab_on_connect: false,
        pin: false,
        title: None,
    };
    const NEW_BLOCK: &str = "load_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify-zellij.wasm\"\n}\n";
    #[test]
    fn appends_a_block_when_there_is_none_and_keeps_the_rest() {
        let base = "// my config\nkeybinds {\n    normal {\n        bind \"Alt h\" { MoveFocus \"Left\"; }  // left\n    }\n}\n\ntheme   \"nord\" /* c */\n// tail\n";
        let out = edit_install(base, E, &NONE).unwrap().unwrap();
        assert!(out.starts_with(base.trim_end_matches("// tail\n")), "{out}");
        assert!(
            out.contains("// tail\n\nload_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify-zellij.wasm\"\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn adds_a_child_to_an_existing_block() {
        let base = "a 1\nload_plugins {\n    // mine\n    \"file:/x.wasm\"\n}\nb 2\n";
        let out = edit_install(base, E, &NONE).unwrap().unwrap();
        assert_eq!(
            out,
            "a 1\nload_plugins {\n    // mine\n    \"file:/x.wasm\"\n    \"file:/d/warpify/warpify-zellij.wasm\"\n}\nb 2\n"
        );
    }

    #[test]
    fn empty_and_inline_blocks_work() {
        for base in [
            "load_plugins {}\n",
            "load_plugins\n",
            "load_plugins { \"file:/x.wasm\"; }\n",
        ] {
            let out = edit_install(base, E, &NONE)
                .unwrap_or_else(|e| panic!("{base:?}: {e}"))
                .unwrap();
            assert!(
                out.contains("\"file:/d/warpify/warpify-zellij.wasm\""),
                "{out}"
            );
            assert!(edit_install(&out, E, &NONE).unwrap().is_none(), "{out}");
            assert!(out.parse::<kdl::KdlDocument>().is_ok());
        }
    }

    #[test]
    fn already_present_is_unchanged_and_second_run_is_a_no_op() {
        let once = edit_install("", E, &NONE).unwrap().unwrap();
        assert_eq!(once, NEW_BLOCK);
        assert!(edit_install(&once, E, &NONE).unwrap().is_none());
    }

    #[test]
    fn file_without_trailing_newline_is_extended_cleanly() {
        let out = edit_install("a 1", E, &NONE).unwrap().unwrap();
        assert_eq!(out, format!("a 1\n\n{NEW_BLOCK}"));
    }

    #[test]
    fn uninstall_reverses_install() {
        let base = "// c\na 1\n";
        let installed = edit_install(base, E, &NONE).unwrap().unwrap();
        let removed = edit_uninstall(&installed, E).unwrap().unwrap();
        assert_eq!(removed.trim_end(), base.trim_end());
        assert!(edit_uninstall(&removed, E).unwrap().is_none());
    }

    #[test]
    fn uninstall_restores_the_original_bytes_exactly() {
        // Inputs that end in a newline (or are empty) come back byte for byte.
        for base in [
            "// c\na 1\n",
            "a 1\n",
            "",
            "// only a comment\n",
            "a 1\n// tail\n",
        ] {
            let installed = edit_install(base, E, &NONE).unwrap().unwrap();
            let removed = edit_uninstall(&installed, E).unwrap().unwrap();
            assert_eq!(removed, base, "{removed:?}");
        }
    }

    #[test]
    fn uninstall_adds_the_missing_final_newline() {
        // KDL needs the last line terminated, so install added a newline and uninstall can't
        // tell it from one the user wrote (a blank line before a block is theirs to keep).
        let installed = edit_install("a 1", E, &NONE).unwrap().unwrap();
        assert_eq!(edit_uninstall(&installed, E).unwrap().unwrap(), "a 1\n");
    }

    #[test]
    fn existing_block_gets_only_our_child_and_keeps_a_user_block_on_uninstall() {
        let mine = "load_plugins {\n    \"zellij:link\"\n}\n";
        let out = edit_install(mine, E, &NONE).unwrap().unwrap();
        assert!(!out.contains(DEFAULTS_NOTE), "{out}");
        assert_eq!(edit_uninstall(&out, E).unwrap().unwrap(), mine);
    }

    #[test]
    fn uninstall_keeps_other_plugins_and_the_block() {
        let base =
            "load_plugins {\n    \"file:/x.wasm\"\n    \"file:/d/warpify/warpify-zellij.wasm\"\n}\n";
        let out = edit_uninstall(base, E).unwrap().unwrap();
        assert_eq!(out, "load_plugins {\n    \"file:/x.wasm\"\n}\n");
    }

    #[test]
    fn marker_rules_decide_what_uninstall_may_delete() {
        let created = mark_created(&edit_install("", E, &NONE).unwrap().unwrap());
        let after = edit_uninstall(&created, E).unwrap().unwrap();
        assert!(only_ours_left(&created, &after), "{after:?}");
        // no marker: the user's file, however empty it ends up
        let theirs = edit_install("", E, &NONE).unwrap().unwrap();
        assert!(!only_ours_left(&theirs, ""));
        // marker, but something else is in the file
        let mixed = format!("{created}theme \"x\"\n");
        let after = edit_uninstall(&mixed, E).unwrap().unwrap();
        assert!(!only_ours_left(&mixed, &after), "{after:?}");
        // marker, but the block holds another plugin
        let shared = created.replace(
            "    \"zellij:link\"",
            "    \"zellij:link\"\n    \"file:/x.wasm\"",
        );
        let after = edit_uninstall(&shared, E).unwrap().unwrap();
        assert!(!only_ours_left(&shared, &after), "{after:?}");
    }

    #[test]
    fn manual_advice_follows_the_config() {
        let f = Path::new("/c/config.kdl");
        let text = |t: &str| Seen::Text(t.to_owned());
        let add = |s: &Seen| manual_advice(f, s, E, &NONE, true, "nix");
        let line = format!("\"{E}\"");
        let with_block = add(&text("load_plugins {\n    \"zellij:link\"\n}\n"));
        assert!(
            with_block.starts_with(&format!(
                "{line}\nadd this line inside your existing load_plugins block"
            )),
            "{with_block}"
        );
        assert!(!with_block.contains("zellij:link"), "{with_block}");
        for s in [Seen::Missing, text("a 1\n")] {
            let out = add(&s);
            assert!(out.starts_with(NEW_BLOCK.trim_end()), "{out}");
            assert!(out.contains("add this block"), "{out}");
        }
        let present = add(&text(NEW_BLOCK));
        assert!(present.contains("nothing to add"), "{present}");
        let broken = add(&text("load_plugins {"));
        assert!(
            broken.starts_with(&line) && broken.contains("not valid KDL"),
            "{broken}"
        );
        let rm = manual_advice(f, &text(NEW_BLOCK), E, &NONE, false, "nix");
        assert_eq!(
            rm,
            format!("remove this line from your load_plugins block: {line}, then run `warpify uninstall zellij` again to delete the plugin file (it's managed outside warpify: nix)")
        );
        assert!(!rm.contains("block itself"), "{rm}");
    }

    #[test]
    fn unreadable_config_gets_the_full_block_and_the_reason() {
        let f = Path::new("/c/config.kdl");
        let seen = Seen::Unreadable("Permission denied".into());
        let add = manual_advice(f, &seen, E, &NONE, true, "it is read-only");
        assert!(add.starts_with(NEW_BLOCK.trim_end()), "{add}");
        assert!(
            add.ends_with(
                "(couldn't read /c/config.kdl: Permission denied; showing the full block)"
            ),
            "{add}"
        );
        let rm = manual_advice(f, &seen, E, &NONE, false, "it is read-only");
        assert!(
            rm.starts_with(
                "if \"file:/d/warpify/warpify-zellij.wasm\" is in your load_plugins block"
            ) && rm.contains("couldn't read /c/config.kdl: Permission denied"),
            "{rm}"
        );
    }

    #[test]
    fn lists_entry_looks_inside_load_plugins_only() {
        assert!(lists_entry(NEW_BLOCK, E));
        assert!(!lists_entry("load_plugins {\n    \"zellij:link\"\n}\n", E));
        assert!(!lists_entry(&format!("other \"{E}\"\n"), E));
        assert!(lists_entry(&format!("load_plugins {{ \"{E}\""), E));
    }

    #[test]
    fn unparsable_config_is_refused() {
        assert!(edit_install("load_plugins {", E, &NONE).is_err());
    }

    const BOTH: PluginOptions = PluginOptions {
        new_tab_on_connect: true,
        pin: true,
        title: None,
    };
    const NEW_TAB_ONLY: PluginOptions = PluginOptions {
        new_tab_on_connect: true,
        pin: false,
        title: None,
    };

    #[test]
    fn options_become_children_of_our_entry_and_zellij_reads_them() {
        let out = edit_install("", E, &BOTH).unwrap().unwrap();
        assert_eq!(
            out,
            "load_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify-zellij.wasm\" {\n        on_connect \"new_tab\"\n        pin \"true\"\n    }\n}\n"
        );
        // zellij's own config parser sees them in the plugin configuration map
        let config = zellij_utils::input::config::Config::from_kdl(&out, None).unwrap();
        let ours = config
            .background_plugins
            .iter()
            .find_map(|p| match p {
                zellij_utils::input::layout::RunPluginOrAlias::RunPlugin(run) => run
                    .location
                    .to_string()
                    .contains("warpify")
                    .then(|| run.configuration.clone()),
                zellij_utils::input::layout::RunPluginOrAlias::Alias(_) => None,
            })
            .expect("our plugin is loaded");
        assert_eq!(
            ours.inner().get("on_connect").map(String::as_str),
            Some("new_tab")
        );
        assert_eq!(ours.inner().get("pin").map(String::as_str), Some("true"));
    }

    #[test]
    fn the_title_options_are_children_and_quoted() {
        let titled = PluginOptions {
            title: Some("🟠 a \"b\" \\c".into()),
            ..NONE
        };
        let out = edit_install("", E, &titled).unwrap().unwrap();
        assert!(out.contains("terminal_title \"true\""), "{out}");
        assert!(
            out.contains("title_prefix \"🟠 a \\\"b\\\" \\\\c\""),
            "{out}"
        );
        let doc = parse(&out, "config").unwrap();
        let ours = doc
            .get("load_plugins")
            .and_then(|b| b.children())
            .and_then(|c| c.get(E))
            .unwrap();
        let value = |name: &str| {
            ours.children()
                .and_then(|c| c.get(name))
                .and_then(|n| n.entries().first())
                .and_then(|e| e.value().as_string().map(str::to_owned))
        };
        assert_eq!(value("title_prefix").as_deref(), Some("🟠 a \"b\" \\c"));
        // the same options again change nothing; dropping them removes both children
        assert_eq!(edit_install(&out, E, &titled).unwrap(), None);
        let back = edit_install(&out, E, &NONE).unwrap().unwrap();
        assert!(
            !back.contains("terminal_title") && !back.contains("title_prefix"),
            "{back}"
        );
        // an empty prefix writes only `title`
        let bare = PluginOptions {
            title: Some(String::new()),
            ..NONE
        };
        let out = edit_install("", E, &bare).unwrap().unwrap();
        assert!(
            out.contains("terminal_title \"true\"") && !out.contains("title_prefix"),
            "{out}"
        );
    }

    #[test]
    fn reinstall_is_idempotent_and_updates_the_options() {
        let first = edit_install("a 1\n", E, &NEW_TAB_ONLY).unwrap().unwrap();
        assert!(first.contains("on_connect \"new_tab\""), "{first}");
        assert!(!first.contains("pin"), "{first}");
        assert!(edit_install(&first, E, &NEW_TAB_ONLY).unwrap().is_none());
        let both = edit_install(&first, E, &BOTH).unwrap().unwrap();
        assert!(both.contains("pin \"true\""), "{both}");
        assert!(edit_install(&both, E, &BOTH).unwrap().is_none());
        let back = edit_install(&both, E, &NONE).unwrap().unwrap();
        assert!(
            !back.contains("on_connect") && !back.contains("pin"),
            "{back}"
        );
        assert!(back.contains(E), "{back}");
        assert!(edit_install(&back, E, &NONE).unwrap().is_none());
    }

    #[test]
    fn options_are_added_to_an_entry_already_in_a_block_and_other_children_stay() {
        let base = format!("load_plugins {{\n    \"{E}\" {{\n        cwd \"/x\"\n    }}\n}}\n");
        let out = edit_install(&base, E, &BOTH).unwrap().unwrap();
        assert!(out.contains("cwd \"/x\""), "{out}");
        assert!(out.contains("on_connect \"new_tab\""), "{out}");
        assert!(out.parse::<kdl::KdlDocument>().is_ok(), "{out}");
        let plain = format!("load_plugins {{\n    \"{E}\"\n    \"file:/y.wasm\"\n}}\n");
        let out = edit_install(&plain, E, &NEW_TAB_ONLY).unwrap().unwrap();
        assert!(out.contains("\"file:/y.wasm\""), "{out}");
        assert!(out.parse::<kdl::KdlDocument>().is_ok(), "{out}");
    }

    #[test]
    fn uninstall_removes_the_entry_with_its_options() {
        let installed = edit_install("a 1\n", E, &BOTH).unwrap().unwrap();
        let removed = edit_uninstall(&installed, E).unwrap().unwrap();
        assert_eq!(removed, "a 1\n");
    }

    #[test]
    fn manual_advice_shows_the_options() {
        let out = manual_advice(
            Path::new("/c/config.kdl"),
            &Seen::Missing,
            E,
            &BOTH,
            true,
            "nix",
        );
        assert!(out.contains("on_connect \"new_tab\""), "{out}");
        let with_block = manual_advice(
            Path::new("/c/config.kdl"),
            &Seen::Text("load_plugins {\n    \"zellij:link\"\n}\n".into()),
            E,
            &BOTH,
            true,
            "nix",
        );
        assert!(
            with_block.starts_with(&format!(
                "\"{E}\" {{ on_connect \"new_tab\"; pin \"true\"; }}\n"
            )),
            "{with_block}"
        );
    }
}
