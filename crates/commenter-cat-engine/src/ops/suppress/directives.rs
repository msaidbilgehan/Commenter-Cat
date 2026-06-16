//! The `commenter-cat:*` directive grammar (Idea §5; task 7.4).
//!
//! Suppression is authoritative at Commenter-Cat's normalization layer — never native
//! directives written into source — so one syntax covers all four tools. A
//! `commenter-cat:*` comment is classified `kind = directive` (never itself a finding
//! target). The target granularity is a dividend of filtering up: a `RULE` may
//! be a `provider_rule_id` (`ruff:D417` or bare `D417`), a `category`
//! (`doc_drift`), an `origin` (`ruff`), or omitted = **all**.

/// A directive's scope (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `commenter-cat:disable-line` — the line the directive sits on.
    DisableLine,
    /// `commenter-cat:disable-next-line` — the following line.
    DisableNextLine,
    /// `commenter-cat:disable` — opens a region (closed by `commenter-cat:enable`, else to EOF).
    Disable,
    /// `commenter-cat:enable` — closes a region.
    Enable,
    /// `commenter-cat:disable-file` — the whole file.
    DisableFile,
}

/// A parsed `commenter-cat:*` directive at a 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The directive scope.
    pub kind: DirectiveKind,
    /// Targets (rule id / category / origin); empty means **all**.
    pub targets: Vec<String>,
    /// The 1-based line the directive sits on.
    pub line: u32,
}

impl Directive {
    /// A human-readable form for `suppressed_by` / audit (`commenter-cat:disable-line=D417`).
    #[must_use]
    pub fn describe(&self) -> String {
        let verb = match self.kind {
            DirectiveKind::DisableLine => "commenter-cat:disable-line",
            DirectiveKind::DisableNextLine => "commenter-cat:disable-next-line",
            DirectiveKind::Disable => "commenter-cat:disable",
            DirectiveKind::Enable => "commenter-cat:enable",
            DirectiveKind::DisableFile => "commenter-cat:disable-file",
        };
        if self.targets.is_empty() {
            verb.to_owned()
        } else {
            format!("{verb}={}", self.targets.join(","))
        }
    }
}

/// Parses a `commenter-cat:*` directive from a comment's text at `line`, or `None` if the
/// comment is not a `commenter-cat:` directive.
#[must_use]
pub fn parse(text: &str, line: u32) -> Option<Directive> {
    let token = commenter_cat_token(text)?;
    let (verb, targets) = match token.split_once('=') {
        Some((verb, raw)) => (verb, parse_targets(raw)),
        None => (token, Vec::new()),
    };
    let kind = match verb {
        "disable-line" => DirectiveKind::DisableLine,
        "disable-next-line" => DirectiveKind::DisableNextLine,
        "disable" => DirectiveKind::Disable,
        "enable" => DirectiveKind::Enable,
        "disable-file" => DirectiveKind::DisableFile,
        _ => return None,
    };
    Some(Directive {
        kind,
        targets,
        line,
    })
}

/// Extracts the directive token after `commenter-cat:`, stopping at whitespace and dropping
/// a trailing block-comment close.
fn commenter_cat_token(text: &str) -> Option<&str> {
    let after = &text[text.find("commenter-cat:")? + "commenter-cat:".len()..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let token = &after[..end];
    Some(token.strip_suffix("*/").unwrap_or(token))
}

/// Splits a comma-separated target list, trimming and dropping empties.
fn parse_targets(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_each_scope() {
        assert_eq!(
            parse("# commenter-cat:disable-line=D417", 3).unwrap().kind,
            DirectiveKind::DisableLine
        );
        assert_eq!(
            parse("// commenter-cat:disable-next-line=jsdoc/x", 3)
                .unwrap()
                .kind,
            DirectiveKind::DisableNextLine
        );
        assert_eq!(
            parse("# commenter-cat:disable=ruff", 3).unwrap().kind,
            DirectiveKind::Disable
        );
        assert_eq!(
            parse("# commenter-cat:enable=ruff", 3).unwrap().kind,
            DirectiveKind::Enable
        );
        assert_eq!(
            parse("# commenter-cat:disable-file", 3).unwrap().kind,
            DirectiveKind::DisableFile
        );
    }

    #[test]
    fn test_targets_and_all() {
        assert_eq!(
            parse("# commenter-cat:disable-line=D417,D100", 1)
                .unwrap()
                .targets,
            vec!["D417", "D100"]
        );
        assert!(
            parse("# commenter-cat:disable-line", 1)
                .unwrap()
                .targets
                .is_empty(),
            "omitted = all"
        );
    }

    #[test]
    fn test_block_comment_close_stripped() {
        assert_eq!(
            parse("/* commenter-cat:disable=D417 */", 1)
                .unwrap()
                .targets,
            vec!["D417"]
        );
    }

    #[test]
    fn test_non_directive_is_none() {
        assert!(parse("# just a comment", 1).is_none());
        assert!(parse("# commenter-cat:unknown-verb", 1).is_none());
    }

    #[test]
    fn test_describe_round_trips() {
        assert_eq!(
            parse("# commenter-cat:disable-line=D417", 1)
                .unwrap()
                .describe(),
            "commenter-cat:disable-line=D417"
        );
        assert_eq!(
            parse("# commenter-cat:disable-file", 1).unwrap().describe(),
            "commenter-cat:disable-file"
        );
    }
}
