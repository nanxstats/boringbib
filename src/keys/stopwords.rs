//! Stop words skipped when picking the title word of a key.
//!
//! Two lists are kept apart on purpose. [`VERIFIED`] was checked against real
//! Google Scholar keys: each word is skipped by Scholar in at least one key
//! of the corpus in `tests/fixtures/scholar_keys.tsv`. [`ASSUMED`] is what
//! one would expect by analogy (articles, prepositions, conjunctions,
//! relative pronouns) but has not been observed. [`NOT_STOP_WORDS`] lists
//! words other tools skip that Scholar demonstrably keeps; they exist so
//! that nobody "fixes" the list by adding them.
//!
//! Both lists can be overridden from the configuration file (`[keys]
//! stop_words` replaces them, `stop_words_extra` extends them).

/// Stop words verified against real Scholar keys.
pub const VERIFIED: &[&str] = &[
    "a", "an", "the", "on", "in", "is", "are", "do", "how", "when", "what", "why", "from", "to",
    "with", "this",
];

/// Stop words assumed by analogy, not verified.
pub const ASSUMED: &[&str] = &[
    "of", "for", "at", "by", "and", "or", "as", "into", "which", "where", "who", "that", "these",
    "those", "was", "were",
];

/// Words that are *not* stop words for Scholar, although other tools skip
/// them: `not2017...`, `no1997...`, `does2018...`, `can2019...`,
/// `towards2017...` are all real keys.
pub const NOT_STOP_WORDS: &[&str] = &[
    "not", "no", "does", "can", "should", "towards", "beyond", "very", "we", "your", "if", "one",
];

/// The built-in list: [`VERIFIED`] followed by [`ASSUMED`].
pub const DEFAULT: &[&str] = &[
    // VERIFIED
    "a", "an", "the", "on", "in", "is", "are", "do", "how", "when", "what", "why", "from", "to",
    "with", "this", // ASSUMED
    "of", "for", "at", "by", "and", "or", "as", "into", "which", "where", "who", "that", "these",
    "those", "was", "were",
];

/// True if `word` (already lowercase) is in `stop_words`.
pub fn is_stop_word(word: &str, stop_words: &[String]) -> bool {
    stop_words.iter().any(|w| w == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_verified_then_assumed() {
        let expected: Vec<&str> = VERIFIED.iter().chain(ASSUMED.iter()).copied().collect();
        assert_eq!(DEFAULT, expected.as_slice());
    }

    #[test]
    fn lists_are_lowercase_and_disjoint() {
        for word in DEFAULT.iter().chain(NOT_STOP_WORDS.iter()) {
            assert_eq!(*word, word.to_lowercase(), "{word} must be lowercase");
        }
        for word in NOT_STOP_WORDS {
            assert!(!DEFAULT.contains(word), "{word} must not be a stop word");
        }
        for word in VERIFIED {
            assert!(!ASSUMED.contains(word), "{word} is in both lists");
        }
    }

    #[test]
    fn membership() {
        let words: Vec<String> = DEFAULT.iter().map(|&w| w.to_owned()).collect();
        assert!(is_stop_word("the", &words));
        assert!(!is_stop_word("not", &words));
        assert!(!is_stop_word("", &words));
    }
}
