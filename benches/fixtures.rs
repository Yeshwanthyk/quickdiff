use quickdiff::core::{DiffResult, TextBuffer};

#[allow(dead_code)]
pub fn generate_lines(n: usize, prefix: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(n * 32);
    for i in 0..n {
        buf.extend_from_slice(format!("{} line number {}\n", prefix, i).as_bytes());
    }
    buf
}

#[allow(dead_code)]
pub fn generate_with_changes(n: usize, every: usize) -> (Vec<u8>, Vec<u8>) {
    let old = generate_lines(n, "original");
    let mut new = Vec::with_capacity(old.len() + n * 4);
    for i in 0..n {
        let text = if i % every == 0 {
            format!("modified line number {}\n", i)
        } else {
            format!("original line number {}\n", i)
        };
        new.extend_from_slice(text.as_bytes());
    }
    (old, new)
}

#[allow(dead_code)]
pub fn generate_long_wrapped_source(lines: usize, width: usize) -> String {
    let segment = "abcdefghijklmnopqrstuvwxyz0123456789";
    (0..lines)
        .map(|i| {
            let repeated = segment.repeat(width / segment.len() + 2);
            format!("fn line_{i}() {{ let text = \"{}\"; }}", &repeated[..width])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
pub fn diff_with_many_hunks(lines: usize, every: usize) -> DiffResult {
    let (old, new) = generate_with_changes(lines, every);
    let old_buf = TextBuffer::new(&old);
    let new_buf = TextBuffer::new(&new);
    DiffResult::compute(&old_buf, &new_buf)
}
