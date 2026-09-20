# Ideas

This document records possible additions to boringbib. They are outside the
scope of version 0.1; listing them here does not commit us to building them.

## Out of scope for 0.1

- Fetching metadata from the network (DOI, Crossref, arXiv).
- Finding and removing duplicate entries.
- Changing title capitalization or adding braces to protect it.
- Checking entries against the biblatex data model.
- A language server (LSP) for editors.
- Python bindings using PyO3/maturin and R bindings using extendr. The
  library in `lib.rs` would let us add these without changing the command
  line interface.
- A template language for citation keys, similar to JabRef's. A new
  `keys::Style` could combine the existing `keys::KeyParts` in a different
  way, reusing the name and title parsers.

## Citation rewriting

Citation rewriting is planned for phase 5, but implementation should wait
until it is requested. The proposed `boringbib keys --rewrite GLOB...`
would update citations in `.tex`, `.qmd`, `.Rmd`, and `.md` files using the
mapping saved by `--map` (`old<TAB>new<TAB>file`).

It would support the `\cite` family of commands, including forms with a
star or optional arguments. For Pandoc documents, it would support `@key`,
`[@key]`, `-@key`, and `@{key}`.

## Editor and release integration

Editor support and release automation are also outside the scope of 0.1.
Editors can already use `boringbib fmt -` to send text through stdin and
receive the formatted result on stdout.

- A VS Code task with a keyboard shortcut to run `boringbib fmt` on the
  current file.
- A small VS Code extension for local use, estimated at about 40 lines.
  It would register a `DocumentFormattingEditProvider` for `bibtex` and
  pass the editor buffer through `boringbib fmt -`. You could package it
  with `vsce package` and install the resulting `.vsix` file.
- A Neovim `conform.nvim` formatter entry (`command = "boringbib"`,
  `args = { "fmt", "-" }`).
- A release workflow using `cargo-dist` to produce binaries for the
  Homebrew tap.

## Other ideas

- `--stdin-filepath PATH` for editors that send text through `fmt -`.
  This would let boringbib find `boringbib.toml` relative to the file being
  edited rather than the working directory.
- `--sort year-desc` to put the newest entries first, as LaTeX Workshop offers.
