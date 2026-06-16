//! Generic SARIF ingester (Idea §5; task 6.3).
//!
//! `format = "sarif"` needs **no field-paths** — any SARIF-emitting tool needs
//! only `command` + `[capabilities]`. This built-in mapper reads the standard
//! `runs[].results[]` shape (Idea §5; Commenter-Cat already *emits* SARIF, §8, and now
//! ingests it).

use serde_json::Value;

use super::jsonpath::RawFinding;

/// Maps a SARIF document into raw findings.
#[must_use]
pub fn extract(output: &Value) -> Vec<RawFinding> {
    let mut raw = Vec::new();
    let Some(runs) = output.get("runs").and_then(Value::as_array) else {
        return raw;
    };
    for run in runs {
        let Some(results) = run.get("results").and_then(Value::as_array) else {
            continue;
        };
        for result in results {
            raw.push(result_to_raw(result));
        }
    }
    raw
}

/// Maps one SARIF `result` into a raw finding (missing fields default).
fn result_to_raw(result: &Value) -> RawFinding {
    let physical = result
        .get("locations")
        .and_then(Value::as_array)
        .and_then(|locations| locations.first())
        .and_then(|location| location.get("physicalLocation"));
    let region = physical.and_then(|p| p.get("region"));

    RawFinding {
        native_rule_id: str_at(result, "ruleId").unwrap_or_default(),
        native_severity: str_at(result, "level"),
        message: result
            .get("message")
            .and_then(|m| m.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        file: physical
            .and_then(|p| p.get("artifactLocation"))
            .and_then(|a| a.get("uri"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        line: region.and_then(|r| u32_at(r, "startLine")).unwrap_or(1),
        column: region.and_then(|r| u32_at(r, "startColumn")),
    }
}

fn str_at(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn u32_at(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_generic_sarif_mapping_no_field_paths() {
        let sarif = json!({
            "runs": [{
                "results": [{
                    "ruleId": "SC2086",
                    "level": "warning",
                    "message": { "text": "double quote to prevent globbing" },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": { "uri": "deploy.sh" },
                            "region": { "startLine": 7, "startColumn": 6 }
                        }
                    }]
                }]
            }]
        });
        let raw = extract(&sarif);
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].native_rule_id, "SC2086");
        assert_eq!(raw[0].native_severity.as_deref(), Some("warning"));
        assert_eq!(raw[0].file, "deploy.sh");
        assert_eq!(raw[0].line, 7);
        assert_eq!(raw[0].column, Some(6));
    }

    #[test]
    fn test_empty_sarif() {
        assert!(extract(&json!({ "runs": [] })).is_empty());
        assert!(extract(&json!({})).is_empty());
    }
}
