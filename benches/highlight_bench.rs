mod fixtures;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use quickdiff::highlight::{query_scopes, FileHighlightCache, HighlighterCache, LanguageId};

fn bench_highlight_compute(c: &mut Criterion) {
    let mut group = c.benchmark_group("highlight_compute");
    let cache = HighlighterCache::new();

    for size in [100usize, 1_000, 5_000] {
        let source = fixtures::generate_long_wrapped_source(size, 160);
        group.bench_with_input(BenchmarkId::from_parameter(size), &source, |b, source| {
            b.iter(|| {
                let mut file_cache = FileHighlightCache::new();
                file_cache.compute(&cache, LanguageId::Rust, black_box(source));
                black_box(file_cache.line_spans(0));
            });
        });
    }

    group.finish();
}

fn bench_scope_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("scope_queries");
    for size in [100usize, 1_000, 5_000] {
        let source = fixtures::generate_long_wrapped_source(size, 120);
        group.bench_with_input(BenchmarkId::from_parameter(size), &source, |b, source| {
            b.iter(|| query_scopes(LanguageId::Rust, black_box(source)));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_highlight_compute, bench_scope_queries);
criterion_main!(benches);
