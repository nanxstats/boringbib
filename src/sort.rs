//! Ordering of blocks for the printer.
//!
//! Sorting works on *groups*, not on blocks: every run of junk and
//! `@comment` blocks is attached to the block that follows it, so a comment
//! describing an entry moves with that entry. A trailing run at the end of
//! the file forms a group of its own that always stays last.
//!
//! # Orderings
//!
//! * `key`: `@preamble` and `@string` groups first, in their original
//!   relative order; then entries ordered by `key.to_lowercase()`, ties broken
//!   by the original key (byte order), then by original position.
//! * `year`: numerically ascending by the first 4-digit number in `year`
//!   (fallback `date`); entries without one come last; ties by key.
//! * `type`: by entry type (lowercase), then key.
//! * `author`: by the first author's last name as computed by the key
//!   generator, then year, then key.
//! * `none`: file order.
//!
//! Every ordering is stable.

use serde::Deserialize;

use crate::cst::{Block, Cst, Entry};
use crate::keys;

/// Ordering of entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    /// By citation key, case-insensitively.
    #[default]
    Key,
    /// Keep file order.
    None,
    /// By year, then key.
    Year,
    /// By entry type, then key.
    Type,
    /// By first author's last name, then year, then key.
    Author,
}

/// A block together with the junk and comments that precede it.
#[derive(Debug, Clone)]
pub struct Group<'a> {
    /// Junk and `@comment` blocks, in file order.
    pub leading: Vec<&'a Block>,
    /// The entry, `@string` or `@preamble`; `None` for the trailing run at
    /// the end of the file.
    pub block: Option<&'a Block>,
    /// Position of the group in the file, starting at 0.
    pub index: usize,
}

/// Splits the blocks of a tree into groups, in file order.
pub fn group(cst: &Cst) -> Vec<Group<'_>> {
    let mut groups = Vec::new();
    let mut leading = Vec::new();
    for block in cst.blocks() {
        if block.is_trivia() {
            leading.push(block);
        } else {
            groups.push(Group {
                leading: std::mem::take(&mut leading),
                block: Some(block),
                index: groups.len(),
            });
        }
    }
    if !leading.is_empty() {
        groups.push(Group {
            leading,
            block: None,
            index: groups.len(),
        });
    }
    groups
}

/// Reorders groups in place according to `key`. The trailing group, if any,
/// stays last.
pub fn sort(cst: &Cst, groups: &mut [Group<'_>], key: SortKey) {
    if key == SortKey::None {
        return;
    }
    groups.sort_by_cached_key(|group| rank(cst, group, key));
}

/// One component of a sort key. The derived `Ord` compares variants in
/// declaration order, so `Missing` sorts after every value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Ordinal {
    Text(String),
    Number(u32),
    Missing,
}

/// `(tier, components, original position)`: tier 0 for `@preamble` and
/// `@string`, 1 for entries, 2 for the trailing group.
fn rank(cst: &Cst, group: &Group<'_>, key: SortKey) -> (u8, Vec<Ordinal>, usize) {
    let entry = match group.block {
        None => return (2, Vec::new(), group.index),
        Some(Block::Entry(entry)) => entry,
        Some(_) => return (0, Vec::new(), group.index),
    };
    let key_text = cst.text(entry.key);
    let mut components = match key {
        SortKey::Key | SortKey::None => Vec::new(),
        SortKey::Year => vec![year_ordinal(cst, entry)],
        SortKey::Type => vec![Ordinal::Text(cst.text(entry.kind).to_ascii_lowercase())],
        SortKey::Author => vec![author_ordinal(cst, entry), year_ordinal(cst, entry)],
    };
    components.push(Ordinal::Text(key_text.to_lowercase()));
    components.push(Ordinal::Text(key_text.to_owned()));
    (1, components, group.index)
}

fn year_ordinal(cst: &Cst, entry: &Entry) -> Ordinal {
    let text = |name: &str| entry.field(cst, name).map(|f| cst.value_text(&f.value));
    let year = keys::year_part(text("year").as_deref(), text("date").as_deref());
    year.parse().map_or(Ordinal::Missing, Ordinal::Number)
}

fn author_ordinal(cst: &Cst, entry: &Entry) -> Ordinal {
    keys::names_field(cst, entry)
        .and_then(|names| keys::author_part(&names))
        .map_or(Ordinal::Missing, Ordinal::Text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::{Comment, Delim, Junk, Span};

    #[test]
    fn grouping_attaches_trivia_to_the_next_block() {
        // "junk@comment{c}@misc{a}@misc{b}tail"
        let source = "junk@comment{c}@misc{a}@misc{b}tail";
        let entry = |start: usize| {
            Block::Entry(Entry {
                span: Span::new(start, start + 8),
                kind: Span::new(start + 1, start + 5),
                delim: Delim::Brace,
                key: Span::new(start + 6, start + 7),
                fields: Vec::new(),
                trailing_comma: false,
            })
        };
        let blocks = vec![
            Block::Junk(Junk {
                span: Span::new(0, 4),
            }),
            Block::Comment(Comment {
                span: Span::new(4, 15),
                name: Span::new(5, 12),
                delim: Delim::Brace,
                body: Span::new(13, 14),
            }),
            entry(15),
            entry(23),
            Block::Junk(Junk {
                span: Span::new(31, 35),
            }),
        ];
        let cst = Cst::new(false, source.to_owned(), blocks, Vec::new());
        let groups = group(&cst);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].leading.len(), 2);
        assert_eq!(
            cst.text(groups[0].block.expect("entry a").span()),
            "@misc{a}"
        );
        assert!(groups[1].leading.is_empty());
        assert_eq!(
            cst.text(groups[1].block.expect("entry b").span()),
            "@misc{b}"
        );
        assert!(groups[2].block.is_none());
        assert_eq!(cst.text(groups[2].leading[0].span()), "tail");
        assert_eq!(
            groups.iter().map(|g| g.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    fn order(input: &str, key: SortKey) -> Vec<String> {
        let cst = crate::parse(input).expect("parses");
        let mut groups = group(&cst);
        sort(&cst, &mut groups, key);
        groups
            .iter()
            .map(|g| match g.block {
                None => "<trailing>".to_owned(),
                Some(Block::Entry(e)) => cst.text(e.key).to_owned(),
                Some(Block::StringDef(_)) => "<string>".to_owned(),
                Some(Block::Preamble(_)) => "<preamble>".to_owned(),
                Some(_) => unreachable!(),
            })
            .collect()
    }

    const MIXED: &str = "@misc{b, year = {2001}, note = {x}}\n@string{s = \"x\"}\n@Article{A, year = 1999}\n@preamble{\"p\"}\n@misc{a, date = {2005-01}}\n% comment\n@Book{c}\ntail\n";

    #[test]
    fn key_order_is_case_insensitive_with_byte_order_ties() {
        assert_eq!(
            order(MIXED, SortKey::Key),
            ["<string>", "<preamble>", "A", "a", "b", "c", "<trailing>"]
        );
    }

    #[test]
    fn none_keeps_file_order() {
        assert_eq!(
            order(MIXED, SortKey::None),
            ["b", "<string>", "A", "<preamble>", "a", "c", "<trailing>"]
        );
    }

    #[test]
    fn year_order_is_numeric_with_missing_years_last() {
        assert_eq!(
            order(MIXED, SortKey::Year),
            ["<string>", "<preamble>", "A", "b", "a", "c", "<trailing>"]
        );
    }

    #[test]
    fn type_order_then_key() {
        assert_eq!(
            order(MIXED, SortKey::Type),
            ["<string>", "<preamble>", "A", "c", "a", "b", "<trailing>"]
        );
    }

    #[test]
    fn sorting_is_stable_for_identical_keys() {
        let input = "@misc{k, note = {1}}@misc{k, note = {2}}@misc{K, note = {3}}";
        let cst = crate::parse(input).expect("parses");
        let mut groups = group(&cst);
        sort(&cst, &mut groups, SortKey::Key);
        let notes: Vec<&str> = groups
            .iter()
            .map(|g| match g.block {
                Some(Block::Entry(e)) => cst.text(e.fields[0].value.parts[0].inner()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            notes,
            ["3", "1", "2"],
            "uppercase K sorts first by byte order, then file order"
        );
    }
}
