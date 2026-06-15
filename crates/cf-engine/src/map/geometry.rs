//! Line geometry and sibling navigation for mapping (Idea §3; task 2.4).
//!
//! The non-doc binding rules are pure geometry — no scoring heuristic (Idea §3):
//! a lead comment binds *down* to the next sibling on the very next line; a
//! trailing comment binds to the code on its own line. The residual rule is
//! blank-line adjacency, captured by [`binds_down`].

use tree_sitter::Node;

use cf_core::comment::Comment;

pub use crate::extract::coalesce::is_own_line;

/// The 1-based start line of a node.
#[must_use]
pub fn start_line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

/// Whether `node_start_line` is the line immediately below the comment, i.e. the
/// lead-comment "no blank line between" residual rule (Idea §3).
#[must_use]
pub fn binds_down(comment: &Comment, node_start_line: u32) -> bool {
    node_start_line == comment.range.end_line + 1
}

/// The next named, non-comment sibling of `container` starting at or after
/// `after_byte` — the statement a lead comment binds down to.
#[must_use]
pub fn next_code_sibling<'tree>(container: Node<'tree>, after_byte: u32) -> Option<Node<'tree>> {
    let mut cursor = container.walk();
    let mut found = None;
    for child in container.named_children(&mut cursor) {
        if child.kind() != "comment" && child.start_byte() as u32 >= after_byte {
            found = Some(child);
            break;
        }
    }
    found
}

/// The code node a trailing comment annotates: a same-line statement before the
/// comment, or the container itself when it is that statement.
#[must_use]
pub fn trailing_target<'tree>(
    container: Node<'tree>,
    comment_line: u32,
    before_byte: u32,
) -> Option<Node<'tree>> {
    if let Some(found) = same_line_before(container, comment_line, before_byte) {
        return Some(found);
    }
    // The comment may be attached *inside* the statement it trails.
    let is_statement = container.kind() != "module" && !container.kind().ends_with("block");
    if is_statement
        && start_line(container) == comment_line
        && (container.start_byte() as u32) < before_byte
    {
        return Some(container);
    }
    None
}

/// The closest named, non-comment child of `container` that starts on
/// `comment_line` before `before_byte`.
fn same_line_before<'tree>(
    container: Node<'tree>,
    comment_line: u32,
    before_byte: u32,
) -> Option<Node<'tree>> {
    let mut cursor = container.walk();
    let mut best: Option<Node<'tree>> = None;
    for child in container.named_children(&mut cursor) {
        let eligible = child.kind() != "comment"
            && start_line(child) == comment_line
            && (child.start_byte() as u32) < before_byte;
        if eligible && best.is_none_or(|b| child.start_byte() > b.start_byte()) {
            best = Some(child);
        }
    }
    best
}
