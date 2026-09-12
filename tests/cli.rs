//! End-to-end tests of the `boringbib` binary.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

/// A scratch directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("boringbib-cli-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn entries(&self) -> usize {
        fs::read_dir(&self.0).expect("read temp dir").count()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn messy() -> String {
    fixture("messy.bib")
}

fn expected() -> String {
    fixture("messy.expected.bib")
}

fn boringbib() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boringbib"))
}

#[test]
fn stdin_to_stdout() {
    boringbib()
        .args(["fmt", "-"])
        .write_stdin(messy())
        .assert()
        .success()
        .stdout(expected())
        .stderr("");
    boringbib()
        .arg("fmt")
        .write_stdin(messy())
        .assert()
        .success()
        .stdout(expected());
}

#[test]
fn check_reports_files_that_would_change_and_writes_nothing() {
    let dir = TempDir::new("check");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .args(["fmt", "--check"])
        .arg(&path)
        .assert()
        .code(1)
        .stdout(
            predicate::str::starts_with("would reformat ")
                .and(predicate::str::contains("refs.bib")),
        );
    assert_eq!(fs::read_to_string(&path).expect("read"), messy());

    fs::write(&path, expected()).expect("write");
    boringbib()
        .args(["fmt", "--check"])
        .arg(&path)
        .assert()
        .code(0)
        .stdout("");

    boringbib()
        .args(["fmt", "--check", "-"])
        .write_stdin(messy())
        .assert()
        .code(1)
        .stdout("would reformat <stdin>\n");
}

#[test]
fn diff_prints_a_unified_diff_and_writes_nothing() {
    let dir = TempDir::new("diff");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .args(["fmt", "--diff"])
        .arg(&path)
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("--- a/")
                .and(predicate::str::contains("+++ b/"))
                .and(predicate::str::contains("-@Article{Vaswani2017-xy,"))
                .and(predicate::str::contains("+@article{feifei2006,")),
        );
    assert_eq!(fs::read_to_string(&path).expect("read"), messy());

    fs::write(&path, expected()).expect("write");
    boringbib()
        .args(["fmt", "--diff"])
        .arg(&path)
        .assert()
        .code(0)
        .stdout("");
}

#[test]
fn in_place_write_produces_the_golden_output() {
    let dir = TempDir::new("write");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .arg("fmt")
        .arg(&path)
        .assert()
        .success()
        .stdout("")
        .stderr("");
    assert_eq!(fs::read_to_string(&path).expect("read"), expected());
    boringbib().arg("fmt").arg(&path).assert().success();
    assert_eq!(fs::read_to_string(&path).expect("read"), expected());
    assert_eq!(dir.entries(), 1, "no temporary files are left behind");
}

#[test]
fn parse_error_leaves_the_file_untouched() {
    let dir = TempDir::new("error");
    let path = dir.file("broken.bib");
    let broken = "@misc{k,\n  title = {unclosed\n";
    fs::write(&path, broken).expect("write");
    boringbib()
        .arg("fmt")
        .arg(&path)
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "broken.bib:2:11: unbalanced braces: this `{` is never closed",
        ));
    assert_eq!(fs::read_to_string(&path).expect("read"), broken);
    assert_eq!(dir.entries(), 1);
}

#[test]
fn several_files_are_processed_independently() {
    let dir = TempDir::new("several");
    let good = dir.file("good.bib");
    let bad = dir.file("bad.bib");
    fs::write(&good, messy()).expect("write");
    fs::write(&bad, "@misc{k, title = \"oops\n").expect("write");
    boringbib()
        .arg("fmt")
        .arg(&bad)
        .arg(&good)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "bad.bib:1:18: unterminated quoted string",
        ));
    assert_eq!(fs::read_to_string(&good).expect("read"), expected());
    assert_eq!(
        fs::read_to_string(&bad).expect("read"),
        "@misc{k, title = \"oops\n"
    );
}

#[test]
fn crlf_is_preserved_and_can_be_forced() {
    let dir = TempDir::new("crlf");
    let path = dir.file("crlf.bib");
    fs::write(&path, "@misc{k,\r\n  title = {a\r\n   b}\r\n}\r\n").expect("write");
    boringbib().arg("fmt").arg(&path).assert().success();
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        "@misc{k,\r\n  title = {a b}\r\n}\r\n"
    );
    boringbib()
        .args(["fmt", "--line-ending", "lf"])
        .arg(&path)
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        "@misc{k,\n  title = {a b}\n}\n"
    );
    boringbib()
        .args(["fmt", "--line-ending", "crlf"])
        .arg(&path)
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        "@misc{k,\r\n  title = {a b}\r\n}\r\n"
    );
}

#[test]
fn bom_is_dropped_unless_kept() {
    let input = "\u{FEFF}@misc{k}\n";
    boringbib()
        .args(["fmt", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k\n}\n");
    boringbib()
        .args(["fmt", "--keep-bom", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("\u{FEFF}@misc{k\n}\n");
}

#[test]
fn warnings_go_to_stderr_and_do_not_fail() {
    boringbib()
        .args(["fmt", "-"])
        .write_stdin("@misc{k, title = {a}, title = {b}}\n")
        .assert()
        .success()
        .stdout("@misc{k,\n  title = {a},\n  title = {b}\n}\n")
        .stderr(
            "<stdin>:1:23: warning: duplicate field `title` in entry `k` (BibTeX uses the first)\n",
        );
}

#[test]
fn invalid_utf8_is_an_error() {
    let dir = TempDir::new("utf8");
    let path = dir.file("latin1.bib");
    fs::write(&path, b"@misc{k, title = {caf\xe9}}\n").expect("write");
    boringbib()
        .arg("fmt")
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("latin1.bib: not valid UTF-8"));
}

#[test]
fn config_file_is_discovered_and_overridden_by_flags() {
    let dir = TempDir::new("config");
    fs::write(
        dir.file("boringbib.toml"),
        "[fmt]\nindent = 4\ntrailing_comma = true\n",
    )
    .expect("write");
    let input = "@misc{k, title = {a}}\n";
    boringbib()
        .current_dir(&dir.0)
        .args(["fmt", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k,\n    title = {a},\n}\n");
    boringbib()
        .current_dir(&dir.0)
        .args(["fmt", "--indent", "2", "--no-trailing-comma", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k,\n  title = {a}\n}\n");

    fs::write(dir.file("other.toml"), "[fmt]\nindent = \"tab\"\n").expect("write");
    boringbib()
        .arg("--config")
        .arg(dir.file("other.toml"))
        .args(["fmt", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k,\n\ttitle = {a}\n}\n");

    fs::write(dir.file("bad.toml"), "[fmt]\nalign-equal = true\n").expect("write");
    boringbib()
        .arg("--config")
        .arg(dir.file("bad.toml"))
        .args(["fmt", "-"])
        .write_stdin(input)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("bad.toml").and(predicate::str::contains("align-equal")));
}

#[cfg(unix)]
#[test]
fn permissions_survive_an_in_place_write() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("perms");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
    boringbib().arg("fmt").arg(&path).assert().success();
    assert_eq!(
        fs::metadata(&path).expect("stat").permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), expected());
}

#[cfg(unix)]
#[test]
fn symlinks_are_followed() {
    let dir = TempDir::new("symlink");
    let target = dir.file("real.bib");
    let link = dir.file("link.bib");
    fs::write(&target, messy()).expect("write");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    boringbib().arg("fmt").arg(&link).assert().success();
    assert!(
        fs::symlink_metadata(&link)
            .expect("lstat")
            .file_type()
            .is_symlink(),
        "the link survives"
    );
    assert_eq!(fs::read_to_string(&target).expect("read"), expected());
}

fn expected_keys_map() -> String {
    fixture("messy.keys.expected.tsv")
}

/// The mapping as `keys` prints it: old keys padded to the longest.
fn expected_keys_stdout() -> String {
    expected_keys_map()
        .lines()
        .map(|line| {
            let (old, new) = line.split_once('\t').expect("two columns");
            format!("{old:<15}  {new}\n")
        })
        .collect()
}

#[test]
fn keys_prints_the_mapping_and_changes_nothing() {
    let dir = TempDir::new("keys-print");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .arg("keys")
        .arg(&path)
        .assert()
        .success()
        .stdout(expected_keys_stdout())
        .stderr("");
    assert_eq!(fs::read_to_string(&path).expect("read"), messy());
}

#[test]
fn keys_write_is_atomic_and_idempotent() {
    let dir = TempDir::new("keys-write");
    let path = dir.file("refs.bib");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .args(["keys", "--write"])
        .arg(&path)
        .assert()
        .success()
        .stdout(expected_keys_stdout());
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        fixture("messy.keys.expected.bib")
    );
    boringbib()
        .args(["keys", "--write"])
        .arg(&path)
        .assert()
        .success()
        .stdout("");
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        fixture("messy.keys.expected.bib")
    );
    assert_eq!(dir.entries(), 1, "no temporary files are left behind");
}

#[test]
fn keys_map_file_has_three_columns() {
    let dir = TempDir::new("keys-map");
    let path = dir.file("refs.bib");
    let map = dir.file("keys.tsv");
    fs::write(&path, messy()).expect("write");
    boringbib()
        .args(["keys", "--write", "--map"])
        .arg(&map)
        .arg(&path)
        .assert()
        .success();
    let expected: String = expected_keys_map()
        .lines()
        .map(|line| format!("{line}\t{}\n", path.display()))
        .collect();
    assert_eq!(fs::read_to_string(&map).expect("read map"), expected);
}

#[test]
fn keys_only_and_keep() {
    let dir = TempDir::new("keys-only");
    let path = dir.file("refs.bib");
    let input = "@misc{a, author = {Doe, Jane}, year = {2020}, title = {Same}}\n\
        @misc{doe2020same, author = {Roe, Richard}, year = {2020}, title = {Other}}\n";
    fs::write(&path, input).expect("write");
    boringbib()
        .args(["keys", "--only", "a"])
        .arg(&path)
        .assert()
        .success()
        .stdout("a  doe2020samea\n");
    fs::write(dir.file("boringbib.toml"), "[keys]\nkeep = [\"a\"]\n").expect("write");
    boringbib()
        .current_dir(&dir.0)
        .args(["keys", "refs.bib"])
        .assert()
        .success()
        .stdout("doe2020same  roe2020other\n");
}

#[test]
fn keys_parse_error_changes_nothing() {
    let dir = TempDir::new("keys-error");
    let path = dir.file("broken.bib");
    let broken = "@misc{k, author = {Doe, Jane}, title = {unclosed\n";
    fs::write(&path, broken).expect("write");
    boringbib()
        .args(["keys", "--write"])
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "broken.bib:1:40: unbalanced braces",
        ));
    assert_eq!(fs::read_to_string(&path).expect("read"), broken);
}

#[test]
fn keys_from_stdin_writes_to_stdout() {
    boringbib()
        .args(["keys", "--write", "-"])
        .write_stdin(messy())
        .assert()
        .success()
        .stdout(fixture("messy.keys.expected.bib"));
    boringbib()
        .args(["keys", "-"])
        .write_stdin(messy())
        .assert()
        .success()
        .stdout(expected_keys_stdout());
}

#[test]
fn keys_reports_go_to_stderr() {
    boringbib()
        .args(["keys", "-"])
        .write_stdin(
            "@misc{k, title = {No author}}\n@misc{j, author = {Doe, Jane}, title = {No year}}\n",
        )
        .assert()
        .success()
        .stdout("j  doeno\n")
        .stderr(
            "<stdin>:1:1: warning: entry `k` left unchanged: no author or editor\n\
             <stdin>:2:1: warning: entry `j` has no four-digit year; its key gets no year part\n",
        );
}

#[test]
fn keys_with_several_files_names_the_file() {
    let dir = TempDir::new("keys-several");
    let a = dir.file("a.bib");
    let b = dir.file("b.bib");
    fs::write(
        &a,
        "@misc{x, author = {Doe, Jane}, year = {2020}, title = {A}}\n",
    )
    .expect("write");
    fs::write(
        &b,
        "@misc{y, author = {Roe, Richard}, year = {2021}, title = {B}}\n",
    )
    .expect("write");
    boringbib()
        .arg("keys")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(format!(
            "x  doe2020a  {}\ny  roe2021b  {}\n",
            a.display(),
            b.display()
        ));
}

#[test]
fn fmt_sorts_by_author() {
    boringbib()
        .args(["fmt", "--sort", "author", "-"])
        .write_stdin("@misc{b, author = {Zola, Émile}}\n@misc{a, author = {van der Maaten, Laurens}}\n")
        .assert()
        .success()
        .stdout("@misc{a,\n  author = {van der Maaten, Laurens}\n}\n\n@misc{b,\n  author = {Zola, Émile}\n}\n");
}

#[test]
fn wrap_and_sort_fields_from_config_and_flags() {
    let dir = TempDir::new("wrap-config");
    fs::write(
        dir.file("boringbib.toml"),
        "[fmt]\nwrap = 30\nsort_fields = [\"year\"]\n",
    )
    .expect("write");
    let input = "@misc{k, title = {one two three four five six}, year = {2020}}\n";
    boringbib()
        .current_dir(&dir.0)
        .args(["fmt", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(
            "@misc{k,\n  year  = {2020},\n  title = {one two three four\n           five six}\n}\n",
        );
    boringbib()
        .current_dir(&dir.0)
        .args(["fmt", "--no-wrap", "--no-sort-fields", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k,\n  title = {one two three four five six},\n  year  = {2020}\n}\n");
    boringbib()
        .current_dir(&dir.0)
        .args(["fmt", "--wrap", "0", "--sort-fields", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout("@misc{k,\n  title = {one two three four five six},\n  year  = {2020}\n}\n");
}
