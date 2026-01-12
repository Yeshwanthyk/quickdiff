//! Integration tests for large file handling.

use quickdiff::core::{DiffResult, TextBuffer};

#[test]
fn diff_large_file_completes_in_reasonable_time() {
    use std::time::Instant;

    // Generate 10k line file with single change
    let old_content: String = (0..10000).map(|i| format!("line {}\n", i)).collect();
    let new_content: String = (0..10000)
        .map(|i| {
            if i == 5000 {
                "modified line\n".to_string()
            } else {
                format!("line {}\n", i)
            }
        })
        .collect();

    let old = TextBuffer::new(old_content.as_bytes());
    let new = TextBuffer::new(new_content.as_bytes());

    let start = Instant::now();
    let diff = DiffResult::compute(&old, &new);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "Diff took too long: {:?}",
        elapsed
    );
    assert!(diff.has_changes());
}

#[test]
fn diff_handles_scattered_changes() {
    use std::time::Instant;

    // File with changes scattered throughout (more realistic than all-different)
    let old_content: String = (0..5000).map(|i| format!("line {}\n", i)).collect();
    let new_content: String = (0..5000)
        .map(|i| {
            if i % 100 == 0 {
                format!("modified line {}\n", i)
            } else {
                format!("line {}\n", i)
            }
        })
        .collect();

    let old = TextBuffer::new(old_content.as_bytes());
    let new = TextBuffer::new(new_content.as_bytes());

    let start = Instant::now();
    let diff = DiffResult::compute(&old, &new);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "Diff took too long: {:?}",
        elapsed
    );
    assert!(diff.has_changes());
    // Should have ~50 hunks (one per modified line, possibly merged)
    assert!(diff.hunks().len() >= 10);
}
