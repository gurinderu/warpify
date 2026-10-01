//! zellij's `config.kdl`: add or drop our entry in the top-level `load_plugins` block.

use kdl::{KdlDocument, KdlNode};

use crate::kdl_edit::{add_child, append_node, has_child, parse, remove_child};
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

/// What to tell a user whose config warpify must not edit: `text` is the file (`None`: there is
/// none), `why` is why it is not editable. Adding to an existing `load_plugins` block is one line;
/// without a block it is the whole block, zellij's defaults included.
pub(crate) fn manual_advice(text: Option<&str>, entry: &str, adding: bool, why: &str) -> String {
    let line = format!("\"{entry}\"");
    let tail = format!("(it's managed outside warpify: {why})");
    if !adding {
        return format!("remove this line from your load_plugins block: {line} {tail}");
    }
    let doc = match text.map(|t| parse(t, WHAT)) {
        Some(Ok(doc)) => doc,
        Some(Err(e)) => {
            return format!("{line}\nadd this line inside your load_plugins block ({e}) {tail}")
        }
        None => KdlDocument::new(),
    };
    match doc.get(BLOCK) {
        Some(block) if has_child(block, entry) => {
            format!("{line}\nalready in your load_plugins block, nothing to add {tail}")
        }
        Some(_) => {
            format!("{line}\nadd this line inside your existing load_plugins block {tail}")
        }
        None => {
            let block = edit_install("", entry).ok().flatten().unwrap_or_default();
            format!(
                "{}\nadd this block to your zellij config (load_plugins replaces zellij's defaults, so they are in it) {tail}",
                block.trim_end()
            )
        }
    }
}

/// Adds `entry` (e.g. `file:/abs/warpify.wasm`) to `load_plugins`. A block that exists only gets
/// our child; when there is none it is created at the end with zellij's default entries too: new text, `None` when the entry is already there.
///
/// # Errors
/// When `text` is not valid KDL.
pub fn edit_install(text: &str, entry: &str) -> Result<Option<String>> {
    let mut doc = parse(text, WHAT)?;
    if let Some(block) = doc.get_mut(BLOCK) {
        if has_child(block, entry) {
            return Ok(None);
        }
        add_child(block, entry);
    } else {
        let mut block = defaults_block();
        add_child(&mut block, entry);
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
        doc.nodes_mut().retain(|n| n.name().value() != BLOCK);
    }
    Ok(Some(doc.to_string()))
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

    const E: &str = "file:/d/warpify/warpify.wasm";
    const NEW_BLOCK: &str = "load_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify.wasm\"\n}\n";
    #[test]
    fn appends_a_block_when_there_is_none_and_keeps_the_rest() {
        let base = "// my config\nkeybinds {\n    normal {\n        bind \"Alt h\" { MoveFocus \"Left\"; }  // left\n    }\n}\n\ntheme   \"nord\" /* c */\n// tail\n";
        let out = edit_install(base, E).unwrap().unwrap();
        assert!(out.starts_with(base.trim_end_matches("// tail\n")), "{out}");
        assert!(
            out.contains("// tail\n\nload_plugins {\n    // zellij defaults kept: load_plugins replaces them\n    \"zellij:link\"\n    \"file:/d/warpify/warpify.wasm\"\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn adds_a_child_to_an_existing_block() {
        let base = "a 1\nload_plugins {\n    // mine\n    \"file:/x.wasm\"\n}\nb 2\n";
        let out = edit_install(base, E).unwrap().unwrap();
        assert_eq!(
            out,
            "a 1\nload_plugins {\n    // mine\n    \"file:/x.wasm\"\n    \"file:/d/warpify/warpify.wasm\"\n}\nb 2\n"
        );
    }

    #[test]
    fn empty_and_inline_blocks_work() {
        for base in [
            "load_plugins {}\n",
            "load_plugins\n",
            "load_plugins { \"file:/x.wasm\"; }\n",
        ] {
            let out = edit_install(base, E)
                .unwrap_or_else(|e| panic!("{base:?}: {e}"))
                .unwrap();
            assert!(out.contains("\"file:/d/warpify/warpify.wasm\""), "{out}");
            assert!(edit_install(&out, E).unwrap().is_none(), "{out}");
            assert!(out.parse::<kdl::KdlDocument>().is_ok());
        }
    }

    #[test]
    fn already_present_is_unchanged_and_second_run_is_a_no_op() {
        let once = edit_install("", E).unwrap().unwrap();
        assert_eq!(once, NEW_BLOCK);
        assert!(edit_install(&once, E).unwrap().is_none());
    }

    #[test]
    fn file_without_trailing_newline_is_extended_cleanly() {
        let out = edit_install("a 1", E).unwrap().unwrap();
        assert_eq!(out, format!("a 1\n\n{NEW_BLOCK}"));
    }

    #[test]
    fn uninstall_reverses_install() {
        let base = "// c\na 1\n";
        let installed = edit_install(base, E).unwrap().unwrap();
        let removed = edit_uninstall(&installed, E).unwrap().unwrap();
        assert_eq!(removed.trim_end(), base.trim_end());
        assert!(edit_uninstall(&removed, E).unwrap().is_none());
    }

    #[test]
    fn uninstall_restores_the_original_bytes_exactly_and_drops_our_block() {
        for base in ["// c\na 1\n", "a 1\n", ""] {
            let installed = edit_install(base, E).unwrap().unwrap();
            let removed = edit_uninstall(&installed, E).unwrap().unwrap();
            assert_eq!(removed, base, "{removed:?}");
        }
        // The one inexact case: KDL needs the last line terminated, so install added a newline
        // and uninstall can't tell it from one the user wrote (a blank line before a block is
        // theirs to keep).
        let installed = edit_install("a 1", E).unwrap().unwrap();
        assert_eq!(edit_uninstall(&installed, E).unwrap().unwrap(), "a 1\n");
        assert_eq!(
            edit_uninstall(&edit_install("", E).unwrap().unwrap(), E).unwrap(),
            Some(String::new())
        );
    }

    #[test]
    fn existing_block_gets_only_our_child_and_keeps_a_user_block_on_uninstall() {
        let mine = "load_plugins {\n    \"zellij:link\"\n}\n";
        let out = edit_install(mine, E).unwrap().unwrap();
        assert!(!out.contains(DEFAULTS_NOTE), "{out}");
        assert_eq!(edit_uninstall(&out, E).unwrap().unwrap(), mine);
    }

    #[test]
    fn uninstall_keeps_other_plugins_and_the_block() {
        let base =
            "load_plugins {\n    \"file:/x.wasm\"\n    \"file:/d/warpify/warpify.wasm\"\n}\n";
        let out = edit_uninstall(base, E).unwrap().unwrap();
        assert_eq!(out, "load_plugins {\n    \"file:/x.wasm\"\n}\n");
    }

    #[test]
    fn marker_rules_decide_what_uninstall_may_delete() {
        let created = mark_created(&edit_install("", E).unwrap().unwrap());
        let after = edit_uninstall(&created, E).unwrap().unwrap();
        assert!(only_ours_left(&created, &after), "{after:?}");
        // no marker: the user's file, however empty it ends up
        let theirs = edit_install("", E).unwrap().unwrap();
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
        let add = |t: Option<&str>| manual_advice(t, E, true, "nix");
        let line = format!("\"{E}\"");
        let with_block = add(Some("load_plugins {\n    \"zellij:link\"\n}\n"));
        assert!(
            with_block.starts_with(&format!(
                "{line}\nadd this line inside your existing load_plugins block"
            )),
            "{with_block}"
        );
        assert!(!with_block.contains("zellij:link"), "{with_block}");
        for t in [None, Some("a 1\n")] {
            let out = add(t);
            assert!(out.starts_with(NEW_BLOCK.trim_end()), "{out}");
            assert!(out.contains("add this block"), "{out}");
        }
        let present = add(Some(NEW_BLOCK));
        assert!(present.contains("nothing to add"), "{present}");
        let broken = add(Some("load_plugins {"));
        assert!(
            broken.starts_with(&line) && broken.contains("not valid KDL"),
            "{broken}"
        );
        let rm = manual_advice(Some(NEW_BLOCK), E, false, "nix");
        assert_eq!(
            rm,
            format!("remove this line from your load_plugins block: {line} (it's managed outside warpify: nix)")
        );
        assert!(!rm.contains("block itself"), "{rm}");
    }

    #[test]
    fn unparsable_config_is_refused() {
        assert!(edit_install("load_plugins {", E).is_err());
    }
}
