//! Path/identifier existence (detector 3) — `comment.md` 3.
//!
//! A comment that names a file (`account_deletion_requested.html`,
//! `module_email/dispatch.py`) makes a checkable claim: that file should exist.
//! This detector extracts path-shaped tokens and flags any that name no file in
//! the repo, catching the deleted-template / renamed-module class directly.
//!
//! Resolution is against a pre-built [`RepoPaths`] set (the walk already
//! enumerated every non-ignored file), never a fresh filesystem probe — so
//! `SEC_PATH_TRAVERSAL` is satisfied *by construction*: a `..`/absolute token is
//! rejected before any lookup and nothing ever stats an attacker-named path. A
//! **bare** filename resolves by basename (so `foo.html` in `templates/` is not
//! mis-flagged); a **slashed** path resolves by suffix (so a package-relative
//! `module_email/dispatch.py` matches its repo-root path).

use std::collections::HashSet;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::{Category, Finding, Range};

use crate::ops::triage::native_finding;
use crate::rot::intent::CommentIntent;

/// The stable canonical rule id for path-existence findings.
const RULE: &str = "rot_path";

/// Unambiguous file extensions that mark a token as a path. Single-letter
/// extensions (`.c`, `.h`, `.s`, `.r`) are deliberately excluded — they collide
/// with dotted prose like `a.b.c` (6-Risks.md R5).
const PATH_EXTENSIONS: [&str; 27] = [
    "py", "pyi", "ts", "tsx", "js", "jsx", "mjs", "cjs", "sh", "bash", "html", "css", "scss",
    "json", "toml", "yaml", "yml", "md", "sql", "txt", "cfg", "ini", "env", "rs", "go", "rb",
    "vue",
];

/// The repo's file paths, for existence resolution without a filesystem probe.
#[derive(Debug, Default, Clone)]
pub struct RepoPaths {
    relative: HashSet<String>,
    basenames: HashSet<String>,
}

impl RepoPaths {
    /// Builds the set from repo-relative, `/`-separated paths (the walk universe).
    #[must_use]
    pub fn from_paths(paths: &[String]) -> Self {
        let mut relative = HashSet::with_capacity(paths.len());
        let mut basenames = HashSet::with_capacity(paths.len());
        for path in paths {
            relative.insert(path.clone());
            if let Some(base) = path.rsplit('/').next() {
                basenames.insert(base.to_owned());
            }
        }
        Self {
            relative,
            basenames,
        }
    }

    /// Whether `candidate` names a known repo file. A slashed path matches its
    /// full or suffix form; a bare filename matches any file's basename.
    #[must_use]
    pub fn knows(&self, candidate: &str) -> bool {
        if self.relative.contains(candidate) {
            return true;
        }
        if candidate.contains('/') {
            let suffix = format!("/{candidate}");
            self.relative.iter().any(|path| path.ends_with(&suffix))
        } else {
            self.basenames.contains(candidate)
        }
    }
}

/// The `rot_path` findings for a comment — one per named path that no repo file
/// satisfies. Returns empty for an intent that makes no checkable claim, and
/// silently skips traversal/absolute tokens (never a finding, never a probe).
#[must_use]
pub fn path_findings(comment: &Comment, repo: &RepoPaths, intent: CommentIntent) -> Vec<Finding> {
    if !intent.allows_reference_checks() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for (token, offset) in path_candidates(&comment.raw_text) {
        let candidate = token.strip_prefix("./").unwrap_or(&token);
        if is_unsafe(candidate) || repo.knows(candidate) {
            continue;
        }
        let len = token.len();
        findings.push(native_finding(
            comment,
            path_range(comment, offset, len),
            Category::PathMissing,
            RULE.to_owned(),
            Category::PathMissing.canonical_severity(),
            format!("comment names `{token}`, which exists at no path in the repo"),
        ));
    }
    findings
}

/// The path-shaped tokens in `text`, each with its byte offset. A token is a run
/// of path characters that ends in a known file extension.
fn path_candidates(text: &str) -> Vec<(String, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_path_char(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_path_char(bytes[i]) {
            i += 1;
        }
        let run = trim_trailing_punct(&text[start..i]);
        if !run.is_empty() && has_known_extension(run) {
            out.push((run.to_owned(), start));
        }
    }
    out
}

/// Whether `token`'s final `.ext` is one of the recognized file extensions.
fn has_known_extension(token: &str) -> bool {
    match token.rsplit_once('.') {
        Some((_, ext)) => PATH_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known)),
        None => false,
    }
}

/// Whether resolving `path` could escape the repo — an absolute path or one with
/// a `..` component. Such tokens are skipped (never probed, never flagged).
fn is_unsafe(path: &str) -> bool {
    path.is_empty() || path.starts_with('/') || path.split('/').any(|segment| segment == "..")
}

/// Trims trailing sentence punctuation from a path run (`dispatch.py.` →
/// `dispatch.py`) without touching a leading `./` or `../`.
fn trim_trailing_punct(run: &str) -> &str {
    run.trim_end_matches(['.', ',', ';', ':', ')'])
}

/// Whether `b` may appear in a path token (`[A-Za-z0-9_./-]`).
fn is_path_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')
}

/// The sub-range of `comment` for a path token at byte `offset` (length `len`).
fn path_range(comment: &Comment, offset: usize, len: usize) -> Range {
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

    fn comment(text: &str) -> Comment {
        Comment::new(
            "src/a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, u32::try_from(text.len()).unwrap_or(0), 1, 1),
            text,
        )
    }

    fn repo() -> RepoPaths {
        RepoPaths::from_paths(&[
            "backend/module_email/dispatch.py".to_owned(),
            "templates/home.html".to_owned(),
            "src/a.py".to_owned(),
        ])
    }

    fn messages(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| f.message.clone()).collect()
    }

    #[test]
    fn test_missing_path_is_flagged() {
        let c = comment("// see account_deletion_requested.html for the copy");
        let findings = path_findings(&c, &repo(), CommentIntent::Directive);
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert_eq!(findings[0].canonical_rule_id, RULE);
        assert_eq!(findings[0].category, Category::PathMissing);
        assert!(findings[0]
            .message
            .contains("account_deletion_requested.html"));
    }

    #[test]
    fn test_present_bare_filename_resolves_by_basename() {
        // `home.html` lives at templates/home.html → not flagged.
        let c = comment("// renders home.html on success");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_package_relative_slashed_path_resolves_by_suffix() {
        // `module_email/dispatch.py` exists under backend/ → suffix match, no flag.
        let c = comment("// mirror send_password_reset_email in module_email/dispatch.py");
        let findings = path_findings(&c, &repo(), CommentIntent::Directive);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_traversal_token_is_rejected_not_flagged() {
        // SEC_PATH_TRAVERSAL: a `..`-bearing token is skipped — no finding, no probe.
        let c = comment("// reads ../../etc/passwd.txt somehow");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_absolute_path_is_rejected() {
        let c = comment("// writes /etc/hosts.txt");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_dotted_prose_is_not_a_path() {
        // `a.b.c` has a single-letter extension → never a path (R5).
        let c = comment("// the a.b.c chain and the api/v1 section");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_trailing_sentence_punctuation_is_trimmed() {
        // `gone.py.` (end of sentence) → token is `gone.py`, missing → flagged.
        let c = comment("// the helper moved to gone.py.");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert!(findings[0].message.contains("gone.py"));
        assert!(!findings[0].message.contains("gone.py."));
    }

    #[test]
    fn test_log_level_intent_suppresses_path_checks() {
        let c = comment("// logs missing.html at WARNING level");
        let findings = path_findings(&c, &repo(), CommentIntent::LogLevelReference);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_relative_dot_slash_prefix_resolves() {
        // `./home.html` is the current-dir form of a known basename → no flag.
        let c = comment("// open ./home.html");
        let findings = path_findings(&c, &repo(), CommentIntent::ExplanatoryNote);
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }
}
