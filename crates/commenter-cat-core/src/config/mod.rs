//! Layered TOML configuration (Idea §12).
//!
//! Configuration is assembled from four precedence layers — `COMMENTER_CAT_*` environment
//! variables, a nearest-first walk-up of `commenter-cat.toml` files, an XDG
//! global config, and built-in defaults — then collapsed into a single
//! [`ResolvedConfig`] the engine consumes.
//!
//! * [`model`] — the data model: partial (on-disk, mergeable) vs. resolved
//!   (concrete) structs, the scalar enums, and the merge/resolve logic.
//! * [`discovery`] — finding, loading, validating, and cascading the layers.
//!
//! ```no_run
//! use std::path::Path;
//! let config = commenter_cat_core::config::discover(Path::new("."))?;
//! println!("fail_on = {}", config.severity.fail_on);
//! # Ok::<(), commenter_cat_core::CommenterCatError>(())
//! ```

pub mod discovery;
pub mod model;

pub use discovery::{
    discover, discover_in, from_env, load_file, CONFIG_FILENAME, ENV_PREFIX, GLOBAL_CONFIG_SUBPATH,
};
pub use model::{
    ConfigFile, EmbeddingsMode, MarkersConfig, MarkersSection, MergeOver, OnMissing, OutputConfig,
    OutputFormat, OutputSection, ProvidersConfig, ProvidersSection, ResolvedConfig, RotConfig,
    RotSection, ScanConfig, ScanSection, SearchConfig, SearchSection, SeverityConfig,
    SeveritySection,
};
