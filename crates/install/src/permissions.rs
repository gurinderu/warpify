//! zellij's permission cache (`permissions.kdl`): one top-level node per plugin, named by the
//! plugin's path, whose children are the granted permissions.

use std::path::Path;

use kdl::KdlNode;

use crate::kdl_edit::{add_child, append_node, has_child, parse};
use crate::{Error, Result};

fn key(wasm: &Path) -> Result<&str> {
    wasm.to_str()
        .ok_or_else(|| Error::new("the plugin path is not valid UTF-8"))
}

/// Grants `perms` to the plugin at `wasm`: new text, `None` when already granted. Other plugins'
/// entries and existing grants stay.
///
/// # Errors
/// When `text` is not valid KDL (zellij would lose every grant if we rewrote it).
pub fn merge_permissions(text: &str, wasm: &Path, perms: &[&str]) -> Result<Option<String>> {
    let key = key(wasm)?;
    let mut doc = parse(text, "zellij's permissions.kdl")?;
    let mut changed = false;
    if let Some(node) = doc.get_mut(key) {
        for p in perms {
            if !has_child(node, p) {
                add_child(node, p);
                changed = true;
            }
        }
    } else {
        let mut node = KdlNode::new(key);
        for p in perms {
            add_child(&mut node, p);
        }
        node.fmt();
        append_node(&mut doc, node);
        changed = true;
    }
    Ok(changed.then(|| doc.to_string()))
}

/// Drops the plugin's entry: new text, `None` when there was none.
///
/// # Errors
/// When `text` is not valid KDL.
pub fn remove_permissions(text: &str, wasm: &Path) -> Result<Option<String>> {
    let key = key(wasm)?;
    let mut doc = parse(text, "zellij's permissions.kdl")?;
    let before = doc.nodes().len();
    doc.nodes_mut().retain(|n| n.name().value() != key);
    Ok((doc.nodes().len() != before).then(|| doc.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "/d/warpify/warpify-zellij.wasm";
    const P: &[&str] = &["ReadApplicationState", "ReadCliPipes"];

    fn merged(text: &str) -> String {
        merge_permissions(text, Path::new(W), P).unwrap().unwrap()
    }

    #[test]
    fn new_file_gets_one_entry() {
        assert_eq!(
            merged(""),
            "\"/d/warpify/warpify-zellij.wasm\" {\n    ReadApplicationState\n    ReadCliPipes\n}\n"
        );
    }

    #[test]
    fn other_plugins_are_kept_byte_for_byte() {
        let other = "\"/x/other.wasm\" {\n    ChangeApplicationState\n}\n";
        let out = merged(other);
        assert!(out.starts_with(other), "{out}");
        assert!(out.contains("\"/d/warpify/warpify-zellij.wasm\" {"));
    }

    #[test]
    fn existing_grants_are_unioned() {
        let before = "\"/d/warpify/warpify-zellij.wasm\" {\n    ReadCliPipes\n    OpenFiles\n}\n";
        let out = merged(before);
        assert_eq!(
            out,
            "\"/d/warpify/warpify-zellij.wasm\" {\n    ReadCliPipes\n    OpenFiles\n    ReadApplicationState\n}\n"
        );
        assert!(merge_permissions(&out, Path::new(W), P).unwrap().is_none());
    }

    #[test]
    fn unparsable_file_is_refused() {
        let err = merge_permissions("\"/x\" {", Path::new(W), P).unwrap_err();
        assert!(err.to_string().contains("not valid KDL"), "{err}");
        assert!(remove_permissions("\"/x\" {", Path::new(W)).is_err());
    }

    #[test]
    fn remove_drops_only_our_entry() {
        let other = "\"/x/other.wasm\" {\n    ChangeApplicationState\n}\n";
        let out = remove_permissions(&merged(other), Path::new(W))
            .unwrap()
            .unwrap();
        assert_eq!(out, other);
        assert!(remove_permissions(other, Path::new(W)).unwrap().is_none());
    }
}
