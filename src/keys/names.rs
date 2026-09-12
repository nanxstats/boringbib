//! BibTeX name lists and name forms.
//!
//! An `author` or `editor` field is a list of names separated by the word
//! `and` (case-insensitive, surrounded by white space, at brace depth 0). A
//! name has one of three forms, told apart by counting commas at brace
//! depth 0:
//!
//! * `First von Last` (no comma): tokenize on white space at depth 0. A token
//!   is lowercase-initial if its first alphabetic character, after LaTeX
//!   decoding and ignoring braces, is lowercase. Among all tokens except the
//!   last, the first lowercase-initial one starts the last-name part, which
//!   runs to the end (`Laurens van der Maaten` gives `van der Maaten`,
//!   `Jean de La Fontaine` gives `de La Fontaine`). If there is none, the
//!   last-name part is the final token alone (`Yann LeCun` gives `LeCun`, and
//!   BibTeX's famous `Ludwig Van Beethoven` gives `Beethoven`, on purpose).
//!   A single token is the whole last name.
//! * `von Last, First` (one comma) and `von Last, Jr, First` (two commas):
//!   the last-name part is everything before the first comma.
//! * A name entirely wrapped in one brace group (`{OpenAI}`,
//!   `{Barnes and Noble}`) is a corporate author: the last-name part is the
//!   whole braced text.
//!
//! The functions here return raw text; normalization (decoding, ASCII
//! transliteration, lowercasing) happens in [`crate::keys`].

/// Splits a name list on ` and ` at brace depth 0.
///
/// Each returned name is trimmed. A trailing `and others` is returned as the
/// name `others`; the caller decides what to do with it.
pub fn split_names(_list: &str) -> Vec<&str> {
    todo!("phase 3: name list splitting")
}

/// The last-name part of one name, in raw (still LaTeX) form.
///
/// Returns `None` for an empty name and for `others`.
pub fn last_name_part(_name: &str) -> Option<String> {
    todo!("phase 3: name form parsing")
}
