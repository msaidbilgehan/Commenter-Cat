//! Reads the *real* signature of a bound function by re-parsing (task 3.2).
//!
//! Tree-sitter trees are discarded after mapping, so this re-parses the source
//! and locates the function node at the comment's `bound_node_range`, then reads
//! its parameter names (skipping `self`/`cls`), whether it returns a value, and
//! the exception types it raises/throws.
//!
//! Return and raise extraction is deliberately **generous** — a return-type
//! annotation, any value-bearing `return`, a `yield`, or an expression-bodied
//! arrow all count as "returns", and raises are collected across the whole body.
//! That keeps the detector conservative: it never claims a documented return or
//! raise is absent unless the code truly has none.

use std::path::Path;

use tree_sitter::{Node, Parser};

use commenter_cat_core::finding::Range;
use commenter_cat_core::lang::Language;

use crate::extract::grammars::grammar_for;
use crate::map::doc_binding::node_text;

/// The real signature read from a function node.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RealSignature {
    /// Declared parameter names, in order (`self`/`cls` removed for Python).
    pub params: Vec<String>,
    /// Whether the function returns a value (annotation, `return x`, `yield`, or
    /// an expression-bodied arrow).
    pub has_return: bool,
    /// Exception type names the body raises/throws (best-effort).
    pub raises: Vec<String>,
}

/// Reads the signature of the function at `node_range`, or `None` when the bound
/// node is not a function (a class or module docstring) or the source cannot be
/// parsed — in which case the detector no-ops (Idea §5, generalized).
#[must_use]
pub fn extract_signature(
    source: &str,
    language: Language,
    path: &Path,
    node_range: Range,
) -> Option<RealSignature> {
    let mut parser = Parser::new();
    parser.set_language(&grammar_for(language, path)).ok()?;
    let tree = parser.parse(source, None)?;
    let start = node_range.start_byte as usize;
    let end = node_range.end_byte as usize;
    let node = tree.root_node().descendant_for_byte_range(start, end)?;
    let func = resolve_function(node)?;
    Some(RealSignature {
        params: parameters(func, source, language),
        has_return: has_return_value(func),
        raises: raised_types(func, source),
    })
}

/// Resolves the bound node to the function it represents: the node itself when it
/// is a function, `None` for a class/module, else the first function nested in a
/// declaration wrapper (a `const f = () => …`).
fn resolve_function(node: Node<'_>) -> Option<Node<'_>> {
    if is_function_kind(node.kind()) {
        return Some(node);
    }
    if matches!(
        node.kind(),
        "class_definition"
            | "class_declaration"
            | "abstract_class_declaration"
            | "module"
            | "program"
    ) {
        return None;
    }
    first_descendant_function(node)
}

/// The first function-like node in `node`'s subtree (pre-order), for a binding
/// that lands on a declaration wrapping an arrow/function expression.
fn first_descendant_function(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let mut stack: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    stack.reverse();
    while let Some(current) = stack.pop() {
        if is_function_kind(current.kind()) {
            return Some(current);
        }
        let mut child_cursor = current.walk();
        let mut children: Vec<Node<'_>> = current.named_children(&mut child_cursor).collect();
        children.reverse();
        stack.extend(children);
    }
    None
}

/// Whether `kind` is a function-like node across the supported grammars.
fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "generator_function"
            | "method_definition"
            | "arrow_function"
            | "function_expression"
            | "function"
    )
}

/// The function's declared parameter names, dropping Python's `self`/`cls`.
fn parameters(func: Node<'_>, source: &str, language: Language) -> Vec<String> {
    let Some(params) = func.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = params.walk();
    for child in params.named_children(&mut cursor) {
        let Some(name) = param_name(child, source) else {
            continue;
        };
        if language == Language::Python && (name == "self" || name == "cls") {
            continue;
        }
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The identifier name of a single parameter node across the grammars, or `None`
/// for a destructuring pattern (handled conservatively as "no simple name").
fn param_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => Some(node_text(node, source)),
        "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => node
            .named_children(&mut node.walk())
            .find(|child| child.kind() == "identifier")
            .map(|child| node_text(child, source)),
        "default_parameter" | "typed_default_parameter" => node
            .child_by_field_name("name")
            .map(|name| node_text(name, source)),
        "required_parameter" | "optional_parameter" => {
            let pattern = node.child_by_field_name("pattern")?;
            (pattern.kind() == "identifier").then(|| node_text(pattern, source))
        }
        _ => None,
    }
}

/// Whether the function returns a value: a return-type annotation, an
/// expression-bodied arrow, or a value-bearing `return`/`yield` in its body.
fn has_return_value(func: Node<'_>) -> bool {
    if func.child_by_field_name("return_type").is_some() {
        return true;
    }
    let Some(body) = func.child_by_field_name("body") else {
        return false;
    };
    if func.kind() == "arrow_function" && body.kind() != "statement_block" {
        return true; // `() => expr` implicitly returns its expression
    }
    body_has(body, |kind, node| {
        (kind == "return_statement" && node.named_child_count() > 0) || kind.contains("yield")
    })
}

/// The exception type names raised/thrown anywhere in the function body.
fn raised_types(func: Node<'_>, source: &str) -> Vec<String> {
    let Some(body) = func.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_raises(body, source, &mut out);
    out
}

/// Walks the body (skipping nested functions) collecting raised type names.
fn collect_raises(body: Node<'_>, source: &str, out: &mut Vec<String>) {
    let mut cursor = body.walk();
    let mut stack: Vec<Node<'_>> = body.named_children(&mut cursor).collect();
    while let Some(node) = stack.pop() {
        if is_function_kind(node.kind()) {
            continue; // a nested function's raises are not this function's
        }
        if matches!(node.kind(), "raise_statement" | "throw_statement") {
            if let Some(name) = raised_type_name(node, source) {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        let mut child_cursor = node.walk();
        stack.extend(node.named_children(&mut child_cursor));
    }
}

/// The type name of a `raise`/`throw` statement's expression (best-effort).
fn raised_type_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let first = node.named_children(&mut cursor).next()?;
    type_name_of(first, source)
}

/// Resolves the type name of a raised expression: a call's callee, a `new`
/// expression's constructor, or a bare identifier / member access.
fn type_name_of(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "type_identifier" => Some(node_text(node, source)),
        "attribute" | "member_expression" => Some(node_text(node, source)),
        "call" | "call_expression" => node
            .child_by_field_name("function")
            .and_then(|callee| type_name_of(callee, source)),
        "new_expression" => node
            .child_by_field_name("constructor")
            .and_then(|ctor| type_name_of(ctor, source)),
        _ => None,
    }
}

/// Whether any node in `body`'s subtree (excluding nested functions) satisfies
/// `predicate`, given the node's kind.
fn body_has(body: Node<'_>, predicate: impl Fn(&str, Node<'_>) -> bool) -> bool {
    let mut cursor = body.walk();
    let mut stack: Vec<Node<'_>> = body.named_children(&mut cursor).collect();
    while let Some(node) = stack.pop() {
        if is_function_kind(node.kind()) {
            continue;
        }
        if predicate(node.kind(), node) {
            return true;
        }
        let mut child_cursor = node.walk();
        stack.extend(node.named_children(&mut child_cursor));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;
    use crate::map::map_comments;

    /// Re-runs the map pipeline and extracts the signature of the first comment
    /// that binds down to a node.
    fn signature_of(source: &str, language: Language, file: &str) -> Option<RealSignature> {
        let path = Path::new(file);
        let mut comments = coalesce(
            source,
            extract_source(source, language, path, file).unwrap(),
        );
        map_comments(source, language, path, &mut comments).unwrap();
        let bound = comments
            .iter()
            .find_map(|comment| comment.bound_node_range)?;
        extract_signature(source, language, path, bound)
    }

    #[test]
    fn test_python_params_skip_self_and_collect_raises() {
        let source = "class C:\n    def acquire(self, timeout_s: float = 1.0) -> bool:\n        \"\"\"Claim a token.\"\"\"\n        if timeout_s < 0:\n            raise ValueError(\"bad\")\n        return True\n";
        let sig = signature_of(source, Language::Python, "m.py").expect("a function signature");
        assert_eq!(sig.params, vec!["timeout_s"], "self is dropped");
        assert!(sig.has_return, "annotated -> bool and returns True");
        assert_eq!(sig.raises, vec!["ValueError"]);
    }

    #[test]
    fn test_python_no_return_no_raise() {
        let source =
            "def store(key, value):\n    \"\"\"Persist a value.\"\"\"\n    cache[key] = value\n";
        let sig = signature_of(source, Language::Python, "m.py").expect("a function signature");
        assert_eq!(sig.params, vec!["key", "value"]);
        assert!(!sig.has_return, "no annotation, no value-bearing return");
        assert!(sig.raises.is_empty());
    }

    #[test]
    fn test_python_star_args() {
        let source = "def f(a, *args, **kwargs):\n    \"\"\"Doc.\"\"\"\n    return a\n";
        let sig = signature_of(source, Language::Python, "m.py").expect("a function signature");
        assert_eq!(sig.params, vec!["a", "args", "kwargs"]);
    }

    #[test]
    fn test_typescript_function_declaration() {
        let source = "/** Sends it. */\nexport function send(recipient: string, retries: number): Promise<void> {\n    if (!recipient) throw new ValidationError('empty');\n    return doSend();\n}\n";
        let sig = signature_of(source, Language::TypeScript, "a.ts").expect("a function signature");
        assert_eq!(sig.params, vec!["recipient", "retries"]);
        assert!(sig.has_return, "annotated Promise<void>");
        assert_eq!(sig.raises, vec!["ValidationError"]);
    }

    #[test]
    fn test_class_docstring_is_not_a_function() {
        let source =
            "class Worker:\n    \"\"\"A worker.\"\"\"\n    def run(self):\n        return 1\n";
        // The class docstring binds to the class node → not a function signature.
        let sig = signature_of(source, Language::Python, "m.py");
        assert!(sig.is_none(), "a class docstring has no signature: {sig:?}");
    }
}
