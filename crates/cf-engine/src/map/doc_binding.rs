//! Symbol naming and doc-comment binding (Idea §3; task 2.4).
//!
//! Produces the `bound_symbol` qualified name and resolves the language-standard
//! doc binding: PEP 257 for Python (a docstring is the first statement of a
//! module / class / function body, so it binds to that scope) and JSDoc/TSDoc
//! adjacency for TS/JS (handled as a lead comment in [`super`], since a JSDoc
//! block is a lead comment immediately above the symbol).

use tree_sitter::Node;

/// The source text of a node.
#[must_use]
pub fn node_text(node: Node<'_>, source: &str) -> String {
    source[node.start_byte()..node.end_byte()].to_owned()
}

/// The declared name of a definition node, or `None` if it is not a definition.
#[must_use]
pub fn definition_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "function_definition"
        | "class_definition"
        | "function_declaration"
        | "generator_function_declaration"
        | "class_declaration"
        | "abstract_class_declaration"
        | "method_definition"
        | "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration" => node
            .child_by_field_name("name")
            .map(|n| node_text(n, source)),
        // `const f = …` / `let g = …` bind to the first declarator's name.
        "lexical_declaration" | "variable_declaration" => first_declarator_name(node, source),
        _ => None,
    }
}

/// The name of the first `variable_declarator` child (for `const`/`let`/`var`).
fn first_declarator_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            return child
                .child_by_field_name("name")
                .map(|name| node_text(name, source));
        }
    }
    None
}

/// The fully-qualified name of the scope at `node`: the dotted chain of
/// enclosing definition names (e.g. `Class.method`), or `<module>` at top level.
#[must_use]
pub fn qualified_name(node: Node<'_>, source: &str) -> String {
    let mut parts = Vec::new();
    let mut current = Some(node);
    while let Some(n) = current {
        if let Some(name) = definition_name(n, source) {
            parts.push(name);
        }
        current = n.parent();
    }
    if parts.is_empty() {
        "<module>".to_owned()
    } else {
        parts.reverse();
        parts.join(".")
    }
}

/// The nearest enclosing scope node (module / class / function) — the PEP 257
/// owner of a Python docstring.
#[must_use]
pub fn nearest_scope(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    loop {
        if matches!(
            current.kind(),
            "module" | "function_definition" | "class_definition"
        ) {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return current,
        }
    }
}

/// Unwraps an `export`/`default` wrapper to the declaration it exports, so a
/// JSDoc above `export function f` still binds to `f`.
#[must_use]
pub fn unwrap_export(node: Node<'_>) -> Node<'_> {
    if node.kind() == "export_statement" {
        if let Some(declaration) = node.child_by_field_name("declaration") {
            return declaration;
        }
        // Fall back to the first non-trivial named child.
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "comment" && !child.kind().is_empty() {
                return child;
            }
        }
    }
    node
}
