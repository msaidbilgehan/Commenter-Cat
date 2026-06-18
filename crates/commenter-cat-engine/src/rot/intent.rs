//! Comment-intent classification — the shared gate for the silent-rot detectors
//! (Idea §3; `comment.md` "classify comment intent first").
//!
//! Before any detector runs, a comment is bucketed by *intent* so each detector
//! fires only where a checkable claim exists. This is the lever the dogfooding
//! session called out as helping both directions: it tells detectors 1/2/5 which
//! comments make checkable claims, and it quiets the `NOTE`/`WARNING` log-level
//! false positives.
//!
//! The classifier is a pure function of a comment's text, kind, and binding — no
//! infrastructure — so it unit-tests in isolation. Priority is deliberate:
//!
//! 1. [`CommentIntent::DocContract`] — a bound doc comment with a contract shape
//!    (a docstring, or `Args:`/`Returns:`/`@param`); it outranks a mid-body
//!    `NOTE:` so the signature and reference detectors still see its claims.
//! 2. [`CommentIntent::Directive`] — a leading marker (`TODO`/`FIXME`/…); marker
//!    triage owns it, but reference/path checks still run (a `TODO` can name a
//!    dangling symbol or a deleted template).
//! 3. [`CommentIntent::LogLevelReference`] — a prose log-level mention ("log at
//!    `WARNING` level"); it makes no code claim, so reference/path stay quiet.
//! 4. [`CommentIntent::ExplanatoryNote`] — everything else.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::kind::CommentKind;

use crate::markers::leads_with_builtin_marker;

/// The coarse intent of a comment — the gate detectors consult before firing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentIntent {
    /// A leading marker line (`TODO`/`FIXME`/`NOTE`/…). Marker triage owns the
    /// marker itself; reference and path checks may still run on its body.
    Directive,
    /// A doc comment bound to a symbol that makes checkable claims (a docstring,
    /// or one carrying `Args:`/`Returns:`/`Raises:`/`@param`). Detectors 1, 2,
    /// and 5 act on it.
    DocContract,
    /// A prose mention of a log level in a logging context ("log at `WARNING`
    /// level"). No code claim — the reference/path detectors skip it.
    LogLevelReference,
    /// A plain explanatory comment. The reference/path detectors act on it; the
    /// signature/semantic detectors do not (it is not a doc contract).
    ExplanatoryNote,
}

impl CommentIntent {
    /// Whether the reference-liveness and path-existence detectors may run on a
    /// comment of this intent — every intent except [`Self::LogLevelReference`].
    #[must_use]
    pub fn allows_reference_checks(self) -> bool {
        !matches!(self, CommentIntent::LogLevelReference)
    }

    /// Whether the docstring↔signature and semantic detectors may run — only on
    /// a [`Self::DocContract`].
    #[must_use]
    pub fn is_doc_contract(self) -> bool {
        matches!(self, CommentIntent::DocContract)
    }
}

/// Classifies a comment's intent (see the module docs for the priority order).
#[must_use]
pub fn classify_intent(comment: &Comment) -> CommentIntent {
    // 1. A bound doc comment with a contract shape is the strongest signal; it
    //    outranks a leading/embedded marker so its claims stay checkable.
    if comment.bound_symbol.is_some()
        && (comment.kind == CommentKind::Docstring || has_contract_section(&comment.raw_text))
    {
        return CommentIntent::DocContract;
    }
    // 2. A leading marker is a directive (already tagged in the pipeline, or
    //    re-derived here so the classifier stands alone).
    if !comment.markers.is_empty() || leads_with_builtin_marker(&comment.raw_text) {
        return CommentIntent::Directive;
    }
    // 3. A log-level mention in a logging context is not a checkable claim.
    if is_log_level_reference(&comment.raw_text) {
        return CommentIntent::LogLevelReference;
    }
    // 4. Everything else is a plain explanatory note.
    CommentIntent::ExplanatoryNote
}

/// Docstring contract-section headers across the supported dialects (Python
/// Google/NumPy + reStructuredText roles + JSDoc/TSDoc) whose presence marks a
/// doc comment as making a checkable signature claim.
const CONTRACT_MARKERS: [&str; 14] = [
    "Args:",
    "Arguments:",
    "Parameters:",
    "Returns:",
    "Return:",
    "Yields:",
    "Raises:",
    "@param",
    "@returns",
    "@return",
    "@throws",
    ":param",
    ":returns:",
    ":raises:",
];

/// Whether `text` carries a recognizable docstring contract section.
fn has_contract_section(text: &str) -> bool {
    CONTRACT_MARKERS.iter().any(|marker| text.contains(marker))
}

/// Log-level words a comment might mention in prose.
const LOG_LEVELS: [&str; 9] = [
    "error", "warn", "warning", "info", "debug", "trace", "critical", "fatal", "notice",
];

/// Logging-context words that, alongside a log level, mark a prose log-level
/// mention rather than a checkable claim.
const LOG_CONTEXT: [&str; 8] = [
    "log", "logs", "logged", "logging", "logger", "emit", "emits", "level",
];

/// Whether `text` reads as a log-level mention — both a log-level word and a
/// logging-context word appear (case-insensitive, whole-word). Conservative on
/// the side of suppression, per the session's false-positive triage.
fn is_log_level_reference(text: &str) -> bool {
    let mut has_level = false;
    let mut has_context = false;
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        has_level |= LOG_LEVELS
            .iter()
            .any(|level| word.eq_ignore_ascii_case(level));
        has_context |= LOG_CONTEXT
            .iter()
            .any(|context| word.eq_ignore_ascii_case(context));
        if has_level && has_context {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::symbol::BoundSymbol;

    /// Builds a comment for classification; `bound` attaches a symbol.
    fn comment(kind: CommentKind, bound: bool, text: &str) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            kind,
            Range::new(0, 10, 1, 1),
            text,
        );
        if bound {
            c.bound_symbol = Some(BoundSymbol::new("mod.func"));
        }
        c
    }

    #[test]
    fn test_leading_marker_is_a_directive() {
        let c = comment(
            CommentKind::Line,
            false,
            "# TODO: swap for send_x once ready",
        );
        assert_eq!(classify_intent(&c), CommentIntent::Directive);
        assert!(
            classify_intent(&c).allows_reference_checks(),
            "a TODO can name a dangling symbol — reference checks still run"
        );
    }

    #[test]
    fn test_docstring_kind_bound_is_a_doc_contract() {
        let c = comment(CommentKind::Docstring, true, "Does a thing.");
        assert_eq!(classify_intent(&c), CommentIntent::DocContract);
        assert!(classify_intent(&c).is_doc_contract());
    }

    #[test]
    fn test_args_section_marks_a_doc_contract_even_on_a_line_comment() {
        let c = comment(
            CommentKind::Line,
            true,
            "Validate context.\n\nArgs:\n    slug: the key\n",
        );
        assert_eq!(classify_intent(&c), CommentIntent::DocContract);
    }

    #[test]
    fn test_doc_contract_outranks_a_mid_body_note_marker() {
        // session_manager.py:183 — a docstring whose body carries a leading
        // `NOTE:` line. It must classify as a doc contract (so the reference
        // detector sees `check_and_create_session()`), not a directive.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Return True if within limit.\n\nNOTE: prefer check_and_create_session().",
        );
        assert_eq!(classify_intent(&c), CommentIntent::DocContract);
    }

    #[test]
    fn test_log_level_mention_is_not_a_checkable_claim() {
        let c = comment(
            CommentKind::Line,
            false,
            "// log at WARNING level when the queue is full",
        );
        assert_eq!(classify_intent(&c), CommentIntent::LogLevelReference);
        assert!(
            !classify_intent(&c).allows_reference_checks(),
            "the reference/path detectors must skip a log-level mention"
        );
    }

    #[test]
    fn test_emits_a_warning_and_a_note_is_a_log_level_reference() {
        // markers.rs already proves this is not a marker; here it is also not a
        // checkable reference — both a context word and a level word appear.
        let c = comment(
            CommentKind::Line,
            false,
            "# emits a WARNING and a NOTE to the operator",
        );
        assert_eq!(classify_intent(&c), CommentIntent::LogLevelReference);
    }

    #[test]
    fn test_plain_prose_is_an_explanatory_note() {
        let c = comment(CommentKind::Line, false, "// explains the retry loop");
        assert_eq!(classify_intent(&c), CommentIntent::ExplanatoryNote);
        assert!(classify_intent(&c).allows_reference_checks());
        assert!(!classify_intent(&c).is_doc_contract());
    }

    #[test]
    fn test_mid_sentence_todo_reference_is_a_note_not_a_directive() {
        // "see the TODO in the parser above" — the TODO is a reference, not a
        // leading marker, and there is no log level, so it is a plain note.
        let c = comment(
            CommentKind::Line,
            false,
            "// see the TODO in the parser above",
        );
        assert_eq!(classify_intent(&c), CommentIntent::ExplanatoryNote);
    }
}

/// Regression tests that lock in the noise-reduction half of the value: the exact
/// comment shapes the dogfooding session triaged by hand must produce **no**
/// detector output, proving the intent gate (and the conservative detectors) hold
/// (`comment.md` "the mirror problem"; plan task 5.3).
#[cfg(test)]
mod regression {
    use super::*;
    use crate::rot::path_ref::{path_findings, RepoPaths};
    use crate::rot::reference::reference_findings;
    use crate::rot::symbol_index::SymbolIndex;
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::symbol::BoundSymbol;
    use std::path::Path;

    fn comment(kind: CommentKind, bound: bool, text: &str) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            kind,
            Range::new(0, 10, 1, 1),
            text,
        );
        if bound {
            c.bound_symbol = Some(BoundSymbol::new("mod.func"));
        }
        c
    }

    /// A symbol index that knows the definitions in `source`.
    fn index_of(source: &str) -> SymbolIndex {
        let mut index = SymbolIndex::new();
        index.add_definitions(source, Language::Python, Path::new("m.py"));
        index
    }

    /// Asserts neither the reference nor the path detector fires on `c`.
    fn assert_silent(c: &Comment, index: &SymbolIndex, paths: &RepoPaths) {
        let intent = classify_intent(c);
        let refs = reference_findings(c, index, intent);
        let path = path_findings(c, paths, intent);
        assert!(
            refs.is_empty() && path.is_empty(),
            "expected silence for {:?}: refs={:?} paths={:?}",
            c.raw_text,
            refs.iter().map(|f| &f.message).collect::<Vec<_>>(),
            path.iter().map(|f| &f.message).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_log_at_warning_level_is_silent() {
        // throttle.py / the canonical mirror-problem case.
        let c = comment(
            CommentKind::Line,
            false,
            "// log at WARNING level when the queue is full",
        );
        assert_eq!(classify_intent(&c), CommentIntent::LogLevelReference);
        assert_silent(&c, &SymbolIndex::new(), &RepoPaths::default());
    }

    #[test]
    fn test_emits_a_warning_and_a_note_is_silent() {
        let c = comment(
            CommentKind::Line,
            false,
            "# emits a WARNING and a NOTE to the operator",
        );
        assert_silent(&c, &SymbolIndex::new(), &RepoPaths::default());
    }

    #[test]
    fn test_see_the_todo_in_the_parser_above_is_silent() {
        // ws/handlers shape — a reference to a TODO elsewhere, not a checkable claim.
        let c = comment(
            CommentKind::Line,
            false,
            "// see the TODO in the parser above",
        );
        assert_silent(&c, &SymbolIndex::new(), &RepoPaths::default());
    }

    #[test]
    fn test_docstring_bare_values_are_silent() {
        // throttle.py:109 — a docstring naming `timeout_s`, `True`, `False` (a
        // parameter and language values) and mentioning WARNING. None are dangling
        // symbols; nothing must fire.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Claim a token, waiting up to `timeout_s`.\n\nReturns `True` on success, `False` on timeout, and logs at WARNING on a Redis outage.",
        );
        assert_eq!(classify_intent(&c), CommentIntent::DocContract);
        assert_silent(&c, &SymbolIndex::new(), &RepoPaths::default());
    }

    #[test]
    fn test_resolved_call_reference_is_silent() {
        // session_manager.py:183 — "prefer check_and_create_session()", which
        // exists, must not be flagged.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Return True if within limit.\n\nNOTE: prefer check_and_create_session() for safety.",
        );
        let index = index_of("def check_and_create_session():\n    return 1\n");
        assert_silent(&c, &index, &RepoPaths::default());
    }

    #[test]
    fn test_existing_path_reference_is_silent() {
        // ws/handlers/__init__.py:20 — names ws/handlers/version.py, which exists.
        let c = comment(
            CommentKind::Line,
            false,
            "# version negotiation now lives in ws/handlers/version.py",
        );
        let paths = RepoPaths::from_paths(&["ws/handlers/version.py".to_owned()]);
        assert_silent(&c, &SymbolIndex::new(), &paths);
    }

    #[test]
    fn test_resolved_func_role_is_silent() {
        // module_email/contracts.py:171 — ":func:`send_campaign_validated`", which
        // exists, must not be flagged even though it is a doc contract.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Use :func:`send_campaign_validated` for the validate-then-dispatch pattern.",
        );
        let index = index_of("def send_campaign_validated():\n    return 1\n");
        assert_silent(&c, &index, &RepoPaths::default());
    }
}
