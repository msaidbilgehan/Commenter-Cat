//! Git-hook cache warming (Idea §7, §9).
//!
//! `cf install-hooks` distributes non-fatal warmer hooks via `core.hooksPath` so
//! the cache stays warm on the events that change comment↔code mappings, without
//! the hooks ever owning the data or blocking a commit.

pub mod install;

pub use install::{hook_script, hooks_dir, install, HookEvent, InstallReport};
