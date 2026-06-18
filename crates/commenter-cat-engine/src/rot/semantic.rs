//! Semantic contradiction (detector 5; `comment.md` 5).
//!
//! The only detector that can catch a *behavioral* lie a structural check
//! cannot ("Returns `None` on failure" when the code now raises). It scores the
//! embedding alignment between a comment and its bound code using the on-device
//! model the engine already ships, and surfaces a candidate only when alignment
//! is low.
//!
//! It is the noisiest detector, so it is **gated** three ways (all hard
//! requirements from the session): it is **default-off** (`[rot]
//! semantic_contradiction`), it runs **only behind the structural detectors**
//! (`structural_clean` — never on a comment a cheap detector already flagged),
//! and it surfaces **only high-confidence disagreement** (an integer alignment
//! score below the configured threshold).
//!
//! The score is mapped to an integer bucket (`0..=100`) so determinism holds and
//! float-equality bugs are impossible (general.md `TIME_FLOAT_EPOCH`). It
//! degrades to a no-op — never an error, never a false finding — when embeddings
//! are unavailable, the comment is unbound, or the bound span cannot be read
//! (Idea §5, generalized).

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config::RotConfig;
use commenter_cat_core::finding::{Category, Finding};

use crate::embed::Embedder;
use crate::ops::triage::native_finding;

/// The stable canonical rule id for semantic-contradiction findings.
const RULE: &str = "rot_semantic";

/// The full-alignment integer score (identical vectors).
const MAX_SCORE: f32 = 100.0;

/// Maps the cosine similarity of two embedding vectors to a deterministic
/// integer score in `0..=100`. Negative cosines clamp to `0` (maximal
/// disagreement); identical vectors score `100`.
#[must_use]
pub fn alignment_score(comment_vec: &[f32], code_vec: &[f32]) -> u32 {
    let cosine = cosine_similarity(comment_vec, code_vec).clamp(0.0, 1.0);
    (cosine * MAX_SCORE).round() as u32
}

/// A `rot_semantic` candidate for a comment, or `None` when the detector does
/// not fire. Surfaces only when the toggle is on, the comment passed every
/// structural detector (`structural_clean`), embeddings are available, the
/// comment is bound, and the alignment score is below the configured threshold.
#[must_use]
pub fn semantic_candidate(
    comment: &Comment,
    source: &str,
    embedder: &dyn Embedder,
    config: &RotConfig,
    structural_clean: bool,
) -> Option<Finding> {
    if !config.semantic_contradiction || !structural_clean {
        return None;
    }
    let code_range = comment.bound_node_range?;
    let code_text = source.get(code_range.start_byte as usize..code_range.end_byte as usize)?;
    // Embeddings unavailable → degrade to a no-op, never an error (Idea §5).
    let comment_vec = embedder.embed(&comment.raw_text).ok()?;
    let code_vec = embedder.embed(code_text).ok()?;
    let score = alignment_score(&comment_vec, &code_vec);
    if score >= config.semantic_confidence_threshold {
        return None;
    }
    Some(native_finding(
        comment,
        comment.range,
        Category::RotCandidate,
        RULE.to_owned(),
        Category::RotCandidate.canonical_severity(),
        format!(
            "comment and its bound code are semantically misaligned (alignment {score}/100) — possible silent contradiction"
        ),
    ))
}

/// Cosine similarity of two equal-length vectors; `0.0` for a length mismatch or
/// a zero vector (so the score degrades to "no alignment", never a NaN).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::config::ResolvedConfig;
    use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;

    /// An embedder whose vector is chosen by a marker in the text, so a test can
    /// force orthogonal (`AAA` vs `BBB`) or identical pairs deterministically.
    struct MarkerEmbedder;

    impl Embedder for MarkerEmbedder {
        fn model_version(&self) -> &str {
            "marker-test"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed(&self, text: &str) -> CommenterCatResult<Vec<f32>> {
            Ok(if text.contains("AAA") {
                vec![1.0, 0.0]
            } else if text.contains("BBB") {
                vec![0.0, 1.0]
            } else {
                vec![0.5, 0.5]
            })
        }
    }

    /// An embedder that always fails — the "embeddings unavailable" path.
    struct FailingEmbedder;

    impl Embedder for FailingEmbedder {
        fn model_version(&self) -> &str {
            "failing"
        }
        fn dimensions(&self) -> usize {
            2
        }
        fn embed(&self, _text: &str) -> CommenterCatResult<Vec<f32>> {
            Err(CommenterCatError::storage("embeddings unavailable"))
        }
    }

    /// A comment carrying `marker` in its text, bound to a 4-byte code span whose
    /// text is `code` (placed right after the comment in `source`).
    fn bound(marker: &str, code: &str) -> (Comment, String) {
        let comment_text = format!("# {marker}");
        let source = format!("{comment_text}\n{code}\n");
        let comment_len = u32::try_from(comment_text.len()).unwrap_or(0);
        let code_start = comment_len + 1;
        let code_end = code_start + u32::try_from(code.len()).unwrap_or(0);
        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Docstring,
            Range::new(0, comment_len, 1, 1),
            comment_text,
        );
        comment.bound_node_range = Some(Range::new(code_start, code_end, 2, 2));
        (comment, source)
    }

    fn enabled_config() -> RotConfig {
        let mut config = ResolvedConfig::default().rot;
        config.semantic_contradiction = true; // default-off; the test opts in
        config
    }

    #[test]
    fn test_alignment_score_is_high_for_identical_low_for_orthogonal() {
        assert_eq!(alignment_score(&[1.0, 0.0], &[1.0, 0.0]), 100);
        assert_eq!(alignment_score(&[1.0, 0.0], &[0.0, 1.0]), 0);
        assert_eq!(alignment_score(&[0.0, 0.0], &[1.0, 0.0]), 0, "zero vector");
    }

    #[test]
    fn test_contradiction_surfaces_only_when_structural_clean() {
        let (comment, source) = bound("AAA describes one thing", "BBB does another");
        let config = enabled_config();
        // structural_clean = true → the orthogonal pair (score 0 < 35) surfaces.
        let finding = semantic_candidate(&comment, &source, &MarkerEmbedder, &config, true)
            .expect("a contradiction candidate");
        assert_eq!(finding.canonical_rule_id, RULE);
        assert_eq!(finding.category, Category::RotCandidate);
        // structural_clean = false → the gate suppresses it entirely.
        assert!(
            semantic_candidate(&comment, &source, &MarkerEmbedder, &config, false).is_none(),
            "a structurally-flagged comment never reaches semantic scoring"
        );
    }

    #[test]
    fn test_aligned_pair_is_not_flagged() {
        let (comment, source) = bound("AAA the shared topic", "AAA the shared topic");
        let config = enabled_config();
        assert!(
            semantic_candidate(&comment, &source, &MarkerEmbedder, &config, true).is_none(),
            "an aligned pair scores 100 ≥ threshold → no finding"
        );
    }

    #[test]
    fn test_toggle_off_is_a_no_op() {
        let (comment, source) = bound("AAA one", "BBB two");
        let config = ResolvedConfig::default().rot; // semantic_contradiction = false
        assert!(semantic_candidate(&comment, &source, &MarkerEmbedder, &config, true).is_none());
    }

    #[test]
    fn test_unbound_comment_yields_none() {
        let mut comment = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 5, 1, 1),
            "# AAA",
        );
        comment.bound_node_range = None;
        assert!(
            semantic_candidate(
                &comment,
                "# AAA\n",
                &MarkerEmbedder,
                &enabled_config(),
                true
            )
            .is_none(),
            "no bound code → nothing to contradict"
        );
    }

    #[test]
    fn test_missing_embeddings_yields_none_without_error() {
        let (comment, source) = bound("AAA one", "BBB two");
        assert!(
            semantic_candidate(&comment, &source, &FailingEmbedder, &enabled_config(), true)
                .is_none(),
            "embeddings unavailable → no-op, never a false finding"
        );
    }

    // Heavy + networked: the real model proves the score is *meaningful* — a
    // genuinely contradictory pair aligns less than a faithful one. Excluded from
    // the fast offline suite (mirrors embed::onnx::test_real_onnx_embeddings).
    #[test]
    #[ignore = "downloads the all-MiniLM-L6-v2 ONNX model (~90 MB)"]
    fn test_real_onnx_alignment_is_meaningful() {
        use crate::embed::onnx::OnnxEmbedder;

        let embedder = OnnxEmbedder::new().expect("model loads");
        let comment = embedder
            .embed("# returns the number of currently active user sessions")
            .unwrap();
        let faithful = embedder
            .embed("def active_session_count(): return len(self.sessions)")
            .unwrap();
        let contradictory = embedder
            .embed("def purge_all(): delete_every_record_from_disk()")
            .unwrap();

        assert!(
            alignment_score(&comment, &faithful) > alignment_score(&comment, &contradictory),
            "the faithful pair must align more than the contradictory one"
        );
    }
}
