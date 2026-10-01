//! zellij's `config.kdl`: add or drop our entry in the top-level `load_plugins` block.

use kdl::KdlNode;

use crate::kdl_edit::{add_child, append_node, has_child, parse, remove_child};
use crate::Result;

const BLOCK: &str = "load_plugins";
const WHAT: &str = "the zellij config";

/// Adds `entry` (e.g. `file:/abs/warpify.wasm`) to `load_plugins`, creating the block at the
/// end when there is none: new text, `None` when the entry is already there.
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
        let mut block = KdlNode::new(BLOCK);
        add_child(&mut block, entry);
        append_node(&mut doc, block);
    }
    Ok(Some(doc.to_string()))
}

/// Drops `entry` from `load_plugins`, and the block itself when that leaves it empty: new text,
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
    if block.children().is_some_and(|c| c.nodes().is_empty()) {
        doc.nodes_mut().retain(|n| n.name().value() != BLOCK);
    }
    Ok(Some(doc.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: &str = "file:/d/warpify/warpify.wasm";
    #[test]
    fn appends_a_block_when_there_is_none_and_keeps_the_rest() {
        let base = "// my config\nkeybinds {\n    normal {\n        bind \"Alt h\" { MoveFocus \"Left\"; }  // left\n    }\n}\n\ntheme   \"nord\" /* c */\n// tail\n";
        let out = edit_install(base, E).unwrap().unwrap();
        assert!(out.starts_with(base.trim_end_matches("// tail\n")), "{out}");
        assert!(
            out.contains("// tail\n\nload_plugins {\n    \"file:/d/warpify/warpify.wasm\"\n}\n"),
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
        assert_eq!(
            once,
            "load_plugins {\n    \"file:/d/warpify/warpify.wasm\"\n}\n"
        );
        assert!(edit_install(&once, E).unwrap().is_none());
    }

    #[test]
    fn file_without_trailing_newline_is_extended_cleanly() {
        let out = edit_install("a 1", E).unwrap().unwrap();
        assert_eq!(
            out,
            "a 1\n\nload_plugins {\n    \"file:/d/warpify/warpify.wasm\"\n}\n"
        );
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
