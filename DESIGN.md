# Design

`boringbib` is boring in the "choose boring technology" sense. Everything
below serves three properties, in this order of priority whenever they
conflict:

1. **Lossless.** The user's data is never changed except where they asked.
2. **Idempotent.** `fmt(fmt(x)) == fmt(x)`; `keys` twice is a no-op.
3. **Deterministic.** Same input and options, same bytes, on every platform.

Where the specification was silent, the choice that maximizes these was
taken and is recorded in the [decision log](#decision-log) at the end.

## Architecture

One crate, one binary, a library underneath so that bindings (Python via
PyO3, R via extendr) can be added later without touching the CLI.

```
src/
  main.rs        the binary: calls cli::run()
  lib.rs         crate docs, module list, the Error type
  cli.rs         clap definitions, option resolution (CLI > file > default), run()
  lexer.rs       cursor over the text, character classes, positions, ParseError
  parser.rs      the grammar: text -> Cst
  cst.rs         the lossless tree: spans into the source, to_source(), splicing
  printer.rs     fmt: options and the pretty-printer
  sort.rs        grouping of blocks and the entry orderings
  config.rs      boringbib.toml
  keys/
    mod.rs       keys: plan (compute renames + reference edits) and apply
    names.rs     BibTeX name lists and the three name forms
    latex.rs     LaTeX -> Unicode table, for key generation only
    stopwords.rs verified and assumed stop word lists
tests/
  fixtures/      .bib inputs, golden outputs, the Scholar key corpus (TSV)
```

Data flow:

```
text ──parse()──▶ Cst ──sort::group()/sort()──▶ printer::format() ──▶ text
                   │
                   └──keys::plan()──▶ Plan ──keys::apply()──▶ text
```

Dependencies, all boring: `clap` (derive) for the CLI, `anyhow` +
`thiserror` for errors, `deunicode` for Unicode-to-ASCII transliteration,
`serde` + `toml` for the configuration file, `similar` for `--diff`. Dev:
`insta`, `assert_cmd`, `predicates`. No BibTeX parsing crate: the parser is
the product. `unicode-normalization` will be added in phase 3 for the NFC
step of key generation (see [Keys](#keys)). No `unsafe`.

## The tree

The `Cst` owns the exact source text (minus a leading byte-order mark, which
is recorded as a flag) and a list of blocks that **tile** it: block *n* ends
where block *n+1* starts, the first starts at byte 0 and the last ends at the
last byte. Nodes hold `Span`s (byte ranges) into the source rather than
copies of the text.

This makes losslessness structural rather than something each node has to
get right: `to_source()` is the source plus the mark, and the tiling
invariant is asserted when a tree is built. A targeted edit (renaming a key,
rewriting a `crossref`) is a list of `(Span, replacement)` pairs spliced into
the source by `Cst::with_replacements`; every byte outside those spans is
untouched by construction. After an edit the tree is simply re-parsed.

Line endings are **not** normalized in the tree: `\r` is white space to the
lexer, so CRLF files parse as-is and round-trip byte for byte. The printer
is the only place that knows about line endings (see
[Line endings and the byte-order mark](#line-endings-and-the-byte-order-mark)).

## Grammar

Transcribed from what BibTeX accepts (`bibtex.web`, via biblib's faithful
transcription), with the relaxations listed after it.

```
file      := block*
block     := junk | comment | preamble | string | entry
comment   := '@' 'comment' ( '{' balanced '}' | '(' balanced ')' )
preamble  := '@' 'preamble' ( '{' value '}' | '(' value ')' )
string    := '@' 'string' ( '{' ident '=' value '}' | '(' ident '=' value ')' )
entry     := '@' ident ( '{' key fields '}' | '(' key fields ')' )
fields    := ( ',' ident '=' value )* ','?
value     := part ( '#' part )*
part      := '{' balanced '}' | '"' quoted '"' | number | ident
```

White space may appear between any two tokens above. The block name and
the field names are case-insensitive and stored as written. Character
classes (from BibTeX's `id_class` and key scanning):

- White space: space, tab, LF, CR, form feed (`char::is_ascii_whitespace`).
- Identifier: non-empty run of characters that are neither white space nor
  one of `" # % ' ( ) , = { }`.
- Key: possibly empty run of characters that are neither white space nor one
  of `, { } ( ) "`.
- Number: identifier consisting only of ASCII digits. Any other bare token in
  value position is a macro name. Macros are never resolved.
- `balanced`: braces nest and must balance. In a `"..."` string braces must
  balance and `"` may only appear inside braces. A `(...)` body ends at the
  first `)` at brace depth 0; parentheses do not nest, as in BibTeX.

Relaxations, none of which can lose data:

- **Junk.** `@` starts a block only when followed by optional white space,
  an identifier, optional white space and `{` or `(`. Any other `@` is
  junk. This is what makes prose, email addresses and `%` comment lines safe.
  A `@name` that sits at the start of a line (after optional white space),
  is not `@comment`, and is not followed by a delimiter gets a **warning**,
  since it is usually a broken entry; BibTeX itself would stop with an error.
- **Identifiers** may start with a digit and may contain non-ASCII
  characters. BibTeX would reject `3d = {...}`; boringbib prints it back
  unchanged, which is not its job to police.
- **Keys** may be empty (`@misc{, title = ...}`), as in BibTeX.
- **Duplicate field names** in one entry are kept, both of them, with a
  warning. BibTeX uses the first; `Entry::field` returns the first.

One deliberate deviation from BibTeX, shared with biber, JabRef and
bibtex-tidy: `@comment{...}` has a brace-balanced body that is preserved
verbatim. BibTeX proper treats `@comment` as "skip to the next `@`", so an
`@entry` inside a `@comment{...}` body is an entry to BibTeX and a comment
to everyone else. A `@comment` that is not followed by a delimiter falls
under the junk rule, which matches BibTeX exactly.

Errors stop at the first problem and are reported as `line:col: message`
(the CLI prefixes the file name); line and column are 1-based and the
column counts characters, not bytes. An unclosed `{`, `(` or `"` is reported
at its opening character, since that is where the fix goes. Messages:
`unbalanced braces: this \`{\` is never closed`, `unterminated quoted
string`, `expected \`=\` after field name`, `expected \`,\` or \`}\``,
`expected a value ({...}, "...", a number or a macro name)`, `unbalanced
parentheses`. Warnings go to stderr as `file:line:col: warning: message` and
never change the exit status.

## Formatting

The printer writes every block from scratch in the order chosen by the
sorter; it never copies an entry's original layout. The rules, with the
defaults matching LaTeX Workshop's formatter with `align-equal` and sorting
on:

- Block names, entry types and field names in lowercase; keys untouched.
  All blocks use braces, even if written with parentheses; there is no white
  space between `@name` and `{`.
- Entry: `@type{key,` on one line; one field per line, indented (`--indent`,
  default two spaces); ` = ` between the field name (padded to the longest
  field name **in that entry** when `--align` is on) and the value; `,` after
  every field but the last (unless `--trailing-comma`); `}` on its own line.
  An entry without fields prints as `@type{key` newline `}` (with
  `--trailing-comma`: `@type{key,`), which is what LaTeX Workshop does and
  what BibTeX accepts.
- Values: numbers stay bare, macros stay macros, concatenations print as
  `{First } # {edition}` with single spaces around `#`. Inside every `{...}`
  or `"..."` part, each run of white space (newlines included) collapses to
  one space, at every brace depth. The **whole value** is then trimmed at
  its two ends: leading white space of the first part and trailing white
  space of the last part. Parts are not trimmed individually, because
  `"First " # "edition"` needs its space. This is exactly the normalization
  BibTeX performs when it reads a value, so it never changes what BibTeX
  sees. With `--quotes braces` (default), `"..."` parts become `{...}`,
  which is always safe because a `"` inside a quoted string can only occur
  inside braces. `--wrap` is the only thing that introduces newlines into
  values; continuation lines align under the value's first character, i.e.
  indent + padded name + ` = ` + one delimiter, as in LaTeX Workshop.
- `@string{name = value}` follows the value rules. `@preamble{...}` and
  `@comment{...}` bodies are printed verbatim (including surrounding white
  space inside the delimiters), except that CRLF inside them becomes the
  output line ending.
- Junk is printed verbatim except that leading and trailing blank lines
  (lines that are empty or only white space) are dropped; the indentation
  and trailing spaces of the remaining lines are kept. Junk that is only
  white space disappears; that is how the blank lines between blocks are
  normalized.
- Exactly one blank line between blocks. A run of junk and `@comment`
  blocks is printed immediately before the block it precedes, with no blank
  line between them or before that block. A trailing run at the end of the
  file is printed last, after one blank line. The output ends with exactly
  one newline, or is empty when there is nothing to print (an empty or
  white-space-only file).
- `--sort-fields` reorders the fields of each entry by the built-in order
  (`author, editor, title, booktitle, journal, year, month, volume, number,
  pages, publisher, address, edition, series, chapter, howpublished,
  institution, organization, school, type, note, doi, url, urldate, isbn,
  issn, eprint, archiveprefix, primaryclass, keywords, abstract, file`), then
  the remaining fields alphabetically; `--sort-fields=a,b` puts `a` and `b`
  before the built-in order. Duplicated fields keep their relative order
  (the sort is stable).

## Sorting

Sorting acts on **groups**: every run of junk and `@comment` blocks is
attached to the block that follows it, so a comment describing an entry
moves with the entry. A trailing run at the end of the file is a group of
its own and always stays last.

- `key` (default): `@preamble` and `@string` groups first, in their original
  relative order; then entries ordered by `key.to_lowercase()`, ties broken
  by the original key in byte order, then by original position. Lowercase
  comparison replaces LaTeX Workshop's `localeCompare`, which depends on the
  machine's locale and is therefore not deterministic.
- `year`: numerically ascending by the first 4-digit number in `year`, else
  in `date`; entries without one come last; ties by key.
- `type`: by entry type (lowercase), then key.
- `author`: by the first author's last name as computed by the key
  generator (so `van der Maaten` sorts under `v`), then year, then key.
- `none`: file order, nothing moves, `@string` blocks included.

All sorts are stable.

## Line endings and the byte-order mark

- **Reading.** A leading UTF-8 byte-order mark is stripped and remembered.
  Invalid UTF-8 is an error naming the file and the byte offset. Line
  endings are left alone in the tree.
- **`fmt` output.** The printer produces LF; verbatim pieces (junk,
  `@comment` and `@preamble` bodies) have their CRLF turned into LF as they
  are copied, and a final pass turns LF into CRLF when the target is CRLF.
  A lone `\r` (no following `\n`) is kept where it was. The target is
  `--line-ending`: `lf`, `crlf`, or `auto` (default), where `auto` means
  CRLF if the **first line ending in the input** is CRLF and LF otherwise.
  A file with mixed endings therefore comes out consistent, following its
  first line. The byte-order mark is dropped unless `--keep-bom`.
- **`keys` output.** `keys` is a splice of the original text: it keeps the
  mark, the line endings and every byte outside the edited spans, and
  `--line-ending`/`--keep-bom` do not apply to it.

## Writing files

In-place writes go to a temporary file in the same directory, which is then
renamed over the original, so a crash or a parse error can never leave a
half-written file. The original's permissions are copied to the temporary
file first. If the path is a symbolic link, the link is resolved and the
target is replaced, not the link. A file whose formatted output is identical
to its content is not rewritten at all (no timestamp change). With several
files, each is processed independently: an error in one is reported and the
others are still processed; the exit status is 2 if any failed, else 1 if
`--check` found a difference, else 0.

`fmt -` reads stdin and writes stdout; so does `fmt` with no files when
stdin is not a terminal. `--check` and `--diff` work on stdin too, reporting
the file as `<stdin>`.

## Keys

The algorithm is specified in full in the project brief and summarized in
`src/keys/mod.rs`; the corpus of real Google Scholar keys in
`tests/fixtures/scholar_keys.tsv` is the ground truth, and its fifth column
records whether a row was verified against Scholar's export. Design choices
around it:

- The name and title parsers produce plain `KeyParts { author, year,
  title }`; a `Style` assembles them. A JabRef-style template engine would be
  another `Style` consuming the same parts.
- LaTeX decoding (`keys/latex.rs`) works on a copy of the field text; the
  tree is never interpreted. The decoder keeps braces so that corporate
  authors (`{OpenAI}`) can still be recognized; braces are stripped
  afterwards.
- `$...$` math is removed from titles **before** LaTeX decoding, so that an
  escaped `\$` in a title is not mistaken for a math delimiter once decoded.
- NFC normalization uses the `unicode-normalization` crate (added in phase
  3): accents typed as combining sequences and accents produced by the
  decoder must transliterate identically.
- A value that contains macro parts (`author = goossens # and # mittelbach`)
  contributes the raw macro names, since macros are never resolved. Such
  entries are rare and the result is reported like any other.
- Reference fields (`crossref`, `xref`; the lists `related`, `ids`,
  `entryset`, `xdata`) are edited only when the value is a single `{...}` or
  `"..."` part; the inner text is spliced, the delimiter kept. Anything
  else (a concatenation, a macro) is reported and left alone. List entries
  are matched as whole keys, trimmed of white space.
- Collision suffixes: entries sharing a base key are ordered by file
  position; the first keeps the bare key, then `a`, `b`, ... `z`, `aa`, ...
  Keys of entries that are not selected (`--only`, `[keys] keep`) and keys
  that already equal their would-be new key count as taken. Consequence:
  inserting a colliding entry above an existing one shifts suffixes below
  it; `--map` records the change and the phase-5 `--rewrite` consumes it.
- Case: keys are compared as written when checking for collisions, because
  BibTeX treats keys case-insensitively only in the sense of matching
  `\cite` against entries; two entries whose keys differ by case are
  distinct entries to biber and a warning in BibTeX, and boringbib does not
  merge them. Generated keys are always lowercase ASCII.

## Configuration

`boringbib.toml`, discovered in the current directory or its ancestors,
stopping after the first directory that contains `.git` (so a file outside
the repository is never picked up); `--config PATH` overrides discovery and
must exist. Two tables, `[fmt]` and `[keys]`, with keys named exactly like
the long flags with underscores. Unknown keys are errors, so a typo cannot
silently do nothing. `wrap = 0` means off; `sort_fields` is `false`, `true`
(built-in order) or a list; `indent` is a number or `"tab"`.

Resolution: command line over file over built-in default. To make that
possible in both directions, every boolean flag has a `--no-…` twin
(`--align`/`--no-align`, `--trailing-comma`/`--no-trailing-comma`,
`--wrap N`/`--no-wrap`, `--sort-fields`/`--no-sort-fields`,
`--keep-bom`/`--no-keep-bom`, `--sort X`/`--no-sort`); when both are given,
the last one wins. `--sort-fields` takes its optional value only with `=`
(`--sort-fields=author,title`), so that `boringbib fmt --sort-fields
refs.bib` cannot swallow the file name.

## Decision log

Choices made where the brief was silent, with the property they serve.

1. Spans into an owned source instead of a tree of owned strings:
   losslessness by construction, edits as splices. (lossless)
2. `\r` is white space; the tree never normalizes line endings; only the
   printer chooses an ending, `auto` following the first line ending of the
   input. (lossless, deterministic)
3. `keys` keeps the byte-order mark and line endings; `fmt` drops the mark
   unless `--keep-bom`. (lossless for `keys`, as specified for `fmt`)
4. `@` is a block start only before `ident` + `{`/`(`; other `@` are junk;
   a line-initial `@name` without delimiter warns. (lossless, no surprises)
5. Identifiers may start with a digit and be non-ASCII; a bare value token
   is a number only when entirely digits, otherwise a macro. (lossless)
6. Empty keys are accepted. (lossless)
7. `@comment` bodies are brace-balanced, not "skip to next `@`". (matches
   every modern tool; the difference only shows for an `@` inside a comment)
8. All blocks are printed with braces, `@comment` and `@preamble` included;
   their bodies stay verbatim. (deterministic; always safe because bodies are
   brace-balanced)
9. White space in values collapses at every depth and the value is trimmed
   as a whole, never per part. (idempotent, BibTeX-equivalent)
10. Junk: leading and trailing blank lines dropped, everything else verbatim,
    white-space-only junk dropped; junk after an entry's `}` on the same line
    moves in front of the next block. (deterministic, idempotent)
11. Entries without fields print as `@type{key` newline `}`. (matches LaTeX
    Workshop; BibTeX accepts it)
12. Empty input formats to empty output; otherwise exactly one trailing
    newline. (idempotent)
13. Unclosed delimiters are reported at the opening character; columns count
    characters. (no surprises: that is where the fix goes and what editors
    show)
14. Duplicate fields are kept and warned about; the first one is the one
    that counts. (lossless)
15. In-place writes are temp-file-and-rename with permissions copied and
    symlinks resolved; unchanged files are not rewritten; several files are
    processed independently with exit status 2 over 1 over 0. (no surprises)
16. Every boolean flag has a `--no-` twin; `--sort-fields` requires `=` for
    its value; `--wrap 0` disables. (config overridable from the CLI;
    unambiguous argument parsing)
17. Unknown configuration keys are errors. (no surprises)
18. Key collisions count unselected and kept keys as taken; case-different
    keys are distinct. (idempotent, matches biber)
19. NFC via `unicode-normalization`; math stripped before decoding;
    macro parts contribute their names. (deterministic)
20. Reference fields are edited only when they are a single string part;
    everything else is reported. (lossless)

## What was borrowed, and from where

Read, not copied:

- **LaTeX Workshop** (`src/lint/bibtex-formatter/utils.ts`, MIT): per-entry
  alignment to the longest field name, the continuation indent for wrapped
  values (tab + padded name + ` = ` + delimiter), `@string` blocks sorting
  before entries, and the `localeCompare` key ordering that boringbib
  replaces with lowercase byte comparison.
- **bibtex-tidy** (MIT): option naming (`--sort-fields`, `--trailing-comma`,
  `--wrap`, `--no-…` twins) and, as a counter-example, its `--generate-keys`:
  it keeps hyphens (`fei-fei2006one-shot`), drops accented letters instead of
  transliterating them (`schlkopf2002learning`), and leaves `crossref`
  dangling after a rename.
- **biblib** (`biblib/bib.py`, MIT): the identifier and key character
  classes, the "number is a run of digits, else macro" rule, and BibTeX's
  white space compression and trimming semantics for values.
- **Tame the BeaST**, section on the `author` field: the three name forms,
  comma counting, and the "first lowercase-initial token starts the von
  part" rule.
