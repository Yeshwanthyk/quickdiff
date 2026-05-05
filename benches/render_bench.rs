mod fixtures;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use quickdiff::ui::windowing::{overscanned_range, visible_range};

fn bench_windowing_ranges(c: &mut Criterion) {
    let mut group = c.benchmark_group("windowing_ranges");
    for total in [1_000usize, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::from_parameter(total), &total, |b, total| {
            b.iter(|| {
                black_box(visible_range(*total, 250, 80));
                black_box(overscanned_range(*total, 250, 80, 20));
            });
        });
    }
    group.finish();
}

fn bench_hunk_view_planning(c: &mut Criterion) {
    let mut group = c.benchmark_group("hunk_view_planning");
    for size in [1_000usize, 10_000, 25_000] {
        let diff = fixtures::diff_with_many_hunks(size, 25);
        group.bench_with_input(BenchmarkId::from_parameter(size), &diff, |b, diff| {
            b.iter(|| {
                let rows: Vec<usize> = diff
                    .hunks()
                    .iter()
                    .flat_map(|hunk| hunk.start_row..(hunk.start_row + hunk.row_count))
                    .collect();
                black_box(rows.len())
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_windowing_ranges, bench_hunk_view_planning);
criterion_main!(benches);
