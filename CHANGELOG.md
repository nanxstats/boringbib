# Changelog

## boringbib 0.1.1

### Documentation

- Rewrote the README, changelog, design notes, and ideas document with
  plainer wording, shorter sentences, and clearer organization (#6).

## boringbib 0.1.0

### New features

- `boringbib fmt` sorts entries by citation key and aligns the `=` signs
  within each entry. The default format follows LaTeX Workshop. Use `--wrap`
  to wrap long values and `--sort-fields` to choose the field order.
- `boringbib fmt --check` reports files that need formatting, and `--diff`
  shows the proposed changes. You can format files in place or pass text
  through stdin and stdout. Files are replaced only after the output has
  been written successfully.
- `boringbib keys` rewrites citation keys in the style used by Google
  Scholar. It also updates references in `crossref`, `xref`, `related`,
  `ids`, `entryset`, and `xdata` fields within the file. Use `--only` to
  select keys, `--write` to apply changes, and `--map` to save the old and
  new keys.
- You can save settings in an optional `boringbib.toml` file.
