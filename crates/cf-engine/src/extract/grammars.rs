//! Pinned tree-sitter grammars (Idea §3, §10).
//!
//! Grammar versions are pinned in `Cargo.toml`; bumps are gated by the
//! extraction tests (Idea §11 golden-file discipline). TypeScript needs two
//! grammars — the plain `.ts` grammar and the TSX grammar — because neither is a
//! strict superset of the other (`<T>cast` vs JSX); the right one is chosen by
//! file extension. JavaScript's grammar already handles JSX.

use std::path::Path;

use tree_sitter::Language as TsLanguage;

use cf_core::lang::Language;

/// Builds the tree-sitter [`TsLanguage`] for a CF language, picking the TSX
/// grammar for `.tsx` files. The grammar `LANGUAGE` constants are
/// `tree_sitter_language::LanguageFn` (a transitive crate); `TsLanguage::new`
/// accepts them directly, so the type is never named here.
fn ts_language(language: Language, is_tsx: bool) -> TsLanguage {
    match language {
        Language::Python => TsLanguage::new(tree_sitter_python::LANGUAGE),
        Language::JavaScript => TsLanguage::new(tree_sitter_javascript::LANGUAGE),
        Language::Shell => TsLanguage::new(tree_sitter_bash::LANGUAGE),
        Language::TypeScript if is_tsx => TsLanguage::new(tree_sitter_typescript::LANGUAGE_TSX),
        Language::TypeScript => TsLanguage::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
    }
}

/// The tree-sitter [`TsLanguage`] for a file, honoring the `.tsx` distinction.
#[must_use]
pub fn grammar_for(language: Language, path: &Path) -> TsLanguage {
    let is_tsx = path.extension().and_then(|ext| ext.to_str()) == Some("tsx");
    ts_language(language, is_tsx)
}

/// The tree-sitter [`TsLanguage`] for a language (non-TSX TypeScript), for
/// source-string extraction without a backing path.
#[must_use]
pub fn grammar(language: Language) -> TsLanguage {
    ts_language(language, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_grammar_loads_and_parses() {
        // A grammar that fails to load panics in `Language::new`; parsing a
        // trivial snippet proves each is wired correctly.
        for language in Language::ALL {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&grammar(language))
                .expect("grammar loads");
            let tree = parser.parse("x", None).expect("parses");
            assert!(!tree.root_node().kind().is_empty());
        }
    }

    #[test]
    fn test_tsx_grammar_selected_by_extension() {
        // The .tsx grammar parses JSX that the plain .ts grammar rejects.
        let jsx = "const e = <div>hi</div>;\n";
        let mut tsx = tree_sitter::Parser::new();
        tsx.set_language(&grammar_for(Language::TypeScript, Path::new("a.tsx")))
            .unwrap();
        assert!(!tsx.parse(jsx, None).unwrap().root_node().has_error());
    }
}
