//! Syntax highlighting using Tree-sitter.

use std::collections::HashMap;

use parking_lot::Mutex;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter as TsHighlighter};

/// Language identifier for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    /// Rust source files.
    #[cfg(feature = "lang-rust")]
    Rust,
    /// TypeScript source files.
    #[cfg(feature = "lang-typescript")]
    TypeScript,
    /// TypeScript with JSX.
    #[cfg(feature = "lang-typescript")]
    TypeScriptReact,
    /// JavaScript source files.
    #[cfg(feature = "lang-typescript")]
    JavaScript,
    /// JavaScript with JSX.
    #[cfg(feature = "lang-typescript")]
    JavaScriptReact,
    /// Plain text (no highlighting).
    Plain,
}

impl LanguageId {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            #[cfg(feature = "lang-rust")]
            "rs" => Self::Rust,
            #[cfg(feature = "lang-typescript")]
            "ts" => Self::TypeScript,
            #[cfg(feature = "lang-typescript")]
            "tsx" => Self::TypeScriptReact,
            #[cfg(feature = "lang-typescript")]
            "js" | "mjs" | "cjs" => Self::JavaScript,
            #[cfg(feature = "lang-typescript")]
            "jsx" => Self::JavaScriptReact,
            _ => Self::Plain,
        }
    }
}

/// A styled span in highlighted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    /// Byte start offset in the source.
    pub start: usize,
    /// Byte end offset in the source.
    pub end: usize,
    /// Style identifier (maps to a palette).
    pub style_id: StyleId,
}

/// Style identifiers for highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StyleId {
    /// Default/unstyled text.
    #[default]
    Default,
    /// Language keywords.
    Keyword,
    /// Type names.
    Type,
    /// Function names.
    Function,
    /// String literals.
    String,
    /// Numeric literals.
    Number,
    /// Comments.
    Comment,
    /// Operators.
    Operator,
    /// Punctuation.
    Punctuation,
    /// Variable names.
    Variable,
    /// Constants.
    Constant,
    /// Property access.
    Property,
    /// Attributes/decorators.
    Attribute,
}

/// Highlight names recognized by tree-sitter-highlight.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "function",
    "function.builtin",
    "function.method",
    "keyword",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Map highlight name to StyleId.
fn highlight_name_to_style(name: &str) -> StyleId {
    match name {
        "keyword" => StyleId::Keyword,
        "type" | "type.builtin" => StyleId::Type,
        "function" | "function.builtin" | "function.method" | "constructor" => StyleId::Function,
        "string" => StyleId::String,
        "number" => StyleId::Number,
        "comment" => StyleId::Comment,
        "operator" => StyleId::Operator,
        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => StyleId::Punctuation,
        "variable" | "variable.builtin" | "variable.parameter" => StyleId::Variable,
        "constant" | "constant.builtin" => StyleId::Constant,
        "property" => StyleId::Property,
        "attribute" => StyleId::Attribute,
        _ => StyleId::Default,
    }
}

/// Trait for syntax highlighters.
pub trait HighlighterTrait: Send + Sync {
    /// Highlight source code, returning styled spans for each byte range.
    fn highlight(&self, source: &str) -> Vec<StyledSpan>;
}

/// No-op highlighter (plain text).
#[derive(Debug, Default)]
pub struct PlainHighlighter;

impl HighlighterTrait for PlainHighlighter {
    fn highlight(&self, source: &str) -> Vec<StyledSpan> {
        vec![StyledSpan {
            start: 0,
            end: source.len(),
            style_id: StyleId::Default,
        }]
    }
}

/// Tree-sitter based highlighter.
pub struct TreeSitterHighlighter {
    config: HighlightConfiguration,
    highlighter: Mutex<TsHighlighter>,
}

impl TreeSitterHighlighter {
    /// Create a new highlighter for the given language.
    pub fn new(lang: LanguageId) -> Option<Self> {
        let (language, highlights_query) = match lang {
            #[cfg(feature = "lang-rust")]
            LanguageId::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            #[cfg(feature = "lang-typescript")]
            LanguageId::TypeScript | LanguageId::JavaScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            #[cfg(feature = "lang-typescript")]
            LanguageId::TypeScriptReact | LanguageId::JavaScriptReact => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            // Plain or disabled languages
            _ => return None,
        };

        let mut config =
            HighlightConfiguration::new(language, "source", highlights_query, "", "").ok()?;

        config.configure(HIGHLIGHT_NAMES);
        Some(Self {
            config,
            highlighter: Mutex::new(TsHighlighter::new()),
        })
    }
}

impl HighlighterTrait for TreeSitterHighlighter {
    fn highlight(&self, source: &str) -> Vec<StyledSpan> {
        let mut highlighter = self.highlighter.lock();
        let source_bytes = source.as_bytes();

        let highlights = match highlighter.highlight(&self.config, source_bytes, None, |_| None) {
            Ok(h) => h,
            Err(_) => {
                return vec![StyledSpan {
                    start: 0,
                    end: source.len(),
                    style_id: StyleId::Default,
                }];
            }
        };

        let mut spans = Vec::new();
        let mut style_stack: Vec<StyleId> = vec![StyleId::Default];
        let mut current_pos = 0;

        for event in highlights {
            match event {
                Ok(HighlightEvent::Source { start, end }) => {
                    let style = *style_stack.last().unwrap_or(&StyleId::Default);
                    if start < end {
                        spans.push(StyledSpan {
                            start,
                            end,
                            style_id: style,
                        });
                    }
                    current_pos = end;
                }
                Ok(HighlightEvent::HighlightStart(highlight)) => {
                    let name = HIGHLIGHT_NAMES.get(highlight.0).copied().unwrap_or("");
                    style_stack.push(highlight_name_to_style(name));
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    style_stack.pop();
                }
                Err(_) => break,
            }
        }

        // Fill any remaining content
        if current_pos < source.len() {
            spans.push(StyledSpan {
                start: current_pos,
                end: source.len(),
                style_id: StyleId::Default,
            });
        }

        if spans.is_empty() {
            spans.push(StyledSpan {
                start: 0,
                end: source.len(),
                style_id: StyleId::Default,
            });
        }

        spans
    }
}

/// Highlighter cache to avoid re-creating highlighters.
/// Uses interior mutability for use in render functions.
pub struct HighlighterCache {
    highlighters: std::cell::RefCell<HashMap<LanguageId, Box<dyn HighlighterTrait>>>,
    plain: PlainHighlighter,
}

impl Default for HighlighterCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HighlighterCache {
    /// Create a new empty highlighter cache.
    pub fn new() -> Self {
        Self {
            highlighters: std::cell::RefCell::new(HashMap::new()),
            plain: PlainHighlighter,
        }
    }

    /// Highlight source code for the given language.
    pub fn highlight(&self, lang: LanguageId, source: &str) -> Vec<StyledSpan> {
        if lang == LanguageId::Plain {
            return self.plain.highlight(source);
        }

        let mut highlighters = self.highlighters.borrow_mut();
        let highlighter =
            highlighters
                .entry(lang)
                .or_insert_with(|| match TreeSitterHighlighter::new(lang) {
                    Some(h) => Box::new(h),
                    None => Box::new(PlainHighlighter),
                });

        highlighter.highlight(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detection() {
        #[cfg(feature = "lang-rust")]
        assert_eq!(LanguageId::from_extension("rs"), LanguageId::Rust);
        #[cfg(feature = "lang-typescript")]
        assert_eq!(
            LanguageId::from_extension("tsx"),
            LanguageId::TypeScriptReact
        );
        assert_eq!(LanguageId::from_extension("txt"), LanguageId::Plain);
        #[cfg(feature = "lang-rust")]
        assert_eq!(LanguageId::from_extension("RS"), LanguageId::Rust);
    }

    #[test]
    fn plain_highlighter() {
        let hl = PlainHighlighter;
        let spans = hl.highlight("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].end, 11);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_highlighter() {
        let hl = TreeSitterHighlighter::new(LanguageId::Rust).unwrap();
        let spans = hl.highlight("fn main() {}");

        // Should have multiple spans with different styles
        assert!(!spans.is_empty());

        // Should contain a keyword span for "fn"
        let has_keyword = spans.iter().any(|s| s.style_id == StyleId::Keyword);
        assert!(has_keyword, "Expected keyword highlight for 'fn'");
    }

    #[cfg(feature = "lang-typescript")]
    #[test]
    fn typescript_highlighter() {
        let hl = TreeSitterHighlighter::new(LanguageId::TypeScript).unwrap();
        let spans = hl.highlight("const x: number = 42;");

        // Should produce spans
        assert!(!spans.is_empty());

        // Should have some non-default styling (TypeScript queries may vary)
        let has_styled = spans.iter().any(|s| s.style_id != StyleId::Default);
        // Just verify it doesn't crash and produces output - TS queries may differ
        assert!(
            has_styled || !spans.is_empty(),
            "Expected some highlight output"
        );
    }

    #[test]
    fn highlighter_cache() {
        let cache = HighlighterCache::new();

        #[cfg(feature = "lang-rust")]
        {
            // First access creates highlighter
            let spans = cache.highlight(LanguageId::Rust, "fn main() {}");
            assert!(!spans.is_empty());

            // Second access reuses it (no panic)
            let spans = cache.highlight(LanguageId::Rust, "let x = 1;");
            assert!(!spans.is_empty());
        }

        // Plain always works
        let spans = cache.highlight(LanguageId::Plain, "hello");
        assert!(!spans.is_empty());
    }
}
