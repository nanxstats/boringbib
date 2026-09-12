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

use super::latex;
use crate::lexer;

/// Splits a name list on the word `and` at brace depth 0.
///
/// Each returned name is trimmed. A trailing `and others` is returned as the
/// name `others`; the caller decides what to do with it. An empty list gives
/// one empty name.
pub fn split_names(list: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    let bytes = list.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b if depth == 0 && b.is_ascii_whitespace() => {
                let rest = &list[i..];
                let ws = rest.len() - rest.trim_start_matches(lexer::is_whitespace).len();
                let after = &rest[ws..];
                if after.len() > 3
                    && after.is_char_boundary(3)
                    && after[..3].eq_ignore_ascii_case("and")
                    && after.as_bytes()[3].is_ascii_whitespace()
                {
                    names.push(list[start..i].trim_matches(lexer::is_whitespace));
                    i += ws + 3;
                    start = i;
                    continue;
                }
                i += ws;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    names.push(list[start..].trim_matches(lexer::is_whitespace));
    names
}

/// The last-name part of one name, in raw (still LaTeX) form.
///
/// Returns `None` for an empty name, for `others`, and for a name whose
/// last-name part would be empty (`, John`).
pub fn last_name_part(name: &str) -> Option<String> {
    let name = name.trim_matches(lexer::is_whitespace);
    if name.is_empty() || name.eq_ignore_ascii_case("others") {
        return None;
    }
    if let Some(inner) = corporate(name) {
        return Some(inner.to_owned());
    }
    if let Some(comma) = depth0_positions(name, b',').first() {
        let last = name[..*comma].trim_matches(lexer::is_whitespace);
        return (!last.is_empty()).then(|| last.to_owned());
    }
    let tokens = depth0_tokens(name);
    let last_index = tokens.len().checked_sub(1)?;
    let von_start = tokens[..last_index]
        .iter()
        .position(|token| is_lowercase_initial(token))
        .unwrap_or(last_index);
    Some(tokens[von_start..].join(" "))
}

/// The inside of the brace group if `name` is exactly one brace group.
fn corporate(name: &str) -> Option<&str> {
    if !name.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in name.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (i == name.len() - 1).then(|| &name[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Byte offsets of `byte` at brace depth 0.
fn depth0_positions(text: &str, byte: u8) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut depth = 0usize;
    for (i, b) in text.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b if b == byte && depth == 0 => positions.push(i),
            _ => {}
        }
    }
    positions
}

/// Non-empty runs of characters separated by white space at brace depth 0.
fn depth0_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && lexer::is_whitespace(c) {
            if let Some(s) = start.take() {
                tokens.push(&text[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push(&text[s..]);
    }
    tokens
}

/// True if the token's first alphabetic character, after LaTeX decoding and
/// ignoring braces, is lowercase.
fn is_lowercase_initial(token: &str) -> bool {
    latex::strip_braces(&latex::decode(token))
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(char::is_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_on_and() {
        assert_eq!(
            split_names("Vaswani, Ashish and Shazeer, Noam"),
            ["Vaswani, Ashish", "Shazeer, Noam"]
        );
        assert_eq!(split_names("A AND B And C"), ["A", "B", "C"]);
        assert_eq!(
            split_names("{Barnes and Noble} and Smith, John"),
            ["{Barnes and Noble}", "Smith, John"]
        );
        assert_eq!(split_names("Brand and Landers"), ["Brand", "Landers"]);
        assert_eq!(split_names("Sand and Andy"), ["Sand", "Andy"]);
        assert_eq!(split_names("Smith and others"), ["Smith", "others"]);
        assert_eq!(split_names("A and\n\tB"), ["A", "B"]);
        assert_eq!(split_names("  Smith  "), ["Smith"]);
        assert_eq!(split_names(""), [""]);
        assert_eq!(
            split_names("Smith and"),
            ["Smith and"],
            "a trailing `and` is not a separator"
        );
        assert_eq!(split_names("Émile and Zoë"), ["Émile", "Zoë"]);
    }

    fn last(name: &str) -> Option<String> {
        last_name_part(name)
    }

    #[test]
    fn first_von_last() {
        assert_eq!(
            last("Laurens van der Maaten").as_deref(),
            Some("van der Maaten")
        );
        assert_eq!(
            last("Jean de La Fontaine").as_deref(),
            Some("de La Fontaine")
        );
        assert_eq!(last("Yann LeCun").as_deref(), Some("LeCun"));
        assert_eq!(last("Ludwig Van Beethoven").as_deref(), Some("Beethoven"));
        assert_eq!(last("Knuth").as_deref(), Some("Knuth"));
        assert_eq!(last("Donald E. Knuth").as_deref(), Some("Knuth"));
        assert_eq!(last("{\\'E}mile Zola").as_deref(), Some("Zola"));
        assert_eq!(last("Jean-Baptiste Poquelin").as_deref(), Some("Poquelin"));
        assert_eq!(
            last("jean de la fontaine").as_deref(),
            Some("jean de la fontaine")
        );
        assert_eq!(last("Razvan Pascanu").as_deref(), Some("Pascanu"));
        assert_eq!(last("{Barnes} {Noble}").as_deref(), Some("{Noble}"));
        assert_eq!(
            last("Jean {de} La Fontaine").as_deref(),
            Some("{de} La Fontaine")
        );
        assert_eq!(last("  Yann   LeCun  ").as_deref(), Some("LeCun"));
    }

    #[test]
    fn von_last_first_and_jr() {
        assert_eq!(
            last("van der Maaten, Laurens").as_deref(),
            Some("van der Maaten")
        );
        assert_eq!(
            last("Sch{\\\"o}lkopf, Bernhard").as_deref(),
            Some("Sch{\\\"o}lkopf")
        );
        assert_eq!(last("Ford, Jr., Henry").as_deref(), Some("Ford"));
        assert_eq!(
            last("Gonzalez Ortiz, Jose Javier").as_deref(),
            Some("Gonzalez Ortiz")
        );
        assert_eq!(last("Smith,").as_deref(), Some("Smith"));
        assert_eq!(last(", John"), None);
        assert_eq!(last("{Smith, Jr.}, John").as_deref(), Some("{Smith, Jr.}"));
    }

    #[test]
    fn corporate_authors() {
        assert_eq!(last("{OpenAI}").as_deref(), Some("OpenAI"));
        assert_eq!(
            last("{Barnes and Noble}").as_deref(),
            Some("Barnes and Noble")
        );
        assert_eq!(last("{Barnes, Noble}").as_deref(), Some("Barnes, Noble"));
        assert_eq!(last("{{Nested}}").as_deref(), Some("{Nested}"));
        assert_eq!(
            last("{Barnes} Inc").as_deref(),
            Some("Inc"),
            "not a single group"
        );
    }

    #[test]
    fn nothing_usable() {
        assert_eq!(last("others"), None);
        assert_eq!(last("OTHERS"), None);
        assert_eq!(last(""), None);
        assert_eq!(last("  \t"), None);
    }

    #[test]
    fn lowercase_detection_decodes_latex() {
        assert!(is_lowercase_initial("von"));
        assert!(is_lowercase_initial("{\\'e}tienne"));
        assert!(!is_lowercase_initial("{\\'E}tienne"));
        assert!(!is_lowercase_initial("LeCun"));
        assert!(!is_lowercase_initial("3M"));
        assert!(!is_lowercase_initial("{-}"));
        assert!(is_lowercase_initial("{de}"));
    }
}
