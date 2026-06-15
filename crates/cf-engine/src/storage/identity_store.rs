//! Identity persistence (Idea §4; task 5.2).
//!
//! Writes the composite identity into the `index.db` identity table created in
//! Phase 4. Because identity is `(bound_symbol, kind, cosmetic_fingerprint)` —
//! not line or byte offset — it survives line shifts. The `ordinal` column
//! disambiguates comments that share the triple.

use rusqlite::{params, OptionalExtension};

use cf_core::error::{CfError, CfResult};
use cf_core::identity::CommentIdentity;
use cf_core::kind::CommentKind;
use cf_core::symbol::BoundSymbol;

use super::index_db::IndexDb;

/// Stores a comment's identity (upserting by comment id).
///
/// # Errors
/// Returns [`CfError::Identity`] on a write error.
pub fn store(index: &IndexDb, comment_id: i64, identity: &CommentIdentity) -> CfResult<()> {
    index
        .conn()
        .execute(
            "INSERT OR REPLACE INTO identity
             (comment_id, bound_symbol, kind, cosmetic_fingerprint, ordinal)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                comment_id,
                identity.bound_symbol.as_ref().map(BoundSymbol::as_str),
                identity.kind.as_str(),
                identity.cosmetic_fingerprint,
                identity.ordinal,
            ],
        )
        .map_err(|e| CfError::identity("storing identity").caused_by(e))?;
    Ok(())
}

/// Reads a comment's identity, if present.
///
/// # Errors
/// Returns [`CfError::Identity`] on a read error or an unrecognized stored kind.
pub fn get(index: &IndexDb, comment_id: i64) -> CfResult<Option<CommentIdentity>> {
    let row = index
        .conn()
        .query_row(
            "SELECT bound_symbol, kind, cosmetic_fingerprint, ordinal
             FROM identity WHERE comment_id = ?1",
            params![comment_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| CfError::identity("reading identity").caused_by(e))?;

    match row {
        None => Ok(None),
        Some((bound_symbol, kind_token, cosmetic_fingerprint, ordinal)) => {
            let kind = CommentKind::from_token(&kind_token)
                .ok_or_else(|| CfError::identity(format!("unknown stored kind {kind_token:?}")))?;
            Ok(Some(CommentIdentity {
                bound_symbol: bound_symbol.map(BoundSymbol::new),
                kind,
                cosmetic_fingerprint,
                ordinal,
            }))
        }
    }
}

/// Comment ids sharing the `(bound_symbol, kind, fingerprint)` base, ordered by
/// ordinal — the Tier-2 cosmetic-match candidates (Idea §4).
///
/// # Errors
/// Returns [`CfError::Identity`] on a read error.
pub fn by_base(
    index: &IndexDb,
    bound_symbol: Option<&str>,
    kind: CommentKind,
    cosmetic_fingerprint: &str,
) -> CfResult<Vec<(i64, u32)>> {
    let mut stmt = index
        .conn()
        // `IS` (not `=`) so a NULL bound_symbol matches NULL rows.
        .prepare(
            "SELECT comment_id, ordinal FROM identity
             WHERE bound_symbol IS ?1 AND kind = ?2 AND cosmetic_fingerprint = ?3
             ORDER BY ordinal",
        )
        .map_err(|e| CfError::identity("preparing identity query").caused_by(e))?;
    let rows = stmt
        .query_map(
            params![bound_symbol, kind.as_str(), cosmetic_fingerprint],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u32>(1)?)),
        )
        .map_err(|e| CfError::identity("querying identities").caused_by(e))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CfError::identity("reading identity row").caused_by(e))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::comment::Comment;
    use cf_core::finding::Range;
    use cf_core::identity::assign_identities;
    use cf_core::lang::Language;

    fn comment(text: &str, symbol: &str, range: Range) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            range,
            text,
        );
        c.bound_symbol = Some(BoundSymbol::new(symbol));
        c
    }

    #[test]
    fn test_store_and_read_round_trip() {
        let index = IndexDb::open_in_memory().unwrap();
        let id = index
            .insert_comment(&comment("# note", "m.f", Range::new(0, 6, 1, 1)))
            .unwrap();
        let identity = &assign_identities(&[comment("# note", "m.f", Range::new(0, 6, 1, 1))])[0];

        store(&index, id, identity).unwrap();
        assert_eq!(get(&index, id).unwrap().as_ref(), Some(identity));
    }

    #[test]
    fn test_ordinal_disambiguation_and_line_shift_stability() {
        let index = IndexDb::open_in_memory().unwrap();
        // Two identical comments (different lines) bound to the same symbol.
        let comments = vec![
            comment("# TODO: x", "m.f", Range::new(0, 9, 1, 1)),
            comment("# TODO: x", "m.f", Range::new(200, 209, 50, 50)),
        ];
        let identities = assign_identities(&comments);
        let id0 = index.insert_comment(&comments[0]).unwrap();
        let id1 = index.insert_comment(&comments[1]).unwrap();
        store(&index, id0, &identities[0]).unwrap();
        store(&index, id1, &identities[1]).unwrap();

        // Both share the base (line shift did not change identity) and are
        // disambiguated by ordinal.
        let base = identities[0].base_key();
        let matches = by_base(&index, base.0, base.1, base.2).unwrap();
        assert_eq!(matches, vec![(id0, 0), (id1, 1)]);
    }

    #[test]
    fn test_orphan_null_bound_symbol_matches() {
        let index = IndexDb::open_in_memory().unwrap();
        let mut orphan = comment("# loose", "ignored", Range::new(0, 7, 1, 1));
        orphan.bound_symbol = None;
        let identity = &assign_identities(&[orphan.clone()])[0];
        let id = index.insert_comment(&orphan).unwrap();
        store(&index, id, identity).unwrap();

        let base = identity.base_key();
        assert_eq!(base.0, None);
        assert_eq!(
            by_base(&index, None, base.1, base.2).unwrap(),
            vec![(id, 0)]
        );
    }
}
