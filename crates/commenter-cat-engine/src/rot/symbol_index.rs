//! A lightweight in-repo symbol index for reference-liveness (detector 1).
//!
//! Resolves a reference token (a backtick span, a `:func:` role, a "see X"
//! target) against the symbols the repo actually defines. The index is the union
//! of two cheap sources:
//!
//! * every comment's `bound_symbol` (the qualified path mapping already produced);
//! * every definition name found by re-parsing each file (tree-sitter trees are
//!   discarded after mapping, so the index re-parses — [`crate::map::doc_binding`]
//!   supplies `definition_name` / `qualified_name`).
//!
//! Both the **leaf** name (`compute`) and the **qualified** name (`Class.method`)
//! are indexed, so a `` `compute` `` backtick and a `Class.method` reference both
//! resolve. A dotted reference also resolves on its leaf segment, so a
//! module-path reference (`module_email.dispatch.send_x`) resolves when `send_x`
//! is defined anywhere — resolution is deliberately *generous* (it suppresses a
//! finding) so a genuinely-defined symbol is never mis-flagged as dangling
//! (6-Risks.md R1).

use std::collections::HashSet;
use std::path::Path;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::lang::Language;

use crate::extract::grammars::grammar_for;
use crate::map::doc_binding::{definition_name, qualified_name};

/// The sentinel `bound_symbol` for a top-level (unscoped) comment — not a real
/// definition, so it is never indexed.
const MODULE_SCOPE: &str = "<module>";

/// A resolve-only set of the symbol names the repo defines.
#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    names: HashSet<String>,
}

impl SymbolIndex {
    /// An empty index. Populate it with [`Self::add_bound_symbols`] and
    /// [`Self::add_definitions`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the qualified path and leaf of every comment's `bound_symbol`.
    pub fn add_bound_symbols(&mut self, comments: &[Comment]) {
        for comment in comments {
            let Some(symbol) = &comment.bound_symbol else {
                continue;
            };
            let qualified = symbol.as_str();
            if qualified == MODULE_SCOPE {
                continue;
            }
            self.insert_name_and_leaf(qualified);
        }
    }

    /// Re-parses `source` and adds every definition's leaf and qualified name.
    ///
    /// A grammar load or parse failure degrades to "no definitions from this
    /// file" — never an error or a panic (Idea §5, generalized).
    pub fn add_definitions(&mut self, source: &str, language: Language, path: &Path) {
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&grammar_for(language, path)).is_err() {
            return;
        }
        let Some(tree) = parser.parse(source, None) else {
            return;
        };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if let Some(leaf) = definition_name(node, source) {
                self.names.insert(leaf);
                self.names.insert(qualified_name(node, source));
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }

    /// Whether `name` names a symbol the repo defines. A dotted reference
    /// resolves on its leaf segment, so `module.sub.func` resolves when `func`
    /// is defined anywhere.
    #[must_use]
    pub fn resolve(&self, name: &str) -> bool {
        if self.names.contains(name) {
            return true;
        }
        match name.rsplit('.').next() {
            Some(leaf) if leaf != name => self.names.contains(leaf),
            _ => false,
        }
    }

    /// The number of indexed names — for tests and diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the index holds no names.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Inserts a qualified name plus its leaf segment.
    fn insert_name_and_leaf(&mut self, qualified: &str) {
        self.names.insert(qualified.to_owned());
        if let Some(leaf) = qualified.rsplit('.').next() {
            self.names.insert(leaf.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::symbol::BoundSymbol;

    const PY: &str =
        "def compute():\n    return 1\n\n\nclass Worker:\n    def run(self):\n        return 2\n";

    fn index_of(source: &str, lang: Language, path: &str) -> SymbolIndex {
        let mut index = SymbolIndex::new();
        index.add_definitions(source, lang, Path::new(path));
        index
    }

    #[test]
    fn test_resolves_definition_leaf_and_qualified() {
        let index = index_of(PY, Language::Python, "m.py");
        assert!(index.resolve("compute"), "top-level function leaf");
        assert!(index.resolve("Worker"), "class leaf");
        assert!(index.resolve("run"), "method leaf");
        assert!(index.resolve("Worker.run"), "qualified method name");
        assert!(!index.resolve("nonexistent"));
    }

    #[test]
    fn test_dotted_reference_resolves_on_its_leaf() {
        let index = index_of(PY, Language::Python, "m.py");
        // A module-path reference resolves because its leaf is a known def.
        assert!(index.resolve("pkg.module.compute"));
        assert!(!index.resolve("pkg.module.absent"));
    }

    #[test]
    fn test_typescript_definitions_indexed() {
        let ts = "export function send(x: number) { return x; }\nconst helper = () => 1;\n";
        let index = index_of(ts, Language::TypeScript, "a.ts");
        assert!(index.resolve("send"), "exported function");
        assert!(index.resolve("helper"), "const arrow binding");
    }

    #[test]
    fn test_bound_symbols_contribute_names() {
        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Docstring,
            Range::new(0, 5, 1, 1),
            "doc",
        );
        comment.bound_symbol = Some(BoundSymbol::new("pkg.Service.dispatch"));
        let mut index = SymbolIndex::new();
        index.add_bound_symbols(&[comment]);
        assert!(index.resolve("dispatch"), "leaf of the bound symbol");
        assert!(index.resolve("pkg.Service.dispatch"), "full qualified path");
        assert!(
            !index.resolve("Service"),
            "intermediate segment not indexed"
        );
    }

    #[test]
    fn test_module_scope_sentinel_is_not_indexed() {
        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 5, 1, 1),
            "c",
        );
        comment.bound_symbol = Some(BoundSymbol::new("<module>"));
        let mut index = SymbolIndex::new();
        index.add_bound_symbols(&[comment]);
        assert!(index.is_empty(), "the <module> sentinel adds nothing");
    }
}
