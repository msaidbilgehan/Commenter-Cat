//! Coordinate reconciliation (Idea §5, risk R6).
//!
//! Every provider speaks a different coordinate dialect — 1- vs 0-based, and
//! columns counted in UTF-8 bytes, UTF-16 code units (eslint), or Unicode
//! scalar values (ruff). The engine's one true coordinate is the **UTF-8 byte
//! offset**. This module centralizes the conversion so an off-by-one can only
//! ever be wrong in one place, and exposes [`CoordinateSystem`] as the
//! declared-convention enum a provider/manifest names in its `[capabilities]`
//! (Idea §5; consumed by Phase 6).
//!
//! The UTF-16↔byte path is property-tested in Phase 10 (Idea §11); the unit
//! tests here cover the load-bearing cases: a UTF-16 column after an astral
//! char, and a 0-based vs 1-based line.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The unit a provider counts columns in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnUnit {
    /// UTF-8 bytes (tree-sitter columns).
    Utf8,
    /// UTF-16 code units (eslint columns) — an astral char counts as two.
    Utf16,
    /// Unicode scalar values / characters (ruff columns).
    Char,
}

/// A provider's declared coordinate convention (Idea §5 `coordinate_system`).
///
/// Line and column share a base for every dialect Commenter-Cat targets. New conventions
/// are added here as providers require them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoordinateSystem {
    /// 0-based line, 0-based UTF-8-byte column — tree-sitter's native output.
    #[serde(rename = "0-based-utf8")]
    ZeroBasedUtf8,
    /// 1-based line, 1-based UTF-8-byte column.
    #[serde(rename = "1-based-utf8")]
    OneBasedUtf8,
    /// 1-based line, 1-based UTF-16-code-unit column — eslint.
    #[serde(rename = "1-based-utf16")]
    OneBasedUtf16,
    /// 1-based line, 1-based character (scalar-value) column — ruff.
    #[serde(rename = "1-based-char")]
    OneBasedChar,
}

impl CoordinateSystem {
    /// tree-sitter's convention (0-based, byte columns).
    #[must_use]
    pub const fn tree_sitter() -> Self {
        CoordinateSystem::ZeroBasedUtf8
    }

    /// eslint's convention (1-based, UTF-16 columns, Idea §5).
    #[must_use]
    pub const fn eslint() -> Self {
        CoordinateSystem::OneBasedUtf16
    }

    /// ruff's convention (1-based, character columns).
    #[must_use]
    pub const fn ruff() -> Self {
        CoordinateSystem::OneBasedChar
    }

    /// The base (`0` or `1`) lines are numbered from.
    #[must_use]
    pub const fn line_base(self) -> u32 {
        match self {
            CoordinateSystem::ZeroBasedUtf8 => 0,
            CoordinateSystem::OneBasedUtf8
            | CoordinateSystem::OneBasedUtf16
            | CoordinateSystem::OneBasedChar => 1,
        }
    }

    /// The base (`0` or `1`) columns are numbered from.
    #[must_use]
    pub const fn column_base(self) -> u32 {
        // Every dialect Commenter-Cat targets shares its line and column base.
        self.line_base()
    }

    /// The unit columns are counted in.
    #[must_use]
    pub const fn column_unit(self) -> ColumnUnit {
        match self {
            CoordinateSystem::ZeroBasedUtf8 | CoordinateSystem::OneBasedUtf8 => ColumnUnit::Utf8,
            CoordinateSystem::OneBasedUtf16 => ColumnUnit::Utf16,
            CoordinateSystem::OneBasedChar => ColumnUnit::Char,
        }
    }

    /// The kebab token for this convention (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CoordinateSystem::ZeroBasedUtf8 => "0-based-utf8",
            CoordinateSystem::OneBasedUtf8 => "1-based-utf8",
            CoordinateSystem::OneBasedUtf16 => "1-based-utf16",
            CoordinateSystem::OneBasedChar => "1-based-char",
        }
    }

    /// Parses a convention token, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<CoordinateSystem> {
        const ALL: [CoordinateSystem; 4] = [
            CoordinateSystem::ZeroBasedUtf8,
            CoordinateSystem::OneBasedUtf8,
            CoordinateSystem::OneBasedUtf16,
            CoordinateSystem::OneBasedChar,
        ];
        ALL.into_iter().find(|c| c.as_str() == token)
    }

    /// Converts a provider `(line, column)` in this convention into a 0-based
    /// UTF-8 byte offset within `text`, or `None` if the position does not land
    /// on a valid char boundary inside the file.
    #[must_use]
    pub fn to_byte_offset(self, text: &str, line: u32, column: u32) -> Option<u32> {
        let line0 = line.checked_sub(self.line_base())?;
        let column0 = column.checked_sub(self.column_base())?;

        let line_start = nth_line_start(text, line0)?;
        let line_end = line_end_byte(text, line_start);
        let line_slice = &text[line_start..line_end];

        let within = byte_for_column(line_slice, column0, self.column_unit())?;
        u32::try_from(line_start + within).ok()
    }
}

impl fmt::Display for CoordinateSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The byte index at which 0-based line `line0` begins, or `None` if `text` has
/// fewer lines. Lines are delimited by `\n` (CRLF's `\r` stays in the content).
fn nth_line_start(text: &str, line0: u32) -> Option<usize> {
    if line0 == 0 {
        return Some(0);
    }
    let mut remaining = line0;
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            remaining -= 1;
            if remaining == 0 {
                return Some(i + 1);
            }
        }
    }
    None
}

/// The byte index of the end of the line beginning at `line_start` — the next
/// `\n`, or end-of-text. A trailing `\r` (CRLF) remains part of the slice.
fn line_end_byte(text: &str, line_start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = line_start;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    end
}

/// The byte offset within `line` of 0-based `column0`, counted in `unit`, or
/// `None` if it overshoots the line or splits a character / surrogate pair.
fn byte_for_column(line: &str, column0: u32, unit: ColumnUnit) -> Option<usize> {
    match unit {
        ColumnUnit::Utf8 => {
            let offset = column0 as usize;
            (offset <= line.len() && line.is_char_boundary(offset)).then_some(offset)
        }
        ColumnUnit::Char => {
            if column0 == 0 {
                return Some(0);
            }
            let mut seen = 0u32;
            for (byte_index, _ch) in line.char_indices() {
                if seen == column0 {
                    return Some(byte_index);
                }
                seen += 1;
            }
            (seen == column0).then_some(line.len())
        }
        ColumnUnit::Utf16 => {
            if column0 == 0 {
                return Some(0);
            }
            let mut units = 0u32;
            for (byte_index, ch) in line.char_indices() {
                if units == column0 {
                    return Some(byte_index);
                }
                units += ch.len_utf16() as u32;
            }
            // `units > column0` here means the column split a surrogate pair.
            (units == column0).then_some(line.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_round_trip_and_serde() {
        for token in [
            "0-based-utf8",
            "1-based-utf8",
            "1-based-utf16",
            "1-based-char",
        ] {
            let system = CoordinateSystem::from_token(token).unwrap();
            assert_eq!(system.as_str(), token);
            // Matches the serde (kebab/renamed) form.
            let json = serde_json::to_string(&system).unwrap();
            assert_eq!(json, format!("\"{token}\""));
        }
    }

    #[test]
    fn test_zero_vs_one_based_line() {
        let text = "ab\ncd";
        // 0-based (tree-sitter): line 1, col 0 → start of "cd" = byte 3.
        assert_eq!(
            CoordinateSystem::tree_sitter().to_byte_offset(text, 1, 0),
            Some(3)
        );
        // 1-based (utf8): line 1, col 1 → start of "ab" = byte 0.
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 1, 1),
            Some(0)
        );
        // 1-based: line 2, col 1 → start of "cd" = byte 3.
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 2, 1),
            Some(3)
        );
    }

    #[test]
    fn test_eslint_utf16_column_after_astral_char() {
        // "💩x": 💩 is U+1F4A9 — 1 char, 2 UTF-16 units, 4 UTF-8 bytes.
        let text = "💩x";
        // eslint (1-based UTF-16): 💩 spans columns 1–2, so 'x' is column 3.
        let offset = CoordinateSystem::eslint().to_byte_offset(text, 1, 3);
        assert_eq!(offset, Some(4), "'x' begins at byte 4, after the 4-byte 💩");
        // A column that splits the surrogate pair (col 2 → between the units) is invalid.
        assert_eq!(CoordinateSystem::eslint().to_byte_offset(text, 1, 2), None);
    }

    #[test]
    fn test_char_vs_byte_columns_diverge_on_multibyte() {
        // "é!": é is U+00E9 — 1 char, 1 UTF-16 unit, 2 UTF-8 bytes.
        let text = "é!";
        // ruff (1-based char): '!' is char column 2 → byte 2.
        assert_eq!(CoordinateSystem::ruff().to_byte_offset(text, 1, 2), Some(2));
        // utf8 byte columns: '!' is at 1-based byte column 3 → byte 2.
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 1, 3),
            Some(2)
        );
        // A byte column landing inside é (1-based col 2 → byte 1) splits the char → None.
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 1, 2),
            None
        );
    }

    #[test]
    fn test_crlf_line_offsets() {
        // CRLF: the \r stays in the line content; line starts follow \n.
        let text = "a\r\nbc";
        // Line 2 (1-based), col 1 → start of "bc" = byte 3 (after "a\r\n").
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 2, 1),
            Some(3)
        );
    }

    #[test]
    fn test_out_of_range_is_none() {
        let text = "abc";
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 5, 1),
            None,
            "no such line"
        );
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 1, 99),
            None,
            "column past EOL"
        );
        // A 1-based system can never see column 0.
        assert_eq!(
            CoordinateSystem::OneBasedUtf8.to_byte_offset(text, 1, 0),
            None
        );
    }
}
