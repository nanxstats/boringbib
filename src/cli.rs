//! The command-line interface: argument definitions, option resolution and
//! the `run` function the binary calls.
//!
//! Option resolution follows one rule: a value given on the command line
//! wins over the configuration file, which wins over the built-in default.
//! Every boolean flag therefore has a `--no-…` counterpart, so that a setting
//! from `boringbib.toml` can be overridden either way.
//!
//! Exit status: 0 on success, 1 when `fmt --check` found a file that would
//! change, 2 on any error (syntax, I/O, invalid arguments).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};

use crate::config::{self, Config, FmtConfig, KeysConfig};
use crate::keys::{KeysOptions, Style};
use crate::printer::{FmtOptions, Indent, LineEnding, Quotes, SortFields};
use crate::sort::SortKey;

/// Exit status for success.
pub const EXIT_OK: u8 = 0;
/// Exit status when `fmt --check` found a file that would change.
pub const EXIT_CHANGED: u8 = 1;
/// Exit status for errors: syntax, I/O, invalid arguments.
pub const EXIT_ERROR: u8 = 2;

/// A boring BibTeX formatter. That's the point.
#[derive(Debug, Parser)]
#[command(name = "boringbib", version, about, long_about = None, propagate_version = true)]
pub struct Cli {
    /// Configuration file (default: `boringbib.toml` found in the current
    /// directory or its ancestors, stopping at a `.git` directory).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sort entries and align fields, LaTeX Workshop style.
    Fmt(FmtArgs),
    /// Rewrite citation keys in Google Scholar style.
    Keys(KeysArgs),
}

/// Arguments of `boringbib fmt`.
#[derive(Debug, Args)]
pub struct FmtArgs {
    /// Files to format in place; `-` formats stdin to stdout, which is also
    /// what happens with no files when stdin is not a terminal.
    #[arg(value_name = "FILES")]
    pub files: Vec<PathBuf>,

    /// Exit with status 1 if any file would change, and print which.
    #[arg(long, conflicts_with = "diff")]
    pub check: bool,

    /// Print a unified diff of what would change; write nothing.
    #[arg(long)]
    pub diff: bool,

    /// Ordering of entries.
    #[arg(long, value_enum, value_name = "ORDER", overrides_with = "no_sort")]
    pub sort: Option<SortKey>,

    /// Keep entries in file order (same as `--sort none`).
    #[arg(long, overrides_with = "sort")]
    pub no_sort: bool,

    /// Indentation of field lines: a number of spaces, or `tab`.
    #[arg(long, value_name = "N|tab")]
    pub indent: Option<Indent>,

    /// Pad field names to the longest field name in the entry.
    #[arg(long, overrides_with = "no_align")]
    pub align: bool,

    /// Do not pad field names.
    #[arg(long, overrides_with = "align")]
    pub no_align: bool,

    /// What to do with "quoted" values.
    #[arg(long, value_enum, value_name = "STYLE")]
    pub quotes: Option<Quotes>,

    /// Put a comma after the last field of every entry.
    #[arg(long, overrides_with = "no_trailing_comma")]
    pub trailing_comma: bool,

    /// No comma after the last field.
    #[arg(long, overrides_with = "trailing_comma")]
    pub no_trailing_comma: bool,

    /// Wrap values at column N, continuation lines aligned under the value
    /// (0 disables).
    #[arg(long, value_name = "N", overrides_with = "no_wrap")]
    pub wrap: Option<usize>,

    /// Do not wrap values.
    #[arg(long, overrides_with = "wrap")]
    pub no_wrap: bool,

    /// Reorder the fields of each entry: `--sort-fields` uses the built-in
    /// order, `--sort-fields=author,title` puts those first.
    #[arg(
        long,
        value_name = "ORDER",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "",
        overrides_with = "no_sort_fields"
    )]
    pub sort_fields: Option<String>,

    /// Keep fields in file order.
    #[arg(long, overrides_with = "sort_fields")]
    pub no_sort_fields: bool,

    /// Line ending of the output.
    #[arg(long, value_enum, value_name = "STYLE")]
    pub line_ending: Option<LineEnding>,

    /// Re-emit a leading byte-order mark if the input has one.
    #[arg(long, overrides_with = "no_keep_bom")]
    pub keep_bom: bool,

    /// Drop a leading byte-order mark.
    #[arg(long, overrides_with = "keep_bom")]
    pub no_keep_bom: bool,
}

/// Arguments of `boringbib keys`.
#[derive(Debug, Args)]
pub struct KeysArgs {
    /// Files to process; `-` reads stdin and, with `--write`, prints the
    /// result to stdout.
    #[arg(value_name = "FILES", required = true)]
    pub files: Vec<PathBuf>,

    /// Apply the new keys (atomically). Without it, only the old -> new
    /// mapping is printed and nothing changes.
    #[arg(long)]
    pub write: bool,

    /// Also write the mapping to this file, one `old<TAB>new<TAB>file` line
    /// per renamed key.
    #[arg(long, value_name = "PATH")]
    pub map: Option<PathBuf>,

    /// Restrict the rewrite to these current keys (comma-separated; may be
    /// repeated).
    #[arg(long, value_name = "KEY[,KEY...]", value_delimiter = ',')]
    pub only: Vec<String>,

    /// Key style.
    #[arg(long, value_enum, value_name = "STYLE")]
    pub style: Option<Style>,
}

/// Parses the command line, runs the command and converts the outcome into
/// an exit status. Errors are printed to stderr.
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match try_run(&cli) {
        Ok(status) => ExitCode::from(status),
        Err(err) => {
            eprintln!("boringbib: error: {err:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn try_run(cli: &Cli) -> anyhow::Result<u8> {
    let config = load_config(cli.config.as_deref())?;
    match &cli.command {
        Command::Fmt(args) => {
            let _options = resolve_fmt(args, &config.fmt);
            bail!("`fmt` is not implemented yet (phase 2 of the work plan)")
        }
        Command::Keys(args) => {
            let _options = resolve_keys(args, &config.keys);
            bail!("`keys` is not implemented yet (phase 3 of the work plan)")
        }
    }
}

/// Loads the configuration: the explicit file if given (it must exist),
/// otherwise the discovered one, otherwise the defaults.
pub fn load_config(explicit: Option<&Path>) -> anyhow::Result<Config> {
    if let Some(path) = explicit {
        return Ok(config::load(path)?);
    }
    let cwd = std::env::current_dir().context("cannot determine the current directory")?;
    match config::discover(&cwd) {
        Some(path) => Ok(config::load(&path)?),
        None => Ok(Config::default()),
    }
}

/// `Some(true)` for `--flag`, `Some(false)` for `--no-flag`, `None` if
/// neither was given. clap's `overrides_with` guarantees at most one is set.
fn tri(yes: bool, no: bool) -> Option<bool> {
    if no {
        Some(false)
    } else if yes {
        Some(true)
    } else {
        None
    }
}

/// Merges `fmt` arguments with the configuration file and the defaults.
pub fn resolve_fmt(args: &FmtArgs, file: &FmtConfig) -> FmtOptions {
    let defaults = FmtOptions::default();
    FmtOptions {
        sort: if args.no_sort {
            SortKey::None
        } else {
            args.sort.or(file.sort).unwrap_or(defaults.sort)
        },
        indent: args
            .indent
            .clone()
            .or_else(|| file.indent.clone())
            .unwrap_or(defaults.indent),
        align: tri(args.align, args.no_align)
            .or(file.align)
            .unwrap_or(defaults.align),
        quotes: args.quotes.or(file.quotes).unwrap_or(defaults.quotes),
        trailing_comma: tri(args.trailing_comma, args.no_trailing_comma)
            .or(file.trailing_comma)
            .unwrap_or(defaults.trailing_comma),
        wrap: if args.no_wrap {
            None
        } else {
            args.wrap.or(file.wrap).filter(|&n| n > 0)
        },
        sort_fields: if args.no_sort_fields {
            SortFields::Off
        } else if let Some(order) = &args.sort_fields {
            SortFields::from_list(order)
        } else {
            file.sort_fields.clone().unwrap_or(defaults.sort_fields)
        },
        line_ending: args
            .line_ending
            .or(file.line_ending)
            .unwrap_or(defaults.line_ending),
        keep_bom: tri(args.keep_bom, args.no_keep_bom)
            .or(file.keep_bom)
            .unwrap_or(defaults.keep_bom),
    }
}

/// Merges `keys` arguments with the configuration file and the defaults.
pub fn resolve_keys(args: &KeysArgs, file: &KeysConfig) -> KeysOptions {
    KeysOptions {
        style: args.style.or(file.style).unwrap_or_default(),
        only: (!args.only.is_empty()).then(|| args.only.clone()),
        keep: file.keep.clone().unwrap_or_default(),
        stop_words: file.stop_words.clone(),
        stop_words_extra: file.stop_words_extra.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("boringbib").chain(args.iter().copied()))
            .expect("arguments parse")
    }

    fn fmt_args(args: &[&str]) -> FmtArgs {
        match parse(args).command {
            Command::Fmt(args) => args,
            Command::Keys(_) => panic!("expected fmt"),
        }
    }

    #[test]
    fn cli_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn fmt_defaults_when_nothing_is_given() {
        let options = resolve_fmt(&fmt_args(&["fmt", "a.bib"]), &FmtConfig::default());
        assert_eq!(options, FmtOptions::default());
    }

    #[test]
    fn cli_wins_over_file_wins_over_default() {
        let file = FmtConfig {
            align: Some(false),
            indent: Some(Indent::Tab),
            wrap: Some(80),
            sort_fields: Some(SortFields::Default),
            ..FmtConfig::default()
        };
        let options = resolve_fmt(&fmt_args(&["fmt"]), &file);
        assert!(!options.align);
        assert_eq!(options.indent, Indent::Tab);
        assert_eq!(options.wrap, Some(80));
        assert_eq!(options.sort_fields, SortFields::Default);

        let options = resolve_fmt(
            &fmt_args(&[
                "fmt",
                "--align",
                "--indent",
                "4",
                "--no-wrap",
                "--no-sort-fields",
            ]),
            &file,
        );
        assert!(options.align);
        assert_eq!(options.indent, Indent::Spaces(4));
        assert_eq!(options.wrap, None);
        assert_eq!(options.sort_fields, SortFields::Off);
    }

    #[test]
    fn last_flag_wins() {
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--no-align", "--align"]),
            &FmtConfig::default(),
        );
        assert!(options.align);
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--align", "--no-align"]),
            &FmtConfig::default(),
        );
        assert!(!options.align);
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--sort", "year", "--no-sort"]),
            &FmtConfig::default(),
        );
        assert_eq!(options.sort, SortKey::None);
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--no-sort", "--sort", "year"]),
            &FmtConfig::default(),
        );
        assert_eq!(options.sort, SortKey::Year);
    }

    #[test]
    fn sort_fields_syntax() {
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--sort-fields", "a.bib"]),
            &FmtConfig::default(),
        );
        assert_eq!(options.sort_fields, SortFields::Default);
        let options = resolve_fmt(
            &fmt_args(&["fmt", "--sort-fields=title,author"]),
            &FmtConfig::default(),
        );
        assert_eq!(
            options.sort_fields,
            SortFields::Custom(vec!["title".to_owned(), "author".to_owned()])
        );
        let args = fmt_args(&["fmt", "--sort-fields", "a.bib"]);
        assert_eq!(
            args.files,
            [PathBuf::from("a.bib")],
            "the file is not eaten as the order"
        );
    }

    #[test]
    fn wrap_zero_disables() {
        let options = resolve_fmt(&fmt_args(&["fmt", "--wrap", "0"]), &FmtConfig::default());
        assert_eq!(options.wrap, None);
        let options = resolve_fmt(&fmt_args(&["fmt", "--wrap", "72"]), &FmtConfig::default());
        assert_eq!(options.wrap, Some(72));
    }

    #[test]
    fn check_and_diff_conflict() {
        let result = Cli::try_parse_from(["boringbib", "fmt", "--check", "--diff", "a.bib"]);
        assert!(result.is_err());
    }

    #[test]
    fn keys_arguments() {
        let cli = parse(&[
            "keys", "--write", "--map", "m.tsv", "--only", "a,b", "--only", "c", "x.bib",
        ]);
        let Command::Keys(args) = cli.command else {
            panic!("expected keys");
        };
        assert!(args.write);
        assert_eq!(args.map, Some(PathBuf::from("m.tsv")));
        assert_eq!(args.only, ["a", "b", "c"]);
        let options = resolve_keys(
            &args,
            &KeysConfig {
                keep: Some(vec!["k".to_owned()]),
                ..KeysConfig::default()
            },
        );
        assert_eq!(options.style, Style::Scholar);
        assert_eq!(
            options.only.as_deref(),
            Some(["a".to_owned(), "b".to_owned(), "c".to_owned()].as_slice())
        );
        assert_eq!(options.keep, ["k"]);

        assert!(
            Cli::try_parse_from(["boringbib", "keys"]).is_err(),
            "FILES is required"
        );
    }

    #[test]
    fn global_config_flag_works_anywhere() {
        let cli = parse(&["fmt", "--config", "x.toml", "a.bib"]);
        assert_eq!(cli.config, Some(PathBuf::from("x.toml")));
        let cli = parse(&["--config", "x.toml", "keys", "a.bib"]);
        assert_eq!(cli.config, Some(PathBuf::from("x.toml")));
    }
}
