# boringbib

[![crates.io](https://img.shields.io/crates/v/boringbib.svg)](https://crates.io/crates/boringbib)
[![CI tests](https://github.com/nanxstats/boringbib/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/boringbib/actions/workflows/ci.yml)

A boring BibTeX formatter. That's the point.

`boringbib` does two things to `.bib` files, deterministically, idempotently,
and without touching a byte you did not ask it to change:

- `boringbib fmt` sorts entries by citation key and pretty-prints each entry
  with the `=` signs aligned, exactly like LaTeX Workshop's "Align and sort"
  action.
- `boringbib keys` rewrites citation keys into Google Scholar style
  (`vaswani2017attention`) and updates `crossref` and friends inside the file
  so nothing dangles.

> **Status:** under construction. `fmt` works (except `--wrap`,
> `--sort-fields` and `--sort author`); `keys` is next. The command-line
> surface below is final; see [`DESIGN.md`](DESIGN.md) for the plan.

## Install

With Homebrew:

```console
brew install nanxstats/tap/boringbib
```

Or with Cargo:

```console
cargo install boringbib
```

## Format

```console
boringbib fmt refs.bib            # rewrite in place (atomically)
boringbib fmt -                   # stdin to stdout (also with no files and piped stdin)
boringbib fmt --check refs.bib    # exit 1 if the file would change, print which
boringbib fmt --diff refs.bib     # unified diff of what would change, no write
```

The defaults reproduce LaTeX Workshop's formatter with `align-equal` and
sorting on:

| Option | Default | Meaning |
| --- | --- | --- |
| `--sort <key\|none\|year\|type\|author>` | `key` | Ordering of entries; `--no-sort` is `--sort none` |
| `--indent <N\|tab>` | `2` | Indentation of field lines |
| `--align` / `--no-align` | on | Pad field names to the longest field name in the entry |
| `--quotes <braces\|keep>` | `braces` | Convert `"..."` values to `{...}`, or leave delimiters alone |
| `--trailing-comma` / `--no-trailing-comma` | off | Comma after the last field |
| `--wrap <N>` / `--no-wrap` | off | Greedy word-wrap of values at column N |
| `--sort-fields[=ORDER]` / `--no-sort-fields` | off | Reorder fields inside each entry |
| `--line-ending <auto\|lf\|crlf>` | `auto` | `auto` writes CRLF only if the file was CRLF |
| `--keep-bom` / `--no-keep-bom` | off | Re-emit a leading byte-order mark |

What `fmt` does to an entry:

```bibtex
@Article{Vaswani2017-xy,
  title={Attention is all you need},
  author={Vaswani, Ashish and Shazeer, Noam},
  year={2017}
}
```

becomes

```bibtex
@article{Vaswani2017-xy,
  title  = {Attention is all you need},
  author = {Vaswani, Ashish and Shazeer, Noam},
  year   = {2017}
}
```

Entry types and field names are lowercased, keys are left alone, runs of
white space inside values collapse to one space, and exactly one blank line
separates blocks. Comments and `%` lines between entries move together with
the entry they precede. Everything else (macros, numbers, `@preamble` and
`@comment` bodies, nested braces) is printed as written.

## Rewrite keys

```console
boringbib keys refs.bib                    # print the old -> new mapping, change nothing
boringbib keys --write refs.bib            # apply, atomically
boringbib keys --write --map keys.tsv ...  # also write the mapping (old<TAB>new<TAB>file)
boringbib keys --only KEY[,KEY...] ...     # restrict to these current keys
boringbib keys --style scholar ...         # the only style in 0.1
```

A key is `lastname` + `year` + first meaningful title word, the way Google
Scholar's BibTeX export builds it: `Sch{\"o}lkopf` gives `scholkopf`,
`van der Maaten` gives `vandermaaten`, `Fei-Fei` gives `fei`, `{OpenAI}`
gives `openai`, and "On the difficulty of training recurrent neural
networks" gives `difficulty`. Entries that share a key get suffixes `a`,
`b`, ... in file order. `crossref`, `xref`, `related`, `ids`, `entryset`
and `xdata` fields that point at a renamed key are updated in the same pass.

`keys` never reformats; run `fmt` afterwards if you want both.

Suffixes depend on file order: inserting a new entry that collides with an
existing key above it shifts the suffixes of the entries below. That is why
`--map` exists, and why a later phase adds `--rewrite` to update citations in
your documents from that mapping.

## Configuration

An optional `boringbib.toml` in the current directory or any ancestor (the
search stops at a `.git` directory), or the file given with `--config PATH`.
Keys are named like the long flags, with underscores. Command-line flags win.

```toml
[fmt]
sort = "key"
indent = 2
align = true
quotes = "braces"
trailing_comma = false
sort_fields = false
line_ending = "auto"

[keys]
style = "scholar"
stop_words_extra = ["towards"]
keep = ["knuth1984"]
```

## Editor integration

Copy-paste snippets for a `pre-commit` hook, a VS Code task, a Neovim
`conform.nvim` formatter entry, and a private VS Code extension
(`docs/vscode-extension.md`) are part of a later phase.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | `fmt --check` found a file that would change |
| 2 | Error: syntax, I/O, or invalid arguments. Files are never partially written |

Syntax errors are reported as `file:line:col: message`.

## Design

The parser keeps every byte of the input in a lossless syntax tree; the
formatter is a separate pass, and `keys` is a targeted edit of that tree. See
[`DESIGN.md`](DESIGN.md) for the grammar, the output rules and the decisions
behind them, and [`IDEAS.md`](IDEAS.md) for what was deliberately left out.

## License

MIT
