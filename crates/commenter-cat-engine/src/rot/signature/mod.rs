//! Docstring↔signature contract (detector 2; `comment.md` 2).
//!
//! Parses what a doc comment *claims* about a function — `Args:`/`Returns:`/
//! `Raises:` (Python Google) or `@param`/`@returns`/`@throws` (JSDoc) — and diffs
//! it against what the function's signature *actually is*. The canonical catch is
//! a docstring that still lists `timeout` after the parameter was renamed to
//! `timeout_s`. It is purely structural, so it is the second-lowest-false-
//! positive detector.
//!
//! Three pieces:
//!
//! * [`doc_contract`] — parse the *claimed* contract from the comment text.
//! * [`sig_extract`] — read the *real* signature by re-parsing the bound node.
//! * [`detect`] — diff the two and emit `rot_signature` findings.

pub mod detect;
pub mod doc_contract;
pub mod sig_extract;

pub use detect::signature_findings;
