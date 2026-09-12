//! The optional `boringbib.toml` configuration file.
//!
//! The file is looked up in the current directory and its ancestors, stopping
//! at the first directory that contains `.git`; `--config PATH` overrides the
//! search. It has two tables, `[fmt]` and `[keys]`, whose keys are named
//! exactly like the long command-line flags with hyphens replaced by
//! underscores. Values given on the command line win over the file.
//!
//! ```toml
//! [fmt]
//! sort = "key"            # key | none | year | type | author
//! indent = 2              # or "tab"
//! align = true
//! quotes = "braces"       # braces | keep
//! trailing_comma = false
//! wrap = 80               # absent or 0: no wrapping
//! sort_fields = false     # true for the built-in order, or ["author", "title"]
//! line_ending = "auto"    # auto | lf | crlf
//! keep_bom = false
//!
//! [keys]
//! style = "scholar"
//! stop_words = ["a", "an", "the"]   # replaces the built-in list
//! stop_words_extra = ["towards"]    # extends it
//! keep = ["knuth1984", "lamport1994"]  # never rewritten
//! ```
//!
//! Unknown keys are errors, so that a typo cannot silently do nothing.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Error;
use crate::keys::Style;
use crate::printer::{Indent, LineEnding, Quotes, SortFields};
use crate::sort::SortKey;

/// The configuration file name.
pub const FILE_NAME: &str = "boringbib.toml";

/// The whole configuration file. Every value is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `[fmt]`
    #[serde(default)]
    pub fmt: FmtConfig,
    /// `[keys]`
    #[serde(default)]
    pub keys: KeysConfig,
}

/// `[fmt]`: mirrors the `fmt` options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FmtConfig {
    /// `--sort`
    pub sort: Option<SortKey>,
    /// `--indent`
    pub indent: Option<Indent>,
    /// `--align` / `--no-align`
    pub align: Option<bool>,
    /// `--quotes`
    pub quotes: Option<Quotes>,
    /// `--trailing-comma`
    pub trailing_comma: Option<bool>,
    /// `--wrap`; 0 means off
    pub wrap: Option<usize>,
    /// `--sort-fields`
    pub sort_fields: Option<SortFields>,
    /// `--line-ending`
    pub line_ending: Option<LineEnding>,
    /// `--keep-bom`
    pub keep_bom: Option<bool>,
}

/// `[keys]`: mirrors the `keys` options, plus settings that have no flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysConfig {
    /// `--style`
    pub style: Option<Style>,
    /// Replaces the built-in stop word list.
    pub stop_words: Option<Vec<String>>,
    /// Extends the stop word list.
    pub stop_words_extra: Option<Vec<String>>,
    /// Keys that `keys` must never rewrite.
    pub keep: Option<Vec<String>>,
}

/// Looks for [`FILE_NAME`] in `start` and its ancestors.
///
/// The search stops after the first directory that contains a `.git` entry,
/// so that a configuration outside the repository is never picked up.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if current.join(".git").exists() {
            return None;
        }
        dir = current.parent();
    }
    None
}

/// Reads and parses a configuration file.
pub fn load(path: &Path) -> Result<Config, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| Error::Io {
        path: path.display().to_string(),
        error,
    })?;
    parse(&text).map_err(|message| Error::Config {
        path: path.display().to_string(),
        message,
    })
}

/// Parses configuration text.
pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_all_defaults() {
        assert_eq!(parse("").expect("valid"), Config::default());
    }

    #[test]
    fn every_key_parses() {
        let config = parse(
            r#"
            [fmt]
            sort = "year"
            indent = "tab"
            align = false
            quotes = "keep"
            trailing_comma = true
            wrap = 80
            sort_fields = ["title", "author"]
            line_ending = "lf"
            keep_bom = true

            [keys]
            style = "scholar"
            stop_words = ["a"]
            stop_words_extra = ["towards"]
            keep = ["knuth1984"]
            "#,
        )
        .expect("valid");
        assert_eq!(config.fmt.sort, Some(SortKey::Year));
        assert_eq!(config.fmt.indent, Some(Indent::Tab));
        assert_eq!(config.fmt.align, Some(false));
        assert_eq!(config.fmt.quotes, Some(Quotes::Keep));
        assert_eq!(config.fmt.trailing_comma, Some(true));
        assert_eq!(config.fmt.wrap, Some(80));
        assert_eq!(
            config.fmt.sort_fields,
            Some(SortFields::Custom(vec![
                "title".to_owned(),
                "author".to_owned()
            ]))
        );
        assert_eq!(config.fmt.line_ending, Some(LineEnding::Lf));
        assert_eq!(config.fmt.keep_bom, Some(true));
        assert_eq!(config.keys.style, Some(Style::Scholar));
        assert_eq!(config.keys.stop_words, Some(vec!["a".to_owned()]));
        assert_eq!(
            config.keys.stop_words_extra,
            Some(vec!["towards".to_owned()])
        );
        assert_eq!(config.keys.keep, Some(vec!["knuth1984".to_owned()]));
    }

    #[test]
    fn unknown_keys_are_errors() {
        let err = parse("[fmt]\nalign-equal = true\n").unwrap_err();
        assert!(err.contains("align-equal"), "{err}");
        let err = parse("[format]\n").unwrap_err();
        assert!(err.contains("format"), "{err}");
    }

    #[test]
    fn discovery_walks_up_and_stops_at_git() {
        let root = std::env::temp_dir().join(format!("boringbib-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let deep = repo.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("mkdir");
        std::fs::write(root.join(FILE_NAME), "").expect("outer config");

        // Nothing inside the repo yet, and the outer file is behind `.git`.
        std::fs::create_dir(repo.join(".git")).expect("git dir");
        assert_eq!(discover(&deep), None);

        // A file at the repo root is found from a nested directory.
        std::fs::write(repo.join(FILE_NAME), "").expect("repo config");
        assert_eq!(discover(&deep), Some(repo.join(FILE_NAME)));

        // The nearest file wins.
        std::fs::write(deep.join(FILE_NAME), "").expect("nested config");
        assert_eq!(discover(&deep), Some(deep.join(FILE_NAME)));

        // Without a `.git` boundary the search reaches the outer file.
        std::fs::remove_file(deep.join(FILE_NAME)).expect("rm");
        std::fs::remove_file(repo.join(FILE_NAME)).expect("rm");
        std::fs::remove_dir(repo.join(".git")).expect("rm");
        assert_eq!(discover(&deep), Some(root.join(FILE_NAME)));

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn load_reports_missing_and_invalid_files() {
        let missing = std::env::temp_dir().join("boringbib-definitely-missing.toml");
        let err = load(&missing).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err}");

        let bad = std::env::temp_dir().join(format!("boringbib-bad-{}.toml", std::process::id()));
        std::fs::write(&bad, "[fmt]\nindent = \"wide\"\n").expect("write");
        let err = load(&bad).unwrap_err();
        assert!(matches!(err, Error::Config { .. }), "{err}");
        assert!(
            err.to_string()
                .contains("expected a number of spaces or `tab`"),
            "{err}"
        );
        std::fs::remove_file(&bad).expect("cleanup");
    }
}
