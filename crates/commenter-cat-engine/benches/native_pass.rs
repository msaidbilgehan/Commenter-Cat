//! Criterion benches for the native pass (Idea §6, §11).
//!
//! Guards the per-stage budget shape: extraction and the load-bearing
//! comment→code mapping are the hot path of `commenter-cat check`. A regression here shows
//! up as a wall-clock change against this baseline (the `commenter-cat check --stats`
//! budget, §6).

// Bench code: unwrap on known-good fixtures is idiomatic, and bench fns are not
// a public API surface.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::Path;
use std::time::Duration;

use commenter_cat_core::lang::Language;
use commenter_cat_engine::extract::coalesce::coalesce;
use commenter_cat_engine::extract::extract_source;
use commenter_cat_engine::map::map_comments;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};

/// A representative Python module: many comments, docstrings, and bound symbols.
fn fixture_source() -> String {
    let mut source = String::from("\"\"\"Module docstring explaining the cache.\"\"\"\n\n");
    for i in 0..50 {
        source.push_str(&format!(
            "# TODO({i}): revisit this helper\n\
             def helper_{i}(value):\n\
             \x20   \"\"\"Doc for helper {i}.\"\"\"\n\
             \x20   # inline note about the branch\n\
             \x20   return value + {i}\n\n"
        ));
    }
    source
}

fn bench_native_pass(c: &mut Criterion) {
    let source = fixture_source();
    let path = Path::new("module.py");

    let mut group = c.benchmark_group("native_pass");
    group.measurement_time(Duration::from_secs(5));

    // Extraction + classification (tree-sitter walk → Comment records).
    group.bench_function("extract", |b| {
        b.iter(|| extract_source(&source, Language::Python, path, "module.py").unwrap())
    });

    // The full hot path: extract → coalesce → map (comment→code binding).
    group.bench_function("extract_coalesce_map", |b| {
        b.iter_batched(
            || source.clone(),
            |src| {
                let extracted = extract_source(&src, Language::Python, path, "module.py").unwrap();
                let mut coalesced = coalesce(&src, extracted);
                map_comments(&src, Language::Python, path, &mut coalesced).unwrap();
                coalesced
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_native_pass);
criterion_main!(benches);
