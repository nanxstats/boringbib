# boringbib

[![crates.io](https://img.shields.io/crates/v/boringbib.svg)](https://crates.io/crates/boringbib)
[![CI tests](https://github.com/nanxstats/boringbib/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/boringbib/actions/workflows/ci.yml)

A boring BibTeX formatter.

`boringbib` gives your `.bib` files a consistent format. The same input and
options always produce the same output, and running it again makes no
further changes.

- `boringbib fmt` sorts entries by citation key and aligns the `=` signs
  within each entry.
- `boringbib keys` rewrites citation keys in the style used by Google
  Scholar. It also updates references to those keys within the file,
  including `crossref` fields.

## Install

You can install boringbib with Homebrew:

```console
brew install nanxstats/tap/boringbib
```

Or with Cargo:

```console
cargo install boringbib
```

## Format

```console
boringbib fmt refs.bib             # format the file in place
boringbib fmt -                   # read stdin and write stdout
boringbib fmt --check refs.bib     # report whether formatting would change the file
boringbib fmt --diff refs.bib      # show the changes without writing them
```

Files are replaced only after the output has been written successfully.
You can also pipe input to `boringbib fmt` without specifying a file.
With `--check`, the command prints the names of files that would change
and exits with status 1.

The default settings match LaTeX Workshop's formatter with `align-equal`
and sorting enabled:

| Option | Default | Meaning |
| --- | --- | --- |
| `--sort <key\|none\|year\|type\|author>` | `key` | Choose the entry order; `--no-sort` keeps the original order |
| `--indent <N\|tab>` | `2` | Indent fields with N spaces or a tab |
| `--align` / `--no-align` | on | Align the `=` signs within each entry |
| `--quotes <braces\|keep>` | `braces` | Convert `"..."` values to `{...}`, or keep the original quotes and braces |
| `--trailing-comma` / `--no-trailing-comma` | off | Add a comma after the last field |
| `--wrap <N>` / `--no-wrap` | off | Wrap values at column N without splitting words |
| `--sort-fields[=ORDER]` / `--no-sort-fields` | off | Reorder fields inside each entry |
| `--line-ending <auto\|lf\|crlf>` | `auto` | Use LF or CRLF; `auto` follows the first line ending in the file |
| `--keep-bom` / `--no-keep-bom` | off | Keep a leading byte order mark |

For example, `fmt` turns this entry:

```bibtex
@Article{Vaswani2017-xy,
  title={Attention is all you need},
  author={Vaswani, Ashish and Shazeer, Noam},
  year={2017}
}
```

into:

```bibtex
@article{Vaswani2017-xy,
  title  = {Attention is all you need},
  author = {Vaswani, Ashish and Shazeer, Noam},
  year   = {2017}
}
```

Entry types and field names become lowercase. Citation keys keep their
original spelling. Within values, each run of white space becomes a single
space. Blocks are separated by one blank line, and comments between entries
stay with the entry that follows them.

The formatter preserves macros, numbers, and nested braces. It also keeps
the contents of `@preamble` and `@comment` blocks, apart from line endings.

You can also wrap values and choose the field order:

- `--wrap 80` wraps values at spaces to fit within 80 columns, including
  any trailing comma. Each continuation line starts under the first
  character of the value. Words are never split, so a long word can exceed
  the limit. Wrapping preserves the value that BibTeX reads.
- `--sort-fields` puts common fields first, using the order below.
  Any other fields follow alphabetically. `--sort-fields=doi,url` puts
  those two first and keeps the rest of the default order.

```text
author, editor, title, booktitle, journal, year, month, volume, number,
pages, publisher, address, edition, series, chapter, howpublished,
institution, organization, school, type, note, doi, url, urldate, isbn,
issn, eprint, archiveprefix, primaryclass, keywords, abstract, file
```

## Rewrite keys

```console
boringbib keys refs.bib                    # preview the old and new keys
boringbib keys --write refs.bib            # apply the changes
boringbib keys --write --map keys.tsv ...  # also save the key mapping
boringbib keys --only KEY[,KEY...] ...     # change only these keys
boringbib keys --style scholar ...        # the only style in 0.1
```

A key combines the first author's last name, the year, and the first
meaningful word of the title. This follows Google Scholar's BibTeX export.
For example:

| Name or title | Key component |
| --- | --- |
| `Sch{\"o}lkopf` | `scholkopf` |
| `van der Maaten` | `vandermaaten` |
| `Fei-Fei` | `fei` |
| `{OpenAI}` | `openai` |
| On the difficulty of training recurrent neural networks | `difficulty` |

When entries would have the same key, the first keeps the key and later
entries get suffixes `a`, `b`, and so on, in file order. References to renamed
keys are updated in `crossref`, `xref`, `related`, `ids`, `entryset`, and
`xdata` fields.

`keys` preserves the file's formatting. Run `fmt` afterwards if you also
want to format it. Entries that cannot be renamed are left unchanged and
reported on stderr. This happens, for example, when an entry has no author
or editor, its first author is `others`, its title is empty, or a required
field uses a macro. References to keys missing from the file are also
reported and left unchanged.

Suffixes depend on file order. If you insert an entry before another that
would have the same key, the suffixes of later entries can change. Use
`--map keys.tsv` to save the old key, new key, and file name as columns
separated by tabs (`old<TAB>new<TAB>file`). A planned `--rewrite` option
will use this mapping to update citations in documents.

## Configuration

You can save your settings in `boringbib.toml`. boringbib looks for this
file in the current directory, then in each parent directory. The search
stops after checking a directory that contains `.git`. Use `--config PATH`
to choose a file explicitly.

Configuration keys use the same names as the long flags, with hyphens
replaced by underscores. Command line options override the file settings.

```toml
[fmt]
sort = "key"            # key | none | year | type | author
indent = 2              # or "tab"
align = true
quotes = "braces"       # braces | keep
trailing_comma = false
wrap = 80               # 0 or absent: no wrapping
sort_fields = false     # true for the default order, or ["author", "title"]
line_ending = "auto"    # auto | lf | crlf
keep_bom = false

[keys]
style = "scholar"
stop_words_extra = ["towards"]   # adds to the default stop words
keep = ["knuth1984"]             # never rewritten
```

Unknown configuration keys produce an error to help you catch typos.
Options such as `--align` and `--wrap` have a corresponding `--no-` form,
so you can disable a setting from the command line as well as enable it.

## Use with `pre-commit`

To run boringbib with `pre-commit`, install boringbib and add a local hook
to `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: boringbib
        name: boringbib fmt
        entry: boringbib fmt
        language: system
        files: \.bib$
```

The hook formats the staged `.bib` files in place. If any files change,
the commit stops so you can review and stage them again. Use
`entry: boringbib fmt --check` if you want the hook to report formatting
differences without changing files.

## Editors

If your editor can pass text through a command, use `boringbib fmt -`.
It reads from stdin and writes the formatted result to stdout. A syntax
error produces a `line:col: message` diagnostic and exit status 2.
Configuration is discovered from the working directory.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | `fmt --check` found a file that would change |
| 2 | Error: syntax, I/O, or invalid arguments. Files are never partially written |

Syntax errors are reported as `file:line:col: message`.

## Design

The parser preserves the original input. The formatter uses the parsed
structure to write a new layout, while `keys` replaces only the text needed
to rename keys and update references.

See [`DESIGN.md`](DESIGN.md) for the grammar, formatting rules, and reasons
behind these choices. [`IDEAS.md`](IDEAS.md) describes possible additions
and the limits of the current release.

## License

MIT
