//! The canonical normalized [`Finding`] (Idea §5).
//!
//! Every provider adapter and every native check reconciles its output to this
//! one schema, so the agent sees one report across four languages. The
//! submodules build the machinery the schema depends on:
//!
//! * [`category`] — the cross-language [`Category`] and its severity anchor.
//! * [`severity`] — the three-tier severity resolver.
//! * [`coordinates`] — provider coordinate → engine byte-offset conversion.
//! * [`dedup`] — same-category overlap merge + canonical ordering.
//!
//! ## Two fields beyond the Idea §5 block, required by §5 itself
//!
//! * `file` — the §5 canonical sort key is `(file, line, col, provider_rule_id)`
//!   and baselines/diffs list findings across files, so a finding must carry its
//!   repo-relative path to be self-contained and sortable.
//! * `also_from` — §5 dedup unions the *origins* of merged same-category
//!   findings; the schema must be able to represent that union without
//!   discarding the primary's lossless `provider_rule_id`.

pub mod category;
pub mod coordinates;
pub mod dedup;
pub mod severity;

use std::collections::BTreeSet;
use std::fmt;

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::severity::Severity;
use crate::symbol::{BoundSymbol, CommentId};

pub use category::Category;
pub use coordinates::{ColumnUnit, CoordinateSystem};
pub use dedup::dedup_findings;
pub use severity::{per_tool_severity, resolve_severity};

/// Which detector produced a finding (Idea §5).
///
/// The five built-ins are named variants; [`Origin::Other`] carries any
/// third-party manifest provider's id (the manifest platform is in scope, Idea
/// §5), so a finding's provenance is never lost. Serializes as a bare string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// `ruff` (Python).
    Ruff,
    /// `eslint` + jsdoc/tsdoc (JS/TS).
    Eslint,
    /// `shellcheck` (Shell).
    Shellcheck,
    /// `gitleaks` (secrets).
    Gitleaks,
    /// A CF-native check (blame-skew, marker triage).
    Native,
    /// A third-party manifest provider, by its declared id.
    Other(String),
}

impl Origin {
    /// The lowercase origin token (the inner id for [`Origin::Other`]).
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Origin::Ruff => "ruff",
            Origin::Eslint => "eslint",
            Origin::Shellcheck => "shellcheck",
            Origin::Gitleaks => "gitleaks",
            Origin::Native => "native",
            Origin::Other(id) => id.as_str(),
        }
    }

    /// Parses an origin token, mapping any unknown id to [`Origin::Other`]
    /// (infallible — third-party providers are open-ended).
    #[must_use]
    pub fn from_token(token: &str) -> Origin {
        match token {
            "ruff" => Origin::Ruff,
            "eslint" => Origin::Eslint,
            "shellcheck" => Origin::Shellcheck,
            "gitleaks" => Origin::Gitleaks,
            "native" => Origin::Native,
            other => Origin::Other(other.to_owned()),
        }
    }

    /// Whether this is the CF-native origin.
    #[must_use]
    pub const fn is_native(&self) -> bool {
        matches!(self, Origin::Native)
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Origin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Origin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        Ok(Origin::from_token(&token))
    }
}

/// How a finding can be fixed (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    /// The provider can autofix via its own `--fix` (CF delegates, Idea §5).
    ProviderAutofix,
    /// Only an agent can resolve it (judgment required).
    AgentOnly,
    /// No fix is offered.
    None,
}

impl Fix {
    /// The snake_case token (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Fix::ProviderAutofix => "provider_autofix",
            Fix::AgentOnly => "agent_only",
            Fix::None => "none",
        }
    }

    /// Parses a fix token, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Fix> {
        match token {
            "provider_autofix" => Some(Fix::ProviderAutofix),
            "agent_only" => Some(Fix::AgentOnly),
            "none" => Some(Fix::None),
            _ => None,
        }
    }
}

/// The engine's canonical location: a 0-based half-open byte span `[start_byte,
/// end_byte)` plus the 1-based inclusive line span it covers (Idea §5).
///
/// Byte offsets are `u32` (matching tree-sitter and bounding a single source
/// file to 4 GiB) so the serialized form is identical on every platform —
/// load-bearing for reproducibility (Idea §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Range {
    /// Inclusive start byte (0-based).
    pub start_byte: u32,
    /// Exclusive end byte (0-based).
    pub end_byte: u32,
    /// Inclusive start line (1-based).
    pub start_line: u32,
    /// Inclusive end line (1-based).
    pub end_line: u32,
}

impl Range {
    /// Builds a range. The byte span is half-open; the line span inclusive.
    #[must_use]
    pub const fn new(start_byte: u32, end_byte: u32, start_line: u32, end_line: u32) -> Self {
        Self {
            start_byte,
            end_byte,
            start_line,
            end_line,
        }
    }

    /// Whether this range's byte span intersects `other`'s (Idea §5 dedup).
    #[must_use]
    pub const fn overlaps(&self, other: &Range) -> bool {
        self.start_byte < other.end_byte && other.start_byte < self.end_byte
    }

    /// The byte length of the span.
    #[must_use]
    pub const fn len_bytes(&self) -> u32 {
        self.end_byte.saturating_sub(self.start_byte)
    }
}

/// What a finding is about (Idea §5): a comment, or the symbol that lacks one
/// (e.g. a `doc_missing` finding targets the undocumented symbol).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingTarget {
    /// A specific comment record.
    Comment(CommentId),
    /// A bound symbol (used when no comment exists to attach to).
    Symbol(BoundSymbol),
}

/// One normalized finding — the schema every provider and native check produces
/// (Idea §5). All fields are public: it is a data record, constructed by struct
/// literal so each field is named at the call site (no positional confusion).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Repo-relative path of the file the finding is in (canonical sort key).
    pub file: String,
    /// What the finding is about.
    pub target: FindingTarget,
    /// The finding's location in engine byte/line coordinates.
    pub range: Range,
    /// The detector that produced it.
    pub origin: Origin,
    /// Origin-qualified, **lossless** rule id (e.g. `"ruff:D417"`), never discarded.
    pub provider_rule_id: String,
    /// The rule id adopted into CF's vocabulary (e.g. `"D417"`).
    pub canonical_rule_id: String,
    /// The coarse cross-language bucket.
    pub category: Category,
    /// The resolved canonical severity.
    pub severity: Severity,
    /// The tool's original severity string, kept for fidelity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub severity_native: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// How (or whether) it can be fixed.
    pub fix: Fix,
    /// Optional rule-documentation link.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// Additional origins that independently reported an overlapping same-
    /// category finding, unioned in by dedup (Idea §5). Empty when not merged.
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub also_from: BTreeSet<Origin>,
}

impl Finding {
    /// Builds the lossless origin-qualified rule id, e.g. `("ruff", "D417") →
    /// "ruff:D417"` (Idea §5). Adapters use this so identity is never lossy.
    #[must_use]
    pub fn qualify_rule_id(origin: &Origin, native_rule_id: &str) -> String {
        format!("{}:{}", origin.as_str(), native_rule_id)
    }

    /// The canonical sort key `(file, start_line, start_byte, provider_rule_id)`
    /// (Idea §5 invocation Order) — stable baselines and byte-identical CI diffs
    /// depend on it (Idea §7).
    #[must_use]
    pub fn canonical_sort_key(&self) -> (&str, u32, u32, &str) {
        (
            self.file.as_str(),
            self.range.start_line,
            self.range.start_byte,
            self.provider_rule_id.as_str(),
        )
    }
}

#[cfg(test)]
pub(crate) fn sample_finding(file: &str, category: Category, range: Range) -> Finding {
    Finding {
        file: file.to_owned(),
        target: FindingTarget::Comment(CommentId::new("c0")),
        range,
        origin: Origin::Native,
        provider_rule_id: "native:rot".to_owned(),
        canonical_rule_id: "rot".to_owned(),
        category,
        severity: category.canonical_severity(),
        severity_native: None,
        message: "sample".to_owned(),
        fix: Fix::None,
        url: None,
        also_from: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_origin_serializes_as_bare_string_including_other() {
        assert_eq!(serde_json::to_string(&Origin::Ruff).unwrap(), "\"ruff\"");
        assert_eq!(
            serde_json::to_string(&Origin::Other("vale".into())).unwrap(),
            "\"vale\""
        );
        assert_eq!(
            serde_json::from_str::<Origin>("\"vale\"").unwrap(),
            Origin::Other("vale".into())
        );
        assert_eq!(
            serde_json::from_str::<Origin>("\"native\"").unwrap(),
            Origin::Native
        );
    }

    #[test]
    fn test_qualify_rule_id_is_lossless() {
        assert_eq!(Finding::qualify_rule_id(&Origin::Ruff, "D417"), "ruff:D417");
        assert_eq!(
            Finding::qualify_rule_id(&Origin::Other("vale".into()), "Spelling"),
            "vale:Spelling"
        );
    }

    #[test]
    fn test_range_overlap() {
        let a = Range::new(0, 10, 1, 1);
        assert!(a.overlaps(&Range::new(5, 15, 1, 1)));
        assert!(
            !a.overlaps(&Range::new(10, 20, 1, 1)),
            "half-open: touching is not overlapping"
        );
        assert_eq!(a.len_bytes(), 10);
    }

    #[test]
    fn test_finding_json_round_trip_is_byte_stable() {
        let finding = Finding {
            file: "src/app.py".to_owned(),
            target: FindingTarget::Symbol(BoundSymbol::new("app.main")),
            range: Range::new(12, 40, 3, 3),
            origin: Origin::Ruff,
            provider_rule_id: "ruff:D417".to_owned(),
            canonical_rule_id: "D417".to_owned(),
            category: Category::DocDrift,
            severity: Severity::Error,
            severity_native: Some("warning".to_owned()),
            message: "documented param 'x' not in signature".to_owned(),
            fix: Fix::AgentOnly,
            url: Some("https://docs.example/D417".to_owned()),
            also_from: BTreeSet::new(),
        };
        let json1 = serde_json::to_string(&finding).unwrap();
        let back: Finding = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json1, json2, "round-trip must be byte-stable");
        assert_eq!(back, finding);
    }

    #[test]
    fn test_optional_fields_omitted_when_empty() {
        let f = sample_finding("a.sh", Category::Shebang, Range::new(0, 2, 1, 1));
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("severity_native"), "None omitted: {json}");
        assert!(!json.contains("also_from"), "empty set omitted: {json}");
        assert!(!json.contains("url"));
    }

    #[test]
    fn test_finding_target_external_tagging() {
        let c = FindingTarget::Comment(CommentId::new("id7"));
        assert_eq!(serde_json::to_string(&c).unwrap(), "{\"comment\":\"id7\"}");
        let s = FindingTarget::Symbol(BoundSymbol::new("m.f"));
        assert_eq!(serde_json::to_string(&s).unwrap(), "{\"symbol\":\"m.f\"}");
    }
}
