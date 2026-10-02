//! Format-preserving edits over `kdl` documents: untouched nodes keep their bytes.

use std::fmt::Write as _;

use kdl::{KdlDocument, KdlNode};

use crate::{Error, Result};

pub(crate) fn parse(text: &str, what: &str) -> Result<KdlDocument> {
    text.parse::<KdlDocument>()
        .map_err(|e| Error::new(format!("{what} is not valid KDL, leaving it alone: {e}")))
}

pub(crate) fn has_child(node: &KdlNode, name: &str) -> bool {
    node.children()
        .is_some_and(|c| c.nodes().iter().any(|n| n.name().value() == name))
}

/// Adds a bare child node. A multi-line block gets the decor of its last child; a one-line or
/// empty block is re-laid-out (the parent's own leading/trailing text is kept).
pub(crate) fn add_child(parent: &mut KdlNode, name: &str) {
    let mut child = KdlNode::new(name);
    let doc = parent.ensure_children();
    let multiline = doc.to_string().contains('\n');
    let decor = doc
        .nodes()
        .last()
        .filter(|last| multiline && last.trailing().is_some_and(|t| t.contains('\n')))
        .map(|last| {
            (
                last.leading()
                    .and_then(|l| l.rsplit('\n').next())
                    .unwrap_or_default()
                    .to_owned(),
                last.trailing().unwrap_or_default().to_owned(),
            )
        });
    if let Some((leading, trailing)) = decor {
        child.set_leading(leading);
        child.set_trailing(trailing);
        doc.nodes_mut().push(child);
    } else {
        doc.nodes_mut().push(child);
        let (leading, trailing) = (
            parent.leading().map(str::to_owned),
            parent.trailing().map(str::to_owned),
        );
        parent.fmt();
        parent.set_leading(leading.unwrap_or_default());
        parent.set_trailing(trailing.unwrap_or_default());
    }
}

/// Appends `node` as it is (call `fmt` first for a fresh one) after everything already in `doc`, separated by a blank line.
pub(crate) fn append_node(doc: &mut KdlDocument, mut node: KdlNode) {
    let tail = doc.trailing().unwrap_or_default().to_owned();
    doc.set_trailing("");
    let body = format!("{doc}{tail}");
    let sep = if body.is_empty() {
        ""
    } else if body.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    node.set_leading(format!("{tail}{sep}"));
    doc.nodes_mut().push(node);
}

/// Drops the child named `name`; reports whether there was one.
pub(crate) fn remove_child(parent: &mut KdlNode, name: &str) -> bool {
    let Some(doc) = parent.children_mut().as_mut() else {
        return false;
    };
    let before = doc.nodes().len();
    doc.nodes_mut().retain(|n| n.name().value() != name);
    doc.nodes().len() != before
}

/// Makes the options the children of the child `name` of `parent` (`key "value"` lines): the
/// keys in `owned` become exactly `options`, other children stay as they are. Reports whether
/// anything changed; the node keeps its own leading and trailing text.
pub(crate) fn set_options(
    parent: &mut KdlNode,
    name: &str,
    owned: &[&str],
    options: &[(&str, &str)],
) -> bool {
    let Some(node) = parent.children_mut().as_mut().and_then(|doc| {
        doc.nodes_mut()
            .iter_mut()
            .find(|n| n.name().value() == name)
    }) else {
        return false;
    };
    let value = |n: &KdlNode| {
        n.entries()
            .first()
            .and_then(|e| e.value().as_string())
            .map(str::to_owned)
    };
    let kids: Vec<KdlNode> = node
        .children()
        .map(|d| d.nodes().to_vec())
        .unwrap_or_default();
    let have: Vec<(String, Option<String>)> = kids
        .iter()
        .filter(|n| owned.contains(&n.name().value()))
        .map(|n| (n.name().value().to_owned(), value(n)))
        .collect();
    let want: Vec<(String, Option<String>)> = options
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Some((*v).to_owned())))
        .collect();
    let mut sorted = have.clone();
    sorted.sort();
    let mut wanted = want.clone();
    wanted.sort();
    if sorted == wanted {
        return false;
    }
    let indent = node
        .leading()
        .and_then(|l| l.rsplit('\n').next())
        .unwrap_or_default()
        .to_owned();
    let mut lines: Vec<String> = kids
        .iter()
        .filter(|n| !owned.contains(&n.name().value()))
        .map(|n| n.to_string().trim().to_owned())
        .collect();
    lines.extend(options.iter().map(|(k, v)| format!("{k} \"{v}\"")));
    let mut text = format!("\"{name}\"");
    if !lines.is_empty() {
        text.push_str(" {\n");
        for line in &lines {
            let _ = writeln!(text, "{indent}    {line}");
        }
        let _ = write!(text, "{indent}}}");
    }
    text.push('\n');
    let mut fresh: KdlDocument = text.parse().expect("generated KDL is valid");
    let mut fresh = fresh.nodes_mut().remove(0);
    fresh.set_leading(node.leading().unwrap_or_default());
    fresh.set_trailing(node.trailing().unwrap_or_default());
    *node = fresh;
    true
}
