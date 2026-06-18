//! Parses the *claimed* contract from a doc comment's text (task 3.1).
//!
//! Pure text → struct, no AST. Supports the three dialects the codebase and its
//! ecosystem use:
//!
//! * **Google** (Python) — `Args:` / `Returns:` / `Raises:` section headers with
//!   indented `name: description` fields.
//! * **JSDoc/TSDoc** — `@param name`, `@returns`, `@throws {Type}`.
//! * **reStructuredText** — `:param name:`, `:returns:`, `:raises Type:`.
//!
//! A comment with no recognizable contract section yields an empty
//! [`DocContract`], and the detector no-ops on it.

/// The structured claims a doc comment makes about a function.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DocContract {
    /// Documented parameter names, in document order, deduplicated.
    pub params: Vec<String>,
    /// Whether the comment documents a return value (a `Returns:` section,
    /// `@returns`, or `:returns:`).
    pub documents_return: bool,
    /// Documented raised/thrown exception type names.
    pub raises: Vec<String>,
}

impl DocContract {
    /// Whether the comment makes no checkable contract claim at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.params.is_empty() && !self.documents_return && self.raises.is_empty()
    }
}

/// The Google-style section a line currently sits under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Outside any field-bearing section.
    None,
    /// An `Args:` / `Parameters:` block — indented fields are parameters.
    Params,
    /// A `Raises:` block — indented fields are exception types.
    Raises,
}

/// Parses the contract claimed by `raw_text` (the comment's full text).
#[must_use]
pub fn parse_doc_contract(raw_text: &str) -> DocContract {
    let mut contract = DocContract::default();
    let mut section = Section::None;
    let mut section_indent = 0;
    for raw_line in raw_text.lines() {
        let indent = leading_whitespace(raw_line);
        let line = doc_line(raw_line);
        if line.is_empty() {
            continue;
        }
        // Inline directives (JSDoc / reST) are recognized at any indent.
        if let Some(name) = jsdoc_param(line).or_else(|| rest_param(line)) {
            push_unique(&mut contract.params, name);
            continue;
        }
        if is_return_directive(line) {
            contract.documents_return = true;
            continue;
        }
        if let Some(exc) = jsdoc_throws(line).or_else(|| rest_raises(line)) {
            push_unique(&mut contract.raises, exc);
            continue;
        }
        // Google section headers (`Args:` etc.).
        if let Some(new_section) = section_header(line) {
            section = new_section.0;
            section_indent = indent;
            if new_section.1 {
                contract.documents_return = true;
            }
            continue;
        }
        // Indented fields under a Google section; a de-indent ends the section.
        match section {
            Section::Params if indent > section_indent => {
                if let Some(name) = google_field_name(line) {
                    push_unique(&mut contract.params, name);
                }
            }
            Section::Raises if indent > section_indent => {
                if let Some(exc) = google_field_name(line) {
                    push_unique(&mut contract.raises, exc);
                }
            }
            _ => section = Section::None,
        }
    }
    contract
}

/// The number of leading whitespace bytes — the indentation of a Google field
/// relative to its section header.
fn leading_whitespace(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Strips a docstring line's leading whitespace and one JSDoc `*` continuation
/// marker, leaving the content.
fn doc_line(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed.strip_prefix('*').unwrap_or(trimmed).trim_start()
}

/// Recognizes a Google section header, returning its [`Section`] and whether it
/// documents a return. `Returns:`/`Yields:` carry no fields but set the flag.
fn section_header(line: &str) -> Option<(Section, bool)> {
    let key = line.trim_end().strip_suffix(':')?;
    match key {
        "Args" | "Arguments" | "Parameters" => Some((Section::Params, false)),
        "Returns" | "Return" | "Yields" | "Yield" => Some((Section::None, true)),
        "Raises" | "Throws" | "Exceptions" => Some((Section::Raises, false)),
        _ => None,
    }
}

/// The field name of an indented Google field line (`name: …` or
/// `name (type): …`), or `None` for a continuation/description line.
fn google_field_name(line: &str) -> Option<&str> {
    let name = first_ident(line)?;
    let after_name = line[name.len()..].trim_start();
    let after_type = match after_name.strip_prefix('(') {
        Some(rest) => &rest[rest.find(')')? + 1..],
        None => after_name,
    };
    after_type.trim_start().starts_with(':').then_some(name)
}

/// The parameter name from a JSDoc `@param [{type}] name` line.
fn jsdoc_param(line: &str) -> Option<&str> {
    let after_tag = line.strip_prefix("@param")?.trim_start();
    let after_type = strip_brace_type(after_tag).trim_start();
    let name_part = after_type.strip_prefix('[').unwrap_or(after_type);
    first_ident(name_part)
}

/// Whether `line` documents a return (`@returns`/`@return`/`:returns:`/`:rtype:`).
fn is_return_directive(line: &str) -> bool {
    const RETURN_DIRECTIVES: [&str; 5] =
        ["@returns", "@return", ":returns:", ":return:", ":rtype:"];
    RETURN_DIRECTIVES
        .iter()
        .any(|directive| line.starts_with(directive))
}

/// The exception type from a JSDoc `@throws {Type}` / `@exception Type` line.
fn jsdoc_throws(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("@throws")
        .or_else(|| line.strip_prefix("@exception"))?
        .trim_start();
    match rest.strip_prefix('{') {
        Some(inner) => {
            let ty = match inner.find('}') {
                Some(close) => &inner[..close],
                None => inner,
            };
            first_ident(ty)
        }
        None => first_ident(rest),
    }
}

/// The parameter name from a reST `:param [type] name:` line.
fn rest_param(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(":param ")?;
    let inner = &rest[..rest.find(':')?];
    inner
        .split_whitespace()
        .last()
        .filter(|word| is_ident(word))
}

/// The exception type from a reST `:raises Type:` / `:raise Type:` line.
fn rest_raises(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix(":raises ")
        .or_else(|| line.strip_prefix(":raise "))?;
    let inner = &rest[..rest.find(':')?];
    inner
        .split_whitespace()
        .last()
        .filter(|word| is_ident(word))
}

/// Drops a leading `{type}` brace group (JSDoc), returning the remainder.
fn strip_brace_type(text: &str) -> &str {
    match text.strip_prefix('{') {
        Some(rest) => rest.find('}').map_or(text, |close| &rest[close + 1..]),
        None => text,
    }
}

/// The leading identifier of `text` (after trimming), or `None` if it does not
/// begin with one.
fn first_ident(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let end = match text.find(|c: char| !is_ident_char(c)) {
        Some(end) => end,
        None => text.len(),
    };
    (end > 0).then_some(&text[..end])
}

/// Whether `name` is a single identifier.
fn is_ident(name: &str) -> bool {
    !name.is_empty() && name.chars().all(is_ident_char)
}

/// Whether `c` may appear in an identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Appends `value` to `target` unless already present (preserves order).
fn push_unique(target: &mut Vec<String>, value: &str) {
    if !target.iter().any(|existing| existing == value) {
        target.push(value.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_google_args_returns_raises() {
        // The contracts.py shape: an Args block, a Raises block, no Returns.
        let doc = "\"\"\"Validate context against the campaign's contract.\n\n    Args:\n        campaign_slug: One of the keys in :data:`CAMPAIGN_CONTRACTS` (or an\n            unregistered slug, which is validated without error but emits a\n            WARNING so operators notice uncatalogued campaigns).\n        context: Template variable mapping to validate.\n\n    Raises:\n        CampaignContractError: One or more required variables are absent.\n    \"\"\"";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["campaign_slug", "context"]);
        assert_eq!(contract.raises, vec!["CampaignContractError"]);
        assert!(!contract.documents_return, "no Returns: section");
    }

    #[test]
    fn test_google_returns_section_sets_flag() {
        let doc = "Summary.\n\n    Args:\n        x: the input\n\n    Returns:\n        The doubled value.\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["x"]);
        assert!(contract.documents_return);
    }

    #[test]
    fn test_continuation_lines_are_not_params() {
        // A description that spills onto an indented continuation line must not
        // be read as a second parameter (would be a false "documented-but-absent").
        let doc = "Summary.\n\n    Args:\n        only: a value whose description wraps onto\n            another indented line entirely.\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["only"]);
    }

    #[test]
    fn test_de_indented_prose_does_not_leak_params() {
        // Prose after the Args block (at base indent) must end the section.
        let doc =
            "Summary.\n\n    Args:\n        x: foo\n\n    This note: is prose, not a param.\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["x"]);
    }

    #[test]
    fn test_param_with_type_annotation() {
        let doc = "Summary.\n\n    Args:\n        count (int): how many\n        name (str): who\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["count", "name"]);
    }

    #[test]
    fn test_jsdoc_contract() {
        let doc = "/**\n * Sends the thing.\n * @param {string} recipient - who to send to\n * @param {number} [retries=3] - attempts\n * @returns {Promise<void>}\n * @throws {ValidationError} when recipient is empty\n */";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["recipient", "retries"]);
        assert!(contract.documents_return);
        assert_eq!(contract.raises, vec!["ValidationError"]);
    }

    #[test]
    fn test_rest_contract() {
        let doc = ":param str campaign_slug: the slug\n:param context: the mapping\n:returns: nothing\n:raises CampaignContractError: when invalid\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["campaign_slug", "context"]);
        assert!(contract.documents_return);
        assert_eq!(contract.raises, vec!["CampaignContractError"]);
    }

    #[test]
    fn test_prose_only_docstring_is_empty() {
        let doc = "\"\"\"Attempt to claim one token, waiting up to `timeout_s`.\n\n    Returns `True` on success and logs at WARNING on a Redis outage.\n    \"\"\"";
        let contract = parse_doc_contract(doc);
        assert!(
            contract.is_empty(),
            "prose `Returns` (no section header) makes no checkable claim: {contract:?}"
        );
    }

    #[test]
    fn test_duplicate_params_deduplicated() {
        let doc = "@param x first\n@param x again\n";
        let contract = parse_doc_contract(doc);
        assert_eq!(contract.params, vec!["x"]);
    }
}
