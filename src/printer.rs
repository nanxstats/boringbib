//! Pretty-printing a [`Cst`]: the `fmt` command.
//!
//! The printer walks the tree in the order chosen by [`crate::sort`] and
//! writes every block from scratch; it never copies an entry's original
//! layout. That is what makes the output independent of the input's
//! formatting, and formatting the output again a no-op.
//!
//! # Output rules
//!
//! * Block names, entry types and field names are lowercase; keys are
//!   untouched. Every block uses braces, even if the input used parentheses.
//! * An entry is `@type{key,` on one line, one field per line indented by
//!   [`Indent`], ` = ` between the (padded) name and the value, a comma after
//!   every field except the last (unless `trailing_comma`), and `}` on its
//!   own line. An entry without fields prints as `@type{key` + newline + `}`.
//! * Numbers stay bare and macros stay macros; concatenations print as
//!   `{First } # {edition}`. Inside `{...}` and `"..."` parts every run of
//!   white space collapses to one space, and the whole value is trimmed at
//!   its two ends (not each part, since `{First } # {edition}` needs its
//!   space). With `quotes = braces`, `"..."` parts become `{...}`.
//! * `@string` follows the same value rules; `@preamble` and `@comment`
//!   bodies are printed verbatim (CRLF inside them becomes the output line
//!   ending).
//! * Junk is printed verbatim except that leading and trailing blank lines
//!   are dropped; junk that is only white space disappears.
//! * Exactly one blank line separates blocks. Junk and `@comment` blocks are
//!   printed immediately before the block they precede, with no blank line in
//!   between. The output ends with exactly one newline, or is empty if there
//!   is nothing to print.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

use crate::cst::{Block, Cst, Entry, PartKind, Value};
use crate::lexer::{self, ParseError};
use crate::sort::{self, SortKey};

/// What to do with `"..."` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quotes {
    /// Convert `"..."` to `{...}`.
    #[default]
    Braces,
    /// Leave delimiters as written.
    Keep,
}

/// Indentation of field lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Indent {
    /// This many spaces.
    Spaces(usize),
    /// One tab.
    Tab,
}

impl Default for Indent {
    fn default() -> Self {
        Self::Spaces(2)
    }
}

impl Indent {
    /// The indentation string.
    pub fn as_string(&self) -> String {
        match self {
            Self::Spaces(n) => " ".repeat(*n),
            Self::Tab => "\t".to_owned(),
        }
    }
}

impl fmt::Display for Indent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spaces(n) => write!(f, "{n}"),
            Self::Tab => f.write_str("tab"),
        }
    }
}

impl FromStr for Indent {
    type Err = String;

    /// Accepts `tab` or a number of spaces.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("tab") {
            return Ok(Self::Tab);
        }
        s.parse::<usize>()
            .map(Self::Spaces)
            .map_err(|_| format!("expected a number of spaces or `tab`, got `{s}`"))
    }
}

impl<'de> Deserialize<'de> for Indent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Spaces(usize),
            Word(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Spaces(n) => Ok(Self::Spaces(n)),
            Raw::Word(word) => word.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// Which line ending to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    /// CRLF if the input's first line ending is CRLF, LF otherwise.
    #[default]
    Auto,
    /// `\n`
    Lf,
    /// `\r\n`
    Crlf,
}

/// A concrete line ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newline {
    /// `\n`
    Lf,
    /// `\r\n`
    Crlf,
}

impl Newline {
    /// The line ending of `text`, judged by its first line break.
    ///
    /// Text without a line break counts as LF. Mixed files follow their
    /// first line, which makes the result deterministic and the output
    /// consistent.
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => Self::Crlf,
            _ => Self::Lf,
        }
    }

    /// The characters to write.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }
}

impl LineEnding {
    /// Resolves `Auto` against the input text.
    pub fn resolve(self, input: &str) -> Newline {
        match self {
            Self::Auto => Newline::detect(input),
            Self::Lf => Newline::Lf,
            Self::Crlf => Newline::Crlf,
        }
    }
}

/// The built-in field order used by `--sort-fields`; remaining fields follow
/// alphabetically.
pub const DEFAULT_FIELD_ORDER: &[&str] = &[
    "author",
    "editor",
    "title",
    "booktitle",
    "journal",
    "year",
    "month",
    "volume",
    "number",
    "pages",
    "publisher",
    "address",
    "edition",
    "series",
    "chapter",
    "howpublished",
    "institution",
    "organization",
    "school",
    "type",
    "note",
    "doi",
    "url",
    "urldate",
    "isbn",
    "issn",
    "eprint",
    "archiveprefix",
    "primaryclass",
    "keywords",
    "abstract",
    "file",
];

/// Whether and how to reorder the fields of each entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SortFields {
    /// Keep the fields in file order.
    #[default]
    Off,
    /// Use [`DEFAULT_FIELD_ORDER`].
    Default,
    /// Put these fields first, then the rest of the default order.
    Custom(Vec<String>),
}

impl SortFields {
    /// Builds the option from a comma-separated list; an empty list means the
    /// default order.
    pub fn from_list(list: &str) -> Self {
        let head: Vec<String> = list
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if head.is_empty() {
            Self::Default
        } else {
            Self::Custom(head)
        }
    }

    /// The full field order: the custom head (if any) followed by the default
    /// order without duplicates. `None` when sorting is off.
    pub fn order(&self) -> Option<Vec<String>> {
        let head: &[String] = match self {
            Self::Off => return None,
            Self::Default => &[],
            Self::Custom(head) => head,
        };
        let mut order: Vec<String> = Vec::with_capacity(head.len() + DEFAULT_FIELD_ORDER.len());
        for name in head
            .iter()
            .map(String::as_str)
            .chain(DEFAULT_FIELD_ORDER.iter().copied())
        {
            if !order.iter().any(|seen| seen == name) {
                order.push(name.to_owned());
            }
        }
        Some(order)
    }
}

impl<'de> Deserialize<'de> for SortFields {
    /// Accepts `false`, `true` (default order) or a list of field names.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            List(Vec<String>),
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Flag(false) => Self::Off,
            Raw::Flag(true) => Self::Default,
            Raw::List(list) => Self::from_list(&list.join(",")),
        })
    }
}

/// Formatting options. The defaults reproduce LaTeX Workshop's formatter with
/// `align-equal` and sorting switched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmtOptions {
    /// Ordering of entries.
    pub sort: SortKey,
    /// Indentation of field lines.
    pub indent: Indent,
    /// Pad field names to the longest field name in the entry.
    pub align: bool,
    /// What to do with `"..."` values.
    pub quotes: Quotes,
    /// Put a comma after the last field.
    pub trailing_comma: bool,
    /// Wrap values at this column; `None` leaves values on one line.
    pub wrap: Option<usize>,
    /// Reorder the fields of each entry.
    pub sort_fields: SortFields,
    /// Line ending of the output.
    pub line_ending: LineEnding,
    /// Re-emit a leading byte-order mark if the input had one.
    pub keep_bom: bool,
}

impl Default for FmtOptions {
    fn default() -> Self {
        Self {
            sort: SortKey::Key,
            indent: Indent::default(),
            align: true,
            quotes: Quotes::Braces,
            trailing_comma: false,
            wrap: None,
            sort_fields: SortFields::Off,
            line_ending: LineEnding::Auto,
            keep_bom: false,
        }
    }
}

/// Formats a parsed file.
pub fn format(cst: &Cst, options: &FmtOptions) -> String {
    let mut groups = sort::group(cst);
    sort::sort(cst, &mut groups, options.sort);

    let mut out = String::with_capacity(cst.source().len() + 64);
    if options.keep_bom && cst.has_bom() {
        out.push('\u{FEFF}');
    }
    let mut chunk = String::new();
    let mut printed_any = false;
    for group in &groups {
        chunk.clear();
        for block in &group.leading {
            print_trivia(cst, block, &mut chunk);
        }
        if let Some(block) = group.block {
            print_block(cst, block, options, &mut chunk);
        }
        if chunk.is_empty() {
            continue;
        }
        if printed_any {
            out.push('\n');
        }
        out.push_str(&chunk);
        printed_any = true;
    }
    match options.line_ending.resolve(cst.source()) {
        Newline::Lf => out,
        Newline::Crlf => out.replace('\n', "\r\n"),
    }
}

/// Prints a junk or `@comment` block; every printed piece ends with `\n`.
fn print_trivia(cst: &Cst, block: &Block, out: &mut String) {
    match block {
        Block::Junk(junk) => {
            let text = normalize_newlines(cst.text(junk.span));
            let trimmed = trim_blank_lines(&text);
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        Block::Comment(comment) => {
            out.push_str("@comment{");
            out.push_str(&normalize_newlines(cst.text(comment.body)));
            out.push_str("}\n");
        }
        Block::Preamble(_) | Block::StringDef(_) | Block::Entry(_) => {
            unreachable!("only junk and comments lead a group")
        }
    }
}

fn print_block(cst: &Cst, block: &Block, options: &FmtOptions, out: &mut String) {
    match block {
        Block::Junk(_) | Block::Comment(_) => print_trivia(cst, block, out),
        Block::Preamble(preamble) => {
            out.push_str("@preamble{");
            out.push_str(&normalize_newlines(cst.text(preamble.body)));
            out.push_str("}\n");
        }
        Block::StringDef(string) => {
            out.push_str("@string{");
            out.push_str(cst.text(string.macro_name));
            out.push_str(" = ");
            out.push_str(&format_value(cst, &string.value, options.quotes));
            out.push_str("}\n");
        }
        Block::Entry(entry) => print_entry(cst, entry, options, out),
    }
}

fn print_entry(cst: &Cst, entry: &Entry, options: &FmtOptions, out: &mut String) {
    out.push('@');
    out.push_str(&cst.text(entry.kind).to_ascii_lowercase());
    out.push('{');
    out.push_str(cst.text(entry.key));
    if entry.fields.is_empty() {
        if options.trailing_comma {
            out.push(',');
        }
        out.push_str("\n}\n");
        return;
    }
    let names: Vec<String> = entry
        .fields
        .iter()
        .map(|field| cst.text(field.name).to_ascii_lowercase())
        .collect();
    let width = if options.align {
        names
            .iter()
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let indent = options.indent.as_string();
    let last = entry.fields.len() - 1;
    out.push_str(",\n");
    for (i, (field, name)) in entry.fields.iter().zip(&names).enumerate() {
        out.push_str(&indent);
        out.push_str(name);
        for _ in name.chars().count()..width {
            out.push(' ');
        }
        out.push_str(" = ");
        out.push_str(&format_value(cst, &field.value, options.quotes));
        if i < last || options.trailing_comma {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("}\n");
}

/// Prints a value: parts joined by ` # `, white space collapsed inside
/// strings, the whole value trimmed at its two ends.
fn format_value(cst: &Cst, value: &Value, quotes: Quotes) -> String {
    let last = value.parts.len().saturating_sub(1);
    let pieces: Vec<String> = value
        .parts
        .iter()
        .enumerate()
        .map(|(i, part)| match part.kind {
            PartKind::Braced { inner } | PartKind::Quoted { inner } => {
                let collapsed = collapse_whitespace(cst.text(inner));
                let mut text = collapsed.as_str();
                if i == 0 {
                    text = text.trim_start_matches(' ');
                }
                if i == last {
                    text = text.trim_end_matches(' ');
                }
                let keep_quotes =
                    quotes == Quotes::Keep && matches!(part.kind, PartKind::Quoted { .. });
                if keep_quotes {
                    format!("\"{text}\"")
                } else {
                    format!("{{{text}}}")
                }
            }
            PartKind::Number | PartKind::Macro => cst.text(part.span).to_owned(),
        })
        .collect();
    pieces.join(" # ")
}

/// Replaces every run of white space with a single space.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for c in text.chars() {
        if lexer::is_whitespace(c) {
            if !in_whitespace {
                out.push(' ');
                in_whitespace = true;
            }
        } else {
            out.push(c);
            in_whitespace = false;
        }
    }
    out
}

/// Drops leading and trailing lines that are empty or only white space,
/// including the final line break. Everything in between is untouched.
fn trim_blank_lines(text: &str) -> &str {
    let mut start = None;
    let mut end = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        if !content.chars().all(lexer::is_whitespace) {
            start.get_or_insert(offset);
            end = offset + content.len();
        }
        offset += line.len();
    }
    start.map_or("", |start| &text[start..end])
}

/// Turns CRLF into LF; the printer re-adds CR at the end if asked to.
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Parses and formats text in one step.
pub fn format_str(input: &str, options: &FmtOptions) -> Result<String, ParseError> {
    let cst = crate::parser::parse(input)?;
    Ok(format(&cst, options))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_parses_and_prints() {
        assert_eq!("4".parse::<Indent>(), Ok(Indent::Spaces(4)));
        assert_eq!("tab".parse::<Indent>(), Ok(Indent::Tab));
        assert_eq!("TAB".parse::<Indent>(), Ok(Indent::Tab));
        assert!("two".parse::<Indent>().is_err());
        assert_eq!(Indent::Spaces(3).as_string(), "   ");
        assert_eq!(Indent::Tab.as_string(), "\t");
        assert_eq!(Indent::Spaces(2).to_string(), "2");
        assert_eq!(Indent::Tab.to_string(), "tab");
        assert_eq!(Indent::default(), Indent::Spaces(2));
    }

    #[test]
    fn newline_detection_uses_the_first_line_break() {
        assert_eq!(Newline::detect("a\nb\r\n"), Newline::Lf);
        assert_eq!(Newline::detect("a\r\nb\n"), Newline::Crlf);
        assert_eq!(Newline::detect("\r\n"), Newline::Crlf);
        assert_eq!(Newline::detect("\n"), Newline::Lf);
        assert_eq!(Newline::detect("no line break"), Newline::Lf);
        assert_eq!(Newline::detect(""), Newline::Lf);
        assert_eq!(LineEnding::Auto.resolve("x\r\n"), Newline::Crlf);
        assert_eq!(LineEnding::Lf.resolve("x\r\n"), Newline::Lf);
        assert_eq!(LineEnding::Crlf.resolve("x\n"), Newline::Crlf);
        assert_eq!(Newline::Crlf.as_str(), "\r\n");
    }

    #[test]
    fn sort_fields_order() {
        assert_eq!(SortFields::Off.order(), None);
        let default = SortFields::Default.order().expect("some");
        assert_eq!(default.len(), DEFAULT_FIELD_ORDER.len());
        assert_eq!(default[0], "author");
        let custom = SortFields::from_list(" Year , title,,")
            .order()
            .expect("some");
        assert_eq!(&custom[..3], ["year", "title", "author"]);
        assert_eq!(custom.len(), DEFAULT_FIELD_ORDER.len(), "no duplicates");
        assert_eq!(SortFields::from_list(" , "), SortFields::Default);
    }

    #[test]
    fn options_deserialize_from_toml() {
        #[derive(Debug, Deserialize)]
        struct Probe {
            indent: Indent,
            sort_fields: SortFields,
            quotes: Quotes,
            line_ending: LineEnding,
        }
        let probe: Probe = toml::from_str(
            "indent = \"tab\"\nsort_fields = true\nquotes = \"keep\"\nline_ending = \"crlf\"\n",
        )
        .expect("valid");
        assert_eq!(probe.indent, Indent::Tab);
        assert_eq!(probe.sort_fields, SortFields::Default);
        assert_eq!(probe.quotes, Quotes::Keep);
        assert_eq!(probe.line_ending, LineEnding::Crlf);

        let probe: Probe = toml::from_str(
            "indent = 4\nsort_fields = [\"title\"]\nquotes = \"braces\"\nline_ending = \"auto\"\n",
        )
        .expect("valid");
        assert_eq!(probe.indent, Indent::Spaces(4));
        assert_eq!(
            probe.sort_fields,
            SortFields::Custom(vec!["title".to_owned()])
        );

        let err = toml::from_str::<Probe>(
            "indent = \"wide\"\nsort_fields = false\nquotes = \"braces\"\nline_ending = \"lf\"\n",
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("expected a number of spaces or `tab`"),
            "{err}"
        );
    }

    fn fmt(input: &str) -> String {
        format_str(input, &FmtOptions::default()).expect("input parses")
    }

    fn fmt_with(input: &str, options: &FmtOptions) -> String {
        format_str(input, options).expect("input parses")
    }

    #[test]
    fn nothing_to_print_gives_empty_output() {
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("\n\n \t \n"), "");
        assert_eq!(fmt("\u{FEFF}"), "");
    }

    #[test]
    fn entry_layout_lowercases_and_aligns() {
        let out = fmt("@Article{Key,\n title={T},\n  AUTHOR = \"A\",\n  Year=2001\n}");
        assert_eq!(
            out,
            "@article{Key,\n  title  = {T},\n  author = {A},\n  year   = 2001\n}\n"
        );
    }

    #[test]
    fn entries_without_fields() {
        assert_eq!(fmt("@misc{k}"), "@misc{k\n}\n");
        assert_eq!(fmt("@misc{k,}"), "@misc{k\n}\n");
        assert_eq!(fmt("@misc{}"), "@misc{\n}\n");
        let trailing = FmtOptions {
            trailing_comma: true,
            ..FmtOptions::default()
        };
        assert_eq!(fmt_with("@misc{k}", &trailing), "@misc{k,\n}\n");
        assert_eq!(
            fmt_with("@misc{k, a = 1, b = 2}", &trailing),
            "@misc{k,\n  a = 1,\n  b = 2,\n}\n"
        );
    }

    #[test]
    fn values_collapse_whitespace_and_trim_the_whole_value() {
        let out = fmt(
            "@misc{k,\n  note = \" First \" # \"edition\" # { , ok },\n  x = {  a\n\t b  },\n  m = jan # { 2020 },\n  n = { 2nd } # \"printing\" # 3\n}",
        );
        assert_eq!(
            out,
            "@misc{k,\n  note = {First } # {edition} # { , ok},\n  x    = {a b},\n  m    = jan # { 2020},\n  n    = {2nd } # {printing} # 3\n}\n"
        );
    }

    #[test]
    fn quotes_can_be_kept() {
        let options = FmtOptions {
            quotes: Quotes::Keep,
            ..FmtOptions::default()
        };
        let out = fmt_with(
            "@string{j = \"J\"}@misc{k, a = \"x {\"} y\", b = {z}}",
            &options,
        );
        assert_eq!(
            out,
            "@string{j = \"J\"}\n\n@misc{k,\n  a = \"x {\"} y\",\n  b = {z}\n}\n"
        );
        assert_eq!(
            fmt("@string{j = \"J\"}@misc{k, a = \"x {\"} y\"}"),
            "@string{j = {J}}\n\n@misc{k,\n  a = {x {\"} y}\n}\n"
        );
    }

    #[test]
    fn indent_and_alignment_options() {
        let options = FmtOptions {
            indent: Indent::Tab,
            align: false,
            ..FmtOptions::default()
        };
        assert_eq!(
            fmt_with("@misc{k, a = 1, bbb = 2}", &options),
            "@misc{k,\n\ta = 1,\n\tbbb = 2\n}\n"
        );
        let options = FmtOptions {
            indent: Indent::Spaces(4),
            ..FmtOptions::default()
        };
        assert_eq!(
            fmt_with("@misc{k, a = 1, bbb = 2}", &options),
            "@misc{k,\n    a   = 1,\n    bbb = 2\n}\n"
        );
    }

    #[test]
    fn blocks_are_separated_by_one_blank_line_and_trivia_attaches() {
        let input = "\n\n% head\n\n\n@misc{b}\n\n\n  % note\n@comment{c}\n@misc{a} % same line\n\ntail text\n\n";
        let expected = "  % note\n@comment{c}\n@misc{a\n}\n\n% head\n@misc{b\n}\n\n % same line\n\ntail text\n";
        assert_eq!(fmt(input), expected);
        assert_eq!(fmt(&fmt(input)), expected);
    }

    #[test]
    fn preamble_and_comment_bodies_are_verbatim_and_braced() {
        let input = "@PREAMBLE( \"\\newcommand{\\x}[1]{}\" # \"y\" )\n@Comment(  keep {this}   spacing  )\n@misc(k, t = {x})";
        let expected = "@preamble{ \"\\newcommand{\\x}[1]{}\" # \"y\" }\n\n@comment{  keep {this}   spacing  }\n@misc{k,\n  t = {x}\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn macro_names_and_keys_keep_their_case() {
        assert_eq!(
            fmt("@STRING{JMLR = {J}}@Misc{KeY, Journal = JMLR}"),
            "@string{JMLR = {J}}\n\n@misc{KeY,\n  journal = JMLR\n}\n"
        );
    }

    #[test]
    fn line_endings_follow_the_input_unless_forced() {
        let input = "% c\r\n@misc{k,\r\n  t = {a\r\n b}\r\n}\r\n";
        assert_eq!(fmt(input), "% c\r\n@misc{k,\r\n  t = {a b}\r\n}\r\n");
        let lf = FmtOptions {
            line_ending: LineEnding::Lf,
            ..FmtOptions::default()
        };
        assert_eq!(fmt_with(input, &lf), "% c\n@misc{k,\n  t = {a b}\n}\n");
        let crlf = FmtOptions {
            line_ending: LineEnding::Crlf,
            ..FmtOptions::default()
        };
        assert_eq!(fmt_with("@misc{k}\n", &crlf), "@misc{k\r\n}\r\n");
        assert_eq!(fmt("@comment{a\r\nb}\r\n"), "@comment{a\r\nb}\r\n");
        assert_eq!(
            fmt("x\ry\n@misc{k}"),
            "x\ry\n@misc{k\n}\n",
            "a lone CR is kept"
        );
    }

    #[test]
    fn bom_is_dropped_unless_kept() {
        assert_eq!(fmt("\u{FEFF}@misc{k}"), "@misc{k\n}\n");
        let keep = FmtOptions {
            keep_bom: true,
            ..FmtOptions::default()
        };
        assert_eq!(fmt_with("\u{FEFF}@misc{k}", &keep), "\u{FEFF}@misc{k\n}\n");
        assert_eq!(
            fmt_with("@misc{k}", &keep),
            "@misc{k\n}\n",
            "no mark is invented"
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(collapse_whitespace("  a \t\r\n b  "), " a b ");
        assert_eq!(
            collapse_whitespace("a\u{a0}b"),
            "a\u{a0}b",
            "NBSP is not white space"
        );
        assert_eq!(trim_blank_lines("\n  \n a \n\n b \n \n\n"), " a \n\n b ");
        assert_eq!(trim_blank_lines("x"), "x");
        assert_eq!(trim_blank_lines("  \n\t\n"), "");
        assert_eq!(trim_blank_lines(""), "");
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\rc\n");
    }

    #[test]
    fn defaults_match_latex_workshop() {
        let options = FmtOptions::default();
        assert_eq!(options.sort, SortKey::Key);
        assert_eq!(options.indent, Indent::Spaces(2));
        assert!(options.align);
        assert_eq!(options.quotes, Quotes::Braces);
        assert!(!options.trailing_comma);
        assert_eq!(options.wrap, None);
        assert_eq!(options.sort_fields, SortFields::Off);
        assert_eq!(options.line_ending, LineEnding::Auto);
        assert!(!options.keep_bom);
    }
}
