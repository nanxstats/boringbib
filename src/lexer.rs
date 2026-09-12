//! Character-level scanning over the source text.
//!
//! BibTeX has no context-free token stream: what a character means depends on
//! where the parser is (junk between blocks, an entry header, a field value).
//! This module therefore provides a cursor ([`Lexer`]) with the scanning
//! primitives the grammar needs, the character classes BibTeX uses, source
//! positions, and the parse error type. The grammar itself is in
//! [`crate::parser`].
//!
//! # Character classes
//!
//! * White space is ASCII white space: space, tab, line feed, carriage return
//!   and form feed. BibTeX itself only knows space, tab and newline; carriage
//!   return is included so that CRLF files scan without being normalized
//!   first, which keeps the tree byte-exact.
//! * An identifier (entry type, field name, macro name) is a non-empty run of
//!   characters that are neither white space nor one of `" # % ' ( ) , = { }`.
//!   This is BibTeX's `id_class`, except that BibTeX also rejects a leading
//!   digit and any non-ASCII character; boringbib accepts both, so that it
//!   never has to refuse a file over a name it would print back unchanged.
//! * A citation key is a possibly empty run of characters that are neither
//!   white space nor one of `, { } ( ) "`.

use std::fmt;

use crate::cst::Span;

/// A syntax error, with the position where parsing stopped.
///
/// Positions are 1-based. The column counts characters rather than bytes, so
/// that it matches what editors display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the source (which excludes any byte-order mark).
    pub offset: usize,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column, in characters.
    pub col: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Returns true for the characters treated as white space.
pub fn is_whitespace(c: char) -> bool {
    c.is_ascii_whitespace()
}

/// Returns true for the characters allowed in identifiers.
pub fn is_identifier_char(c: char) -> bool {
    !is_whitespace(c)
        && !matches!(
            c,
            '"' | '#' | '%' | '\'' | '(' | ')' | ',' | '=' | '{' | '}'
        )
}

/// Returns true for the characters allowed in citation keys.
pub fn is_key_char(c: char) -> bool {
    !is_whitespace(c) && !matches!(c, ',' | '{' | '}' | '(' | ')' | '"')
}

/// Converts a byte offset into a 1-based `(line, column)` pair.
///
/// Lines end at `\n` (so a `\r\n` pair ends its line at the `\n`); the column
/// counts characters since the last line break. An offset inside a multi-byte
/// character is rounded down to the character's start; an offset past the end
/// of the text maps to the position just after the last character.
pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(src.len());
    while !src.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &src[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}

/// A cursor over the source text.
///
/// Every `scan_*` method leaves the cursor just after what it consumed, or
/// where it was if it consumed nothing. Spans returned by the `scan_braced`,
/// `scan_quoted` and `scan_parenthesized` methods exclude the delimiters.
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Creates a cursor at the start of `src`.
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    /// The whole source text.
    pub fn src(&self) -> &'a str {
        self.src
    }

    /// The current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Moves the cursor to `pos`, which must be a character boundary.
    ///
    /// The parser uses this to back up after a failed speculative match.
    pub fn set_pos(&mut self, pos: usize) {
        debug_assert!(
            self.src.is_char_boundary(pos),
            "offset {pos} is not a char boundary"
        );
        self.pos = pos;
    }

    /// True once the whole source has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// The next character, without consuming it.
    pub fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// Consumes and returns the next character.
    pub fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Consumes `c` if it is the next character.
    pub fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// Skips white space.
    pub fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if !is_whitespace(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
    }

    /// The byte offset of the next `c` at or after the cursor, without moving.
    pub fn find(&self, c: char) -> Option<usize> {
        self.src[self.pos..].find(c).map(|i| self.pos + i)
    }

    /// Scans an identifier.
    ///
    /// Returns `None`, without moving, if the next character cannot start one.
    pub fn scan_identifier(&mut self) -> Option<Span> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if !is_identifier_char(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
        (self.pos > start).then(|| Span::new(start, self.pos))
    }

    /// Scans a citation key, which may be empty.
    pub fn scan_key(&mut self) -> Span {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if !is_key_char(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
        Span::new(start, self.pos)
    }

    /// Scans a `{...}` group with balanced nested braces.
    ///
    /// The cursor must be at the opening brace. On success it ends after the
    /// matching closing brace and the span *between* the braces is returned.
    /// An unclosed group is reported at its opening brace.
    pub fn scan_braced(&mut self) -> Result<Span, ParseError> {
        let open = self.pos;
        debug_assert_eq!(self.peek(), Some('{'));
        self.pos += 1;
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => {
                    return Err(self.error_at(open, "unbalanced braces: this `{` is never closed"));
                }
                Some('{') => depth += 1,
                Some('}') => {
                    if depth == 0 {
                        return Ok(Span::new(start, self.pos - 1));
                    }
                    depth -= 1;
                }
                Some(_) => {}
            }
        }
    }

    /// Scans a `"..."` string.
    ///
    /// Braces inside the string must balance, and a `"` may only appear
    /// inside braces. The cursor must be at the opening quote; on success it
    /// ends after the closing quote and the span between the quotes is
    /// returned.
    pub fn scan_quoted(&mut self) -> Result<Span, ParseError> {
        let open = self.pos;
        debug_assert_eq!(self.peek(), Some('"'));
        self.pos += 1;
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => return Err(self.error_at(open, "unterminated quoted string")),
                Some('{') => depth += 1,
                Some('}') => {
                    if depth == 0 {
                        return Err(self.error_at(
                            self.pos - 1,
                            "unbalanced braces: unexpected `}` inside a quoted string",
                        ));
                    }
                    depth -= 1;
                }
                Some('"') if depth == 0 => return Ok(Span::new(start, self.pos - 1)),
                Some(_) => {}
            }
        }
    }

    /// Scans a `(...)` body whose braces must balance.
    ///
    /// The body ends at the first `)` at brace depth 0; parentheses do not
    /// nest, exactly as in BibTeX. The cursor must be at the opening
    /// parenthesis; on success it ends after the closing one and the span
    /// between them is returned.
    pub fn scan_parenthesized(&mut self) -> Result<Span, ParseError> {
        let open = self.pos;
        debug_assert_eq!(self.peek(), Some('('));
        self.pos += 1;
        let start = self.pos;
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => {
                    return Err(
                        self.error_at(open, "unbalanced parentheses: this `(` is never closed")
                    );
                }
                Some('{') => depth += 1,
                Some('}') => {
                    if depth == 0 {
                        return Err(
                            self.error_at(self.pos - 1, "unbalanced braces: unexpected `}`")
                        );
                    }
                    depth -= 1;
                }
                Some(')') if depth == 0 => return Ok(Span::new(start, self.pos - 1)),
                Some(_) => {}
            }
        }
    }

    /// Builds an error located at `offset`.
    pub fn error_at(&self, offset: usize, message: impl Into<String>) -> ParseError {
        let (line, col) = line_col(self.src, offset);
        ParseError {
            offset,
            line,
            col,
            message: message.into(),
        }
    }

    /// Builds an error located at the cursor.
    pub fn error(&self, message: impl Into<String>) -> ParseError {
        self.error_at(self.pos, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'s>(lx: &Lexer<'s>, span: Span) -> &'s str {
        &lx.src()[span.start..span.end]
    }

    #[test]
    fn character_classes() {
        for c in ['a', 'Z', '0', '-', '_', ':', '@', '.', 'é'] {
            assert!(is_identifier_char(c), "{c:?} should be an identifier char");
        }
        for c in "\"#%'(),={} \t\n\r\x0C".chars() {
            assert!(
                !is_identifier_char(c),
                "{c:?} should not be an identifier char"
            );
        }
        for c in ['a', '#', ':', '/', '=', '%', '\'', 'é'] {
            assert!(is_key_char(c), "{c:?} should be a key char");
        }
        for c in ",{}()\" \t\n\r".chars() {
            assert!(!is_key_char(c), "{c:?} should not be a key char");
        }
    }

    #[test]
    fn line_and_column_count_characters() {
        let src = "ab\ncdé\r\nf";
        assert_eq!(line_col(src, 0), (1, 1));
        assert_eq!(line_col(src, 2), (1, 3));
        assert_eq!(line_col(src, 3), (2, 1));
        assert_eq!(line_col(src, 5), (2, 3));
        assert_eq!(
            line_col(src, 6),
            (2, 3),
            "inside a multi-byte char rounds down"
        );
        assert_eq!(line_col(src, 7), (2, 4), "the CR is a column of its own");
        assert_eq!(line_col(src, 9), (3, 1));
        assert_eq!(
            line_col(src, 999),
            (3, 2),
            "past the end clamps to just after the last char"
        );
        assert_eq!(line_col("", 0), (1, 1));
    }

    #[test]
    fn parse_error_display_is_line_col_message() {
        let err = Lexer::new("ab\ncd").error_at(4, "boom");
        assert_eq!(err.to_string(), "2:2: boom");
    }

    #[test]
    fn identifiers_and_keys() {
        let mut lx = Lexer::new("Article {Knuth:1984/a, x");
        let id = lx.scan_identifier().expect("identifier");
        assert_eq!(text(&lx, id), "Article");
        lx.skip_whitespace();
        assert!(lx.eat('{'));
        let key = lx.scan_key();
        assert_eq!(text(&lx, key), "Knuth:1984/a");
        assert_eq!(lx.peek(), Some(','));

        assert!(Lexer::new(",x").scan_identifier().is_none());
        assert!(Lexer::new("").scan_identifier().is_none());
        assert!(Lexer::new("{x").scan_key().is_empty());
    }

    #[test]
    fn braced_scanning() {
        let mut lx = Lexer::new("{a{b{c}}d}rest");
        let span = lx.scan_braced().expect("balanced");
        assert_eq!(text(&lx, span), "a{b{c}}d");
        assert_eq!(lx.peek(), Some('r'));

        let mut lx = Lexer::new("{}");
        assert!(lx.scan_braced().expect("empty group").is_empty());
        assert!(lx.at_end());

        let err = Lexer::new("x\n{a{b}c").tap_to(2).scan_braced().unwrap_err();
        assert_eq!((err.line, err.col), (2, 1));
        assert_eq!(err.message, "unbalanced braces: this `{` is never closed");
    }

    #[test]
    fn quoted_scanning() {
        let mut lx = Lexer::new(r#""a {"} b"x"#);
        let span = lx.scan_quoted().expect("quote inside braces is fine");
        assert_eq!(text(&lx, span), r#"a {"} b"#);
        assert_eq!(lx.peek(), Some('x'));

        let err = Lexer::new("\"abc").scan_quoted().unwrap_err();
        assert_eq!(err.message, "unterminated quoted string");
        assert_eq!((err.line, err.col), (1, 1));

        let err = Lexer::new("\"a}\"").scan_quoted().unwrap_err();
        assert_eq!(err.col, 3);
        assert!(err.message.contains("unexpected `}`"));
    }

    #[test]
    fn parenthesized_scanning() {
        let mut lx = Lexer::new("(a {)} b) c");
        let span = lx.scan_parenthesized().expect("balanced");
        assert_eq!(text(&lx, span), "a {)} b");
        assert_eq!(lx.peek(), Some(' '));

        let err = Lexer::new("(a {b} c").scan_parenthesized().unwrap_err();
        assert_eq!(
            err.message,
            "unbalanced parentheses: this `(` is never closed"
        );
        assert_eq!((err.line, err.col), (1, 1));

        let err = Lexer::new("(a } b)").scan_parenthesized().unwrap_err();
        assert_eq!(err.col, 4);

        let mut lx = Lexer::new("(a (b) c");
        let span = lx.scan_parenthesized().expect("parens do not nest");
        assert_eq!(text(&lx, span), "a (b");
    }

    #[test]
    fn cursor_basics() {
        let mut lx = Lexer::new(" \t\r\nx");
        lx.skip_whitespace();
        assert_eq!(lx.pos(), 4);
        assert_eq!(lx.bump(), Some('x'));
        assert!(lx.at_end());
        assert_eq!(lx.bump(), None);
        lx.set_pos(0);
        assert_eq!(lx.find('x'), Some(4));
        assert_eq!(lx.find('y'), None);
    }

    impl<'a> Lexer<'a> {
        /// Test helper: move to `pos` and return self for chaining.
        fn tap_to(mut self, pos: usize) -> Self {
            self.set_pos(pos);
            self
        }
    }
}
