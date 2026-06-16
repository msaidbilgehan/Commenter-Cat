//! RFC 9535 JSONPath extraction for manifests (Idea §5; task 6.3).
//!
//! Extraction is a **JSON-query specification**, not flat field-paths, so it
//! handles nested objects and arrays. The dialect is pinned (`serde_json_path`,
//! RFC 9535) so manifests are deterministic and portable (Idea §5, §14, risk
//! R4). Each `[[findings]]` spec's `iterate` selects the finding nodes; the field
//! paths are then evaluated **relative to each node** (its `$` is that node).

use serde_json::Value;
use serde_json_path::JsonPath;

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

/// One raw, pre-canonicalization finding pulled from a tool's JSON output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFinding {
    /// The tool's own rule id.
    pub native_rule_id: String,
    /// The tool's native severity string, if the manifest extracts one.
    pub native_severity: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Repo-relative file path.
    pub file: String,
    /// Line, in the provider's coordinate convention.
    pub line: u32,
    /// Column, in the provider's coordinate convention (if any).
    pub column: Option<u32>,
}

/// A `[[findings]]` extraction spec (Idea §5 manifest).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FindingSpec {
    /// JSONPath selecting the finding nodes (e.g. `$.results[*]`).
    pub iterate: String,
    /// JSONPath (relative to a node) for the native rule id.
    pub native_rule_id: String,
    /// JSONPath (relative to a node) for the native severity, optional.
    #[serde(default)]
    pub severity: Option<String>,
    /// JSONPath for the message.
    pub message: String,
    /// JSONPath for the file path.
    pub file: String,
    /// JSONPath for the line.
    pub line: String,
    /// JSONPath for the column, optional.
    #[serde(default)]
    pub column: Option<String>,
}

/// Extracts raw findings from `output` per the manifest's `[[findings]]` specs.
///
/// # Errors
/// Returns [`CommenterCatError::Provider`] if any JSONPath is invalid.
pub fn extract(
    provider: &str,
    output: &Value,
    specs: &[FindingSpec],
) -> CommenterCatResult<Vec<RawFinding>> {
    let mut raw = Vec::new();
    for spec in specs {
        for node in query_all(provider, &spec.iterate, output)? {
            raw.push(RawFinding {
                native_rule_id: query_string(provider, &spec.native_rule_id, node)?
                    .unwrap_or_default(),
                native_severity: spec
                    .severity
                    .as_ref()
                    .map(|path| query_string(provider, path, node))
                    .transpose()?
                    .flatten(),
                message: query_string(provider, &spec.message, node)?.unwrap_or_default(),
                file: query_string(provider, &spec.file, node)?.unwrap_or_default(),
                line: query_u32(provider, &spec.line, node)?.unwrap_or(1),
                column: spec
                    .column
                    .as_ref()
                    .map(|path| query_u32(provider, path, node))
                    .transpose()?
                    .flatten(),
            });
        }
    }
    Ok(raw)
}

fn compile(provider: &str, path: &str) -> CommenterCatResult<JsonPath> {
    JsonPath::parse(path).map_err(|e| {
        CommenterCatError::provider(provider, format!("invalid JSONPath {path:?}: {e}"))
    })
}

/// All values matched by `path` within `root`.
fn query_all<'a>(
    provider: &str,
    path: &str,
    root: &'a Value,
) -> CommenterCatResult<Vec<&'a Value>> {
    Ok(compile(provider, path)?.query(root).all())
}

/// The first scalar value matched by `path` within `node`, as a string. Numbers
/// and booleans are stringified (e.g. shellcheck's numeric `code`).
fn query_string(provider: &str, path: &str, node: &Value) -> CommenterCatResult<Option<String>> {
    Ok(compile(provider, path)?
        .query(node)
        .first()
        .and_then(scalar_to_string))
}

/// Converts a scalar JSON value to a string (`None` for arrays/objects/null).
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The first integer value matched by `path` within `node`.
fn query_u32(provider: &str, path: &str, node: &Value) -> CommenterCatResult<Option<u32>> {
    Ok(compile(provider, path)?
        .query(node)
        .first()
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec() -> FindingSpec {
        FindingSpec {
            iterate: "$.results[*]".to_owned(),
            native_rule_id: "$.rule".to_owned(),
            severity: Some("$.level".to_owned()),
            message: "$.message".to_owned(),
            file: "$.location.path".to_owned(),
            line: "$.location.start.line".to_owned(),
            column: Some("$.location.start.column".to_owned()),
        }
    }

    #[test]
    fn test_nested_extraction() {
        let output = json!({
            "results": [
                {
                    "rule": "D417",
                    "level": "warning",
                    "message": "param drift",
                    "location": { "path": "src/a.py", "start": { "line": 12, "column": 5 } }
                },
                {
                    "rule": "ERA001",
                    "level": "info",
                    "message": "commented code",
                    "location": { "path": "src/b.py", "start": { "line": 3, "column": 1 } }
                }
            ]
        });
        let raw = extract("ruff", &output, &[spec()]).unwrap();
        assert_eq!(raw.len(), 2);
        assert_eq!(raw[0].native_rule_id, "D417");
        assert_eq!(raw[0].native_severity.as_deref(), Some("warning"));
        assert_eq!(raw[0].file, "src/a.py");
        assert_eq!(raw[0].line, 12);
        assert_eq!(raw[0].column, Some(5));
        assert_eq!(raw[1].native_rule_id, "ERA001");
    }

    #[test]
    fn test_empty_results() {
        let output = json!({ "results": [] });
        assert!(extract("ruff", &output, &[spec()]).unwrap().is_empty());
    }

    #[test]
    fn test_invalid_jsonpath_errors() {
        let bad = FindingSpec {
            iterate: "$[".to_owned(),
            ..spec()
        };
        assert!(extract("ruff", &json!({}), &[bad]).is_err());
    }
}
