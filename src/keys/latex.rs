//! LaTeX to Unicode, for key generation only.
//!
//! The parser never interprets LaTeX; this module works on its own copy of
//! a field value when a key is generated. It is hand-written and
//! table-driven: [`ACCENTS`], [`LETTERS`], [`SYMBOLS`] and [`SEPARATORS`]
//! are the whole vocabulary, and every row has a unit test.
//!
//! # Rules
//!
//! * A control word is a backslash followed by ASCII letters, as long as
//!   possible (`\oe` is one command, not `\o` + `e`). White space after a
//!   control word belongs to the command, as in TeX, so `\o rsted` decodes
//!   to `ørsted`.
//! * A control symbol is a backslash followed by one non-letter.
//! * Accent commands take one argument: a `{...}` group, a control sequence
//!   (`\i`, `\j`), or a single character; white space before the argument is
//!   skipped, as TeX does for undelimited arguments. So `\"o`, `\"{o}`,
//!   `{\"o}` and `{\" o}` all give `ö`, and `\'{\i}` gives `í`. The mark is
//!   added as a combining character and the result is NFC-normalized.
//! * [`LETTERS`] and [`SYMBOLS`] map to their text; `--`, `---` and `~`
//!   become a space, since they separate words.
//! * Any other command with a `{...}` argument reduces to the decoded
//!   argument (`\emph{Foo}` is `Foo`); any other command without one is
//!   deleted, keeping the white space after it so that words stay apart.
//!   `\textendash`, `\textemdash`, `\ldots`, `\dots`, `\textquoteleft` and
//!   `\textquoteright` fall under this rule, as the brief specifies.
//! * Braces are kept, so that callers can still see brace groups (corporate
//!   authors, protected words); use [`strip_braces`] afterwards.

use unicode_normalization::UnicodeNormalization;

use crate::lexer;

/// Accent commands and the combining mark each one adds to its argument.
pub const ACCENTS: &[(&str, char)] = &[
    ("\"", '\u{0308}'), // diaeresis
    ("'", '\u{0301}'),  // acute
    ("`", '\u{0300}'),  // grave
    ("^", '\u{0302}'),  // circumflex
    ("~", '\u{0303}'),  // tilde
    ("=", '\u{0304}'),  // macron
    (".", '\u{0307}'),  // dot above
    ("u", '\u{0306}'),  // breve
    ("v", '\u{030C}'),  // caron
    ("H", '\u{030B}'),  // double acute
    ("c", '\u{0327}'),  // cedilla
    ("k", '\u{0328}'),  // ogonek
    ("r", '\u{030A}'),  // ring above
    ("b", '\u{0331}'),  // macron below
    ("d", '\u{0323}'),  // dot below
    ("t", '\u{0361}'),  // tie
];

/// Commands that stand for a letter.
pub const LETTERS: &[(&str, &str)] = &[
    ("ss", "ß"),
    ("o", "ø"),
    ("O", "Ø"),
    ("l", "ł"),
    ("L", "Ł"),
    ("ae", "æ"),
    ("AE", "Æ"),
    ("oe", "œ"),
    ("OE", "Œ"),
    ("aa", "å"),
    ("AA", "Å"),
    ("dh", "ð"),
    ("DH", "Ð"),
    ("th", "þ"),
    ("TH", "Þ"),
    ("ng", "ŋ"),
    ("NG", "Ŋ"),
    ("dj", "đ"),
    ("DJ", "Đ"),
    ("i", "ı"),
    ("j", "ȷ"),
];

/// Control symbols (a backslash and one non-letter) and their text. The
/// control space and the line break `\\` become a space.
pub const SYMBOLS: &[(char, &str)] = &[
    ('&', "&"),
    ('%', "%"),
    ('$', "$"),
    ('#', "#"),
    ('_', "_"),
    ('{', "{"),
    ('}', "}"),
    (' ', " "),
    ('\\', " "),
];

/// Ligatures and active characters that separate words; longest first.
pub const SEPARATORS: &[(&str, &str)] = &[("---", " "), ("--", " "), ("~", " ")];

/// Decodes LaTeX accents and commands to Unicode (NFC), keeping braces.
pub fn decode(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    decode_chars(&chars, &mut out);
    out.nfc().collect()
}

/// Removes every `{` and `}`.
pub fn strip_braces(input: &str) -> String {
    input.chars().filter(|&c| c != '{' && c != '}').collect()
}

fn decode_chars(chars: &[char], out: &mut String) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            i = decode_command(chars, i, out);
        } else if let Some((from, to)) = SEPARATORS
            .iter()
            .find(|(from, _)| starts_with(chars, i, from))
        {
            out.push_str(to);
            i += from.chars().count();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
}

/// Decodes the control sequence at `at` (a backslash); returns the index
/// after everything it consumed.
fn decode_command(chars: &[char], at: usize, out: &mut String) -> usize {
    let Some(&next) = chars.get(at + 1) else {
        return at + 1;
    };
    if next.is_ascii_alphabetic() {
        let mut end = at + 1;
        while chars.get(end).is_some_and(char::is_ascii_alphabetic) {
            end += 1;
        }
        let name: String = chars[at + 1..end].iter().collect();
        let after = skip_whitespace(chars, end);
        if let Some((_, letter)) = LETTERS.iter().find(|(n, _)| *n == name) {
            out.push_str(letter);
            return after;
        }
        if let Some((_, mark)) = ACCENTS.iter().find(|(n, _)| *n == name) {
            return apply_accent(chars, after, *mark, out);
        }
        if chars.get(after) == Some(&'{') {
            let (inner, end) = braced(chars, after);
            decode_chars(&inner, out);
            return end;
        }
        end
    } else {
        let name = next.to_string();
        if let Some((_, mark)) = ACCENTS.iter().find(|(n, _)| *n == name) {
            return apply_accent(chars, skip_whitespace(chars, at + 2), *mark, out);
        }
        if let Some((_, text)) = SYMBOLS.iter().find(|(c, _)| *c == next) {
            out.push_str(text);
        }
        at + 2
    }
}

/// Reads the argument of an accent command starting at `pos` and writes it
/// with `mark` after its first character.
fn apply_accent(chars: &[char], pos: usize, mark: char, out: &mut String) -> usize {
    let (argument, end) = match chars.get(pos) {
        None => return pos,
        Some('{') => {
            let (inner, end) = braced(chars, pos);
            let mut decoded = String::new();
            decode_chars(&inner, &mut decoded);
            (decoded, end)
        }
        Some('\\') => {
            let mut decoded = String::new();
            let end = decode_command(chars, pos, &mut decoded);
            (decoded, end)
        }
        Some(&c) => (c.to_string(), pos + 1),
    };
    let mut rest = argument.chars();
    if let Some(first) = rest.next() {
        // Marks combine with the dotted letters, not with the dotless ones.
        out.push(match first {
            'ı' => 'i',
            'ȷ' => 'j',
            c => c,
        });
        out.push(mark);
        out.extend(rest);
    }
    end
}

/// The contents of the `{...}` group starting at `pos`, and the index after
/// its closing brace. An unclosed group extends to the end.
fn braced(chars: &[char], pos: usize) -> (Vec<char>, usize) {
    let mut depth = 0usize;
    for (i, &c) in chars.iter().enumerate().skip(pos) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (chars[pos + 1..i].to_vec(), i + 1);
                }
            }
            _ => {}
        }
    }
    (chars[pos + 1..].to_vec(), chars.len())
}

fn skip_whitespace(chars: &[char], mut i: usize) -> usize {
    while chars.get(i).is_some_and(|&c| lexer::is_whitespace(c)) {
        i += 1;
    }
    i
}

fn starts_with(chars: &[char], at: usize, prefix: &str) -> bool {
    prefix
        .chars()
        .enumerate()
        .all(|(offset, p)| chars.get(at + offset) == Some(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(text: &str) -> String {
        text.nfc().collect()
    }

    #[test]
    fn every_accent_row_in_every_spelling() {
        for (name, mark) in ACCENTS {
            let expected = nfc(&format!("o{mark}"));
            let braced = format!("\\{name}{{o}}");
            assert_eq!(decode(&braced), expected, "{braced}");
            let spaced = format!("{{\\{name} o}}");
            assert_eq!(decode(&spaced), format!("{{{expected}}}"), "{spaced}");
            if !name.chars().all(|c| c.is_ascii_alphabetic()) {
                let bare = format!("\\{name}o");
                assert_eq!(decode(&bare), expected, "{bare}");
                let grouped = format!("{{\\{name}o}}");
                assert_eq!(decode(&grouped), format!("{{{expected}}}"), "{grouped}");
            }
            let dotless = format!("\\{name}{{\\i}}");
            assert_eq!(decode(&dotless), nfc(&format!("i{mark}")), "{dotless}");
            let dotless_bare = format!("\\{name}\\j");
            assert_eq!(
                decode(&dotless_bare),
                nfc(&format!("j{mark}")),
                "{dotless_bare}"
            );
            let upper = format!("\\{name}{{E}}");
            assert_eq!(decode(&upper), nfc(&format!("E{mark}")), "{upper}");
            let empty = format!("a\\{name}{{}}b");
            assert_eq!(decode(&empty), "ab", "{empty}");
        }
    }

    #[test]
    fn accents_compose_to_the_expected_letters() {
        let cases = [
            ("\\\"{o}", "ö"),
            ("\\'{e}", "é"),
            ("\\`{a}", "à"),
            ("\\^{e}", "ê"),
            ("\\~{n}", "ñ"),
            ("\\={a}", "ā"),
            ("\\.{z}", "ż"),
            ("\\u{g}", "ğ"),
            ("\\v{s}", "š"),
            ("\\H{o}", "ő"),
            ("\\c{c}", "ç"),
            ("\\k{a}", "ą"),
            ("\\r{a}", "å"),
            ("\\b{h}", "ẖ"),
            ("\\d{s}", "ṣ"),
            ("\\t{oo}", "o\u{361}o"),
            ("\\\"{\\i}", "ï"),
            ("\\'{\\i}", "í"),
        ];
        for (input, expected) in cases {
            assert_eq!(decode(input), nfc(expected), "{input}");
        }
    }

    #[test]
    fn every_letter_row() {
        for (name, letter) in LETTERS {
            let bare = format!("\\{name}");
            assert_eq!(decode(&bare), *letter, "{bare}");
            let grouped = format!("{{\\{name}}}");
            assert_eq!(decode(&grouped), format!("{{{letter}}}"), "{grouped}");
            let empty_arg = format!("\\{name}{{}}x");
            assert_eq!(decode(&empty_arg), format!("{letter}{{}}x"), "{empty_arg}");
            let spaced = format!("\\{name} x");
            assert_eq!(
                decode(&spaced),
                format!("{letter}x"),
                "{spaced}: TeX eats the space"
            );
        }
    }

    #[test]
    fn every_symbol_row() {
        for (symbol, text) in SYMBOLS {
            let input = format!("a\\{symbol}b");
            assert_eq!(decode(&input), format!("a{text}b"), "{input}");
        }
    }

    #[test]
    fn every_separator_row() {
        for (from, to) in SEPARATORS {
            let input = format!("a{from}b");
            assert_eq!(decode(&input), format!("a{to}b"), "{input}");
        }
        assert_eq!(decode("a-b"), "a-b", "a single hyphen is not a separator");
    }

    #[test]
    fn unknown_commands() {
        assert_eq!(decode("\\emph{Foo}"), "Foo");
        assert_eq!(decode("\\textbf {x}y"), "xy");
        assert_eq!(decode("\\LaTeX companion"), " companion");
        assert_eq!(decode("\\LaTeX{} companion"), " companion");
        assert_eq!(decode("Foo\\textendash Bar"), "Foo Bar");
        assert_eq!(decode("Foo\\textendash{}Bar"), "FooBar");
        assert_eq!(decode("Foo\\ldots"), "Foo");
        assert_eq!(decode("\\textquoteleft{}Q\\textquoteright{}"), "Q");
        assert_eq!(decode("\\emph{Sch{\\\"o}n}"), "Sch{ö}n");
        assert_eq!(decode("\\emph{oops"), "oops");
        assert_eq!(decode("a\\"), "a");
        assert_eq!(decode("a\\'"), "a");
        assert_eq!(decode("a\\,b"), "ab");
        assert_eq!(decode("hy\\-phen"), "hyphen");
        assert_eq!(decode("\\uo"), "", "`\\uo` is one unknown control word");
    }

    #[test]
    fn real_names_and_titles() {
        assert_eq!(decode("Sch{\\\"o}lkopf"), "Sch{ö}lkopf");
        assert_eq!(decode("G{\\'e}ron, Aur{\\'e}lien"), "G{é}ron, Aur{é}lien");
        assert_eq!(decode("Fran{\\c{c}}ois"), "Fran{ç}ois");
        assert_eq!(decode("Fran\\c cois"), "François");
        assert_eq!(decode("{\\L}ukasz"), "{Ł}ukasz");
        assert_eq!(decode("na\\\"\\i ve"), "naïve");
        assert_eq!(decode("\\AA ngstr\\\"om"), "Ångström");
        assert_eq!(
            decode("{GPT-4} Technical Report"),
            "{GPT-4} Technical Report"
        );
        assert_eq!(decode("pages 1--2 and 3---4"), "pages 1 2 and 3 4");
        assert_eq!(decode("Müller"), "Müller", "Unicode passes through");
        assert_eq!(
            decode("100\\% \\& more \\$5 \\#1 a\\_b \\{x\\}"),
            "100% & more $5 #1 a_b {x}"
        );
    }

    #[test]
    fn strip_braces_removes_only_braces() {
        assert_eq!(strip_braces("{GPT-4} {T}echnical"), "GPT-4 Technical");
        assert_eq!(strip_braces("plain"), "plain");
    }
}
