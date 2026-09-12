//! LaTeX to Unicode, for key generation only.
//!
//! The parser never interprets LaTeX; this module works on its own copy of
//! a field value when a key is generated. It is hand-written and
//! table-driven; every row of the table has a unit test.
//!
//! # Coverage
//!
//! * Accent commands in every spelling, `\"o`, `\"{o}`, `{\"o}`, `{\" o}`,
//!   for `` \" \' \` \^ \~ \= \. \u \v \H \c \k \r \b \d \t ``, applied to any
//!   base letter including `\i` and `\j`.
//! * Letter commands: `\ss \o \O \l \L \ae \AE \oe \OE \aa \AA \dh \DH \th
//!   \TH \ng \NG \dj \DJ \i \j`.
//! * `--` and `---` become a space (they separate words); `~` becomes a
//!   space.
//! * `\& \% \$ \# \_ \{ \}` become the bare character.
//! * `\textendash`, `\textemdash`, `\ldots`, `\dots`, `\textquoteleft`,
//!   `\textquoteright` and any other unknown command with a braced argument
//!   reduce to the argument; unknown commands without an argument are
//!   deleted.
//!
//! Braces are kept, so that callers can still see brace groups (corporate
//! authors, protected words); use [`strip_braces`] afterwards.

/// Decodes LaTeX accents and commands to Unicode, keeping braces.
pub fn decode(_input: &str) -> String {
    todo!("phase 3: LaTeX decoding")
}

/// Removes every `{` and `}`.
pub fn strip_braces(input: &str) -> String {
    input.chars().filter(|&c| c != '{' && c != '}').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_braces_removes_only_braces() {
        assert_eq!(strip_braces("{GPT-4} {T}echnical"), "GPT-4 Technical");
        assert_eq!(strip_braces("plain"), "plain");
    }
}
