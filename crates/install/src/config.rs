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
    fn uninstall_restores_the_original_byte_for_byte_and_drops_our_block() {
        for base in ["// c\na 1\n", "a 1", ""] {
            let installed = edit_install(base, E).unwrap().unwrap();
            let removed = edit_uninstall(&installed, E).unwrap().unwrap();
            assert_eq!(removed.trim_end(), base.trim_end(), "{removed:?}");
        }
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
    fn unparsable_config_is_refused() {
        assert!(edit_install("load_plugins {", E).is_err());
    }
}
