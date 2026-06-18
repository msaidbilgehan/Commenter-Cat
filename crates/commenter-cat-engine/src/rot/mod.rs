//! Native silent-rot detectors (Idea §3, §9; `comment.md` "How it could be fixed").
//!
//! Commenter-Cat catches *announced* rot (markers) and lint-shaped issues, but is
//! otherwise blind to **silent rot** — a comment that reads like good
//! documentation while making a factual claim about its bound code that is no
//! longer true. These detectors close that gap, reusing the substrate the engine
//! already has: comment→code binding (`bound_symbol` / `bound_node_range`), the
//! git [`BlameIndex`], and the on-device embedder.
//!
//! Each detector is a **pure function over an enriched [`Comment`]**, emitting
//! `origin = Native`, `fix = AgentOnly` findings anchored to the offending span —
//! a token-free shortlist the agent judges, never a verdict (Idea §9). A detector
//! that cannot run (no git repo, embeddings unavailable, no binding) degrades to
//! a no-op; it never manufactures a false finding or panics (Idea §5,
//! generalized). [`rot_pass`] is the convergence point: it runs the shared
//! [`intent`] classifier, then the five detectors in a fixed order, gated by the
//! `[rot]` config, and feeds the semantic detector the `structural_clean` signal.

pub mod drift;
pub mod intent;
pub mod path_ref;
pub mod reference;
pub mod semantic;
pub mod signature;
pub mod symbol_index;

pub use drift::flag_rot_candidates;

use std::collections::BTreeMap;
use std::path::Path;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config::RotConfig;
use commenter_cat_core::finding::Finding;
use commenter_cat_core::lang::Language;

use crate::embed::Embedder;
use crate::git::BlameIndex;
use crate::rot::intent::{classify_intent, CommentIntent};
use crate::rot::path_ref::RepoPaths;
use crate::rot::symbol_index::SymbolIndex;

/// One walked source file's text, for re-parsing in the rot pass (tree-sitter
/// trees are discarded after mapping, so the detectors re-parse).
#[derive(Debug, Clone)]
pub struct FileSource {
    /// Repo-relative, `/`-separated path (matches [`Comment::path`]).
    pub path: String,
    /// The file's language.
    pub language: Language,
    /// The full source text.
    pub text: String,
}

/// Runs the five silent-rot detectors over every comment, returning the native
/// findings per comment (aligned with `comments` by index).
///
/// The detectors run in a **fixed, deterministic order** (Idea §11) — reference,
/// path, git-drift, signature, then semantic last — and each is consulted only
/// when its `[rot]` toggle is on. The semantic detector receives
/// `structural_clean = (no structural detector fired for this comment)`,
/// enforcing the session's gate end to end. The repo-wide [`SymbolIndex`] is
/// built once and shared across all comments.
#[must_use]
pub fn rot_pass(
    comments: &[Comment],
    files: &[FileSource],
    repo_paths: &RepoPaths,
    blames: &BlameIndex,
    embedder: &dyn Embedder,
    config: &RotConfig,
) -> Vec<Vec<Finding>> {
    let context = RotContext {
        symbols: build_symbol_index(comments, files),
        source_by_path: files
            .iter()
            .map(|file| (file.path.as_str(), file.text.as_str()))
            .collect(),
        repo_paths,
        blames,
        embedder,
        config,
    };
    comments
        .iter()
        .map(|comment| context.detect(comment))
        .collect()
}

/// The shared, comment-independent state for one rot pass.
struct RotContext<'a> {
    symbols: SymbolIndex,
    source_by_path: BTreeMap<&'a str, &'a str>,
    repo_paths: &'a RepoPaths,
    blames: &'a BlameIndex,
    embedder: &'a dyn Embedder,
    config: &'a RotConfig,
}

impl RotContext<'_> {
    /// Runs the detector chain for one comment in fixed order, enforcing the
    /// structural-clean gate before the semantic detector.
    fn detect(&self, comment: &Comment) -> Vec<Finding> {
        let intent = classify_intent(comment);
        let source = self
            .source_by_path
            .get(comment.path.as_str())
            .copied()
            .unwrap_or("");
        let config = self.config;
        let mut findings = Vec::new();
        if config.reference_liveness {
            findings.extend(reference::reference_findings(
                comment,
                &self.symbols,
                intent,
            ));
        }
        if config.path_existence {
            findings.extend(path_ref::path_findings(comment, self.repo_paths, intent));
        }
        if config.git_drift {
            findings.extend(drift::git_drift_finding(comment, self.blames, config));
        }
        if config.signature_contract {
            findings.extend(signature::signature_findings(comment, source, intent));
        }
        // Semantic runs last, only on a comment that no structural detector
        // flagged and that makes a claim — the session's hard gate (Idea §9).
        if config.semantic_contradiction
            && matches!(
                intent,
                CommentIntent::DocContract | CommentIntent::ExplanatoryNote
            )
        {
            let structural_clean = findings.is_empty();
            if let Some(finding) = semantic::semantic_candidate(
                comment,
                source,
                self.embedder,
                config,
                structural_clean,
            ) {
                findings.push(finding);
            }
        }
        findings
    }
}

/// Builds the repo-wide symbol index from the comments' bound symbols and every
/// file's definitions.
fn build_symbol_index(comments: &[Comment], files: &[FileSource]) -> SymbolIndex {
    let mut symbols = SymbolIndex::new();
    symbols.add_bound_symbols(comments);
    for file in files {
        symbols.add_definitions(&file.text, file.language, Path::new(&file.path));
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::DeterministicEmbedder;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;
    use crate::map::map_comments;
    use commenter_cat_core::config::ResolvedConfig;

    /// Maps a one-file source and returns its comments plus the `FileSource`.
    fn scenario(source: &str, file: &str) -> (Vec<Comment>, Vec<FileSource>) {
        let path = Path::new(file);
        let mut comments = coalesce(
            source,
            extract_source(source, Language::Python, path, file).unwrap(),
        );
        map_comments(source, Language::Python, path, &mut comments).unwrap();
        let files = vec![FileSource {
            path: file.to_owned(),
            language: Language::Python,
            text: source.to_owned(),
        }];
        (comments, files)
    }

    #[test]
    fn test_rot_pass_flags_dangling_reference_and_spares_accurate() {
        // One comment names a call that does not exist; another is accurate.
        let source = "# refers to gone_fn() upstream\ndef real_fn():\n    return real_fn\n";
        let (comments, files) = scenario(source, "m.py");
        let repo_paths = RepoPaths::from_paths(&["m.py".to_owned()]);
        let blames = BlameIndex::new();
        let findings = rot_pass(
            &comments,
            &files,
            &repo_paths,
            &blames,
            &DeterministicEmbedder,
            &ResolvedConfig::default().rot,
        );
        let all: Vec<&Finding> = findings.iter().flatten().collect();
        assert_eq!(all.len(), 1, "exactly the dangling reference: {all:?}");
        assert_eq!(all[0].canonical_rule_id, "rot_ref");
        assert!(all[0].message.contains("gone_fn"));
    }

    #[test]
    fn test_rot_pass_output_aligns_with_comments() {
        let source = "# a plain note\ndef f():\n    return 1\n";
        let (comments, files) = scenario(source, "m.py");
        let findings = rot_pass(
            &comments,
            &files,
            &RepoPaths::from_paths(&["m.py".to_owned()]),
            &BlameIndex::new(),
            &DeterministicEmbedder,
            &ResolvedConfig::default().rot,
        );
        assert_eq!(
            findings.len(),
            comments.len(),
            "one findings bucket per comment"
        );
    }

    #[test]
    fn test_semantic_default_off_means_no_semantic_findings() {
        // Even a wildly misaligned comment/code pair stays silent by default.
        let source = "# alpha beta gamma delta epsilon\ndef zeta():\n    return omega()\n";
        let (comments, files) = scenario(source, "m.py");
        let findings = rot_pass(
            &comments,
            &files,
            &RepoPaths::from_paths(&["m.py".to_owned()]),
            &BlameIndex::new(),
            &DeterministicEmbedder,
            &ResolvedConfig::default().rot,
        );
        assert!(
            findings
                .iter()
                .flatten()
                .all(|f| f.canonical_rule_id != "rot_semantic"),
            "semantic is default-off"
        );
    }

    #[test]
    fn test_all_toggles_off_yields_no_findings() {
        let source = "# refers to gone_fn() upstream\ndef real_fn():\n    return 1\n";
        let (comments, files) = scenario(source, "m.py");
        let mut config = ResolvedConfig::default().rot;
        config.reference_liveness = false;
        config.path_existence = false;
        config.git_drift = false;
        config.signature_contract = false;
        config.semantic_contradiction = false;
        let findings = rot_pass(
            &comments,
            &files,
            &RepoPaths::from_paths(&["m.py".to_owned()]),
            &BlameIndex::new(),
            &DeterministicEmbedder,
            &config,
        );
        assert!(findings.iter().flatten().next().is_none(), "everything off");
    }
}
