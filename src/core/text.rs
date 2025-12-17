//! Text buffer with O(1) line slicing.

use std::sync::Arc;

/// A text buffer optimized for line-based access.
///
/// - Stores bytes as `Arc<[u8]>` for cheap cloning.
/// - Precomputes line start offsets for O(1) line slicing.
/// - Handles missing trailing newline.
/// - Normalizes CRLF to LF internally.
#[derive(Debug, Clone)]
pub struct TextBuffer {
    /// Raw bytes (CRLF normalized to LF).
    bytes: Arc<[u8]>,
    /// Byte offsets where each line starts. Always starts with 0.
    /// Length = line_count + 1 (last entry is bytes.len()).
    line_starts: Vec<usize>,
    /// Whether content appears to be binary.
    is_binary: bool,
}

impl TextBuffer {
    /// Create a new TextBuffer from raw bytes.
    /// Normalizes CRLF to LF. Detects binary content.
    pub fn new(input: &[u8]) -> Self {
        let is_binary = detect_binary(input);
        let bytes = normalize_crlf(input);
        let line_starts = compute_line_starts(&bytes);
        Self {
            bytes: bytes.into(),
            line_starts,
            is_binary,
        }
    }

    /// Create an empty TextBuffer.
    pub fn empty() -> Self {
        Self {
            bytes: Arc::from([]),
            line_starts: vec![0, 0],
            is_binary: false,
        }
    }

    /// Whether content appears to be binary (contains NUL bytes or high ratio of non-text).
    pub fn is_binary(&self) -> bool {
        self.is_binary
    }

    /// Number of lines in the buffer.
    /// An empty buffer has 0 lines.
    /// A buffer with content always has at least 1 line.
    pub fn line_count(&self) -> usize {
        if self.bytes.is_empty() {
            0
        } else {
            self.line_starts.len() - 1
        }
    }

    /// Get the bytes for a specific line (0-indexed).
    /// Does not include the trailing newline.
    /// Returns None if line_num is out of bounds.
    pub fn line(&self, line_num: usize) -> Option<&[u8]> {
        if line_num >= self.line_count() {
            return None;
        }
        let start = self.line_starts[line_num];
        let end = self.line_starts[line_num + 1];
        // Exclude trailing newline if present
        let end = if end > start && self.bytes.get(end - 1) == Some(&b'\n') {
            end - 1
        } else {
            end
        };
        Some(&self.bytes[start..end])
    }

    /// Get line as a lossy UTF-8 string.
    pub fn line_str(&self, line_num: usize) -> Option<String> {
        self.line(line_num)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// Get all lines as strings (for diffing).
    /// Invalid UTF-8 is replaced with U+FFFD.
    pub fn lines(&self) -> Vec<std::borrow::Cow<'_, str>> {
        (0..self.line_count())
            .filter_map(|i| self.line(i).map(String::from_utf8_lossy))
            .collect()
    }

    /// Total byte length.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Is the buffer empty?
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Raw bytes access.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Normalize CRLF to LF.
fn normalize_crlf(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
            output.push(b'\n');
            i += 2;
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    output
}

/// Compute line start offsets.
fn compute_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    // If file doesn't end with newline, we still need the end marker
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        starts.push(bytes.len());
    }
    starts
}

/// Detect if content is likely binary.
/// Uses git's heuristic: NUL byte in first 8000 bytes.
fn detect_binary(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(8000);
    bytes[..check_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let buf = TextBuffer::new(b"");
        assert_eq!(buf.line_count(), 0);
        assert!(buf.is_empty());
        assert_eq!(buf.line(0), None);
    }

    #[test]
    fn single_line_no_newline() {
        let buf = TextBuffer::new(b"hello");
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), Some(b"hello".as_slice()));
        assert_eq!(buf.line(1), None);
    }

    #[test]
    fn single_line_with_newline() {
        let buf = TextBuffer::new(b"hello\n");
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), Some(b"hello".as_slice()));
    }

    #[test]
    fn multiple_lines() {
        let buf = TextBuffer::new(b"one\ntwo\nthree");
        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line_str(0), Some("one".to_string()));
        assert_eq!(buf.line_str(1), Some("two".to_string()));
        assert_eq!(buf.line_str(2), Some("three".to_string()));
    }

    #[test]
    fn crlf_normalization() {
        let buf = TextBuffer::new(b"one\r\ntwo\r\n");
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line_str(0), Some("one".to_string()));
        assert_eq!(buf.line_str(1), Some("two".to_string()));
    }

    #[test]
    fn trailing_newline() {
        let buf = TextBuffer::new(b"a\nb\n");
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line_str(0), Some("a".to_string()));
        assert_eq!(buf.line_str(1), Some("b".to_string()));
    }

    #[test]
    fn lines_iterator() {
        let buf = TextBuffer::new(b"a\nb\nc");
        let lines: Vec<String> = buf.lines().into_iter().map(|c| c.into_owned()).collect();
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn binary_detection() {
        // Binary file (contains NUL)
        let buf = TextBuffer::new(b"hello\x00world");
        assert!(buf.is_binary());

        // Text file
        let buf = TextBuffer::new(b"hello world\n");
        assert!(!buf.is_binary());
    }
}
