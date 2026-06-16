//! The canonical JSONL stream (Idea §8).
//!
//! The structured-first format every other renderer derives from. A leading
//! header line carries the `schema_version` (forward-only, reader supports N and
//! N−1); each subsequent line is one comment record. It round-trips: [`parse`]
//! reverses [`render`].

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::version::SCHEMA_VERSION_JSONL;
use serde::{Deserialize, Serialize};

/// The stream header (first line) — carries the schema version so a reader can
/// reject an unsupported future stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    schema_version: u32,
    tool: String,
}

/// Renders comments as a `schema_version`-tagged JSONL stream.
///
/// # Errors
/// Returns [`CommenterCatError::Render`] if a record fails to serialize (a serde
/// invariant violation, not expected in practice).
pub fn render(comments: &[Comment]) -> CommenterCatResult<String> {
    let header = Header {
        schema_version: SCHEMA_VERSION_JSONL,
        tool: "commenter-cat".to_owned(),
    };
    let mut out = serialize_line(&header)?;
    for comment in comments {
        out.push_str(&serialize_line(comment)?);
    }
    Ok(out)
}

/// Parses a JSONL stream back into comment records, validating the header.
///
/// # Errors
/// Returns [`CommenterCatError::Render`] if the header is missing/unsupported or a record
/// fails to parse.
pub fn parse(stream: &str) -> CommenterCatResult<Vec<Comment>> {
    let mut lines = stream.lines().filter(|line| !line.trim().is_empty());
    let header_line = lines
        .next()
        .ok_or_else(|| CommenterCatError::render("empty JSONL stream"))?;
    let header: Header = serde_json::from_str(header_line)
        .map_err(|e| CommenterCatError::render("malformed JSONL header").caused_by(e))?;
    if header.schema_version > SCHEMA_VERSION_JSONL {
        return Err(CommenterCatError::render(format!(
            "JSONL stream is schema {} but this commenter-cat supports up to {SCHEMA_VERSION_JSONL}",
            header.schema_version
        )));
    }
    lines
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|e| CommenterCatError::render("malformed JSONL record").caused_by(e))
        })
        .collect()
}

/// Serializes one value to a single newline-terminated JSON line.
fn serialize_line<T: Serialize>(value: &T) -> CommenterCatResult<String> {
    let mut line = serde_json::to_string(value)
        .map_err(|e| CommenterCatError::render("serializing JSONL record").caused_by(e))?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use commenter_cat_core::finding::Category;
    use commenter_cat_core::severity::Severity;

    #[test]
    fn test_round_trips_with_schema_version() {
        let comments = vec![comment_with(vec![finding(
            "D417",
            Severity::Error,
            Category::DocDrift,
        )])];
        let stream = render(&comments).unwrap();
        // First line is the header carrying the schema version.
        assert!(stream
            .lines()
            .next()
            .unwrap()
            .contains("\"schema_version\":1"));
        // Reverses cleanly.
        assert_eq!(parse(&stream).unwrap(), comments);
    }

    #[test]
    fn test_rejects_unsupported_future_schema() {
        let stream = "{\"schema_version\":99,\"tool\":\"commenter-cat\"}\n";
        assert!(parse(stream).is_err());
    }

    #[test]
    fn test_empty_stream_is_error() {
        assert!(parse("").is_err());
    }
}
