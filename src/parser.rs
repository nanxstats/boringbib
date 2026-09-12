//! The BibTeX grammar: turns text into a [`Cst`].
//!
//! The grammar is transcribed from what BibTeX itself accepts (see
//! `bibtex.web`, and biblib's faithful Python transcription of it), with two
//! deliberate relaxations that never lose data:
//!
//! * `@` starts a block only when it is followed by optional white space, an
//!   identifier, optional white space and `{` or `(`. Any other `@` is junk,
//!   so prose between entries never causes an error. When such a `@name` sits
//!   at the start of a line and is not `@comment`, a warning is issued, since
//!   it usually is a broken entry.
//! * Identifiers may start with a digit and may contain non-ASCII characters.
//!
//! And one deliberate deviation from BibTeX, shared with every modern tool:
//! `@comment{...}` has a brace-balanced body. (BibTeX itself skips from
//! `@comment` to the next `@`, braces or not.)
//!
//! Errors stop parsing at the first problem and are reported with a position;
//! the file is never partially interpreted. Non-fatal findings (a duplicated
//! field name, a suspicious `@name`) become [`Warning`]s.

use crate::cst::{
    Block, Comment, Cst, Delim, Entry, Field, Junk, Part, PartKind, Preamble, Span, StringDef,
    Value, Warning,
};
use crate::lexer::{self, Lexer, ParseError};

/// Parses a whole file.
///
/// A leading byte-order mark is stripped and recorded on the tree. Line
/// endings are not normalized: the tree is byte-exact.
pub fn parse(input: &str) -> Result<Cst, ParseError> {
    let (bom, src) = match input.strip_prefix('\u{FEFF}') {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    let mut parser = Parser {
        lx: Lexer::new(src),
        blocks: Vec::new(),
        warnings: Vec::new(),
    };
    parser.parse_file()?;
    Ok(Cst::new(
        bom,
        src.to_owned(),
        parser.blocks,
        parser.warnings,
    ))
}

/// Where a block begins: the `@`, its name, and the delimiter that follows.
struct BlockStart {
    at: usize,
    name: Span,
    delim: Delim,
    delim_at: usize,
}

struct Parser<'a> {
    lx: Lexer<'a>,
    blocks: Vec<Block>,
    warnings: Vec<Warning>,
}

impl<'a> Parser<'a> {
    fn text(&self, span: Span) -> &'a str {
        &self.lx.src()[span.start..span.end]
    }

    fn parse_file(&mut self) -> Result<(), ParseError> {
        let mut junk_start = 0;
        while let Some(start) = self.find_block_start() {
            if start.at > junk_start {
                self.blocks.push(Block::Junk(Junk {
                    span: Span::new(junk_start, start.at),
                }));
            }
            let block = self.parse_block(&start)?;
            self.blocks.push(block);
            junk_start = self.lx.pos();
        }
        let end = self.lx.src().len();
        if end > junk_start {
            self.blocks.push(Block::Junk(Junk {
                span: Span::new(junk_start, end),
            }));
        }
        Ok(())
    }

    /// Finds the next `@` that starts a block, searching from the cursor.
    /// Everything skipped over is junk.
    fn find_block_start(&mut self) -> Option<BlockStart> {
        let src = self.lx.src();
        let mut from = self.lx.pos();
        while let Some(at) = src[from..].find('@').map(|i| from + i) {
            self.lx.set_pos(at + 1);
            self.lx.skip_whitespace();
            if let Some(name) = self.lx.scan_identifier() {
                self.lx.skip_whitespace();
                let delim_at = self.lx.pos();
                let delim = match self.lx.peek() {
                    Some('{') => Some(Delim::Brace),
                    Some('(') => Some(Delim::Paren),
                    _ => None,
                };
                match delim {
                    Some(delim) => {
                        return Some(BlockStart {
                            at,
                            name,
                            delim,
                            delim_at,
                        });
                    }
                    None => self.warn_stray(at, name),
                }
            }
            from = at + 1;
        }
        self.lx.set_pos(src.len());
        None
    }

    /// Warns about `@name` without a delimiter when it starts a line, since
    /// that is usually a broken entry. `@comment` is exempt: BibTeX allows it
    /// without a body, and prose (`user@example.com`) never starts a line
    /// with an `@`.
    fn warn_stray(&mut self, at: usize, name: Span) {
        let src = self.lx.src();
        let line_start = src[..at].rfind('\n').map_or(0, |i| i + 1);
        let line_initial = src[line_start..at].chars().all(lexer::is_whitespace);
        let name_text = self.text(name);
        if line_initial && !name_text.eq_ignore_ascii_case("comment") {
            self.warnings.push(Warning {
                offset: at,
                message: format!(
                    "`@{name_text}` is not followed by `{{` or `(` and was treated as text"
                ),
            });
        }
    }

    fn parse_block(&mut self, start: &BlockStart) -> Result<Block, ParseError> {
        self.lx.set_pos(start.delim_at);
        let name = self.text(start.name).to_ascii_lowercase();
        match name.as_str() {
            "comment" => self.parse_comment(start),
            "preamble" => self.parse_preamble(start),
            "string" => self.parse_string(start),
            _ => self.parse_entry(start),
        }
    }

    fn parse_comment(&mut self, start: &BlockStart) -> Result<Block, ParseError> {
        let body = match start.delim {
            Delim::Brace => self.lx.scan_braced()?,
            Delim::Paren => self.lx.scan_parenthesized()?,
        };
        Ok(Block::Comment(Comment {
            span: Span::new(start.at, self.lx.pos()),
            name: start.name,
            delim: start.delim,
            body,
        }))
    }

    fn parse_preamble(&mut self, start: &BlockStart) -> Result<Block, ParseError> {
        self.lx.bump();
        let body_start = self.lx.pos();
        self.lx.skip_whitespace();
        let value = self.parse_value()?;
        self.lx.skip_whitespace();
        self.expect_close(start.delim)?;
        let end = self.lx.pos();
        Ok(Block::Preamble(Preamble {
            span: Span::new(start.at, end),
            name: start.name,
            delim: start.delim,
            body: Span::new(body_start, end - 1),
            value,
        }))
    }

    fn parse_string(&mut self, start: &BlockStart) -> Result<Block, ParseError> {
        self.lx.bump();
        self.lx.skip_whitespace();
        let Some(macro_name) = self.lx.scan_identifier() else {
            return Err(self.expected("a macro name"));
        };
        self.lx.skip_whitespace();
        if !self.lx.eat('=') {
            let what = format!("`=` after macro name `{}`", self.text(macro_name));
            return Err(self.expected(&what));
        }
        self.lx.skip_whitespace();
        let value = self.parse_value()?;
        self.lx.skip_whitespace();
        self.expect_close(start.delim)?;
        Ok(Block::StringDef(StringDef {
            span: Span::new(start.at, self.lx.pos()),
            name: start.name,
            delim: start.delim,
            macro_name,
            value,
        }))
    }

    fn parse_entry(&mut self, start: &BlockStart) -> Result<Block, ParseError> {
        let close = start.delim.close();
        self.lx.bump();
        self.lx.skip_whitespace();
        let key = self.lx.scan_key();
        let mut fields = Vec::new();
        let mut trailing_comma = false;
        loop {
            self.lx.skip_whitespace();
            if self.lx.eat(close) {
                break;
            }
            if !self.lx.eat(',') {
                return Err(self.expected(&format!("`,` or `{close}`")));
            }
            self.lx.skip_whitespace();
            if self.lx.eat(close) {
                trailing_comma = true;
                break;
            }
            let Some(name) = self.lx.scan_identifier() else {
                return Err(self.expected("a field name"));
            };
            self.lx.skip_whitespace();
            if !self.lx.eat('=') {
                let what = format!("`=` after field name `{}`", self.text(name));
                return Err(self.expected(&what));
            }
            self.lx.skip_whitespace();
            let value = self.parse_value()?;
            fields.push(Field {
                span: Span::new(name.start, value.span.end),
                name,
                value,
            });
        }
        self.warn_duplicate_fields(&fields, key);
        Ok(Block::Entry(Entry {
            span: Span::new(start.at, self.lx.pos()),
            kind: start.name,
            delim: start.delim,
            key,
            fields,
            trailing_comma,
        }))
    }

    fn warn_duplicate_fields(&mut self, fields: &[Field], key: Span) {
        for (i, field) in fields.iter().enumerate() {
            let name = self.text(field.name);
            if fields[..i]
                .iter()
                .any(|earlier| self.text(earlier.name).eq_ignore_ascii_case(name))
            {
                self.warnings.push(Warning {
                    offset: field.name.start,
                    message: format!(
                        "duplicate field `{}` in entry `{}` (BibTeX uses the first)",
                        name.to_ascii_lowercase(),
                        self.text(key)
                    ),
                });
            }
        }
    }

    /// `part ( '#' part )*`. The cursor ends right after the last part, so
    /// the value's span carries no trailing white space.
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let first = self.parse_part()?;
        let start = first.span.start;
        let mut parts = vec![first];
        loop {
            let after_part = self.lx.pos();
            self.lx.skip_whitespace();
            if !self.lx.eat('#') {
                self.lx.set_pos(after_part);
                break;
            }
            self.lx.skip_whitespace();
            parts.push(self.parse_part()?);
        }
        let end = parts.last().map_or(start, |part| part.span.end);
        Ok(Value {
            span: Span::new(start, end),
            parts,
        })
    }

    fn parse_part(&mut self) -> Result<Part, ParseError> {
        let start = self.lx.pos();
        match self.lx.peek() {
            Some('{') => {
                let inner = self.lx.scan_braced()?;
                Ok(Part {
                    span: Span::new(start, self.lx.pos()),
                    kind: PartKind::Braced { inner },
                })
            }
            Some('"') => {
                let inner = self.lx.scan_quoted()?;
                Ok(Part {
                    span: Span::new(start, self.lx.pos()),
                    kind: PartKind::Quoted { inner },
                })
            }
            _ => match self.lx.scan_identifier() {
                Some(span) => {
                    let kind = if self.text(span).bytes().all(|b| b.is_ascii_digit()) {
                        PartKind::Number
                    } else {
                        PartKind::Macro
                    };
                    Ok(Part { span, kind })
                }
                None => {
                    Err(self.expected("a value (`{...}`, `\"...\"`, a number or a macro name)"))
                }
            },
        }
    }

    fn expect_close(&mut self, delim: Delim) -> Result<(), ParseError> {
        if self.lx.eat(delim.close()) {
            Ok(())
        } else {
            Err(self.expected(&format!("`{}`", delim.close())))
        }
    }

    fn expected(&self, what: &str) -> ParseError {
        let found = match self.lx.peek() {
            None => "end of file".to_owned(),
            Some(c) => format!("`{c}`"),
        };
        self.lx.error(format!("expected {what}, found {found}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Cst {
        parse(input).expect("input parses")
    }

    fn parse_err(input: &str) -> ParseError {
        parse(input).expect_err("input does not parse")
    }

    fn entry(cst: &Cst, index: usize) -> &Entry {
        cst.entries().nth(index).expect("entry exists")
    }

    fn field_text<'c>(cst: &'c Cst, entry: &Entry, name: &str) -> &'c str {
        let field = entry.field(cst, name).expect("field exists");
        cst.text(field.value.span)
    }

    #[test]
    fn empty_input() {
        let cst = parse_ok("");
        assert!(cst.blocks().is_empty());
        assert_eq!(cst.to_source(), "");
    }

    #[test]
    fn everything_is_junk_without_blocks() {
        let cst = parse_ok("just some text\nwith @ signs and email@example.com\n");
        assert_eq!(cst.blocks().len(), 1);
        assert!(cst.blocks()[0].is_trivia());
        assert!(cst.warnings().is_empty());
        assert_eq!(
            cst.to_source(),
            "just some text\nwith @ signs and email@example.com\n"
        );
    }

    #[test]
    fn entry_structure() {
        let src = "@Article{Knuth:1984/a,\n  Author = \"Knuth\",\n  year = 1984,\n  month = jan # \" 1st\",\n  note = {a {b} c}\n}\n";
        let cst = parse_ok(src);
        assert_eq!(cst.blocks().len(), 2, "entry plus trailing newline junk");
        let e = entry(&cst, 0);
        assert_eq!(cst.text(e.kind), "Article");
        assert_eq!(cst.text(e.key), "Knuth:1984/a");
        assert_eq!(e.delim, Delim::Brace);
        assert!(!e.trailing_comma);
        assert_eq!(e.fields.len(), 4);
        assert_eq!(cst.text(e.fields[0].name), "Author");
        assert_eq!(field_text(&cst, e, "author"), "\"Knuth\"");
        assert!(matches!(
            e.fields[0].value.parts[0].kind,
            PartKind::Quoted { .. }
        ));
        assert_eq!(cst.text(e.fields[0].value.parts[0].inner()), "Knuth");
        assert_eq!(e.fields[1].value.parts[0].kind, PartKind::Number);
        assert_eq!(field_text(&cst, e, "year"), "1984");
        let month = &e.fields[2].value;
        assert_eq!(month.parts.len(), 2);
        assert_eq!(month.parts[0].kind, PartKind::Macro);
        assert_eq!(cst.text(month.span), "jan # \" 1st\"");
        assert_eq!(cst.text(e.fields[3].value.parts[0].inner()), "a {b} c");
        assert_eq!(cst.text(e.fields[3].span), "note = {a {b} c}");
        assert_eq!(cst.text(e.span), src.trim_end());
    }

    #[test]
    fn parentheses_and_other_blocks() {
        let src = "@string(x = \"y\")@preamble( \"p\" # x )@comment(c {)} d)@misc(k, a = 1)";
        let cst = parse_ok(src);
        assert_eq!(cst.blocks().len(), 4);
        let Block::StringDef(s) = &cst.blocks()[0] else {
            panic!("string");
        };
        assert_eq!(s.delim, Delim::Paren);
        assert_eq!(cst.text(s.macro_name), "x");
        assert_eq!(cst.text(s.value.span), "\"y\"");
        let Block::Preamble(p) = &cst.blocks()[1] else {
            panic!("preamble");
        };
        assert_eq!(cst.text(p.body), " \"p\" # x ");
        assert_eq!(p.value.parts.len(), 2);
        let Block::Comment(c) = &cst.blocks()[2] else {
            panic!("comment");
        };
        assert_eq!(cst.text(c.body), "c {)} d");
        let Block::Entry(e) = &cst.blocks()[3] else {
            panic!("entry");
        };
        assert_eq!(e.delim, Delim::Paren);
        assert_eq!(cst.text(e.key), "k");
        assert_eq!(cst.to_source(), src);
    }

    #[test]
    fn keys_may_be_empty_and_entries_may_have_no_fields() {
        let cst = parse_ok("@misc{k}@misc{k2,}@misc{,title={x}}@misc{}@misc{ spaced }");
        let keys: Vec<&str> = cst.entries().map(|e| cst.text(e.key)).collect();
        assert_eq!(keys, ["k", "k2", "", "", "spaced"]);
        let trailing: Vec<bool> = cst.entries().map(|e| e.trailing_comma).collect();
        assert_eq!(trailing, [false, true, false, false, false]);
        assert_eq!(entry(&cst, 2).fields.len(), 1);
    }

    #[test]
    fn junk_attaches_around_blocks_and_round_trips() {
        let src = "% head\n@misc{a}\n  % mid\n\n@comment{c}@misc{b} tail\n";
        let cst = parse_ok(src);
        let kinds: Vec<&str> = cst
            .blocks()
            .iter()
            .map(|b| match b {
                Block::Junk(_) => "junk",
                Block::Comment(_) => "comment",
                Block::Entry(_) => "entry",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["junk", "entry", "junk", "comment", "entry", "junk"]);
        assert_eq!(cst.text(cst.blocks()[2].span()), "\n  % mid\n\n");
        assert_eq!(cst.to_source(), src);
    }

    #[test]
    fn at_signs_that_do_not_start_blocks() {
        let src =
            "email@example.com\n@article without braces\n@comment no braces\n@ misc {k}\n@@misc{j}";
        let cst = parse_ok(src);
        let keys: Vec<&str> = cst.entries().map(|e| cst.text(e.key)).collect();
        assert_eq!(keys, ["k", "j"]);
        assert_eq!(
            cst.warnings().len(),
            1,
            "only the line-initial @article warns"
        );
        let warning = &cst.warnings()[0];
        assert_eq!(cst.line_col(warning.offset), (2, 1));
        assert!(
            warning.message.starts_with("`@article` is not followed by"),
            "{}",
            warning.message
        );
        assert_eq!(cst.to_source(), src);
    }

    #[test]
    fn identifiers_can_start_with_digits_and_be_non_ascii() {
        let cst = parse_ok("@misc{k, 3d = {x}, naïve = 2nd, ünlü = 42}");
        let e = entry(&cst, 0);
        assert_eq!(cst.text(e.fields[0].name), "3d");
        assert_eq!(cst.text(e.fields[1].name), "naïve");
        assert_eq!(e.fields[1].value.parts[0].kind, PartKind::Macro);
        assert_eq!(e.fields[2].value.parts[0].kind, PartKind::Number);
    }

    #[test]
    fn duplicate_fields_are_kept_and_warned_about() {
        let cst = parse_ok("@misc{k,\n  title = {a},\n  Title = {b},\n  year = 1\n}");
        assert_eq!(entry(&cst, 0).fields.len(), 3);
        assert_eq!(cst.warnings().len(), 1);
        let warning = &cst.warnings()[0];
        assert_eq!(cst.line_col(warning.offset), (3, 3));
        assert_eq!(
            warning.message,
            "duplicate field `title` in entry `k` (BibTeX uses the first)"
        );
        assert_eq!(
            cst.text(
                entry(&cst, 0)
                    .field(&cst, "TITLE")
                    .expect("first")
                    .value
                    .span
            ),
            "{a}"
        );
    }

    #[test]
    fn bom_is_recorded_and_reproduced() {
        let cst = parse_ok("\u{FEFF}@misc{k}");
        assert!(cst.has_bom());
        assert_eq!(cst.source(), "@misc{k}");
        assert_eq!(cst.to_source(), "\u{FEFF}@misc{k}");
        assert_eq!(cst.text(entry(&cst, 0).key), "k");
    }

    #[test]
    fn crlf_is_not_normalized() {
        let src = "@misc{k,\r\n  title = {a\r\n b}\r\n}\r\n";
        let cst = parse_ok(src);
        assert_eq!(
            cst.text(entry(&cst, 0).fields[0].value.parts[0].inner()),
            "a\r\n b"
        );
        assert_eq!(cst.to_source(), src);
    }

    #[test]
    fn error_positions_and_messages() {
        let err = parse_err("@misc{k,\n  title = {unclosed\n");
        assert_eq!((err.line, err.col), (2, 11));
        assert_eq!(err.message, "unbalanced braces: this `{` is never closed");

        let err = parse_err("@misc{k,\n  title = {closed}\n");
        assert_eq!((err.line, err.col), (3, 1));
        assert_eq!(err.message, "expected `,` or `}`, found end of file");

        let err = parse_err("@misc{k, title = \"unclosed");
        assert_eq!((err.line, err.col), (1, 18));
        assert_eq!(err.message, "unterminated quoted string");

        let err = parse_err("@misc{k, title = \"unclosed}");
        assert_eq!((err.line, err.col), (1, 27));
        assert_eq!(
            err.message,
            "unbalanced braces: unexpected `}` inside a quoted string"
        );

        let err = parse_err("@misc{k, title {x}}");
        assert_eq!((err.line, err.col), (1, 16));
        assert_eq!(
            err.message,
            "expected `=` after field name `title`, found `{`"
        );

        let err = parse_err("@misc{k, title = {x} year = 1}");
        assert_eq!((err.line, err.col), (1, 22));
        assert_eq!(err.message, "expected `,` or `}`, found `y`");

        let err = parse_err("@misc(k, title = {x}}");
        assert_eq!(err.message, "expected `,` or `)`, found `}`");

        let err = parse_err("@misc{k, title = 'x'}");
        assert_eq!((err.line, err.col), (1, 18));
        assert_eq!(
            err.message,
            "expected a value (`{...}`, `\"...\"`, a number or a macro name), found `'`"
        );
        assert_eq!(
            parse_ok("@misc{k, title = [x]}")
                .entries()
                .next()
                .expect("entry")
                .fields[0]
                .value
                .parts[0]
                .kind,
            PartKind::Macro,
            "`[` is an identifier character, as in BibTeX"
        );

        let err = parse_err("@misc{k, title = {x}");
        assert_eq!(err.message, "expected `,` or `}`, found end of file");

        let err = parse_err("@misc{k, = {x}}");
        assert_eq!(err.message, "expected a field name, found `=`");

        let err = parse_err("@string{ = \"x\"}");
        assert_eq!(err.message, "expected a macro name, found `=`");

        let err = parse_err("@string{x \"y\"}");
        assert_eq!(err.message, "expected `=` after macro name `x`, found `\"`");

        let err = parse_err("@preamble{\"a\" \"b\"}");
        assert_eq!(err.message, "expected `}`, found `\"`");

        let err = parse_err("@comment{never closed");
        assert_eq!((err.line, err.col), (1, 9));

        let err = parse_err("@misc{k, title = {a} # }");
        assert_eq!(
            err.message,
            "expected a value (`{...}`, `\"...\"`, a number or a macro name), found `}`"
        );
    }

    #[test]
    fn error_position_counts_characters_after_a_bom() {
        let err = parse_err("\u{FEFF}@misc{k, é = 'x'}");
        assert_eq!((err.line, err.col), (1, 14));
        assert_eq!(
            err.offset, 14,
            "byte offset after the mark; `é` is two bytes"
        );
    }

    #[test]
    fn value_span_excludes_trailing_whitespace() {
        let cst = parse_ok("@misc{k, a = {x}   ,\n b = 1 # 2   \n}");
        let e = entry(&cst, 0);
        assert_eq!(cst.text(e.fields[0].value.span), "{x}");
        assert_eq!(cst.text(e.fields[1].value.span), "1 # 2");
        assert_eq!(cst.text(e.fields[1].span), "b = 1 # 2");
    }
}
