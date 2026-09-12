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

use crate::cst::Cst;
use crate::lexer::ParseError;
use crate::sort::SortKey;

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
pub fn format(_cst: &Cst, _options: &FmtOptions) -> String {
    todo!("phase 2: printer")
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
