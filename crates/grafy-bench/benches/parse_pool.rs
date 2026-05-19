//! Parser-pool scaling bench — single-threaded vs rayon par_iter.
//!
//! Plan §M0 engineering gate: ≥5× on 8 cores vs 1 core.

use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};
use grafy_bench::collect_sources;
use grafy_parser::{parse, Language};
use rayon::prelude::*;

fn parse_dir(dir: &Path) -> usize {
    let files = collect_sources(dir, &["rs"]);
    if files.is_empty() {
        return 0;
    }
    files
        .iter()
        .filter_map(|p| std::fs::read(p).ok().map(|b| (p, b)))
        .filter_map(|(p, b)| parse(&p.display().to_string(), Language::Rust, &b).ok())
        .count()
}

fn parse_dir_par(dir: &Path) -> usize {
    let files = collect_sources(dir, &["rs"]);
    if files.is_empty() {
        return 0;
    }
    files
        .par_iter()
        .filter_map(|p| std::fs::read(p).ok().map(|b| (p, b)))
        .filter_map(|(p, b)| parse(&p.display().to_string(), Language::Rust, &b).ok())
        .count()
}

fn bench_pool(c: &mut Criterion) {
    let target = std::env::var("GRAFY_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut group = c.benchmark_group("parser_pool");
    group.sample_size(10);
    group.bench_function("single", |b| b.iter(|| parse_dir(&target)));
    group.bench_function("rayon", |b| b.iter(|| parse_dir_par(&target)));
    group.finish();
}

criterion_group!(benches, bench_pool);
criterion_main!(benches);
