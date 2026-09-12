# Ideas

Things that are deliberately not part of boringbib 0.1. Recorded here so
that they are not forgotten and not accidentally built.

## Out of scope for 0.1

- Fetching metadata from the network (DOI, Crossref, arXiv).
- Deduplication of entries.
- Title case or brace protection edits to field values.
- biblatex data model validation.
- An LSP server.
- Python (PyO3/maturin) and R (extendr) bindings. `lib.rs` exists so that
  they can be added without touching the command-line layer.
- A JabRef-style key template language. `keys::KeyParts` and `keys::Style`
  are the seam: a template engine consumes the same parsed parts, so the name
  and title parsing would not change.

## Planned, only when asked (phase 5)

- `boringbib keys --rewrite GLOB...`: update citations in `.tex`, `.qmd`,
  `.Rmd` and `.md` files from the mapping: all `\cite`-family commands,
  including starred and optional-argument forms, and Pandoc `@key`,
  `[@key]`, `-@key` and `@{key}` syntax. The `--map` output
  (`old<TAB>new<TAB>file`) is designed to drive it.

## Editor and release integration

Left out of 0.1 as not central to the tool; the `fmt -` stdin/stdout mode
is all any of them needs.

- A VS Code task bound to a key that runs `boringbib fmt` on the current
  file, and a private ~40-line VS Code extension registering a
  `DocumentFormattingEditProvider` for the `bibtex` language that pipes the
  buffer through `boringbib fmt -`, packaged with `vsce package` and
  installed from the local `.vsix`.
- A Neovim `conform.nvim` formatter entry (`command = "boringbib"`,
  `args = { "fmt", "-" }`).
- A `cargo-dist` release workflow producing binaries for the Homebrew tap.

## Other ideas

- `--stdin-filepath PATH` for editors that pipe a buffer through `fmt -` but
  want `boringbib.toml` discovered relative to the file rather than the
  working directory.
- A `--sort` of `year-desc` (newest first), as LaTeX Workshop offers.
