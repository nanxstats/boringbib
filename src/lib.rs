//! A boring BibTeX formatter. That's the point.
//!
//! `boringbib` is a small command-line tool for BibTeX `.bib` files with two
//! jobs:
//!
//! * [`fmt`](printer): sort entries by citation key and pretty-print each
//!   entry with the `=` signs aligned, exactly like LaTeX Workshop's "Align
//!   and sort" action.
//! * [`keys`]: rewrite citation keys into Google Scholar style
//!   (`vaswani2017attention`), updating in-file references so nothing
//!   dangles.
//!
//! The library is organized as a pipeline over a lossless concrete syntax
//! tree:
//!
//! ```text
//! text ──parse()──▶ Cst ──sort::group()/sort()──▶ printer::format() ──▶ text
//!                    │
//!                    └──keys::plan()──▶ Plan ──keys::apply()──▶ text
//! ```
//!
//! The parser ([`parser`], built on [`lexer`]) keeps every byte of the input
//! in the [`Cst`]; `Cst::to_source()` reproduces the input exactly. All
//! formatting decisions live in [`printer`] and [`sort`]; `keys` edits are
//! splices of the original text at recorded spans. This separation is what
//! makes the tool boring: deterministic output, idempotent runs, and no loss
//! of the user's data.
//!
//! The binary lives in [`cli`]; see `DESIGN.md` in the repository for the
//! decisions behind the grammar and the output rules.

pub mod cli;
pub mod config;
pub mod cst;
pub mod keys;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod sort;

pub use cst::Cst;
pub use lexer::ParseError;
pub use parser::parse;
pub use printer::{FmtOptions, format, format_str};

/// Errors reported by the library when working with files.
///
/// Each variant's message is complete on its own (the cause is part of the
/// text rather than a chained source), so that it prints the same whether
/// or not the caller walks the error chain. Parse errors render as
/// `file:line:col: message`, the format editors know how to jump to.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is not syntactically valid BibTeX.
    #[error("{path}:{error}")]
    Parse {
        /// Display name of the file (`<stdin>` for standard input).
        path: String,
        /// The underlying syntax error.
        error: ParseError,
    },
    /// Reading or writing the file failed.
    #[error("{path}: {error}")]
    Io {
        /// Display name of the file.
        path: String,
        /// The underlying I/O error.
        error: std::io::Error,
    },
    /// The file is not valid UTF-8.
    #[error("{path}: not valid UTF-8 (invalid byte sequence at offset {offset})")]
    Utf8 {
        /// Display name of the file.
        path: String,
        /// Byte offset of the first invalid sequence.
        offset: usize,
    },
    /// The configuration file could not be parsed.
    #[error("{path}: {message}")]
    Config {
        /// Path of the configuration file.
        path: String,
        /// What is wrong with it.
        message: String,
    },
}
