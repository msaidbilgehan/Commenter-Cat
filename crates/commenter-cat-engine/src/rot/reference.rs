//! Reference-liveness (detector 1) — the highest-value, lowest-false-positive
//! detector (`comment.md` 1).
//!
//! Extracts reference-shaped tokens from a comment — reStructuredText roles
//! (`` :func:`name` ``), backtick spans (`` `name` ``, `` `obj.method()` ``), and
//! plain-text calls (`check_and_create_session()`) — and resolves each against
//! the in-repo [`SymbolIndex`]. An unresolved reference becomes a `rot_ref`
//! finding. This automates the manual grep the dogfooding session ran four times.
//!
//! Extraction is deliberately **conservative** (6-Risks.md R1): a residual
//! false positive is an agent dismissal, so the bias is toward *not* flagging.
//!
//! * An explicit role, a call (`()`), or a dotted path is a strong code shape —
//!   always checked.
//! * A bare single-word backtick (`` `compute` ``) is checked only outside a
//!   doc-contract; inside a docstring a bare backtick word is usually a parameter
//!   or a value (`` `timeout_s` ``, `` `True` ``), not a cross-reference.
//! * Language keywords, log levels, and marker words are never references.
//! * Resolution is generous (a dotted reference resolves on its leaf), so a
//!   defined symbol is never mis-flagged.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::{Category, Finding, Range};

use crate::ops::triage::native_finding;
use crate::rot::intent::CommentIntent;
use crate::rot::symbol_index::SymbolIndex;

/// The stable canonical rule id for reference-liveness findings.
const RULE: &str = "rot_ref";

/// Words that are never an in-repo symbol reference (keywords, builtins, log
/// levels, marker tokens), matched case-insensitively.
const NON_SYMBOL_WORDS: [&str; 30] = [
    "true",
    "false",
    "none",
    "null",
    "nil",
    "self",
    "cls",
    "this",
    "super",
    "new",
    "void",
    "async",
    "await",
    "return",
    "yield",
    "todo",
    "fixme",
    "hack",
    "xxx",
    "bug",
    "note",
    "warning",
    "error",
    "info",
    "debug",
    "trace",
    "critical",
    "fatal",
    "notice",
    "deprecated",
];

/// An extracted reference: the resolvable `name` plus the byte span (within the
/// comment text) the finding anchors to.
struct Reference {
    name: String,
    offset: usize,
    len: usize,
}

/// The `rot_ref` findings for a comment — one per unresolved reference. Returns
/// empty for an intent that makes no checkable reference (a log-level mention).
#[must_use]
pub fn reference_findings(
    comment: &Comment,
    index: &SymbolIndex,
    intent: CommentIntent,
) -> Vec<Finding> {
    if !intent.allows_reference_checks() {
        return Vec::new();
    }
    extract_references(&comment.raw_text, intent)
        .into_iter()
        .filter(|reference| is_checkable(&reference.name) && !index.resolve(&reference.name))
        .map(|reference| {
            native_finding(
                comment,
                reference_range(comment, reference.offset, reference.len),
                Category::ReferenceStale,
                RULE.to_owned(),
                Category::ReferenceStale.canonical_severity(),
                format!(
                    "comment references `{}` but it resolves to no in-repo symbol",
                    reference.name
                ),
            )
        })
        .collect()
}

/// Collects the reference tokens in `text`, deduplicated and ordered by position
/// for deterministic output (Idea §11).
fn extract_references(text: &str, intent: CommentIntent) -> Vec<Reference> {
    let mut refs = Vec::new();
    for span in backtick_spans(text) {
        let role_marked = has_role_before(text, span.open);
        let Some(name) = classify(span.content.trim(), role_marked, intent) else {
            continue;
        };
        refs.push(Reference {
            name,
            offset: span.content_byte,
            len: span.content.len(),
        });
    }
    for call in plaintext_calls(text) {
        refs.push(call);
    }
    refs.sort_by(|a, b| a.offset.cmp(&b.offset).then_with(|| a.name.cmp(&b.name)));
    refs.dedup_by(|a, b| a.offset == b.offset && a.name == b.name);
    refs
}

/// Maps a trimmed backtick content to the name to resolve, or `None` if it is
/// not a checkable reference at this intent.
fn classify(content: &str, role_marked: bool, intent: CommentIntent) -> Option<String> {
    if let Some(open) = content.find('(') {
        let callee = content[..open].trim();
        return is_dotted_ident(callee).then(|| callee.to_owned());
    }
    if is_dotted_ident(content) && content.contains('.') {
        return Some(content.to_owned());
    }
    if is_dotted_ident(content) {
        // A bare single word: a strong reference only when role-marked, or in a
        // comment that is not a doc contract (where it is usually a parameter).
        let bare_ok = role_marked || intent != CommentIntent::DocContract;
        return bare_ok.then(|| content.to_owned());
    }
    None
}

/// A backtick span: where its opening backtick run begins, where its content
/// begins, and the content itself.
struct BacktickSpan<'a> {
    open: usize,
    content_byte: usize,
    content: &'a str,
}

/// Finds the backtick-delimited spans in `text`, honoring single and double
/// (reStructuredText literal) backtick runs.
fn backtick_spans(text: &str) -> Vec<BacktickSpan<'_>> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = i;
        let ticks = run_len(bytes, i);
        i += ticks;
        let content_byte = i;
        let mut content_end = None;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let close = run_len(bytes, i);
                if close >= ticks {
                    content_end = Some(i);
                    i += close;
                    break;
                }
                i += close;
            } else {
                i += 1;
            }
        }
        if let Some(end) = content_end {
            spans.push(BacktickSpan {
                open,
                content_byte,
                content: &text[content_byte..end],
            });
        }
    }
    spans
}

/// The length of the run of `` ` `` starting at `i`.
fn run_len(bytes: &[u8], i: usize) -> usize {
    bytes[i..].iter().take_while(|&&b| b == b'`').count()
}

/// Whether the text immediately before `open` is a reStructuredText role like
/// `:func:` — an explicit "this is code" marker.
fn has_role_before(text: &str, open: usize) -> bool {
    let Some(prefix) = text.get(..open).and_then(|p| p.strip_suffix(':')) else {
        return false;
    };
    let role_len = prefix
        .bytes()
        .rev()
        .take_while(u8::is_ascii_alphabetic)
        .count();
    role_len > 0 && prefix[..prefix.len() - role_len].ends_with(':')
}

/// Finds plain-text (non-backtick) references shaped as a call: an identifier (or
/// dotted path) immediately followed by `(`, kept only when the call is empty
/// (`f()`) or dotted (`a.b()`) and the callee is structured — conservative
/// enough that prose pluralization like `token(s)` is not mistaken for a call.
fn plaintext_calls(text: &str) -> Vec<Reference> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_ident_or_dot(bytes[i]) {
            i += 1;
        }
        let run = &text[start..i];
        let followed_by_paren = bytes.get(i) == Some(&b'(');
        if followed_by_paren && is_dotted_ident(run) && is_structured_name(run) {
            let empty = bytes.get(i + 1) == Some(&b')');
            if empty || run.contains('.') {
                out.push(Reference {
                    name: run.to_owned(),
                    offset: start,
                    len: run.len(),
                });
            }
        }
    }
    out
}

/// Whether `name` is worth resolving — long enough and not a keyword / marker /
/// log-level word.
fn is_checkable(name: &str) -> bool {
    let leaf = name.rsplit('.').next().unwrap_or(name);
    name.len() >= 2
        && !NON_SYMBOL_WORDS
            .iter()
            .any(|word| leaf.eq_ignore_ascii_case(word))
}

/// Whether `name` has the structure of a deliberate symbol (a dot, an
/// underscore, or mixed case) rather than a generic prose word.
fn is_structured_name(name: &str) -> bool {
    name.len() >= 4
        && (name.contains('.')
            || name.contains('_')
            || (name.bytes().any(|b| b.is_ascii_uppercase())
                && name.bytes().any(|b| b.is_ascii_lowercase())))
}

/// Whether `name` is an identifier or a dotted chain of identifiers.
fn is_dotted_ident(name: &str) -> bool {
    !name.is_empty()
        && name
            .split('.')
            .all(|segment| !segment.is_empty() && is_ident(segment))
}

/// Whether `name` is a single identifier (`[A-Za-z_][A-Za-z0-9_]*`).
fn is_ident(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(is_ident_start) && bytes.all(is_ident_continue)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ident_or_dot(b: u8) -> bool {
    is_ident_continue(b) || b == b'.'
}

/// The sub-range of `comment` for a reference at byte `offset` (length `len`)
/// within its text — mirrors `triage::marker_range`.
fn reference_range(comment: &Comment, offset: usize, len: usize) -> Range {
    let (Ok(offset), Ok(len)) = (u32::try_from(offset), u32::try_from(len)) else {
        return comment.range;
    };
    let start = comment.range.start_byte + offset;
    Range::new(
        start,
        start + len,
        comment.range.start_line,
        comment.range.end_line,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::symbol::BoundSymbol;

    fn comment(kind: CommentKind, bound: bool, text: &str) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            kind,
            Range::new(0, u32::try_from(text.len()).unwrap_or(0), 1, 1),
            text,
        );
        if bound {
            c.bound_symbol = Some(BoundSymbol::new("mod.func"));
        }
        c
    }

    /// An index that knows only `compute` and `Service.dispatch`.
    fn index() -> SymbolIndex {
        let py = "def compute():\n    return 1\n\n\nclass Service:\n    def dispatch(self):\n        return 2\n";
        let mut index = SymbolIndex::new();
        index.add_definitions(py, Language::Python, std::path::Path::new("m.py"));
        index
    }

    fn names(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| f.message.clone()).collect()
    }

    #[test]
    fn test_unresolved_backtick_call_is_flagged() {
        // `gone()` is not defined → a rot_ref finding; the message names it.
        let c = comment(CommentKind::Line, false, "// mirrors `gone()` upstream");
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert_eq!(findings.len(), 1, "{:?}", names(&findings));
        assert_eq!(findings[0].canonical_rule_id, RULE);
        assert_eq!(findings[0].category, Category::ReferenceStale);
        assert!(findings[0].message.contains("gone"));
    }

    #[test]
    fn test_resolved_reference_is_not_flagged() {
        // `compute` exists in the index → no finding.
        let c = comment(
            CommentKind::Line,
            false,
            "// see `compute` for the algorithm",
        );
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", names(&findings));
    }

    #[test]
    fn test_bare_backtick_word_flagged_outside_a_doc_contract() {
        // A plain note with a dangling bare reference → flagged.
        let c = comment(CommentKind::Line, false, "// the `vanished` helper is gone");
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert_eq!(findings.len(), 1, "{:?}", names(&findings));
        assert!(findings[0].message.contains("vanished"));
    }

    #[test]
    fn test_bare_backtick_word_ignored_inside_a_doc_contract() {
        // In a docstring a bare backtick word is usually a parameter or a value;
        // `timeout_s` / `True` must not be flagged as dangling symbols.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Waits up to `timeout_s`. Returns `True` on success.",
        );
        let findings = reference_findings(&c, &index(), CommentIntent::DocContract);
        assert!(findings.is_empty(), "{:?}", names(&findings));
    }

    #[test]
    fn test_role_marked_reference_checked_even_in_a_doc_contract() {
        // An explicit `:func:` role is a strong code marker — checked even in a
        // doc contract. `missing_fn` is unknown → flagged.
        let c = comment(
            CommentKind::Docstring,
            true,
            "Use :func:`missing_fn` for the validated path.",
        );
        let findings = reference_findings(&c, &index(), CommentIntent::DocContract);
        assert_eq!(findings.len(), 1, "{:?}", names(&findings));
        assert!(findings[0].message.contains("missing_fn"));
    }

    #[test]
    fn test_plaintext_call_is_checked() {
        // session_manager.py:183 shape — a non-backtick call in prose. `dispatch`
        // resolves; a deleted one would not.
        let c = comment(
            CommentKind::Docstring,
            true,
            "NOTE: prefer check_and_create_session() for safety.",
        );
        let findings = reference_findings(&c, &index(), CommentIntent::DocContract);
        assert_eq!(findings.len(), 1, "{:?}", names(&findings));
        assert!(findings[0].message.contains("check_and_create_session"));
    }

    #[test]
    fn test_pluralization_is_not_a_call() {
        // "token(s)" in prose is not a call — no false positive.
        let c = comment(CommentKind::Line, false, "// caches the token(s) per user");
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", names(&findings));
    }

    #[test]
    fn test_log_level_intent_suppresses_all_checks() {
        // The gate: a log-level mention never produces a reference finding, even
        // if it contains a backtick token.
        let c = comment(CommentKind::Line, false, "// log at `WARNING` level here");
        let findings = reference_findings(&c, &index(), CommentIntent::LogLevelReference);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_keywords_are_never_references() {
        let c = comment(CommentKind::Line, false, "// returns `None` not `True`");
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", names(&findings));
    }

    #[test]
    fn test_findings_are_ordered_by_position() {
        // Two dangling refs → two findings, anchored in source order.
        let c = comment(
            CommentKind::Line,
            false,
            "// `aaa_gone()` then `zzz_gone()`",
        );
        let findings = reference_findings(&c, &index(), CommentIntent::ExplanatoryNote);
        assert_eq!(findings.len(), 2, "{:?}", names(&findings));
        assert!(findings[0].range.start_byte < findings[1].range.start_byte);
        assert!(findings[0].message.contains("aaa_gone"));
    }
}
