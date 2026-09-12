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
//!    cannot handle (no author, no usable title, a field that uses a macro)
//!    are reported and left alone.
//! 2. Resolve collisions, in file order: an entry gets its base key if that
//!    is free, else the base key with suffix `a`, `b`, ... `z`, `aa`, ...
//!    Keys of entries that are not rewritten (not selected, or skipped)
//!    count as taken, compared case-insensitively, so a generated key never
//!    collides with them.
//! 3. Rewrite the values of reference fields (`crossref`, `xref`, and the
//!    comma-separated lists `related`, `ids`, `entryset`, `xdata`) that name a
//!    renamed key, matching whole keys only. References to keys that exist
//!    nowhere in the file are reported.
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

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::cst::{Cst, Entry, PartKind, Span};
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
pub fn plan(cst: &Cst, options: &KeysOptions) -> Plan {
    let stop_words = options.effective_stop_words();
    let entries: Vec<&Entry> = cst.entries().collect();
    let mut plan = Plan::default();

    let keep: HashSet<String> = options.keep.iter().map(|k| k.to_lowercase()).collect();
    let only: Option<HashSet<String>> = options
        .only
        .as_ref()
        .map(|list| list.iter().map(|k| k.to_lowercase()).collect());
    if only.is_some() {
        let present: HashSet<String> = entries
            .iter()
            .map(|entry| cst.text(entry.key).to_lowercase())
            .collect();
        for key in options.only.iter().flatten() {
            if !present.contains(&key.to_lowercase()) {
                plan.reports.push(Report {
                    offset: 0,
                    message: format!("no entry with key `{key}`"),
                });
            }
        }
    }

    // Base keys for the selected entries; everything else counts as taken.
    let mut taken: HashSet<String> = HashSet::new();
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let key = cst.text(entry.key);
        let lower = key.to_lowercase();
        let selected =
            !keep.contains(&lower) && only.as_ref().is_none_or(|only| only.contains(&lower));
        if !selected {
            taken.insert(lower);
            continue;
        }
        match base_key(cst, entry, &stop_words, options.style, &mut plan.reports) {
            Ok(base) => candidates.push((index, base)),
            Err(reason) => {
                plan.reports.push(Report {
                    offset: entry.span.start,
                    message: format!("entry `{key}` left unchanged: {reason}"),
                });
                taken.insert(lower);
            }
        }
    }

    // Assign keys in file order, suffixing on collision.
    let mut new_keys: HashMap<String, String> = HashMap::new(); // lowercase old -> new
    for (index, base) in candidates {
        let mut n = 0;
        let new = loop {
            let candidate = if n == 0 {
                base.clone()
            } else {
                format!("{base}{}", suffix(n))
            };
            if !taken.contains(&candidate) {
                break candidate;
            }
            n += 1;
        };
        taken.insert(new.clone());
        let entry = entries[index];
        let old = cst.text(entry.key);
        if old != new {
            plan.edits.push((entry.key, new.clone()));
            plan.renames.push(Rename {
                old: old.to_owned(),
                new: new.clone(),
            });
            new_keys.insert(old.to_lowercase(), new);
        }
    }

    // References.
    let mut existing: HashSet<String> = entries
        .iter()
        .map(|entry| cst.text(entry.key).to_lowercase())
        .collect();
    existing.extend(new_keys.values().map(|new| new.to_lowercase()));
    for entry in &entries {
        let key = cst.text(entry.key);
        for field in &entry.fields {
            let name = cst.text(field.name).to_ascii_lowercase();
            let is_list = REFERENCE_FIELDS_LIST.contains(&name.as_str());
            if !is_list && !REFERENCE_FIELDS_SINGLE.contains(&name.as_str()) {
                continue;
            }
            let plain = match field.value.parts.as_slice() {
                [part] if part.is_string() => Some(part.inner()),
                _ => None,
            };
            let Some(inner) = plain else {
                if !new_keys.is_empty() {
                    plan.reports.push(Report {
                        offset: field.name.start,
                        message: format!(
                            "cannot update `{name}` in entry `{key}`: the value is not a single string"
                        ),
                    });
                }
                continue;
            };
            let text = cst.text(inner);
            for (start, end) in reference_items(text, is_list) {
                let item = &text[start..end];
                let span = Span::new(inner.start + start, inner.start + end);
                if let Some(new) = new_keys.get(&item.to_lowercase()) {
                    plan.edits.push((span, new.clone()));
                } else if !existing.contains(&item.to_lowercase()) {
                    plan.reports.push(Report {
                        offset: span.start,
                        message: format!(
                            "`{name}` in entry `{key}` refers to `{item}`, which is not in this file"
                        ),
                    });
                }
            }
        }
    }
    plan
}

/// Applies a plan: returns the source with only the planned spans replaced.
pub fn apply(cst: &Cst, plan: &Plan) -> String {
    cst.with_replacements(&plan.edits)
}

/// The base key of an entry, or the reason it cannot have one. A missing
/// year is not a reason, only a report.
fn base_key(
    cst: &Cst,
    entry: &Entry,
    stop_words: &[String],
    style: Style,
    reports: &mut Vec<Report>,
) -> Result<String, String> {
    let names = match plain_field(cst, entry, "author")? {
        Some(text) if !is_blank(&text) => text,
        _ => match plain_field(cst, entry, "editor")? {
            Some(text) if !is_blank(&text) => text,
            _ => return Err("no author or editor".to_owned()),
        },
    };
    let author = author_part(&names).ok_or("no usable first author (empty or `others`)")?;
    let year = year_part(
        plain_field(cst, entry, "year")?.as_deref(),
        plain_field(cst, entry, "date")?.as_deref(),
    );
    if year.is_empty() {
        reports.push(Report {
            offset: entry.span.start,
            message: format!(
                "entry `{}` has no four-digit year; its key gets no year part",
                cst.text(entry.key)
            ),
        });
    }
    let title_text = plain_field(cst, entry, "title")?.ok_or("no title")?;
    let title = title_part(&title_text, stop_words).ok_or("the title has no words")?;
    Ok(KeyParts {
        author,
        year,
        title,
    }
    .assemble(style))
}

/// The text of a field, `Ok(None)` if absent, or an error if the value uses
/// a macro, which boringbib never resolves.
fn plain_field(cst: &Cst, entry: &Entry, name: &str) -> Result<Option<String>, String> {
    let Some(field) = entry.field(cst, name) else {
        return Ok(None);
    };
    if field
        .value
        .parts
        .iter()
        .any(|part| part.kind == PartKind::Macro)
    {
        return Err(format!(
            "field `{name}` uses a macro, which is not resolved"
        ));
    }
    Ok(Some(cst.value_text(&field.value)))
}

fn is_blank(text: &str) -> bool {
    text.chars().all(lexer::is_whitespace)
}

/// The author part of a key from an `author` or `editor` field value.
///
/// Splits the name list on `and` at brace depth 0, takes the first name,
/// determines its BibTeX form (see [`names`]), then decodes LaTeX, strips
/// braces, transliterates to ASCII, lowercases, cuts at the first hyphen and
/// drops everything outside `[a-z0-9]`. Returns `None` if there is no usable
/// name.
pub fn author_part(names: &str) -> Option<String> {
    let first = names::split_names(names).into_iter().next()?;
    let last = names::last_name_part(first)?;
    let ascii = to_ascii_lowercase(&last);
    let before_hyphen = ascii.split('-').next().unwrap_or_default();
    let author: String = before_hyphen
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    (!author.is_empty()).then_some(author)
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
/// absent or blank; `None` if neither is usable. Macros contribute their
/// names; this is meant for sorting, where that is harmless.
pub fn names_field(cst: &Cst, entry: &Entry) -> Option<String> {
    ["author", "editor"].iter().find_map(|name| {
        let field = entry.field(cst, name)?;
        let text = cst.value_text(&field.value);
        (!is_blank(&text)).then_some(text)
    })
}

/// The title part: the first title word that is not a stop word.
///
/// Drops `$...$` math, decodes LaTeX, strips braces, transliterates,
/// lowercases, turns hyphens into spaces, tokenizes on `[a-z0-9']` runs with
/// apostrophes removed, and returns the first token that is not a stop word
/// (or the first token if all are). `None` if the title has no tokens.
pub fn title_part(title: &str, stop_words: &[String]) -> Option<String> {
    let ascii = to_ascii_lowercase(&strip_math(title)).replace('-', " ");
    let tokens = title_tokens(&ascii);
    tokens
        .iter()
        .find(|token| !stopwords::is_stop_word(token, stop_words))
        .or(tokens.first())
        .cloned()
}

/// LaTeX decoding, brace stripping, NFC, ASCII transliteration, lowercase.
fn to_ascii_lowercase(raw: &str) -> String {
    let decoded = latex::strip_braces(&latex::decode(raw));
    deunicode::deunicode(&decoded).to_lowercase()
}

/// Removes `$...$` and `$$...$$` segments. Escaped `\$` is kept. Unbalanced
/// math is left alone except that the `$` signs are dropped.
fn strip_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_math = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let next = chars.next();
                if !in_math {
                    out.push(c);
                    out.extend(next);
                }
            }
            '$' => {
                if chars.peek() == Some(&'$') {
                    chars.next();
                }
                in_math = !in_math;
            }
            _ if in_math => {}
            _ => out.push(c),
        }
    }
    if in_math {
        return text.replace('$', "");
    }
    out
}

/// Maximal runs of `[a-z0-9']` with the apostrophes removed; empty tokens
/// are dropped.
fn title_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '\''))
        .map(|run| run.replace('\'', ""))
        .filter(|token| !token.is_empty())
        .collect()
}

/// The `n`-th collision suffix, starting at 1: `a`..`z`, `aa`, `ab`, ...
pub fn suffix(n: usize) -> String {
    let mut n = n;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push(b'a' + u8::try_from(n % 26).unwrap_or(0));
        n /= 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// Byte ranges of the trimmed items in a reference value: the whole text for
/// a single-key field, the comma-separated pieces for a list.
fn reference_items(text: &str, list: bool) -> Vec<(usize, usize)> {
    let mut pieces: Vec<(usize, &str)> = Vec::new();
    if list {
        let mut last = 0;
        for (i, _) in text.match_indices(',') {
            pieces.push((last, &text[last..i]));
            last = i + 1;
        }
        pieces.push((last, &text[last..]));
    } else {
        pieces.push((0, text));
    }
    pieces
        .into_iter()
        .filter_map(|(offset, piece)| {
            let lead = piece.len() - piece.trim_start_matches(lexer::is_whitespace).len();
            let trimmed = piece.trim_matches(lexer::is_whitespace);
            (!trimmed.is_empty()).then(|| (offset + lead, offset + lead + trimmed.len()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for(text: &str) -> (Cst, Plan) {
        let cst = crate::parse(text).expect("parses");
        let plan = plan(&cst, &KeysOptions::default());
        (cst, plan)
    }

    fn renames(plan: &Plan) -> Vec<(&str, &str)> {
        plan.renames
            .iter()
            .map(|r| (r.old.as_str(), r.new.as_str()))
            .collect()
    }

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
    fn author_parts() {
        let cases = [
            ("Vaswani, Ashish and Shazeer, Noam", Some("vaswani")),
            (
                "van der Maaten, Laurens and Hinton, Geoffrey",
                Some("vandermaaten"),
            ),
            (
                "Laurens van der Maaten and Geoffrey Hinton",
                Some("vandermaaten"),
            ),
            ("Sch{\\\"o}lkopf, Bernhard", Some("scholkopf")),
            ("Fei-Fei, Li", Some("fei")),
            ("Moosavi-Dezfooli, Seyed-Mohsen", Some("moosavi")),
            ("Lee-Thorp, James", Some("lee")),
            ("O'Connor, Sinead", Some("oconnor")),
            ("{OpenAI}", Some("openai")),
            ("{Barnes and Noble}", Some("barnesandnoble")),
            ("Gonzalez Ortiz, Jose Javier", Some("gonzalezortiz")),
            ("H{\\'e}naff, Olivier", Some("henaff")),
            ("{\\L}ukasz Kaiser", Some("kaiser")),
            ("Kaiser, {\\L}ukasz", Some("kaiser")),
            ("{\\L}ukasz", Some("lukasz")),
            ("M{\\\"u}ller, Rafael", Some("muller")),
            ("Müller, Rafael", Some("muller")),
            ("Fran{\\c{c}}ois Fleuret", Some("fleuret")),
            ("{\\relax Ch}en, Guang", Some("chen")),
            ("Zola, Émile", Some("zola")),
            ("Ludwig Van Beethoven", Some("beethoven")),
            ("Yann LeCun", Some("lecun")),
            ("Knuth", Some("knuth")),
            ("Smith and others", Some("smith")),
            ("others", None),
            ("", None),
            ("   ", None),
            (", John", None),
            ("{}", None),
            ("--", None),
        ];
        for (input, expected) in cases {
            assert_eq!(author_part(input).as_deref(), expected, "{input}");
        }
    }

    #[test]
    fn title_parts() {
        let stop = KeysOptions::default().effective_stop_words();
        let cases = [
            ("Attention is all you need", Some("attention")),
            ("On the difficulty of training", Some("difficulty")),
            ("Self-normalizing neural networks", Some("self")),
            ("{GPT-4} Technical Report", Some("gpt")),
            ("wav2vec 2.0: A framework", Some("wav2vec")),
            ("\"Why should {I} trust you?\" Explaining", Some("should")),
            ("Can you trust your model's uncertainty?", Some("can")),
            ("beta-{VAE}: Learning basic", Some("beta")),
            ("{3D} {ShapeNets}: A deep", Some("3d")),
            ("$O(n)$ algorithms for sorting", Some("algorithms")),
            ("Costs in \\$ and cents", Some("costs")),
            ("The $\\alpha$", Some("the")),
            ("$x$", None),
            ("On the", Some("on")),
            ("", None),
            ("{}", None),
            ("...", None),
            ("Étude on naïve Bayes", Some("etude")),
            ("An image is worth 16x16 words", Some("image")),
            ("A -- B", Some("b")),
            ("'Tis the season", Some("tis")),
            ("Unbalanced $ math", Some("unbalanced")),
        ];
        for (input, expected) in cases {
            assert_eq!(title_part(input, &stop).as_deref(), expected, "{input}");
        }
        let custom = vec!["attention".to_owned()];
        assert_eq!(
            title_part("Attention is all", &custom).as_deref(),
            Some("is")
        );
    }

    #[test]
    fn suffixes() {
        assert_eq!(suffix(1), "a");
        assert_eq!(suffix(2), "b");
        assert_eq!(suffix(26), "z");
        assert_eq!(suffix(27), "aa");
        assert_eq!(suffix(28), "ab");
        assert_eq!(suffix(52), "az");
        assert_eq!(suffix(53), "ba");
        assert_eq!(suffix(702), "zz");
        assert_eq!(suffix(703), "aaa");
    }

    #[test]
    fn reference_item_ranges() {
        assert_eq!(reference_items(" proc ", false), [(1, 5)]);
        assert_eq!(reference_items("a, b ,c,,", true), [(0, 1), (3, 4), (6, 7)]);
        assert_eq!(reference_items("", true), []);
        assert_eq!(reference_items("  ", false), []);
    }

    const COLLIDING: &str = "@misc{x, author = {Doe, Jane}, year = {2020}, title = {Same title}}\n\
        @misc{y, author = {Doe, John}, year = {2020}, title = {Same again}}\n\
        @misc{z, author = {Doe, Jim}, year = {2020}, title = {Same once more}}\n";

    #[test]
    fn collisions_get_suffixes_in_file_order() {
        let (_, plan) = plan_for(COLLIDING);
        assert_eq!(
            renames(&plan),
            [
                ("x", "doe2020same"),
                ("y", "doe2020samea"),
                ("z", "doe2020sameb")
            ]
        );
        assert!(plan.reports.is_empty());
    }

    #[test]
    fn unselected_and_skipped_keys_count_as_taken_case_insensitively() {
        let cst = crate::parse(COLLIDING).expect("parses");
        let options = KeysOptions {
            only: Some(vec!["y".to_owned(), "z".to_owned()]),
            ..KeysOptions::default()
        };
        let result = plan(&cst, &options);
        assert_eq!(
            renames(&result),
            [("y", "doe2020same"), ("z", "doe2020samea")]
        );

        let options = KeysOptions {
            keep: vec!["X".to_owned()],
            ..KeysOptions::default()
        };
        let result = plan(&cst, &options);
        assert_eq!(
            renames(&result),
            [("y", "doe2020same"), ("z", "doe2020samea")]
        );

        let text = "@misc{Doe2020same, title = {No author here}}\n@misc{k, author = {Doe, Jane}, year = {2020}, title = {Same}}\n";
        let (_, result) = plan_for(text);
        assert_eq!(renames(&result), [("k", "doe2020samea")]);
        assert!(result.reports[0].message.contains("no author or editor"));
    }

    #[test]
    fn only_with_unknown_key_is_reported() {
        let cst = crate::parse(COLLIDING).expect("parses");
        let options = KeysOptions {
            only: Some(vec!["nope".to_owned()]),
            ..KeysOptions::default()
        };
        let result = plan(&cst, &options);
        assert!(result.renames.is_empty());
        assert_eq!(result.reports[0].message, "no entry with key `nope`");
    }

    #[test]
    fn running_twice_is_a_no_op() {
        let (cst, plan) = plan_for(COLLIDING);
        let once = apply(&cst, &plan);
        let (_, again) = plan_for(&once);
        assert!(again.renames.is_empty(), "{:?}", again.renames);
        assert!(again.edits.is_empty());
    }

    #[test]
    fn references_are_rewritten_and_dangling_ones_reported() {
        let text = "@inproceedings{paper, author = {Doe, Jane}, year = {2020}, title = {Paper}, crossref = { proc }}\n\
            @proceedings{proc, editor = {Roe, Richard}, year = {2020}, title = {Proceedings}}\n\
            @misc{m, author = {Poe, Edgar}, year = {2020}, title = {Misc}, related = {paper,PROC , ghost,}, xref = \"paper\", ids = m2, xdata = {a} # {b}}\n";
        let (cst, plan) = plan_for(text);
        assert_eq!(
            renames(&plan),
            [
                ("paper", "doe2020paper"),
                ("proc", "roe2020proceedings"),
                ("m", "poe2020misc")
            ]
        );
        let output = apply(&cst, &plan);
        assert!(
            output.contains("crossref = { roe2020proceedings }"),
            "{output}"
        );
        assert!(
            output.contains("related = {doe2020paper,roe2020proceedings , ghost,}"),
            "{output}"
        );
        assert!(output.contains("xref = \"doe2020paper\""), "{output}");
        let messages: Vec<&str> = plan.reports.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "`related` in entry `m` refers to `ghost`, which is not in this file",
                "cannot update `ids` in entry `m`: the value is not a single string",
                "cannot update `xdata` in entry `m`: the value is not a single string",
            ]
        );
    }

    #[test]
    fn reference_to_an_already_updated_key_is_not_dangling() {
        let text = "@misc{old, author = {Doe, Jane}, year = {2020}, title = {T}}\n@misc{k, author = {Roe, R}, year = {2020}, title = {U}, crossref = {doe2020t}}\n";
        let (_, plan) = plan_for(text);
        assert!(plan.reports.is_empty(), "{:?}", plan.reports);
    }

    #[test]
    fn skipped_entries_are_reported_with_reasons() {
        let text = "@misc{a, title = {No author}}\n\
            @misc{b, author = {others}, title = {T}}\n\
            @misc{c, author = {Doe, Jane}, year = {2020}, title = {}}\n\
            @misc{d, author = {Doe, Jane}, year = {2020}}\n\
            @misc{e, author = doe, year = {2020}, title = {T}}\n\
            @misc{f, author = {Doe, Jane}, title = {No year}}\n\
            @misc{g, author = {Doe, Jane}, year = {2020}, title = jmlr}\n";
        let (_, plan) = plan_for(text);
        assert_eq!(renames(&plan), [("f", "doeno")]);
        let messages: Vec<&str> = plan.reports.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "entry `a` left unchanged: no author or editor",
                "entry `b` left unchanged: no usable first author (empty or `others`)",
                "entry `c` left unchanged: the title has no words",
                "entry `d` left unchanged: no title",
                "entry `e` left unchanged: field `author` uses a macro, which is not resolved",
                "entry `f` has no four-digit year; its key gets no year part",
                "entry `g` left unchanged: field `title` uses a macro, which is not resolved",
            ]
        );
    }

    #[test]
    fn apply_with_an_empty_plan_is_identity() {
        let source = "@misc{k, title = {T}}";
        let cst = crate::parse(source).expect("parses");
        assert_eq!(apply(&cst, &Plan::default()), source);
    }
}
