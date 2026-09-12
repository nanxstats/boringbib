# Changelog

## boringbib 0.1.0

### New features

- `boringbib fmt`: sort entries by citation key and align the `=` signs of
  each entry, LaTeX Workshop style, with `--check` and `--diff` modes,
  atomic in-place writes, stdin/stdout support, and the extra `--wrap` and
  `--sort-fields` options.
- `boringbib keys`: rewrite citation keys into Google Scholar style and
  update `crossref`, `xref`, `related`, `ids`, `entryset` and `xdata`
  references inside the file, with `--only`, `--write` and `--map`.
- Optional `boringbib.toml` configuration file.
