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

use crate::cst::{Block, Cst};

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
pub fn sort(_cst: &Cst, _groups: &mut [Group<'_>], _key: SortKey) {
    todo!("phase 2: sorting")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::{Comment, Delim, Entry, Junk, Span};

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
}
