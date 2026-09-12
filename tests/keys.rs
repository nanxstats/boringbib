//! Key generation tests: the Appendix B corpus, the Appendix A mapping, and
//! the guarantee that `keys` changes nothing but keys and reference fields.

use std::fs;
use std::path::{Path, PathBuf};

use boringbib::cst::Cst;
use boringbib::keys::{self, KeysOptions, Plan, REFERENCE_FIELDS_LIST, REFERENCE_FIELDS_SINGLE};
use boringbib::sort::SortKey;
use boringbib::{FmtOptions, format_str, parse};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn plan_for(text: &str, options: &KeysOptions) -> (Cst, Plan) {
    let cst = parse(text).expect("parses");
    let plan = keys::plan(&cst, options);
    (cst, plan)
}

/// The input fixtures (not the golden outputs), sorted by name.
fn inputs() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixtures_dir())
        .expect("fixtures directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            let name = path.to_string_lossy();
            name.ends_with(".bib") && !name.ends_with(".expected.bib")
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_appendix_b_row_passes() {
    let tsv = read(&fixtures_dir().join("scholar_keys.tsv"));
    let mut rows = 0;
    let mut failures = Vec::new();
    for line in tsv.lines().skip(1) {
        let columns: Vec<&str> = line.split('\t').collect();
        let [author, year, title, expected, verified] = columns[..] else {
            panic!("malformed row: {line}");
        };
        rows += 1;
        let bib = format!(
            "@misc{{k,\n  author = {{{author}}},\n  year = {{{year}}},\n  title = {{{title}}}\n}}\n"
        );
        let (_, plan) = plan_for(&bib, &KeysOptions::default());
        let got = plan
            .renames
            .first()
            .map_or("<unchanged>", |rename| rename.new.as_str());
        if got != expected {
            failures.push(format!(
                "{author} | {year} | {title}: expected {expected}, got {got} (verified: {verified})"
            ));
        }
    }
    assert!(rows >= 49, "the corpus has {rows} rows");
    assert!(
        failures.is_empty(),
        "{} of {rows} rows failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn appendix_a_mapping_and_crossref_update() {
    let input = read(&fixtures_dir().join("messy.bib"));
    let expected_map = read(&fixtures_dir().join("messy.keys.expected.tsv"));
    let (cst, plan) = plan_for(&input, &KeysOptions::default());
    let got: Vec<String> = plan
        .renames
        .iter()
        .map(|rename| format!("{}\t{}", rename.old, rename.new))
        .collect();
    let want: Vec<&str> = expected_map.lines().collect();
    assert_eq!(got, want);
    assert!(plan.reports.is_empty(), "{:?}", plan.reports);

    let output = keys::apply(&cst, &plan);
    let after = parse(&output).expect("output parses");
    let openai = after
        .entries()
        .find(|entry| after.text(entry.key) == "openai2023gpt")
        .expect("renamed entry");
    let crossref = openai.field(&after, "crossref").expect("crossref");
    assert_eq!(
        after.text(crossref.value.parts[0].inner()),
        "klambauer2017self"
    );
    assert_eq!(
        output,
        read(&fixtures_dir().join("messy.keys.expected.bib"))
    );
}

/// Blanks out every entry key and every plain reference value, so that two
/// texts can be compared for "nothing else changed".
fn skeleton(text: &str) -> String {
    let cst = parse(text).expect("parses");
    let mut edits = Vec::new();
    for entry in cst.entries() {
        edits.push((entry.key, "<KEY>".to_owned()));
        for field in &entry.fields {
            let name = cst.text(field.name).to_ascii_lowercase();
            let is_reference = REFERENCE_FIELDS_SINGLE.contains(&name.as_str())
                || REFERENCE_FIELDS_LIST.contains(&name.as_str());
            if let ([part], true) = (field.value.parts.as_slice(), is_reference) {
                if part.is_string() {
                    edits.push((part.inner(), "<REF>".to_owned()));
                }
            }
        }
    }
    cst.with_replacements(&edits)
}

#[test]
fn write_changes_nothing_but_keys_and_reference_fields() {
    for path in inputs() {
        let input = read(&path);
        let (cst, plan) = plan_for(&input, &KeysOptions::default());
        let output = keys::apply(&cst, &plan);
        assert_eq!(skeleton(&output), skeleton(&input), "{}", path.display());
        let after = parse(&output).expect("output parses");
        for rename in &plan.renames {
            assert!(
                after
                    .entries()
                    .any(|entry| after.text(entry.key) == rename.new),
                "{}: {} is missing",
                path.display(),
                rename.new
            );
        }
    }
}

#[test]
fn keys_is_idempotent_for_every_fixture() {
    for path in inputs() {
        let input = read(&path);
        let (cst, plan) = plan_for(&input, &KeysOptions::default());
        let once = keys::apply(&cst, &plan);
        let (_, again) = plan_for(&once, &KeysOptions::default());
        assert!(
            again.renames.is_empty(),
            "{}: {:?}",
            path.display(),
            again.renames
        );
        assert!(again.edits.is_empty(), "{}", path.display());
    }
}

#[test]
fn keys_then_fmt_equals_fmt_then_keys() {
    let input = read(&fixtures_dir().join("messy.bib"));
    let options = FmtOptions::default();
    let (cst, plan) = plan_for(&input, &KeysOptions::default());
    let keys_then_fmt = format_str(&keys::apply(&cst, &plan), &options).expect("parses");
    let formatted = format_str(&input, &options).expect("parses");
    let (cst, plan) = plan_for(&formatted, &KeysOptions::default());
    let fmt_then_keys = format_str(&keys::apply(&cst, &plan), &options).expect("parses");
    assert_eq!(keys_then_fmt, fmt_then_keys);
}

#[test]
fn sort_by_author_uses_the_key_generator() {
    let input = "@misc{b, author = {Zola, Émile}, year = {2000}, title = {G}}\n\
        @misc{a, author = {van der Maaten, Laurens}, year = {2008}, title = {V}}\n\
        @misc{c, author = {Aardvark, A}, year = {1999}, title = {A}}\n\
        @misc{d, title = {No author}}\n\
        @misc{e, author = {Aardvark, B}, year = {1998}, title = {A}}\n";
    let options = FmtOptions {
        sort: SortKey::Author,
        ..FmtOptions::default()
    };
    let output = format_str(input, &options).expect("parses");
    let keys: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with('@'))
        .map(|line| line.trim_start_matches("@misc{").trim_end_matches(','))
        .collect();
    assert_eq!(keys, ["e", "c", "a", "b", "d"]);
}
