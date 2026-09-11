//! The clap verb surface (Idea §4a, §10).
//!
//! One canonical verb set the CLI and the MCP surface (Phase 8.3) share — the
//! primitives (`query`, `context`, `check`, `candidates`, `apply-edit`,
//! `remove`, `strip`) organized under find / understand / rule-check / update,
//! plus the management verbs (`baseline`, `suppressions`, `doctor`,
//! `install-hooks`, `issues`). The CLI is the canonical surface; MCP tools map
//! 1:1 onto it.
//!
//! `clap` lives only here, never in `commenter-cat-core`/the engine domain — the CLI-side
//! [`CliFormat`] and [`CliKind`] map to the domain's `OutputFormat` / `CommentKind`
//! so the domain stays free of the parsing framework (`ARCH_LAYER_VIOLATION`).

pub(crate) mod verbs;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use commenter_cat_core::config::OutputFormat;
use commenter_cat_core::kind::CommentKind;

pub(crate) use verbs::run;

/// `commenter-cat` — the Commenter-Cat command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "commenter-cat",
    version,
    about = "Commenter-Cat — deterministic, multi-language comment intelligence",
    propagate_version = true
)]
pub(crate) struct Cli {
    /// The verb to run.
    #[command(subcommand)]
    pub(crate) command: Command,

    /// Emit timing + cache statistics to stderr.
    #[arg(long, global = true)]
    pub(crate) stats: bool,

    /// Use analyzer tools found on `PATH` instead of the pinned toolchain.
    #[arg(long, global = true)]
    pub(crate) system_tools: bool,

    /// Require the hermetic pinned toolchain; fail if it is unavailable.
    #[arg(long, global = true)]
    pub(crate) hermetic: bool,

    /// Include suppressed findings in the output (audit view).
    #[arg(long, global = true)]
    pub(crate) show_suppressed: bool,

    /// Bypass the provider-result cache (re-run every provider).
    #[arg(long, global = true)]
    pub(crate) no_cache: bool,
}

/// The verb set — 1:1 with the MCP tools (Idea §4a).
#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(crate) enum Command {
    /// FIND: search comments by text (FTS + vector), ranked and bounded.
    Query {
        /// The search text.
        query: String,
        /// Maximum results (token-budget bound).
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Drill cursor (offset) returned by a prior page.
        #[arg(long)]
        cursor: Option<usize>,
    },

    /// UNDERSTAND: fetch one comment, with its bound code on request.
    Context {
        /// The comment id (from a prior `query`/`check`).
        comment_id: String,
        /// Include the bound code span (opt-in; never bundled by default).
        #[arg(long)]
        with_code: bool,
    },

    /// RULE-CHECK: run the analysis over the tree and render the findings.
    Check {
        /// Paths to check (default: the whole repository).
        paths: Vec<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = CliFormat::Terminal)]
        format: CliFormat,
        /// Fail on *any* finding, regardless of `fail_on` (CI gate).
        #[arg(long)]
        strict: bool,
    },

    /// FIND: the native worklist — rot candidates + ranked markers.
    Candidates {
        /// Maximum results (token-budget bound).
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// UPDATE: apply a parse-invariant comment edit, then re-check inline.
    ApplyEdit {
        /// The comment id to edit.
        comment_id: String,
        /// The new comment text.
        new_text: String,
        /// Permit editing a behavior-bearing comment (directive/shebang).
        #[arg(long)]
        allow_significant: bool,
    },

    /// UPDATE: remove a comment, then re-check inline.
    Remove {
        /// The comment id to remove.
        comment_id: String,
        /// Permit removing a behavior-bearing comment.
        #[arg(long)]
        allow_significant: bool,
    },

    /// UPDATE: scan the tree and strip every comment (dry-run without `--apply`).
    Strip {
        /// Root to scan (default: the working directory).
        path: Option<PathBuf>,
        /// Actually rewrite the files. Without it, `strip` prints a plan and
        /// touches nothing.
        #[arg(long)]
        apply: bool,
        /// Also strip behavior-bearing comments (directive / shebang /
        /// encoding-decl).
        #[arg(long)]
        allow_significant: bool,
        /// Also strip license / copyright headers.
        #[arg(long)]
        strip_license: bool,
        /// Preserve a comment kind, on top of the protected defaults (repeatable).
        #[arg(long, value_enum, value_name = "KIND")]
        keep: Vec<CliKind>,
        /// Leave the blank line a removal leaves behind.
        #[arg(long)]
        no_tidy: bool,
    },

    /// Manage the committed baseline (`commenter-cat.baseline.toml`).
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },

    /// Manage suppressions (export to native tool directives).
    Suppressions {
        #[command(subcommand)]
        action: SuppressionsAction,
    },

    /// Validate provider versions and config comparability.
    Doctor,

    /// Serve the MCP protocol over stdio (the agent-facing product, Idea §4a).
    Mcp,

    /// Install the cache-warmer git hooks (post-commit / -checkout / -merge /
    /// -rewrite) via `core.hooksPath` (Idea §7).
    InstallHooks,

    /// Sync flagged comments to the issue tracker.
    Issues {
        #[command(subcommand)]
        action: IssuesAction,
    },
}

/// `commenter-cat baseline …` actions.
#[derive(Debug, Subcommand, PartialEq, Eq)]
#[command(rename_all = "kebab-case")]
pub(crate) enum BaselineAction {
    /// Snapshot current findings into the baseline.
    Accept,
    /// Drop baseline entries whose findings no longer occur.
    Prune,
}

/// `commenter-cat suppressions …` actions.
#[derive(Debug, Subcommand, PartialEq, Eq)]
#[command(rename_all = "kebab-case")]
pub(crate) enum SuppressionsAction {
    /// Materialize the suppression set into native tool directives.
    Export,
}

/// `commenter-cat issues …` actions.
#[derive(Debug, Subcommand, PartialEq, Eq)]
#[command(rename_all = "kebab-case")]
pub(crate) enum IssuesAction {
    /// Sync flagged comments to the issue tracker.
    Sync {
        /// Actually file issues (network + `gh`). Without it, `sync` prints a
        /// dry-run plan and never touches the tracker.
        #[arg(long)]
        apply: bool,
    },
}

/// The CLI's output-format flag, mapped to the engine's `OutputFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub(crate) enum CliFormat {
    /// Grouped human view (default).
    Terminal,
    /// Canonical schema-tagged JSONL stream.
    Jsonl,
    /// SARIF 2.1.0 (GitHub code-scanning).
    Sarif,
    /// Markdown report.
    Markdown,
    /// CSV (RFC 4180).
    Csv,
}

/// The CLI's comment-kind flag (`strip --keep`), mapped to the domain's
/// [`CommentKind`] so `clap` stays out of `commenter-cat-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub(crate) enum CliKind {
    /// A single-line comment (`#`, `//`).
    Line,
    /// A block / multi-line comment.
    Block,
    /// A docstring bound to a symbol (PEP 257, JSDoc/TSDoc).
    Docstring,
    /// A `#!` interpreter line.
    Shebang,
    /// A license / copyright header.
    License,
    /// A source-encoding declaration (PEP 263).
    EncodingDecl,
    /// A control directive (`commenter-cat:*`, `# noqa`, `// eslint-disable`, …).
    Directive,
}

impl From<CliKind> for CommentKind {
    fn from(kind: CliKind) -> Self {
        match kind {
            CliKind::Line => CommentKind::Line,
            CliKind::Block => CommentKind::Block,
            CliKind::Docstring => CommentKind::Docstring,
            CliKind::Shebang => CommentKind::Shebang,
            CliKind::License => CommentKind::License,
            CliKind::EncodingDecl => CommentKind::EncodingDecl,
            CliKind::Directive => CommentKind::Directive,
        }
    }
}

impl From<CliFormat> for OutputFormat {
    fn from(format: CliFormat) -> Self {
        match format {
            CliFormat::Terminal => OutputFormat::Terminal,
            CliFormat::Jsonl => OutputFormat::Jsonl,
            CliFormat::Sarif => OutputFormat::Sarif,
            CliFormat::Markdown => OutputFormat::Markdown,
            CliFormat::Csv => OutputFormat::Csv,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses argv the way `main` does, surfacing clap's error on failure.
    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args parse")
    }

    #[test]
    fn test_all_verbs_parse_to_their_command() {
        assert!(matches!(
            parse(&["commenter-cat", "query", "stale"]).command,
            Command::Query { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "context", "c1"]).command,
            Command::Context { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "check"]).command,
            Command::Check { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "candidates"]).command,
            Command::Candidates { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "apply-edit", "c1", "# new"]).command,
            Command::ApplyEdit { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "remove", "c1"]).command,
            Command::Remove { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "strip"]).command,
            Command::Strip { .. }
        ));
        assert!(matches!(
            parse(&["commenter-cat", "doctor"]).command,
            Command::Doctor
        ));
        assert!(matches!(
            parse(&["commenter-cat", "mcp"]).command,
            Command::Mcp
        ));
        assert!(matches!(
            parse(&["commenter-cat", "install-hooks"]).command,
            Command::InstallHooks
        ));
    }

    #[test]
    fn test_subcommands_parse() {
        let Command::Baseline { action } = parse(&["commenter-cat", "baseline", "accept"]).command
        else {
            panic!("expected baseline");
        };
        assert_eq!(action, BaselineAction::Accept);

        let Command::Suppressions { action } =
            parse(&["commenter-cat", "suppressions", "export"]).command
        else {
            panic!("expected suppressions");
        };
        assert_eq!(action, SuppressionsAction::Export);

        let Command::Issues { action } = parse(&["commenter-cat", "issues", "sync"]).command else {
            panic!("expected issues");
        };
        assert_eq!(action, IssuesAction::Sync { apply: false });

        let Command::Issues { action } =
            parse(&["commenter-cat", "issues", "sync", "--apply"]).command
        else {
            panic!("expected issues");
        };
        assert_eq!(action, IssuesAction::Sync { apply: true });
    }

    #[test]
    fn test_check_flags_and_format() {
        let cli = parse(&[
            "commenter-cat",
            "check",
            "src",
            "--format",
            "jsonl",
            "--strict",
        ]);
        let Command::Check {
            paths,
            format,
            strict,
        } = cli.command
        else {
            panic!("expected check");
        };
        assert_eq!(paths, vec![PathBuf::from("src")]);
        assert_eq!(format, CliFormat::Jsonl);
        assert!(strict);
        assert_eq!(OutputFormat::from(format), OutputFormat::Jsonl);
    }

    #[test]
    fn test_global_flags_apply_across_verbs() {
        let cli = parse(&["commenter-cat", "check", "--show-suppressed", "--hermetic"]);
        assert!(cli.show_suppressed);
        assert!(cli.hermetic);
    }

    #[test]
    fn test_apply_edit_significant_flag() {
        let Command::ApplyEdit {
            comment_id,
            new_text,
            allow_significant,
        } = parse(&[
            "commenter-cat",
            "apply-edit",
            "c9",
            "# updated",
            "--allow-significant",
        ])
        .command
        else {
            panic!("expected apply-edit");
        };
        assert_eq!(comment_id, "c9");
        assert_eq!(new_text, "# updated");
        assert!(allow_significant);
    }

    #[test]
    fn test_strip_defaults_to_a_dry_run_with_everything_protected() {
        let Command::Strip {
            path,
            apply,
            allow_significant,
            strip_license,
            keep,
            no_tidy,
        } = parse(&["commenter-cat", "strip"]).command
        else {
            panic!("expected strip");
        };
        // The destructive switches are all opt-in; tidy is on.
        assert_eq!(path, None);
        assert!(!apply);
        assert!(!allow_significant);
        assert!(!strip_license);
        assert!(keep.is_empty());
        assert!(!no_tidy);
    }

    #[test]
    fn test_strip_flags_parse() {
        let Command::Strip {
            path,
            apply,
            allow_significant,
            strip_license,
            keep,
            no_tidy,
        } = parse(&[
            "commenter-cat",
            "strip",
            "src",
            "--apply",
            "--allow-significant",
            "--strip-license",
            "--keep",
            "docstring",
            "--keep",
            "block",
            "--no-tidy",
        ])
        .command
        else {
            panic!("expected strip");
        };
        assert_eq!(path, Some(PathBuf::from("src")));
        assert!(apply && allow_significant && strip_license && no_tidy);
        assert_eq!(keep, vec![CliKind::Docstring, CliKind::Block]);
        assert_eq!(CommentKind::from(keep[0]), CommentKind::Docstring);
    }

    #[test]
    fn test_strip_rejects_an_unknown_keep_kind() {
        assert!(Cli::try_parse_from(["commenter-cat", "strip", "--keep", "nonsense"]).is_err());
    }

    #[test]
    fn test_help_lists_every_verb() {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        let help = cmd.render_long_help().to_string();
        for verb in [
            "query",
            "context",
            "check",
            "candidates",
            "apply-edit",
            "remove",
            "strip",
            "baseline",
            "suppressions",
            "doctor",
            "mcp",
            "install-hooks",
            "issues",
        ] {
            assert!(help.contains(verb), "`commenter-cat --help` lists `{verb}`");
        }
    }
}
