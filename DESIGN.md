# Design

boringbib aims to be predictable. Its design follows three priorities,
listed here in the order used to resolve conflicts:

1. Preserve the user's data except where they asked for a change.
2. Running `fmt` or `keys` twice should have the same result as running it once.
3. The same input and options should produce exactly the same bytes on every platform.

These properties are often called losslessness, idempotence, and determinism.
The [decision log](#decision-log) explains how they guide individual choices.

## Architecture

The project has one Rust crate containing a binary and a library.
The library does the parsing, formatting, and key generation.
This separation would let us add Python bindings with PyO3 or R bindings
with extendr without changing the command line interface.

```
src/
  main.rs        the binary: calls cli::run()
  lib.rs         crate documentation, modules, and the Error type
  cli.rs         command definitions, settings, and run()
  lexer.rs       reading characters and tracking positions, ParseError
  parser.rs      the grammar: text -> Cst
  cst.rs         source text, syntax tree, and text replacements
  printer.rs     formatting options and output
  sort.rs        grouping and ordering blocks
  config.rs      boringbib.toml
  keys/
    mod.rs       planning and applying key and reference changes
    names.rs     BibTeX name lists and the three name forms
    latex.rs     LaTeX -> Unicode table, for key generation only
    stopwords.rs verified and assumed stop word lists
tests/
  fixtures/      .bib inputs, expected outputs, and Scholar key examples (TSV)
```

Both commands start by parsing the input into a concrete syntax tree (`Cst`).
Formatting sorts and prints the blocks. Key rewriting plans the
text replacements, then applies them:

```
text ──parse()──▶ Cst ──sort::group()/sort()──▶ printer::format() ──▶ text
                   │
                   └──keys::plan()──▶ Plan ──keys::apply()──▶ text
```

The main dependencies are:

| Crates | Purpose |
| --- | --- |
| `clap` with derive support | Parse command line arguments |
| `anyhow`, `thiserror` | Handle errors |
| `deunicode` | Convert Unicode text to ASCII for citation keys |
| `unicode-normalization` | Normalize Unicode before generating keys |
| `serde`, `toml` | Read configuration files |
| `similar` | Produce diffs |

Tests use `insta`, `assert_cmd`, and `predicates`. boringbib has its own
BibTeX parser and uses no `unsafe` code.

## The tree

The `Cst` stores the original source text and a list of blocks. The blocks
cover the entire source without gaps or overlaps: the first starts at
byte 0, each block starts where the previous one ends, and the last reaches
the end of the source. Each node holds a `Span`, a range of bytes in that
source, instead of its own copy of the text.

A leading byte order mark is stored separately as a flag. `to_source()`
returns the original text with the mark restored. When the tree is built,
an assertion checks that the blocks cover the source exactly.

To rename a key or update a `crossref`, `Cst::with_replacements` takes a
list of `(Span, replacement)` pairs. It replaces those ranges and leaves
every other byte untouched. The edited text is then parsed again.

The tree also preserves line endings. The lexer treats `\r` as white
space, so a file with CRLF endings can be parsed and returned byte for
byte. Only the printer changes line endings; see
[Line endings and the byte order mark](#line-endings-and-the-byte-order-mark).

## Grammar

The grammar follows BibTeX's `bibtex.web`, as transcribed by biblib.
The differences are described below.

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

White space may appear between any two tokens above. Block and field names
are stored as written, but matched without regard to case. The character
rules follow BibTeX's `id_class` and key scanning:

- White space: space, tab, LF, CR, form feed (`char::is_ascii_whitespace`).
- Identifier: one or more characters, excluding white space and
  `" # % ' ( ) , = { }`.
- Key: zero or more characters, excluding white space and `, { } ( ) "`.
- Number: an identifier made entirely of ASCII digits. Any other value
  without quotes or braces is a macro name. Macros are never resolved.
- `balanced`: braces may nest, but every opening brace must have a matching
  closing brace. This also applies inside a `"..."` string, where a `"`
  character is allowed only within braces. A `(...)` body ends at the first
  `)` outside braces. Parentheses do not nest, as in BibTeX.

### Accepted input

The parser accepts some input that BibTeX would reject. It preserves that
input and warns where appropriate:

- **Text outside blocks**, called "junk" in the parser, is preserved.
  An `@` starts a block only when followed by an identifier and then `{`
  or `(`, with optional white space between them. Other uses of `@`, such
  as in email addresses or prose, remain ordinary text. If a line starts
  with `@name` after optional white space but has no opening delimiter,
  the parser warns that it may be a broken entry. `@comment` is exempt
  from this warning. BibTeX would stop with an error for the broken entry.
- **Identifiers** may start with a digit and contain characters outside
  ASCII. For example, boringbib preserves `3d = {...}`, which BibTeX would
  reject. As in BibTeX, `[`, `]`, `@`, `:`, and most other punctuation are
  valid identifier characters. Thus `title = [x]` refers to a macro named
  `[x]`.
- **Keys** may be empty (`@misc{, title = ...}`), as in BibTeX.
- **Duplicate field names** produce a warning, but all occurrences are
  kept. `Entry::field` returns the first, matching BibTeX's behavior.

boringbib also differs from BibTeX in how it reads `@comment{...}`. It
requires balanced braces and preserves the entire body, as biber, JabRef,
and `bibtex-tidy` do. BibTeX skips from `@comment` to the next `@`, so it
would read an `@entry` inside that body as an entry. A `@comment` without
an opening delimiter is treated as text outside a block, matching BibTeX.

### Errors and warnings

Parsing stops at the first error. Errors use `line:col: message`, with the
file name added by the command line interface. Lines and columns start at
1, and columns count characters rather than bytes.

An unclosed `{`, `(`, or `"` is reported at its opening character to help
you find what needs fixing. Other errors point to the unexpected character
and say what was found. For example:

```text
expected `,` or `}`, found `y`
expected `,` or `}`, found end of file
```

The parser uses these messages, adding what it found where appropriate:

```text
unbalanced braces: this `{` is never closed
unbalanced braces: unexpected `}` inside a quoted string
unterminated quoted string
unbalanced parentheses: this `(` is never closed
expected `=` after field name `x`
expected `=` after macro name `x`
expected `,` or `}`
expected a field name
expected a macro name
expected a value (`{...}`, `"..."`, a number or a macro name)
expected `}`
```

Warnings go to stderr as `file:line:col: warning: message`. They do not
change the exit status.

## Formatting

The printer creates a new layout for each block, in the order chosen by
the sorter. The defaults match LaTeX Workshop's formatter with
`align-equal` and sorting enabled.

### Names and entry layout

Block names, entry types, and field names become lowercase. This applies
only to ASCII letters, matching BibTeX's rules. Letters outside ASCII,
citation keys, and macro names keep their original spelling. For example,
`JMLR` stays uppercase in both `@string{JMLR = ...}` and `journal = JMLR`.
BibTeX matches macro names without regard to case.

All blocks use braces, including those originally written with parentheses.
There is no white space between `@name` and `{`. Entries use this layout:

- The first line is `@type{key,`.
- Each field gets its own line. `--indent` controls the indentation, which
  defaults to two spaces.
- The field name and value are separated by ` = `. With `--align`, field
  names are padded to the length of the longest name in that entry.
- Each field except the last ends with a comma. `--trailing-comma` adds a
  comma to the last field too.
- The closing `}` is on its own line.

An entry without fields has `@type{key` on the first line and `}` on the
next. With `--trailing-comma`, the first line is `@type{key,`. This matches
LaTeX Workshop, and BibTeX accepts both forms.

### Values

Numbers stay bare and macros remain unresolved. Parts joined with `#`
have one space on either side of it, as in `{First } # {edition}`.

Within each `{...}` or `"..."` part, a run of white space becomes one
space. This includes newlines and white space inside nested braces. The
printer then removes white space from the start and end of the whole
value. It keeps spaces between parts because, for example,
`"First " # "edition"` needs the space after `First`. These rules match
how BibTeX reads values, so the value it sees is unchanged.

By default, `--quotes braces` converts `"..."` parts to `{...}`. This is
safe because any quote character inside a quoted string must already be
enclosed in braces. `@string{name = value}` follows the same value rules.

### Wrapping

`--wrap N` adds newlines to values. It puts as many words as possible on
each line within `N` columns, counting any trailing comma. Columns count
characters, with a tab counting as one. A word that cannot fit gets a line
of its own, even if that line exceeds the limit.

Continuation lines start under the first character inside the value's
opening delimiter. The indentation accounts for the field's indentation,
padded name, ` = `, and opening delimiter, matching LaTeX Workshop.

Lines break only at spaces, and never immediately after an opening brace
or quote or before a closing one. This preserves the space after `First`
in `{First } # {edition}`. Parsing the wrapped text produces the same
value, and formatting it again produces the same output. `@string` values
are never wrapped.

### Comments and spacing

The bodies of `@preamble{...}` and `@comment{...}` are preserved, including
white space inside the delimiters. Any CRLF endings within them are
converted to the output line ending.

Text outside blocks keeps its indentation and trailing spaces. Leading
and trailing blank lines are removed; a blank line is empty or contains
only white space. Text consisting entirely of white space is removed.

Blocks are separated by one blank line. Comments and text attached to a
block appear directly before it, with no blank lines between them or
before the block. Comments and text at the end of the file appear last,
after one blank line.

The output ends with exactly one newline. If the input is empty or
contains only white space, the output is empty.

### Field order

`--sort-fields` puts fields in this order:

```text
author, editor, title, booktitle, journal, year, month, volume, number,
pages, publisher, address, edition, series, chapter, howpublished,
institution, organization, school, type, note, doi, url, urldate, isbn,
issn, eprint, archiveprefix, primaryclass, keywords, abstract, file
```

Any remaining fields follow in alphabetical order. `--sort-fields=a,b`
puts `a` and `b` first, followed by the default order. Names are compared
in lowercase, and duplicate fields keep their relative order. Alignment
still uses the longest name across all fields in the entry.

## Sorting

Sorting moves blocks together with the comments and text that precede
them. These form a **group**, so a comment describing an entry stays with
that entry. Comments and text at the end of the file form their own group
and always stay last.

`--sort` chooses the order:

- `key` (the default) puts `@preamble` and `@string` groups first, keeping
  their original relative order. Entries follow, sorted by
  `key.to_lowercase()`. Ties are broken by the original key in byte order,
  then by the entry's original position.
- `year` sorts from oldest to newest using the first number with four
  digits in `year`, falling back to `date`. Entries without a year come
  last. Ties are broken by key.
- `type` sorts by the lowercase entry type, then by key.
- `author` sorts by the first author's last name, then by year, then by
  key. It uses the key generator's name parser, so `van der Maaten` sorts
  under `v`.
- `none` keeps the original file order, including `@string` blocks.

All sorts are stable: entries that compare equal keep their relative order.
Key sorting uses lowercase byte comparison instead of LaTeX Workshop's
`localeCompare`. This gives the same order regardless of the machine's
locale.

## Line endings and the byte order mark

When reading a file, boringbib removes a leading `UTF-8` byte order mark
and records its presence. Invalid `UTF-8` produces an error with the file
name and byte offset. The tree preserves the original line endings.

For `fmt`, the printer first produces LF endings. It also converts CRLF
to LF in text copied from outside blocks and from `@comment` and
`@preamble` bodies. If CRLF output is requested, a final pass converts LF
to CRLF. A lone `\r`, without a following `\n`, stays where it was.

`--line-ending` accepts `lf`, `crlf`, or `auto`. The default, `auto`, uses
CRLF if the first line ending in the input is CRLF, and LF otherwise.
This makes files with mixed LF and CRLF endings consistent. `fmt` removes
the byte order mark unless `--keep-bom` is set.

`keys` replaces text in the original source, so it preserves the byte order
mark, line endings, and every byte outside the replaced ranges.
`--line-ending` and `--keep-bom` apply only to `fmt`.

## Writing files

To update a file in place, boringbib writes the output to a temporary file
in the same directory. It copies the original permissions, then renames
the temporary file over the original. This prevents a crash or parse error
from leaving the original partly written.

If the path is a symbolic link, boringbib resolves it and replaces the
target, preserving the link. Files whose output matches their contents are
left untouched, including their timestamps.

Each file is processed independently. If one fails, boringbib reports the
error and continues with the others. The exit status is 2 if any file
failed, 1 if `--check` found a difference, and 0 otherwise.

`fmt -` reads stdin and writes stdout. `fmt` also does this when no files
are given and stdin is not a terminal. `--check` and `--diff` accept stdin
too, using `<stdin>` as the file name:

- `--check` prints `would reformat FILE` to stdout for each file that
  would change.
- `--diff` prints a unified diff with `a/FILE` and `b/FILE` headers, so
  you can apply it with `patch -p1`.

Both modes still print warnings. Files that are already formatted produce
no formatting report or diff.

## Keys

`src/keys/mod.rs` summarizes how keys are generated. The examples in
`tests/fixtures/scholar_keys.tsv` provide the expected results. The fifth
column records whether each example was verified against Google Scholar's
export.

The name and title parsers produce `KeyParts { author, year, title }`.
A `Style` combines those parts into a key. A template system similar to
JabRef's could use another `Style` with the same parsed parts.

### Decoding names and titles

LaTeX decoding in `keys/latex.rs` works on a copy of the field text, leaving
the tree unchanged. Braces are kept long enough to recognize corporate
authors such as `{OpenAI}`, then removed.

Math enclosed in `$...$` or `$$...$$` is removed from titles before LaTeX
decoding. This lets the decoder distinguish an escaped `\$` from a math
delimiter. If a title has an unmatched `$`, its text is kept with the `$`
signs removed.

The decoder uses `unicode-normalization` to put Unicode text into NFC,
a standard form that gives equivalent accent spellings the same
representation. It emits combining marks for accents, then normalizes the
whole result. As a result, `\"o`, `\"{o}`, `{\"o}`, `{\" o}`, and `ö`
produce the same text and the same ASCII key component.

LaTeX commands follow TeX's rules:

- A control word includes the longest run of letters after the backslash,
  so `\oe` is one command.
- White space after a recognized control word is consumed. For example,
  `\o rsted` becomes `ørsted`.
- White space before an accent's argument is skipped.
- An unknown command without an argument is removed, but the white space
  after it is kept so adjacent words stay separate. Some commands,
  including `\textendash`, are deliberately treated as unknown.
- The control space `\ ` and line break `\\` each become a space.

### Fields that use macros

An `author`, `editor`, or `title` containing a macro cannot be used to
generate a key because boringbib does not resolve macros. For example,
`author = goossens # and # mittelbach` causes the entry to be reported and
left unchanged. A macro in `year` only omits the year component from the
generated key.

`--sort author` shares the name parser but uses macro names as text. This
lets it order entries without needing to resolve the macros.

### Updating references

`keys` updates `crossref` and `xref`, along with the lists in `related`,
`ids`, `entryset`, and `xdata`. A reference can be edited only when its
value is a single `{...}` or `"..."` part. The text inside is replaced,
and the original delimiters are kept.

Other forms, such as macros or concatenated values, are reported and left
unchanged. Within lists, surrounding white space is ignored when matching
each key, and only a match for the whole key is replaced.

### Avoiding duplicate keys

Keys are assigned in file order. An entry gets its base key if that key is
available. Otherwise it gets the first available suffix from `a`, `b`,
through `z`, then `aa`, `ab`, and so on. For entries that would share a
base key, this usually means the first gets the bare key and the second
gets `a`.

Keys belonging to entries that will stay unchanged are reserved before
assignment starts. This includes entries excluded by `--only`, listed in
`[keys] keep`, or skipped because they lack an author or title. Old keys
of entries being renamed are available for reuse.

Inserting an entry above another with the same base key can change the
suffixes below it. `--map` records these changes. The planned `--rewrite`
option will use that mapping to update citations in documents.

Both duplicate detection and reference matching ignore case, as BibTeX
does when matching citation keys. For example, if `Smith2020foo` is kept,
`smith2020foo` is already taken. Generated keys contain only lowercase
ASCII characters, so this distinction matters for existing keys.

### Reports and mappings

If a key passed to `--only` matches no entry, boringbib prints a warning.
This allows the same selection to be used across several files, where a
key may occur in only one of them.

The key mapping is printed in `old  new` columns, with padding after old
keys to align the new keys. With multiple input files, a third column
shows the file name. `--map` saves the combined mapping as
`old<TAB>new<TAB>file`.

With `--write -`, stdout contains the rewritten bibliography. Use `--map`
to save the mapping separately.

## Configuration

boringbib looks for `boringbib.toml` in the current directory, then in each
parent directory. It stops after checking the first directory that
contains `.git`, which keeps the search within the repository. You can
choose a file with `--config PATH`; that file must exist.

The file has two tables, `[fmt]` and `[keys]`. Their keys use the long
option names with hyphens replaced by underscores. Unknown keys produce
an error to help catch typos. A few settings accept more than one form:

- `wrap = 0` disables wrapping.
- `sort_fields` accepts `false`, `true` for the default order, or a list
  of field names.
- `indent` accepts a number of spaces or `"tab"`.

Command line options override file settings, which override defaults.
Options can be disabled as well as enabled from the command line:

| Enable or choose | Disable |
| --- | --- |
| `--align` | `--no-align` |
| `--trailing-comma` | `--no-trailing-comma` |
| `--wrap N` | `--no-wrap` |
| `--sort-fields` | `--no-sort-fields` |
| `--keep-bom` | `--no-keep-bom` |
| `--sort X` | `--no-sort` |

When both forms are given, the last one wins. `--sort-fields` accepts an
optional value only after `=`, as in `--sort-fields=author,title`. This
lets `boringbib fmt --sort-fields refs.bib` treat `refs.bib` as a file name.

## Decision log

These decisions explain how boringbib preserves data and keeps its
behavior predictable. The sections above describe the rules in detail.

1. The tree stores ranges in the original source text. This lets edits
   replace selected ranges while preserving every other byte.
2. The lexer treats `\r` as white space, and the tree preserves line
   endings. The printer chooses the output ending, following the first
   input ending when `auto` is set.
3. `keys` preserves the byte order mark and line endings. `fmt` drops the
   mark unless `--keep-bom` is set, as part of its formatting defaults.
4. `@` starts a block only before an identifier and `{` or `(`. Other
   occurrences remain text. A line starting with `@name` but missing an
   opening delimiter produces a warning, except for `@comment`. This
   preserves ordinary text while flagging likely mistakes.
5. Identifiers may start with digits or contain characters outside ASCII.
   Only a value made entirely of ASCII digits is a number; other bare
   values are macros. This preserves input that BibTeX might reject.
6. Empty keys are accepted so that parsing does not discard such entries.
7. `@comment` bodies must have balanced braces. This matches biber,
   JabRef, and `bibtex-tidy`, including when a comment contains `@`.
8. All blocks are printed with braces, including `@comment` and
   `@preamble`. Their contents are preserved apart from line endings.
   Requiring balanced braces makes this conversion safe.
9. White space is collapsed at every brace depth, then trimmed from the
   whole value. Spaces between parts are preserved, matching what BibTeX
   reads and making repeated formatting stable.
10. Text outside blocks loses leading and trailing blank lines, but keeps
    its other contents. Text made only of white space is removed. Text
    after an entry's `}` on the same line moves before the next block.
    These rules give comments and spacing a consistent layout.
11. Entries without fields print as `@type{key` followed by a newline
    and `}`. This matches LaTeX Workshop and is accepted by BibTeX.
12. Empty input, including input containing only white space, produces
    empty output. Other output ends with one newline, so formatting it
    again adds nothing.
13. Errors for unclosed delimiters point to the opening character.
    Columns count characters, matching what an editor shows.
14. Duplicate fields are preserved and produce a warning. The first
    occurrence is used, matching BibTeX without losing the others.
15. Files are updated by writing a temporary file and renaming it. This
    preserves permissions and symbolic links and avoids partial writes.
    Unchanged files keep their timestamps. Files are processed
    independently, with exit status 2 taking precedence over 1 and 0.
16. Options have a `--no-` form so file settings can be overridden from
    the command line. `--sort-fields` requires `=` before its value to
    distinguish it from a file name. `--wrap 0` disables wrapping.
17. Unknown configuration keys produce errors so typos are easy to find.
18. Keys of unchanged entries are reserved before new keys are assigned
    in file order. Comparisons ignore case, preventing duplicates that
    BibTeX would treat as the same key.
19. Key generation uses NFC normalization and removes math before LaTeX
    decoding. Entries whose author, editor, or title uses a macro are
    reported and skipped because those values cannot be resolved.
20. References are updated only when their value is one string part.
    Other forms are reported and preserved rather than guessed at.
21. Entry types and field names are lowercased using ASCII rules. Macro
    names keep their spelling. This follows BibTeX's name matching while
    limiting formatting changes to the names that need them.
22. Errors describe what was found, with unclosed delimiters reported
    at their opening character. Identifier punctuation follows BibTeX,
    so `[x]` is a macro name.
23. `--check` reports to stdout, and `--diff` uses `a/` and `b/` headers.
    Files that are already formatted produce no report or diff. This
    makes the output useful in shell scripts.
24. LaTeX decoding follows TeX's command and argument rules. Unknown
    commands keep the white space after them, while `\ ` and `\\`
    become spaces. This keeps words separate during key generation.
25. `keys` prints aligned old and new keys, adding file names for multiple
    inputs. Warnings go to stderr. A missing `--only` key is a warning,
    allowing a selection to span several files.
26. `--wrap` counts trailing commas and treats a tab as one column. It
    preserves spaces next to delimiters and leaves `@string` values
    alone. The result has the same meaning to BibTeX and does not change
    when formatted again.
27. Field sorting compares lowercase names and preserves the order of
    duplicates. Fields outside the configured order follow alphabetically,
    giving a consistent result for every field name.

## Sources

These projects and references informed the design. Their code was read
but not copied.

- **LaTeX Workshop** (`src/lint/bibtex-formatter/utils.ts`, MIT) provided
  the model for aligning fields within an entry and indenting wrapped
  values. It also places `@string` blocks before entries. boringbib
  replaces its `localeCompare` sorting with lowercase byte comparison so
  that the order is the same on every machine.
- **`bibtex-tidy`** (MIT) informed option names such as `--sort-fields`,
  `--trailing-comma`, `--wrap`, and their `--no-` forms. Its
  `--generate-keys` behavior also helped identify changes needed here:
  it keeps hyphens (`fei-fei2006one-shot`), drops accented letters
  (`schlkopf2002learning`), and leaves `crossref` pointing to old keys.
- **biblib** (`biblib/bib.py`, MIT) provided the character rules for
  identifiers and keys. It also documents how BibTeX distinguishes numbers
  from macros and how it collapses and trims white space in values.
- **Tame the BeaST**, in its section on the `author` field, describes the
  three name forms and how commas distinguish them. It also explains
  where the "von" part begins: at the first token whose initial letter
  is lowercase.
