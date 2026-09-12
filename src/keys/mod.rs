//! Citation key rewriting: the `keys` command.
//!
//! A key is `author_part + year_part + title_part`, in the style Google
//! Scholar uses for its BibTeX export (`vaswani2017attention`). The rules were
//! reverse engineered from real Scholar keys; the corpus is in
//! `tests/fixtures/scholar_keys.tsv` and every row must pass.
//!
//! # Pipeline
//!
//! 1. For every selected entry, compute the base key from its fields
//!    ([`author_part`], [`year_part`], [`title_part`]). Entries the algorithm
//!    cannot handle (no author, no usable title, ...) are reported and left
//!    alone.
//! 2. Resolve collisions: entries sharing a base key are ordered by position,
//!    the first keeps the bare key, the others get `a`, `b`, ... `z`, `aa`,
//!    ... Keys of entries that were not selected still count as taken.
//! 3. Rewrite the values of reference fields (`crossref`, `xref`, and the
//!    comma-separated lists `related`, `ids`, `entryset`, `xdata`) that name a
//!    renamed key, matching whole keys only. References to keys that do not
//!    exist in the file are reported.
//!
//! Everything is computed into a [`Plan`] first; [`apply`] then splices the
//! source text at the recorded spans and changes nothing else. Running the
//! command twice is a no-op.
//!
//! # Extensibility
//!
//! The name and title parsing ([`names`], [`latex`], [`stopwords`]) produce
//! plain [`KeyParts`]; only the last step assembles them into a key. A
//! template engine (JabRef-style patterns) could be added later as another
//! [`Style`] that consumes the same parts, without touching the parsers.

pub mod latex;
pub mod names;
pub mod stopwords;

use serde::Deserialize;

use crate::cst::{Cst, Entry, Span};
use crate::lexer;

/// Fields that hold a single citation key.
pub const REFERENCE_FIELDS_SINGLE: &[&str] = &["crossref", "xref"];

/// Fields that hold a comma-separated list of citation keys.
pub const REFERENCE_FIELDS_LIST: &[&str] = &["related", "ids", "entryset", "xdata"];

/// The key style to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Google Scholar style: `lastname` + `year` + first meaningful title word.
    #[default]
    Scholar,
}

/// Options for [`plan`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeysOptions {
    /// The key style.
    pub style: Style,
    /// Restrict the rewrite to these current keys; `None` selects every entry.
    pub only: Option<Vec<String>>,
    /// Current keys that must never be rewritten (`[keys] keep`).
    pub keep: Vec<String>,
    /// Replacement for the built-in stop word list, if configured.
    pub stop_words: Option<Vec<String>>,
    /// Additional stop words.
    pub stop_words_extra: Vec<String>,
}

impl KeysOptions {
    /// The stop words in effect: the configured list or the built-in one,
    /// plus the extras, all lowercase.
    pub fn effective_stop_words(&self) -> Vec<String> {
        let base: Vec<String> = match &self.stop_words {
            Some(list) => list.clone(),
            None => stopwords::DEFAULT.iter().map(|&w| w.to_owned()).collect(),
        };
        base.into_iter()
            .chain(self.stop_words_extra.iter().cloned())
            .map(|w| w.to_lowercase())
            .collect()
    }
}

/// The three components of a generated key, before assembly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyParts {
    /// Normalized last name of the first author (or editor), e.g. `vandermaaten`.
    pub author: String,
    /// Four-digit year, or empty.
    pub year: String,
    /// First non-stop-word of the title, e.g. `visualizing`.
    pub title: String,
}

impl KeyParts {
    /// Assembles the parts in the given style.
    pub fn assemble(&self, style: Style) -> String {
        match style {
            Style::Scholar => format!("{}{}{}", self.author, self.year, self.title),
        }
    }
}

/// One key change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    /// The key as currently written.
    pub old: String,
    /// The new key.
    pub new: String,
}

/// Something the user should know about: an entry that was skipped, a
/// dangling reference, a missing year.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Byte offset the report refers to.
    pub offset: usize,
    /// Human-readable message.
    pub message: String,
}

/// Everything `keys` would change, plus what it wants to say about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Key changes, in file order. Entries whose key does not change are not
    /// listed.
    pub renames: Vec<Rename>,
    /// The splices that implement the renames and the reference updates.
    pub edits: Vec<(Span, String)>,
    /// Findings, in file order.
    pub reports: Vec<Report>,
}

/// Computes the key changes for a parsed file without touching it.
pub fn plan(_cst: &Cst, _options: &KeysOptions) -> Plan {
    todo!("phase 3: key generation")
}

/// Applies a plan: returns the source with only the planned spans replaced.
pub fn apply(cst: &Cst, plan: &Plan) -> String {
    cst.with_replacements(&plan.edits)
}

/// The author part of a key from an `author` or `editor` field value.
///
/// Splits the name list on `and` at brace depth 0, takes the first name,
/// determines its BibTeX form (see [`names`]), then decodes LaTeX, strips
/// braces, transliterates to ASCII, lowercases, cuts at the first hyphen and
/// drops everything outside `[a-z0-9]`. Returns `None` if there is no usable
/// name.
pub fn author_part(_names: &str) -> Option<String> {
    todo!("phase 3: author part")
}

/// The year part: the first 4-digit number in `year`, else in `date`, else
/// empty.
pub fn year_part(year: Option<&str>, date: Option<&str>) -> String {
    year.and_then(first_four_digit_number)
        .or_else(|| date.and_then(first_four_digit_number))
        .map(str::to_owned)
        .unwrap_or_default()
}

/// The first maximal run of exactly four ASCII digits in `text`.
pub fn first_four_digit_number(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i - start == 4 {
            return Some(&text[start..i]);
        }
    }
    None
}

/// The text of the entry's `author` field, or of `editor` if `author` is
/// absent or blank; `None` if neither is usable.
pub fn names_field(cst: &Cst, entry: &Entry) -> Option<String> {
    ["author", "editor"].iter().find_map(|name| {
        let field = entry.field(cst, name)?;
        let text = cst.value_text(&field.value);
        (!text.chars().all(lexer::is_whitespace)).then_some(text)
    })
}

/// The title part: the first title word that is not a stop word.
///
/// Decodes LaTeX, drops `$...$` math, strips braces, transliterates,
/// lowercases, turns hyphens into spaces, tokenizes on `[a-z0-9']` runs with
/// apostrophes removed, and returns the first token that is not a stop word
/// (or the first token if all are). `None` if the title has no tokens.
pub fn title_part(_title: &str, _stop_words: &[String]) -> Option<String> {
    todo!("phase 3: title part")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_assemble_in_scholar_style() {
        let parts = KeyParts {
            author: "vaswani".to_owned(),
            year: "2017".to_owned(),
            title: "attention".to_owned(),
        };
        assert_eq!(parts.assemble(Style::Scholar), "vaswani2017attention");
        let no_year = KeyParts {
            year: String::new(),
            ..parts
        };
        assert_eq!(no_year.assemble(Style::Scholar), "vaswaniattention");
    }

    #[test]
    fn effective_stop_words_merge_and_lowercase() {
        let options = KeysOptions::default();
        let words = options.effective_stop_words();
        assert_eq!(words.len(), stopwords::DEFAULT.len());
        assert!(words.iter().any(|w| w == "the"));

        let options = KeysOptions {
            stop_words: Some(vec!["A".to_owned()]),
            stop_words_extra: vec!["Towards".to_owned()],
            ..KeysOptions::default()
        };
        assert_eq!(options.effective_stop_words(), ["a", "towards"]);
    }

    #[test]
    fn year_part_takes_the_first_four_digit_run() {
        assert_eq!(year_part(Some("2017"), None), "2017");
        assert_eq!(year_part(Some("c. 1850"), None), "1850");
        assert_eq!(year_part(Some("12345 then 1999"), None), "1999");
        assert_eq!(year_part(Some("n.d."), Some("2021-03-04")), "2021");
        assert_eq!(year_part(None, Some("2021-03-04")), "2021");
        assert_eq!(year_part(None, None), "");
        assert_eq!(first_four_digit_number("99 999 9999 99999"), Some("9999"));
        assert_eq!(first_four_digit_number(""), None);
    }

    #[test]
    fn names_field_prefers_author_then_editor() {
        let cst = crate::parse(
            "@misc{a, author = {A}, editor = {E}}@misc{b, author = { }, editor = {E}}@misc{c, editor = {}}@misc{d}",
        )
        .expect("parses");
        let names: Vec<Option<String>> = cst.entries().map(|e| names_field(&cst, e)).collect();
        assert_eq!(
            names,
            [Some("A".to_owned()), Some("E".to_owned()), None, None]
        );
    }

    #[test]
    fn apply_with_an_empty_plan_is_identity() {
        let source = "@misc{k, title = {T}}";
        let cst = Cst::new(
            false,
            source.to_owned(),
            vec![crate::cst::Block::Junk(crate::cst::Junk {
                span: Span::new(0, source.len()),
            })],
            Vec::new(),
        );
        assert_eq!(apply(&cst, &Plan::default()), source);
    }
}
