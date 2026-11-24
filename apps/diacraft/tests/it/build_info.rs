//! The build script's work: which git release and which year a build says. `build_info.rs` is included here, so the code under test is the file the
//! three `build.rs` (this app's, the command line's and the web app's) compile. Nothing it does may fail a build, and git itself is not run: the functions take
//! what git said as text.

#[path = "../../build_info.rs"]
mod under_test;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use under_test::{
    LAST_RESORT_YEAR, MAX_GIT_CHARS, Sources, UNKNOWN, clean_git, clean_version, clean_year, directives, files_to_watch, gather, identify,
    is_this_checkout, resolve, year_of_commit_date, year_of_epoch,
};

fn sources() -> Sources {
    Sources::default()
}

#[test]
fn a_git_release_is_what_describe_prints_and_nothing_that_could_break_a_directive() {
    for good in ["v0.1.0-766-g106427ea", "v0.1.0-766-g106427ea-dirty", "106427ea", "v1.2.3", "my build 7"] {
        assert_eq!(clean_git(good).as_deref(), Some(good), "{good}");
    }
    assert_eq!(clean_git("  v0.1.0-1-gabc \n").as_deref(), Some("v0.1.0-1-gabc"), "describe ends in a newline");
    for bad in ["", "   ", "v1\nINJECTED", "v1\rx", "tab\there", "caf\u{e9}", "v1\u{1b}[2J", "\u{202e}v1", &"x".repeat(MAX_GIT_CHARS + 1)] {
        assert_eq!(clean_git(bad), None, "{bad:?}");
    }
    assert_eq!(clean_git(&"x".repeat(MAX_GIT_CHARS)).map(|s| s.len()), Some(MAX_GIT_CHARS), "the longest that is taken");
}

#[test]
fn a_year_is_four_digits_from_1970() {
    for (raw, year) in [("2026", "2026"), (" 2031\n", "2031"), ("1970", "1970"), ("9999", "9999")] {
        assert_eq!(clean_year(raw).as_deref(), Some(year), "{raw:?}");
    }
    for bad in ["", "26", "20266", "abcd", "1969", "0000", "-2026", "2026-11-22", "２０２６", "20 26"] {
        assert_eq!(clean_year(bad), None, "{bad:?}");
    }
}

#[test]
fn the_year_of_a_commit_is_that_of_its_date() {
    assert_eq!(year_of_commit_date("2026-11-22").as_deref(), Some("2026"));
    assert_eq!(year_of_commit_date("2027-01-01\n").as_deref(), Some("2027"), "`git log --format=%cs` ends in a newline");
    for bad in ["", "garbage", "12026-01-01", "26-11-22", "2026", "2026/11/22", "1969-12-31", "abcd-ef-gh"] {
        assert_eq!(year_of_commit_date(bad), None, "{bad:?}");
    }
}

#[test]
fn the_year_of_source_date_epoch_is_read_in_utc_by_jiff() {
    // 2027-01-01T00:00:00Z is 1798761600: the second before it is still 2026, wherever the build runs.
    for (secs, year) in [("0", "1970"), ("1700000000", "2023"), ("1798761599", "2026"), ("1798761600", "2027"), (" 1798761600\n", "2027")] {
        assert_eq!(year_of_epoch(secs).as_deref(), Some(year), "{secs:?}");
    }
    // Not a number, before 1970 and past the years jiff holds are no year: the next source answers.
    for bad in ["", "abc", "-1", "1.5", "99999999999999999999", "253402300800", "253402300799999"] {
        assert_eq!(year_of_epoch(bad), None, "{bad:?}");
    }
}

#[test]
fn a_tree_with_no_git_and_no_variables_is_unknown_and_the_last_resort_year() {
    assert_eq!(resolve(&sources()), (UNKNOWN.to_owned(), LAST_RESORT_YEAR.to_owned()));
    assert_eq!((UNKNOWN, LAST_RESORT_YEAR), ("unknown", "2026"));
}

#[test]
fn the_variables_win_over_git_and_git_over_nothing() {
    let git = Sources { describe: Some("v0.1.0-766-g106427ea\n".to_owned()), commit_date: Some("2026-11-22\n".to_owned()), ..sources() };
    assert_eq!(resolve(&git), ("v0.1.0-766-g106427ea".to_owned(), "2026".to_owned()));
    let with_override = Sources { env_git: Some("v9.9.9".to_owned()), env_year: Some("2031".to_owned()), ..git.clone() };
    assert_eq!(resolve(&with_override), ("v9.9.9".to_owned(), "2031".to_owned()), "DIACRAFT_BUILD_GIT and DIACRAFT_BUILD_YEAR win");
    let year_only = Sources { env_year: Some("2030".to_owned()), ..git.clone() };
    assert_eq!(resolve(&year_only), ("v0.1.0-766-g106427ea".to_owned(), "2030".to_owned()), "each variable overrides its own");
}

#[test]
fn a_variable_that_is_no_good_is_passed_over_and_never_fails_the_build() {
    let git = Sources { describe: Some("v0.1.0-1-gabc".to_owned()), commit_date: Some("2026-05-06".to_owned()), ..sources() };
    let junk = Sources { env_git: Some("a\nb".to_owned()), env_year: Some("last year".to_owned()), ..git };
    assert_eq!(resolve(&junk), ("v0.1.0-1-gabc".to_owned(), "2026".to_owned()), "junk falls through to git");
    let nothing_else = Sources { env_git: Some("".to_owned()), env_year: Some("x".to_owned()), ..sources() };
    assert_eq!(resolve(&nothing_else), (UNKNOWN.to_owned(), LAST_RESORT_YEAR.to_owned()));
}

#[test]
fn the_year_comes_from_the_commit_then_source_date_epoch_then_2026() {
    let commit = Sources { commit_date: Some("2029-03-04".to_owned()), epoch: Some("1700000000".to_owned()), ..sources() };
    assert_eq!(resolve(&commit).1, "2029", "the commit's date first");
    let epoch = Sources { commit_date: Some("not a date".to_owned()), epoch: Some("1700000000".to_owned()), ..sources() };
    assert_eq!(resolve(&epoch).1, "2023", "with no commit date, SOURCE_DATE_EPOCH");
    let neither = Sources { epoch: Some("soon".to_owned()), ..sources() };
    assert_eq!(resolve(&neither).1, "2026", "last of all, 2026");
    let variable_wins = Sources { env_year: Some("2040".to_owned()), ..commit };
    assert_eq!(resolve(&variable_wins).1, "2040");
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

#[test]
fn the_directives_name_the_two_texts_and_what_makes_cargo_build_again() {
    let lines = directives(&("v0.1.0-766-g106427ea".to_owned(), "2026".to_owned()), &paths(&["/repo/.git/HEAD", "/repo/.git/refs/heads/main"]));
    assert_eq!(
        lines,
        [
            "cargo::rustc-env=DIACRAFT_BUILD_GIT=v0.1.0-766-g106427ea",
            "cargo::rustc-env=DIACRAFT_BUILD_YEAR=2026",
            "cargo::rerun-if-env-changed=DIACRAFT_BUILD_GIT",
            "cargo::rerun-if-env-changed=DIACRAFT_BUILD_YEAR",
            "cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH",
            "cargo::rerun-if-changed=/repo/.git/HEAD",
            "cargo::rerun-if-changed=/repo/.git/refs/heads/main",
        ]
    );
    // With no git there is nothing to watch, and cargo is told only of the variables: a path that is not there would make it build every time.
    let no_git = directives(&(UNKNOWN.to_owned(), LAST_RESORT_YEAR.to_owned()), &[]);
    assert_eq!(no_git.len(), 5, "{no_git:?}");
    assert!(no_git.iter().all(|l| !l.starts_with("cargo::rerun-if-changed=")), "{no_git:?}");
    // A path with a newline in it would end the directive early and let the rest be read as another: it is left out.
    let odd = directives(&(UNKNOWN.to_owned(), LAST_RESORT_YEAR.to_owned()), &paths(&["/repo/.git/HEAD\ncargo::rustc-env=X=1", "/repo/.git/ok"]));
    assert!(odd.iter().all(|l| !l.contains('\n')), "{odd:?}");
    assert!(odd.contains(&"cargo::rerun-if-changed=/repo/.git/ok".to_owned()) && !odd.iter().any(|l| l.contains("X=1")), "{odd:?}");
}

/// A git that answers `rev-parse --git-path` from a table, and files that are read from another.
fn watch(table: &[(&str, &str)], files: &[(&str, &str)]) -> Vec<PathBuf> {
    let table: HashMap<String, PathBuf> = table.iter().map(|(name, path)| (name.to_string(), PathBuf::from(path))).collect();
    let files: HashMap<PathBuf, String> = files.iter().map(|(path, text)| (PathBuf::from(path), text.to_string())).collect();
    files_to_watch(&|name| table.get(name).cloned(), &|path: &Path| files.get(path).cloned())
}

#[test]
fn a_commit_is_watched_through_head_the_ref_it_names_and_packed_refs() {
    let table = [("HEAD", "/r/.git/HEAD"), ("refs/heads/main", "/r/.git/refs/heads/main"), ("packed-refs", "/r/.git/packed-refs")];
    let on_a_branch = watch(&table, &[("/r/.git/HEAD", "ref: refs/heads/main\n")]);
    assert_eq!(on_a_branch, paths(&["/r/.git/HEAD", "/r/.git/refs/heads/main", "/r/.git/packed-refs"]));
    // A detached HEAD is the commit itself: a new commit rewrites HEAD.
    let detached = watch(&table, &[("/r/.git/HEAD", "106427ea0123456789abcdef0123456789abcdef\n")]);
    assert_eq!(detached, paths(&["/r/.git/HEAD", "/r/.git/packed-refs"]));
    // A linked worktree has a HEAD of its own, and the branch's file is in the main checkout's folder.
    let linked = [("HEAD", "/r/.git/worktrees/w/HEAD"), ("refs/heads/task/x", "/r/.git/refs/heads/task/x"), ("packed-refs", "/r/.git/packed-refs")];
    assert_eq!(
        watch(&linked, &[("/r/.git/worktrees/w/HEAD", "ref: refs/heads/task/x\n")]),
        paths(&["/r/.git/worktrees/w/HEAD", "/r/.git/refs/heads/task/x", "/r/.git/packed-refs"])
    );
    // A HEAD that cannot be read, or git that is not there: nothing to watch.
    assert_eq!(watch(&table, &[]), paths(&["/r/.git/HEAD", "/r/.git/packed-refs"]), "HEAD cannot be read: it and packed-refs are still watched");
    assert_eq!(watch(&[], &[]), Vec::<PathBuf>::new(), "no git at all");
    // A ref name that tries to leave the folder is not followed.
    let hostile = watch(&table, &[("/r/.git/HEAD", "ref: ../../../etc/passwd\n")]);
    assert_eq!(hostile, paths(&["/r/.git/HEAD", "/r/.git/packed-refs"]));
}

/// Git is believed only for this checkout: a tree unpacked inside another repository is not described as that repository.
#[test]
fn git_is_believed_only_when_its_top_folder_is_this_workspace() {
    let manifest = Path::new("/work/diacraft/apps/diacraft");
    assert!(is_this_checkout(Path::new("/work/diacraft"), manifest), "the checkout, or a linked worktree of it, whose top folder is the workspace");
    assert!(!is_this_checkout(Path::new("/work"), manifest), "a repository that the tree lies in");
    assert!(!is_this_checkout(Path::new("/home/me"), manifest), "a home folder that is a repository");
    assert!(!is_this_checkout(Path::new("/work/diacraft/apps"), manifest));
    assert!(!is_this_checkout(Path::new("/work/diacraft"), Path::new("/")), "a folder with no workspace above it");
    assert!(!is_this_checkout(Path::new("/work/diacraft"), Path::new("diacraft")), "a relative manifest folder is no workspace");
}

/// The version of the workspace the tests below stand in for, and the build line each shape of `git describe` gives. The line is read next to "DiaCraft 0.2.0" in the
/// splash and the About panel, so a build made before the tag `v0.2.0` exists must not read as a 0.1 build, which is what `git describe` says of it (`v0.1.0-1555-gaab1664d`).
#[test]
fn a_build_line_says_how_the_build_stands_to_the_version_above_it() {
    let table = [
        // The tagged release: the plain version, the tag's `v` dropped.
        ("v0.2.0", "0.2.0"),
        ("0.2.0", "0.2.0"),
        ("v0.2.0\n", "0.2.0"),
        ("v0.2.0-dirty", "0.2.0+dirty"),
        // After the release's tag, and the version still the release's: how far, and which commit.
        ("v0.2.0-12-gaab1664d", "0.2.0+12.aab1664d"),
        ("v0.2.0-12-gaab1664d-dirty", "0.2.0+12.aab1664d.dirty"),
        // The version is newer than the last tag, which is how every build between two releases is made: a development build of the version, never the old tag's name.
        ("v0.1.0-1555-gaab1664d", "0.2.0-dev+aab1664d"),
        ("v0.1.0-1555-gaab1664d-dirty", "0.2.0-dev+aab1664d.dirty"),
        ("v0.1.0-1-gabc1234", "0.2.0-dev+abc1234"),
        ("v0.1.0-rc-1-3-gabc1234", "0.2.0-dev+abc1234"),
        ("v0.20.0-12-gaab1664d", "0.2.0-dev+aab1664d"),
        // No tag at all: git describe was asked for `--always` and says a commit.
        ("aab1664d", "0.2.0-dev+aab1664d"),
        ("aab1664d-dirty", "0.2.0-dev+aab1664d.dirty"),
        // A text that says nothing about the version is passed on as it stands: a rehearsal's name, another tag, words.
        ("dev-aab1664d", "dev-aab1664d"),
        ("nightly", "nightly"),
        ("v0.1.0", "v0.1.0"),
        ("my build 7", "my build 7"),
        ("unknown", "unknown"),
    ];
    for (describe, line) in table {
        assert_eq!(identify("0.2.0", describe), line, "{describe:?}");
    }
    // A version that has a pre-release already: the development build adds to it, and a tag of it is the plain version.
    assert_eq!(identify("0.3.0-rc.1", "v0.2.0-9-gabc1234"), "0.3.0-rc.1.dev+abc1234");
    assert_eq!(identify("0.3.0-rc.1", "v0.3.0-rc.1"), "0.3.0-rc.1");
    // No version to compare with: nothing is changed.
    assert_eq!(identify("", "v0.1.0-766-g106427ea"), "v0.1.0-766-g106427ea");
}

#[test]
fn a_version_is_what_cargo_says_and_nothing_that_could_break_a_directive() {
    for good in ["0.2.0", "1.20.300", "0.3.0-rc.1", " 0.2.0\n"] {
        assert_eq!(clean_version(good).as_deref(), Some(good.trim()), "{good:?}");
    }
    // Build metadata is no part of the version a build line is compared with.
    assert_eq!(clean_version("0.2.0+local").as_deref(), Some("0.2.0"));
    for bad in ["", "  ", "0.2", "x.y.z", "0.2.0\nINJECTED", "0.2.0 x", "v0.2.0", "0.2.0-\u{1b}", &format!("0.2.0-{}", "x".repeat(60))] {
        assert_eq!(clean_version(bad), None, "{bad:?}");
    }
}

#[test]
fn the_workspace_version_decides_what_a_describe_text_comes_to() {
    let before_the_tag = Sources {
        version: Some("0.2.0".to_owned()),
        describe: Some("v0.1.0-1555-gaab1664d\n".to_owned()),
        commit_date: Some("2026-11-23\n".to_owned()),
        ..sources()
    };
    assert_eq!(resolve(&before_the_tag), ("0.2.0-dev+aab1664d".to_owned(), "2026".to_owned()), "newer than the last tag: not a 0.1 build");
    let at_the_tag = Sources { describe: Some("v0.2.0\n".to_owned()), ..before_the_tag.clone() };
    assert_eq!(resolve(&at_the_tag).0, "0.2.0");
    // The release workflow names the tag in DIACRAFT_BUILD_GIT: the same line as a build of the tag with git.
    let named = Sources { env_git: Some("v0.2.0".to_owned()), describe: None, ..before_the_tag.clone() };
    assert_eq!(resolve(&named).0, "0.2.0");
    let rehearsal = Sources { env_git: Some("dev-aab1664d".to_owned()), ..before_the_tag.clone() };
    assert_eq!(resolve(&rehearsal).0, "dev-aab1664d", "a name that says nothing of the version is the variable's own");
    // With no version the texts are what they were.
    let no_version = Sources { version: None, ..before_the_tag.clone() };
    assert_eq!(resolve(&no_version).0, "v0.1.0-1555-gaab1664d");
    // A text that is no good is passed over for the next source, as before.
    let junk = Sources { env_git: Some("a\nb".to_owned()), ..before_the_tag };
    assert_eq!(resolve(&junk).0, "0.2.0-dev+aab1664d");
    // No git and no variable: unknown, whatever the version.
    let nothing = Sources { version: Some("0.2.0".to_owned()), ..sources() };
    assert_eq!(resolve(&nothing).0, UNKNOWN);
    // A line made too long by the version is not taken: the text that came is. (A version that is itself too long or no version is no version: the text as it came, too.)
    let long = Sources { version: Some(format!("0.2.0-{}", "x".repeat(34))), describe: Some("v0.1.0-1-gabc1234".to_owned()), ..sources() };
    assert_eq!(resolve(&long).0, "v0.1.0-1-gabc1234");
    let no_good = Sources { version: Some("soon".to_owned()), describe: Some("v0.1.0-1-gabc1234".to_owned()), ..sources() };
    assert_eq!(resolve(&no_good).0, "v0.1.0-1-gabc1234");
}

/// A repository of the test's own, in a folder of its own: `<root>/apps/diacraft` stands for the crate's folder, so that git is believed for it.
struct Repo {
    root: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Repo {
        let root = std::env::temp_dir().join(format!("diacraft-build-info-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // a folder left by a run that was cut off
        std::fs::create_dir_all(root.join("apps/diacraft")).unwrap();
        let repo = Repo { root: std::fs::canonicalize(&root).unwrap() };
        repo.git(&["init", "-q"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        // The identity, the signing and the config are the test's own, so nothing of the machine's changes what is made.
        let out = std::process::Command::new("git")
            .args(["-C", self.root.to_str().unwrap()])
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .args(["-c", "init.defaultBranch=main", "-c", "core.hooksPath=/dev/null"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn commit(&self, text: &str) {
        std::fs::write(self.root.join("apps/diacraft/file.txt"), text).unwrap();
        self.git(&["add", "apps/diacraft/file.txt"]);
        self.git(&["commit", "-q", "-m", text]);
    }

    /// What the build script would make of the crate's folder for a workspace of `version`.
    fn line(&self, version: &str) -> String {
        let dir = self.root.join("apps/diacraft");
        let (mut found, _) = gather(&dir, &|_| None);
        found.version = clean_version(version);
        resolve(&found).0
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root); // a folder of the test's own, in the temporary folder
    }
}

/// The two cases of against real git, in a repository made for the test (the tags are made there and nowhere else): a version newer than the last tag, and the tag of the
/// version itself.
#[test]
fn a_build_before_the_tag_of_its_version_is_a_dev_build_and_the_tag_itself_is_the_version() {
    let repo = Repo::new("tags");
    assert!(
        repo.root.starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()),
        "{:?}: the test's repository is in the temporary folder",
        repo.root
    );
    repo.commit("one");
    repo.git(&["tag", "-a", "v0.1.0", "-m", "DiaCraft 0.1.0"]);
    // One commit after the old tag, and the workspace says 0.2.0: not the release.
    repo.commit("two");
    let short = repo.git(&["rev-parse", "--short", "HEAD"]);
    assert!((7..=12).contains(&short.len()), "{short}");
    assert_eq!(repo.line("0.2.0"), format!("0.2.0-dev+{short}"), "version 0.2.0 after the tag v0.1.0");
    assert_eq!(repo.line("0.1.0"), format!("0.1.0+1.{short}"), "the old version after its own tag: one commit on");
    // An edit that is not committed.
    std::fs::write(repo.root.join("apps/diacraft/file.txt"), "edited").unwrap();
    assert_eq!(repo.line("0.2.0"), format!("0.2.0-dev+{short}.dirty"));
    repo.git(&["checkout", "-q", "--", "apps/diacraft/file.txt"]);
    // The tag of the release itself, on that commit.
    repo.git(&["tag", "-a", "v0.2.0", "-m", "DiaCraft 0.2.0"]);
    assert_eq!(repo.line("0.2.0"), "0.2.0", "a build of the tag v0.2.0 says plain 0.2.0");
    // And one commit later, before the version is raised.
    repo.commit("three");
    let later = repo.git(&["rev-parse", "--short", "HEAD"]);
    assert_eq!(repo.line("0.2.0"), format!("0.2.0+1.{later}"));
    // The version raised for the next release: a dev build of it again.
    assert_eq!(repo.line("0.3.0"), format!("0.3.0-dev+{later}"));
}

/// A folder that is no repository has nothing to say.
#[test]
fn a_folder_that_is_no_repository_says_nothing() {
    let root = std::env::temp_dir().join(format!("diacraft-build-info-none-{}", std::process::id()));
    std::fs::create_dir_all(root.join("apps/diacraft")).unwrap();
    let (found, watched) = gather(&root.join("apps/diacraft"), &|_| None);
    assert_eq!((found.describe, found.commit_date, watched), (None, None, Vec::new()));
    let _ = std::fs::remove_dir_all(&root);
}
