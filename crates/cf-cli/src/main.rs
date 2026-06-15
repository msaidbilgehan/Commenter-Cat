//! `cf` — the Commenter-Cat command-line binary.
//!
//! Thin shell: parse the clap surface ([`cli`]), dispatch to the engine, and map
//! the result to a process exit code (Idea §8 — `0` clean, `1` findings at/above
//! the gate, `2` an operational error). The cause chain is rendered to stderr so
//! a 3 a.m. failure is actionable.

// `main` reports operational failures to stderr; the verb handlers own stdout via
// explicit writers, so the workspace print guards stay in force everywhere else.
#![allow(clippy::print_stderr)]

use std::process::ExitCode;

use clap::Parser;

mod cli;

fn main() -> ExitCode {
    let parsed = cli::Cli::parse();
    match cli::run(parsed) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(2)),
        Err(error) => {
            eprintln!("cf: {}", cf_core::error::cause_chain(&error));
            ExitCode::from(2)
        }
    }
}
