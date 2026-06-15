//! Reciprocal rank fusion (Idea §6; task 4.7).
//!
//! Fuses several ranked id lists (keyword + semantic) into one. Each list
//! contributes `1 / (K + rank)` to an id's score, so an id ranked highly by
//! *both* signals outranks one ranked highly by only one. Ties break by id
//! ascending, making the fused order deterministic (Idea §7).

use std::cmp::Ordering;
use std::collections::HashMap;

/// The RRF damping constant (the standard value). A larger `K` flattens the
/// contribution of rank position.
const RRF_K: f64 = 60.0;

/// Fuses ranked id lists into one ranked list, capped at `limit`.
#[must_use]
pub fn reciprocal_rank_fusion(lists: &[Vec<i64>], limit: usize) -> Vec<i64> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for list in lists {
        for (position, &id) in list.iter().enumerate() {
            let rank = (position + 1) as f64;
            *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + rank);
        }
    }

    let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
    ranked.sort_by(|(a_id, a_score), (b_id, b_score)| {
        // Higher score first; ties → smaller id first (deterministic).
        b_score
            .partial_cmp(a_score)
            .unwrap_or(Ordering::Equal)
            .then(a_id.cmp(b_id))
    });
    ranked.into_iter().take(limit).map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agreement_outranks_single_signal() {
        // id 2 is high in both lists; id 1 only in the first, id 3 only in the second.
        let keyword = vec![1, 2];
        let semantic = vec![2, 3];
        assert_eq!(
            reciprocal_rank_fusion(&[keyword, semantic], 10),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn test_tie_break_by_id_ascending() {
        // Symmetric ranks → equal scores → smaller id first.
        let a = vec![10, 20];
        let b = vec![20, 10];
        assert_eq!(reciprocal_rank_fusion(&[a, b], 10), vec![10, 20]);
    }

    #[test]
    fn test_limit_applies() {
        assert_eq!(reciprocal_rank_fusion(&[vec![1, 2, 3, 4]], 2), vec![1, 2]);
    }

    #[test]
    fn test_empty_lists() {
        assert!(reciprocal_rank_fusion(&[], 5).is_empty());
        assert!(reciprocal_rank_fusion(&[vec![], vec![]], 5).is_empty());
    }
}
