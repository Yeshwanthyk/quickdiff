//! Benchmarks for quickdiff core operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use quickdiff::core::{DiffResult, TextBuffer};

/// Generate a file with N lines.
fn generate_lines(n: usize, prefix: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(n * 20);
    for i in 0..n {
        buf.extend_from_slice(format!("{} line number {}\n", prefix, i).as_bytes());
    }
    buf
}

/// Generate a file with changes at specific positions.
fn generate_with_changes(n: usize, change_positions: &[usize]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(n * 20);
    for i in 0..n {
        if change_positions.contains(&i) {
            buf.extend_from_slice(format!("MODIFIED line number {}\n", i).as_bytes());
        } else {
            buf.extend_from_slice(format!("original line number {}\n", i).as_bytes());
        }
    }
    buf
}

fn bench_textbuffer_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("TextBuffer::new");

    for size in [100, 1_000, 10_000, 100_000] {
        let data = generate_lines(size, "test");
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| TextBuffer::new(black_box(data)));
        });
    }

    group.finish();
}

fn bench_diff_identical(c: &mut Criterion) {
    let mut group = c.benchmark_group("DiffResult::compute/identical");

    for size in [100, 1_000, 10_000] {
        let data = generate_lines(size, "test");
        let buf = TextBuffer::new(&data);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &buf, |b, buf| {
            b.iter(|| DiffResult::compute(black_box(buf), black_box(buf)));
        });
    }

    group.finish();
}

fn bench_diff_single_change(c: &mut Criterion) {
    let mut group = c.benchmark_group("DiffResult::compute/single_change");

    for size in [100, 1_000, 10_000] {
        let old_data = generate_lines(size, "original");
        let new_data = generate_with_changes(size, &[size / 2]);

        let old_buf = TextBuffer::new(&old_data);
        let new_buf = TextBuffer::new(&new_data);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(old_buf, new_buf),
            |b, (old, new)| {
                b.iter(|| DiffResult::compute(black_box(old), black_box(new)));
            },
        );
    }

    group.finish();
}

fn bench_diff_many_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("DiffResult::compute/many_changes");

    for size in [100, 1_000, 10_000] {
        // Change every 10th line
        let changes: Vec<usize> = (0..size).filter(|i| i % 10 == 0).collect();
        let old_data = generate_lines(size, "original");
        let new_data = generate_with_changes(size, &changes);

        let old_buf = TextBuffer::new(&old_data);
        let new_buf = TextBuffer::new(&new_data);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(old_buf, new_buf),
            |b, (old, new)| {
                b.iter(|| DiffResult::compute(black_box(old), black_box(new)));
            },
        );
    }

    group.finish();
}

fn bench_diff_worst_case(c: &mut Criterion) {
    let mut group = c.benchmark_group("DiffResult::compute/worst_case");

    // Worst case: completely different files (every line changed)
    for size in [100, 500, 1_000] {
        let old_data = generate_lines(size, "old");
        let new_data = generate_lines(size, "new");

        let old_buf = TextBuffer::new(&old_data);
        let new_buf = TextBuffer::new(&new_data);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(old_buf, new_buf),
            |b, (old, new)| {
                b.iter(|| DiffResult::compute(black_box(old), black_box(new)));
            },
        );
    }

    group.finish();
}

fn bench_hunk_navigation(c: &mut Criterion) {
    let mut group = c.benchmark_group("hunk_navigation");

    // Create a diff with many hunks
    let size = 10_000;
    let changes: Vec<usize> = (0..size).filter(|i| i % 50 == 0).collect();
    let old_data = generate_lines(size, "original");
    let new_data = generate_with_changes(size, &changes);

    let old_buf = TextBuffer::new(&old_data);
    let new_buf = TextBuffer::new(&new_data);
    let diff = DiffResult::compute(&old_buf, &new_buf);

    group.bench_function("next_hunk", |b| {
        b.iter(|| {
            let mut row = 0;
            while let Some(next) = diff.next_hunk_row(row) {
                row = next + 1;
            }
            black_box(row)
        });
    });

    group.bench_function("prev_hunk", |b| {
        b.iter(|| {
            let mut row = diff.row_count();
            while let Some(prev) = diff.prev_hunk_row(row) {
                row = prev;
            }
            black_box(row)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_textbuffer_new,
    bench_diff_identical,
    bench_diff_single_change,
    bench_diff_many_changes,
    bench_diff_worst_case,
    bench_hunk_navigation,
);

criterion_main!(benches);
