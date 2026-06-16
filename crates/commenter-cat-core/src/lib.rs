//! # commenter-cat-core — Commenter-Cat domain substrate
//!
//! The pure, infrastructure-free domain layer of Commenter-Cat. It owns
//! the types every other crate shares: the [`error`] hierarchy, the
//! independently-versioned contract constants ([`version`]), the layered
//! [`config`] model, and the small shared primitives ([`lang`], [`kind`],
//! [`severity`]) that the substrate and finding model are built from.
//!
//! ## Layer discipline
//!
//! `commenter-cat-core` imports **no infrastructure** — no SQLite, no subprocess, no git,
//! no tree-sitter. Adapters in `commenter-cat-engine` translate infrastructure failures
//! into [`error::CommenterCatError`] at the seam (general.md `ARCH_LAYER_VIOLATION`). This
//! keeps the domain testable in isolation and reusable across interfaces.

pub mod comment;
pub mod error;
pub mod kind;
pub mod lang;
pub mod severity;
pub mod symbol;
pub mod version;

pub mod config;
pub mod finding;
pub mod identity;

pub use comment::{Comment, GitInfo};
pub use error::{CommenterCatError, CommenterCatResult};
pub use finding::{Category, Finding, FindingTarget, Fix, Origin, Range};
pub use identity::CommentIdentity;
pub use kind::CommentKind;
pub use lang::Language;
pub use severity::Severity;
pub use symbol::{BoundSymbol, CommentId};
