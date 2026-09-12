//! The BibTeX grammar: turns text into a [`Cst`].
//!
//! The grammar is transcribed from what BibTeX itself accepts (see
//! `bibtex.web`, and biblib's faithful Python transcription of it), with two
//! deliberate relaxations that never lose data:
//!
//! * `@` starts a block only when it is followed by optional white space, an
//!   identifier, optional white space and `{` or `(`. Any other `@` is junk,
//!   so prose between entries never causes an error. When such a `@name` sits
//!   at the start of a line and is not `@comment`, a warning is issued, since
//!   it usually is a broken entry.
//! * Identifiers may start with a digit and may contain non-ASCII characters.
//!
//! And one deliberate deviation from BibTeX, shared with every modern tool:
//! `@comment{...}` has a brace-balanced body. (BibTeX itself skips from
//! `@comment` to the next `@`, braces or not.)
//!
//! Errors stop parsing at the first problem and are reported with a position;
//! the file is never partially interpreted. Non-fatal findings (a duplicated
//! field name, a suspicious `@name`) become [`Warning`](crate::cst::Warning)s.

use crate::cst::Cst;
use crate::lexer::ParseError;

/// Parses a whole file.
///
/// A leading byte-order mark is stripped and recorded on the tree. Line
/// endings are not normalized: the tree is byte-exact.
pub fn parse(_input: &str) -> Result<Cst, ParseError> {
    todo!("phase 2: parser")
}
