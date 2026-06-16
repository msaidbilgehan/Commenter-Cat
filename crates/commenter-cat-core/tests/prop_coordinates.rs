//! Property tests for coordinate conversion (Idea §5 risk R6, §11).
//!
//! The UTF-16↔byte path is the off-by-one trap (eslint counts UTF-16 code units,
//! ruff counts scalar values, the engine counts UTF-8 bytes). These properties
//! prove the conversion is *safe* over arbitrary Unicode — it never panics, never
//! returns a non-char-boundary or out-of-range offset, and the three dialects
//! agree on pure-ASCII input.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use commenter_cat_core::finding::CoordinateSystem;
use proptest::prelude::*;

const ONE_BASED: [CoordinateSystem; 3] = [
    CoordinateSystem::OneBasedUtf8,
    CoordinateSystem::OneBasedUtf16,
    CoordinateSystem::OneBasedChar,
];

proptest! {
    /// Over arbitrary text (ASCII, 2-byte, and astral chars), any offset the
    /// converter returns is in range and lands on a char boundary.
    #[test]
    fn offset_is_always_a_valid_boundary(text in "[a-zé💩 ]{0,40}", column in 1u32..50) {
        for system in ONE_BASED {
            if let Some(offset) = system.to_byte_offset(&text, 1, column) {
                let offset = offset as usize;
                prop_assert!(offset <= text.len(), "{system:?} returned an out-of-range offset");
                prop_assert!(text.is_char_boundary(offset), "{system:?} split a character");
            }
        }
    }

    /// On pure ASCII, the three column dialects coincide (a code unit, a scalar
    /// value, and a byte are all one).
    #[test]
    fn ascii_columns_coincide(text in "[a-zA-Z0-9]{1,30}", column in 1u32..31) {
        let utf8 = CoordinateSystem::OneBasedUtf8.to_byte_offset(&text, 1, column);
        let utf16 = CoordinateSystem::OneBasedUtf16.to_byte_offset(&text, 1, column);
        let chars = CoordinateSystem::OneBasedChar.to_byte_offset(&text, 1, column);
        prop_assert_eq!(utf8, utf16);
        prop_assert_eq!(utf8, chars);
    }

    /// A 0-based tree-sitter column on ASCII is exactly the byte index.
    #[test]
    fn tree_sitter_ascii_column_is_byte_index(text in "[a-z]{1,30}", column in 0u32..30) {
        let offset = CoordinateSystem::tree_sitter().to_byte_offset(&text, 0, column);
        if (column as usize) <= text.len() {
            prop_assert_eq!(offset, Some(column));
        }
    }
}
