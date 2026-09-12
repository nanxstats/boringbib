//! Formatting tests over the fixtures: the golden output for Appendix A, a
//! snapshot per fixture, and idempotency and losslessness for every fixture
//! under several option sets.

use std::fs;
use std::path::{Path, PathBuf};

use boringbib::printer::{Indent, LineEnding, Quotes, SortFields};
use boringbib::sort::SortKey;
use boringbib::{FmtOptions, format_str, parse};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn read(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8(bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every `*.bib` file in the fixtures directory, sorted by name.
fn bib_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixtures_dir())
        .expect("fixtures directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "bib"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures found");
    paths
}

/// The fixtures that are inputs (not golden outputs), as `(name, text)`.
fn inputs() -> Vec<(String, String)> {
    bib_files()
        .into_iter()
        .filter(|path| !path.to_string_lossy().ends_with(".expected.bib"))
        .map(|path| {
            let name = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            (name, read(&path))
        })
        .collect()
}

/// Makes byte-order marks and carriage returns visible in snapshots.
fn visible(text: &str) -> String {
    text.replace('\u{FEFF}', "<BOM>").replace('\r', "<CR>")
}

fn option_sets() -> Vec<(&'static str, FmtOptions)> {
    vec![
        ("default", FmtOptions::default()),
        (
            "keep quotes, tab, trailing comma, no align, no sort",
            FmtOptions {
                quotes: Quotes::Keep,
                indent: Indent::Tab,
                trailing_comma: true,
                align: false,
                sort: SortKey::None,
                ..FmtOptions::default()
            },
        ),
        (
            "sort year, indent 4, crlf, keep bom",
            FmtOptions {
                sort: SortKey::Year,
                indent: Indent::Spaces(4),
                line_ending: LineEnding::Crlf,
                keep_bom: true,
                ..FmtOptions::default()
            },
        ),
        (
            "sort type, lf",
            FmtOptions {
                sort: SortKey::Type,
                line_ending: LineEnding::Lf,
                ..FmtOptions::default()
            },
        ),
        (
            "wrap 60, sort fields",
            FmtOptions {
                wrap: Some(60),
                sort_fields: SortFields::Default,
                ..FmtOptions::default()
            },
        ),
        (
            "wrap 24, no align, tab, custom field order, trailing comma",
            FmtOptions {
                wrap: Some(24),
                align: false,
                indent: Indent::Tab,
                sort_fields: SortFields::from_list("year,title"),
                trailing_comma: true,
                ..FmtOptions::default()
            },
        ),
    ]
}

#[test]
fn appendix_a_wrapped_and_field_sorted_snapshot() {
    let input = read(&fixtures_dir().join("messy.bib"));
    let options = FmtOptions {
        wrap: Some(80),
        sort_fields: SortFields::Default,
        ..FmtOptions::default()
    };
    let output = format_str(&input, &options).expect("messy.bib parses");
    for line in output.lines() {
        assert!(line.chars().count() <= 80, "{line:?}");
    }
    insta::assert_snapshot!("fmt__messy_wrap80_sort_fields", visible(&output));
}

#[test]
fn appendix_a_matches_the_golden_file_byte_for_byte() {
    let input = read(&fixtures_dir().join("messy.bib"));
    let expected = read(&fixtures_dir().join("messy.expected.bib"));
    let output = format_str(&input, &FmtOptions::default()).expect("messy.bib parses");
    assert_eq!(output, expected);
}

#[test]
fn every_fixture_has_a_snapshot() {
    for (name, input) in inputs() {
        let output =
            format_str(&input, &FmtOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        insta::assert_snapshot!(format!("fmt__{name}"), visible(&output));
    }
}

#[test]
fn formatting_is_idempotent_for_every_fixture_and_option_set() {
    for (name, input) in inputs() {
        for (label, options) in option_sets() {
            let once = format_str(&input, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
            let twice = format_str(&once, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(once, twice, "{name} with {label}");
        }
    }
}

#[test]
fn parsing_is_lossless_for_every_fixture() {
    for path in bib_files() {
        let text = read(&path);
        let cst = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(cst.to_source(), text, "{}", path.display());
    }
}

#[test]
fn formatted_output_parses_to_the_same_entries() {
    /// Entries as (type, key, sorted field names): the order of fields is
    /// not part of the shape, since `--sort-fields` may change it.
    fn shape(text: &str) -> Vec<(String, String, Vec<String>)> {
        let cst = parse(text).expect("parses");
        let mut shape: Vec<_> = cst
            .entries()
            .map(|entry| {
                let mut names: Vec<String> = entry
                    .fields
                    .iter()
                    .map(|field| cst.text(field.name).to_ascii_lowercase())
                    .collect();
                names.sort();
                (
                    cst.text(entry.kind).to_ascii_lowercase(),
                    cst.text(entry.key).to_owned(),
                    names,
                )
            })
            .collect();
        shape.sort();
        shape
    }
    for (name, input) in inputs() {
        for (label, options) in option_sets() {
            let output = format_str(&input, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(shape(&output), shape(&input), "{name} with {label}");
        }
    }
}
