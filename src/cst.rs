//! The lossless concrete syntax tree.
//!
//! A [`Cst`] owns the exact source text and a list of [`Block`]s that tile it
//! from the first byte to the last: every byte of the input belongs to exactly
//! one block, and [`Cst::to_source`] reproduces the input byte for byte. Nodes
//! never copy text; they carry [`Span`]s into the source. Everything the
//! formatter changes (case, white space, delimiters, order) is therefore a
//! decision of the printer, never of the parser, and a targeted edit such as
//! renaming a key is a splice of the source at a span, see
//! [`Cst::with_replacements`].
//!
//! # Blocks
//!
//! ```text
//! file      := block*
//! block     := junk | comment | preamble | string | entry
//! junk      := any text outside the other blocks
//! comment   := '@' 'comment' ( '{' balanced '}' | '(' balanced ')' )
//! preamble  := '@' 'preamble' ( '{' value '}' | '(' value ')' )
//! string    := '@' 'string' ( '{' ident '=' value '}' | '(' ident '=' value ')' )
//! entry     := '@' ident ( '{' key fields '}' | '(' key fields ')' )
//! fields    := ( ',' ident '=' value )* ','?
//! value     := part ( '#' part )*
//! part      := '{' balanced '}' | '"' quoted '"' | number | ident
//! ```
//!
//! Names are case-insensitive; the tree stores them as written.

use crate::lexer;

/// A byte range into the source text; `end` is exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    /// Start offset, inclusive.
    pub start: usize,
    /// End offset, exclusive.
    pub end: usize,
}

impl Span {
    /// Creates a span; `start` must not exceed `end`.
    pub const fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// Length in bytes.
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// True if the span covers no bytes.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// The delimiter pair a block was written with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delim {
    /// `{` ... `}`
    Brace,
    /// `(` ... `)`
    Paren,
}

impl Delim {
    /// The opening character.
    pub const fn open(self) -> char {
        match self {
            Self::Brace => '{',
            Self::Paren => '(',
        }
    }

    /// The closing character.
    pub const fn close(self) -> char {
        match self {
            Self::Brace => '}',
            Self::Paren => ')',
        }
    }
}

/// A non-fatal problem found while parsing, such as a duplicated field.
///
/// Warnings never change the exit status. The CLI prints them as
/// `file:line:col: warning: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Byte offset the warning refers to.
    pub offset: usize,
    /// Human-readable message.
    pub message: String,
}

/// A parsed file: the source text plus the blocks that tile it.
#[derive(Debug, Clone)]
pub struct Cst {
    bom: bool,
    source: String,
    blocks: Vec<Block>,
    warnings: Vec<Warning>,
}

impl Cst {
    /// Assembles a tree from its parts. The parser is the normal caller.
    ///
    /// # Panics
    ///
    /// Panics if `blocks` do not tile `source` exactly, in order, since such
    /// a tree could not reproduce its source.
    pub fn new(bom: bool, source: String, blocks: Vec<Block>, warnings: Vec<Warning>) -> Self {
        let cst = Self {
            bom,
            source,
            blocks,
            warnings,
        };
        assert!(cst.is_tiled(), "blocks do not tile the source");
        cst
    }

    /// True if the input started with a UTF-8 byte-order mark.
    ///
    /// The mark is not part of [`Cst::source`]; offsets are relative to the
    /// text after it.
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// The source text without the byte-order mark.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The blocks, in file order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Warnings collected while parsing, in file order.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The text covered by `span`.
    pub fn text(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    /// Reproduces the input byte for byte, including the byte-order mark.
    pub fn to_source(&self) -> String {
        self.with_replacements(&[])
    }

    /// Converts a byte offset into a 1-based `(line, column)` pair.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        lexer::line_col(&self.source, offset)
    }

    /// The entries, in file order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.blocks.iter().filter_map(|block| match block {
            Block::Entry(entry) => Some(entry),
            _ => None,
        })
    }

    /// The raw text of a value: the parts' contents concatenated, without
    /// delimiters. Macros are not resolved; they contribute their name.
    pub fn value_text(&self, value: &Value) -> String {
        value
            .parts
            .iter()
            .map(|part| self.text(part.inner()))
            .collect()
    }

    /// Returns the source with each span replaced by the paired text.
    ///
    /// This is the primitive behind every targeted edit: the result contains
    /// every byte of the input except the replaced ranges. The byte-order
    /// mark, if any, is kept. Spans must lie on character boundaries and must
    /// not overlap; they may be given in any order.
    ///
    /// # Panics
    ///
    /// Panics if two spans overlap or a span is out of bounds, which would be
    /// a bug in the caller.
    pub fn with_replacements(&self, edits: &[(Span, String)]) -> String {
        let mut edits: Vec<&(Span, String)> = edits.iter().collect();
        edits.sort_by_key(|(span, _)| (span.start, span.end));
        let mut out = String::with_capacity(self.source.len() + 3);
        if self.bom {
            out.push('\u{FEFF}');
        }
        let mut cursor = 0;
        for (span, replacement) in edits {
            assert!(
                span.start >= cursor && span.end <= self.source.len(),
                "replacement spans overlap or exceed the source"
            );
            out.push_str(&self.source[cursor..span.start]);
            out.push_str(replacement);
            cursor = span.end;
        }
        out.push_str(&self.source[cursor..]);
        out
    }

    /// True if the blocks cover the source exactly once, in order.
    fn is_tiled(&self) -> bool {
        let mut cursor = 0;
        for block in &self.blocks {
            let span = block.span();
            if span.start != cursor {
                return false;
            }
            cursor = span.end;
        }
        cursor == self.source.len()
    }
}

/// One top-level piece of a file.
#[derive(Debug, Clone)]
pub enum Block {
    /// Text outside any `@` block, preserved verbatim.
    Junk(Junk),
    /// `@comment{...}`, preserved verbatim.
    Comment(Comment),
    /// `@preamble{...}`.
    Preamble(Preamble),
    /// `@string{name = value}`.
    StringDef(StringDef),
    /// A bibliography entry.
    Entry(Entry),
}

impl Block {
    /// The bytes the block covers, delimiters included.
    pub fn span(&self) -> Span {
        match self {
            Self::Junk(b) => b.span,
            Self::Comment(b) => b.span,
            Self::Preamble(b) => b.span,
            Self::StringDef(b) => b.span,
            Self::Entry(b) => b.span,
        }
    }

    /// True for the blocks that attach to the next block when sorting:
    /// junk and `@comment`.
    pub fn is_trivia(&self) -> bool {
        matches!(self, Self::Junk(_) | Self::Comment(_))
    }
}

/// Text between blocks. BibTeX ignores it; people use it for comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Junk {
    /// The text, verbatim.
    pub span: Span,
}

/// `@comment{...}` with a brace-balanced body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// The whole block.
    pub span: Span,
    /// The block name as written (`comment`, `Comment`, ...).
    pub name: Span,
    /// The delimiter pair used.
    pub delim: Delim,
    /// The body between the delimiters, verbatim.
    pub body: Span,
}

/// `@preamble{...}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preamble {
    /// The whole block.
    pub span: Span,
    /// The block name as written.
    pub name: Span,
    /// The delimiter pair used.
    pub delim: Delim,
    /// The body between the delimiters, verbatim (this is what gets printed).
    pub body: Span,
    /// The body parsed as a value (this is what gets validated).
    pub value: Value,
}

/// `@string{name = value}`, a macro definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringDef {
    /// The whole block.
    pub span: Span,
    /// The block name as written.
    pub name: Span,
    /// The delimiter pair used.
    pub delim: Delim,
    /// The macro name as written.
    pub macro_name: Span,
    /// The macro's value.
    pub value: Value,
}

/// A bibliography entry: `@type{key, field = value, ...}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The whole block.
    pub span: Span,
    /// The entry type as written (`Article`, `INPROCEEDINGS`, ...).
    pub kind: Span,
    /// The delimiter pair used.
    pub delim: Delim,
    /// The citation key as written; may be empty.
    pub key: Span,
    /// The fields, in file order, duplicates included.
    pub fields: Vec<Field>,
    /// True if a comma follows the last field.
    pub trailing_comma: bool,
}

impl Entry {
    /// The first field with this name, compared case-insensitively.
    ///
    /// BibTeX uses the first occurrence of a duplicated field, so this is the
    /// field that counts.
    pub fn field<'c>(&'c self, cst: &Cst, name: &str) -> Option<&'c Field> {
        self.fields
            .iter()
            .find(|field| cst.text(field.name).eq_ignore_ascii_case(name))
    }
}

/// `name = value` inside an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// From the start of the name to the end of the value.
    pub span: Span,
    /// The field name as written.
    pub name: Span,
    /// The value.
    pub value: Value,
}

/// A field value: one or more parts joined by `#`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// From the start of the first part to the end of the last.
    pub span: Span,
    /// The parts, in order; never empty.
    pub parts: Vec<Part>,
}

/// One part of a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The part including its delimiters, if any.
    pub span: Span,
    /// What kind of part it is.
    pub kind: PartKind,
}

impl Part {
    /// The text of the part without delimiters: the inside of a string, or
    /// the whole token for a number or a macro.
    pub fn inner(&self) -> Span {
        match self.kind {
            PartKind::Braced { inner } | PartKind::Quoted { inner } => inner,
            PartKind::Number | PartKind::Macro => self.span,
        }
    }

    /// True for `{...}` and `"..."` parts.
    pub fn is_string(&self) -> bool {
        matches!(self.kind, PartKind::Braced { .. } | PartKind::Quoted { .. })
    }
}

/// The kind of a value part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    /// `{...}`; `inner` excludes the braces.
    Braced {
        /// The text between the braces.
        inner: Span,
    },
    /// `"..."`; `inner` excludes the quotes.
    Quoted {
        /// The text between the quotes.
        inner: Span,
    },
    /// A bare run of digits.
    Number,
    /// A bare macro name such as `jan` or `jmlr`; never resolved.
    Macro,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn junk_only(bom: bool, source: &str) -> Cst {
        let span = Span::new(0, source.len());
        Cst::new(
            bom,
            source.to_owned(),
            vec![Block::Junk(Junk { span })],
            Vec::new(),
        )
    }

    #[test]
    fn spans() {
        let span = Span::new(2, 5);
        assert_eq!(span.len(), 3);
        assert!(!span.is_empty());
        assert!(Span::new(4, 4).is_empty());
    }

    #[test]
    fn delimiters() {
        assert_eq!((Delim::Brace.open(), Delim::Brace.close()), ('{', '}'));
        assert_eq!((Delim::Paren.open(), Delim::Paren.close()), ('(', ')'));
    }

    #[test]
    fn to_source_round_trips_including_bom() {
        let cst = junk_only(false, "hello\r\nworld");
        assert_eq!(cst.to_source(), "hello\r\nworld");
        let cst = junk_only(true, "hello");
        assert!(cst.has_bom());
        assert_eq!(cst.source(), "hello");
        assert_eq!(cst.to_source(), "\u{FEFF}hello");
    }

    #[test]
    fn replacements_splice_in_any_order() {
        let cst = junk_only(true, "@misc{old, crossref = {old}}");
        let edits = vec![
            (Span::new(23, 26), "new".to_owned()),
            (Span::new(6, 9), "new".to_owned()),
        ];
        assert_eq!(
            cst.with_replacements(&edits),
            "\u{FEFF}@misc{new, crossref = {new}}"
        );
        assert_eq!(cst.text(Span::new(6, 9)), "old");
    }

    #[test]
    #[should_panic(expected = "overlap")]
    fn overlapping_replacements_are_a_bug() {
        let cst = junk_only(false, "abcdef");
        let edits = vec![
            (Span::new(0, 3), String::new()),
            (Span::new(2, 4), String::new()),
        ];
        let _ = cst.with_replacements(&edits);
    }

    #[test]
    fn line_col_delegates_to_lexer() {
        let cst = junk_only(false, "a\nbc");
        assert_eq!(cst.line_col(3), (2, 2));
    }

    #[test]
    fn entry_field_lookup_is_case_insensitive_and_first_wins() {
        let source = "@misc{k, Title = {A}, title = {B}}";
        let field = |name: Span, inner: Span| Field {
            span: Span::new(name.start, inner.end + 1),
            name,
            value: Value {
                span: Span::new(inner.start - 1, inner.end + 1),
                parts: vec![Part {
                    span: Span::new(inner.start - 1, inner.end + 1),
                    kind: PartKind::Braced { inner },
                }],
            },
        };
        let entry = Entry {
            span: Span::new(0, source.len()),
            kind: Span::new(1, 5),
            delim: Delim::Brace,
            key: Span::new(6, 7),
            fields: vec![
                field(Span::new(9, 14), Span::new(18, 19)),
                field(Span::new(22, 27), Span::new(31, 32)),
            ],
            trailing_comma: false,
        };
        let cst = Cst::new(
            false,
            source.to_owned(),
            vec![Block::Entry(entry)],
            Vec::new(),
        );
        let entry = cst.entries().next().expect("one entry");
        let found = entry.field(&cst, "TITLE").expect("found");
        assert_eq!(cst.text(found.value.parts[0].inner()), "A");
        assert_eq!(cst.value_text(&found.value), "A");
        assert!(entry.field(&cst, "author").is_none());
        assert_eq!(cst.text(entry.key), "k");
        assert!(!cst.blocks()[0].is_trivia());
    }
}
