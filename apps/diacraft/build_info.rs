//! The work of the build scripts that say which build this is (the build line: the version, or a development build of it, or the git release) and which year it was made in.
//!
//! Three `build.rs` take this file with `#[path]`: this app's, the command line's (`apps/diacraft-cli`) and the web app's (`apps/diacraft-web`), so that the
//! window's splash and About panel, `diacraft-cli`'s `app.version` and the browser's page all read one text. The tests of this app include it too
//! (`tests/it/build_info.rs`), so the code they run is the code the build scripts compile.
//!
//! **Where the texts come from.** The build line (the "git release" of the earlier name, still the name of the field) is made from the output of `git describe --tags --always --dirty`
//! (`v0.1.0-766-g106427ea`) and the workspace version, or from the variable `DIACRAFT_BUILD_GIT` for a build from a tree with no `.git` (the public export), and is `unknown` when there is
//! neither. **It is read under the version** ("DiaCraft 0.2.0" in the splash and the About panel), so it follows it ([`identify`]): a text that names the tag of the version is the
//! plain version (`v0.2.0` is `0.2.0`, and `v0.2.0-12-gaab1664d` is `0.2.0+12.aab1664d`), one that names an older tag or none is a development build of the version (`0.2.0-dev+aab1664d`, where
//! `git describe` says `v0.1.0-1555-gaab1664d` for every build between two releases), `.dirty` comes last for an edit that is not committed, and any other text (`dev-<commit>`, a tag of another
//! name) is the line as it stands. The year is that of the commit's date
//! (`git log -1 --format=%cs`), or `DIACRAFT_BUILD_YEAR`, then the year of `SOURCE_DATE_EPOCH` (the convention of reproducible builds), and last of all
//! [`LAST_RESORT_YEAR`]. **The clock is never read**, so a build of one commit is the same build every time. A variable overrides its own text, and one that is no good (not
//! four digits, a newline in it) is passed over for the next source: **nothing here fails a build**, and git being absent is an ordinary case. **Git is believed only for this
//! checkout**: a tree unpacked inside some other repository (a tarball in a home folder that is itself a repository) would be described as that repository, so git is asked
//! where its top folder is, and the answer counts only if it is the workspace this crate is in ([`is_this_checkout`]).
//!
//! **When cargo builds again.** The crate that takes the texts is rebuilt when the commit changes and not otherwise: cargo is told to watch `HEAD`, the branch file `HEAD` names
//! and `packed-refs`, and the three variables. (`--dirty` is the state of the tree when the script last ran: an edit that is not committed does not run it again, as
//! an edit would otherwise rebuild the crate every time.) A path that is not there is never named, because cargo builds again at every run for a file it cannot find.

use std::path::{Path, PathBuf};

/// What the git release reads when nothing says.
pub const UNKNOWN: &str = "unknown";

/// The year of last resort.
pub const LAST_RESORT_YEAR: &str = "2026";

/// The longest build line that is taken: `v0.1.0-766-g106427ea-dirty` is 26 characters, and the splash has room for about 40.
pub const MAX_GIT_CHARS: usize = 48;

/// The longest version that is taken. A longer one is no version of ours, and a line made from it would not fit [`MAX_GIT_CHARS`].
pub const MAX_VERSION_CHARS: usize = 40;

/// The first year that is taken: nothing was built before it.
const FIRST_YEAR: u16 = 1970;

/// The variables and what git said, as text. [`resolve`] chooses among them.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Sources {
    /// `CARGO_PKG_VERSION` of the crate the script builds, which is the workspace version (`cargo xtask version`). `None` says no version, and the texts below are then taken as they stand.
    pub version: Option<String>,
    /// `DIACRAFT_BUILD_GIT`.
    pub env_git: Option<String>,
    /// `DIACRAFT_BUILD_YEAR`.
    pub env_year: Option<String>,
    /// `git describe --tags --always --dirty`.
    pub describe: Option<String>,
    /// `git log -1 --format=%cs`: `2026-11-22`.
    pub commit_date: Option<String>,
    /// `SOURCE_DATE_EPOCH`: seconds since 1970.
    pub epoch: Option<String>,
}

/// A git release fit to go in a cargo directive and a window: trimmed, of printable ASCII (a newline would end the directive and let the rest be read as another;
/// a control or direction character would reach a person's screen), and at most [`MAX_GIT_CHARS`] long. `None` for anything else.
pub fn clean_git(raw: &str) -> Option<String> {
    let text = raw.trim();
    let fits = !text.is_empty() && text.len() <= MAX_GIT_CHARS && text.chars().all(|c| (' '..='~').contains(&c));
    fits.then(|| text.to_owned())
}

/// A version fit to be compared with a tag and put in a line: `MAJOR.MINOR.PATCH` and an optional `-pre.release`, as `cargo xtask version` writes it, trimmed, with the build metadata
/// (`+local`) left off, of at most [`MAX_VERSION_CHARS`] and of the characters a version has. `None` for anything else.
pub fn clean_version(raw: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.split_once('+').map_or(text, |(version, _)| version);
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let numbers = core.split('.').count() == 3 && core.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let pre_ok = pre.is_none_or(|pre| !pre.is_empty() && pre.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'));
    (numbers && pre_ok && text.len() <= MAX_VERSION_CHARS).then(|| text.to_owned())
}

/// What `git describe --tags --always --dirty` said, taken apart: `v0.1.0-1555-gaab1664d-dirty` is the tag `v0.1.0`, `1555` commits on, the commit `aab1664d`, with edits that are not
/// committed. A text of the commit alone (`--always`, with no tag in the history) has no tag, and one of a tag alone (the tag's own commit) has no commit.
struct Described<'a> {
    tag: Option<&'a str>,
    ahead: Option<&'a str>,
    sha: Option<&'a str>,
    dirty: bool,
}

/// Whether `text` is an abbreviated commit name: lowercase hexadecimal digits, 7 to 40 of them (git abbreviates to 7 at the least).
fn is_sha(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `text` as `git describe` writes it. A text that is none of its shapes is taken for the name of a tag.
fn parse_describe(text: &str) -> Described<'_> {
    let (text, dirty) = match text.strip_suffix("-dirty") {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    if is_sha(text) {
        return Described { tag: None, ahead: None, sha: Some(text), dirty };
    }
    if let Some((rest, sha)) = text.rsplit_once("-g")
        && is_sha(sha)
        && let Some((tag, ahead)) = rest.rsplit_once('-')
        && !tag.is_empty()
        && !ahead.is_empty()
        && ahead.len() <= 9
        && ahead.bytes().all(|b| b.is_ascii_digit())
    {
        return Described { tag: Some(tag), ahead: Some(ahead), sha: Some(sha), dirty };
    }
    Described { tag: Some(text), ahead: None, sha: None, dirty }
}

/// The build line for a build of the workspace at `version` that `git describe` (or `DIACRAFT_BUILD_GIT`) called `describe`. The line is read under "DiaCraft <version>", and
/// `git describe` names a build between two releases after the last one (`v0.1.0-1555-gaab1664d`, which reads as a 0.1 build), so the line is made to say how the build stands to the version:
///
/// * the tag of the version itself (`v0.2.0`, with or without the `v`): the plain version, `0.2.0`;
/// * commits after that tag (`v0.2.0-12-gaab1664d`): `0.2.0+12.aab1664d`;
/// * any other tag, or none (`v0.1.0-1555-gaab1664d`, `aab1664d`): a development build of the version, `0.2.0-dev+aab1664d` (`.dev` where the version has a pre-release already);
/// * `.dirty` last when the tree has edits that are not committed (`0.2.0+dirty` for the tag itself);
/// * any other text, such as `dev-<commit>` or the name of another tag, as it stands.
///
/// With no version the text is returned as it stands.
pub fn identify(version: &str, describe: &str) -> String {
    let (version, text) = (version.trim(), describe.trim());
    if version.is_empty() {
        return text.to_owned();
    }
    let Described { tag, ahead, sha, dirty } = parse_describe(text);
    let dirty_tail = if dirty { ".dirty" } else { "" };
    let names_the_version = |tag: &str| tag.strip_prefix('v').unwrap_or(tag) == version;
    match (tag, ahead, sha) {
        (Some(tag), None, None) if names_the_version(tag) => format!("{version}{}", if dirty { "+dirty" } else { "" }),
        (Some(tag), Some(ahead), Some(sha)) if names_the_version(tag) => format!("{version}+{ahead}.{sha}{dirty_tail}"),
        (_, _, Some(sha)) => format!("{version}{}+{sha}{dirty_tail}", if version.contains('-') { ".dev" } else { "-dev" }),
        _ => text.to_owned(),
    }
}

/// A year: four ASCII digits, not before 1970.
pub fn clean_year(raw: &str) -> Option<String> {
    let text = raw.trim();
    let four_digits = text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit());
    (four_digits && text.parse::<u16>().is_ok_and(|year| year >= FIRST_YEAR)).then(|| text.to_owned())
}

/// The year of a date `git log --format=%cs` printed (`2026-11-22`).
pub fn year_of_commit_date(raw: &str) -> Option<String> {
    let (year, rest) = raw.trim().split_at_checked(4)?;
    rest.starts_with('-').then(|| clean_year(year)).flatten()
}

/// The year, in UTC, of `SOURCE_DATE_EPOCH` seconds. The calendar is jiff's; a number that is no number, a time before 1970 and one past the years jiff holds are no year.
pub fn year_of_epoch(raw: &str) -> Option<String> {
    let secs: i64 = raw.trim().parse().ok()?;
    let year = jiff::tz::Offset::UTC.to_datetime(jiff::Timestamp::from_second(secs).ok()?).year();
    clean_year(&format!("{year:04}"))
}

/// The build line and the year the build says, from what there is: the variable, then git, for each text, and the last resort when there is none. The line is made to follow the version
/// ([`identify`]) when there is one, whichever of the variable and git it comes from; a line that the version would make too long is not taken, and the text that came is.
pub fn resolve(sources: &Sources) -> (String, String) {
    let version = sources.version.as_deref().and_then(clean_version);
    let line = |raw: &str| {
        let text = clean_git(raw)?;
        let said = version.as_deref().map_or_else(|| text.clone(), |version| identify(version, &text));
        Some(clean_git(&said).unwrap_or(text))
    };
    let git = sources.env_git.as_deref().and_then(line).or_else(|| sources.describe.as_deref().and_then(line)).unwrap_or_else(|| UNKNOWN.to_owned());
    let year = sources
        .env_year
        .as_deref()
        .and_then(clean_year)
        .or_else(|| sources.commit_date.as_deref().and_then(year_of_commit_date))
        .or_else(|| sources.epoch.as_deref().and_then(year_of_epoch))
        .unwrap_or_else(|| LAST_RESORT_YEAR.to_owned());
    (git, year)
}

/// The lines a build script prints for cargo: the two texts for the crate (`env!("DIACRAFT_BUILD_GIT")`, `env!("DIACRAFT_BUILD_YEAR")`), the variables that make
/// it run again, and the `watched` files that make it run again when they change. The texts have passed [`resolve`], so they hold no newline; a watched path that
/// has one is left out for the same reason.
pub fn directives(resolved: &(String, String), watched: &[PathBuf]) -> Vec<String> {
    let mut lines = vec![
        format!("cargo::rustc-env=DIACRAFT_BUILD_GIT={}", resolved.0),
        format!("cargo::rustc-env=DIACRAFT_BUILD_YEAR={}", resolved.1),
        "cargo::rerun-if-env-changed=DIACRAFT_BUILD_GIT".to_owned(),
        "cargo::rerun-if-env-changed=DIACRAFT_BUILD_YEAR".to_owned(),
        "cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH".to_owned(),
    ];
    for path in watched {
        let text = path.display().to_string();
        if !text.chars().any(char::is_control) {
            lines.push(format!("cargo::rerun-if-changed={text}"));
        }
    }
    lines
}

/// The branch a `HEAD` file names (`ref: refs/heads/main`): `None` for a detached `HEAD` (a commit), and for a name that is not under `refs/`, that climbs out of the
/// folder with `..`, or holds a control character, which is not followed.
fn branch_named(head: &str) -> Option<&str> {
    let name = head.trim().strip_prefix("ref: ")?.trim();
    let safe = name.starts_with("refs/")
        && name.len() <= 200
        && !name.split('/').any(|part| part == ".." || part.is_empty())
        && !name.chars().any(char::is_control);
    safe.then_some(name)
}

/// The files whose change means another commit: `HEAD` (a new commit when it is detached, another branch when it is not), the branch it names, and
/// `packed-refs` (where git keeps a branch it has packed). `git_path` is `git rev-parse --git-path <name>`: a linked worktree has a `HEAD` of its own, and
/// the branch files are in the main checkout's folder. `read` gives a file's text. Which of them exist is for the caller to ask.
pub fn files_to_watch(git_path: &dyn Fn(&str) -> Option<PathBuf>, read: &dyn Fn(&Path) -> Option<String>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(head) = git_path("HEAD") {
        let branch = read(&head).as_deref().and_then(branch_named).and_then(git_path);
        files.push(head);
        files.extend(branch);
    }
    files.extend(git_path("packed-refs"));
    files
}

/// Whether the folder git says is the top of the repository (`git rev-parse --show-toplevel`) is the workspace of the crate in `manifest_dir`, which is two folders down
/// (`apps/<crate>`): the checkout, or a linked worktree of it, and not a repository the tree happens to lie in. Both paths are compared as they are: the caller has made
/// them canonical.
pub fn is_this_checkout(top: &Path, manifest_dir: &Path) -> bool {
    manifest_dir.parent().and_then(Path::parent).is_some_and(|workspace| workspace == top)
}

/// What `git <args>` printed in `dir`, if it ran and succeeded. A git that is not there, a folder that is no repository and a refusal (git's
/// "dubious ownership") are all `None`: the build goes on with the next source.
#[allow(dead_code)]
fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// A variable's text: `None` when it is not set or is not Unicode.
#[allow(dead_code)]
fn variable(name: &str) -> Option<String> {
    std::env::var_os(name).and_then(|value| value.into_string().ok())
}

/// The first bytes of a file as text, for `HEAD`: a line of it is all that is read.
#[allow(dead_code)]
fn read_start(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(1024).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

/// What the build knows of itself for the crate in `dir` (the crate's own folder, which is inside the checkout or a linked worktree of it): the texts git and the variables give, as text, and the files
/// whose change means another commit. `variable` reads a variable of the environment (a test gives it its own). Git answers for this checkout or not at all.
pub fn gather(dir: &Path, variable: &dyn Fn(&str) -> Option<String>) -> (Sources, Vec<PathBuf>) {
    let believed = run_git(dir, &["rev-parse", "--show-toplevel"])
        .and_then(|top| Some((std::fs::canonicalize(top.trim()).ok()?, std::fs::canonicalize(dir).ok()?)))
        .is_some_and(|(top, manifest)| is_this_checkout(&top, &manifest));
    let git = |args: &[&str]| if believed { run_git(dir, args) } else { None };
    let git_path = |name: &str| {
        let printed = git(&["rev-parse", "--git-path", name])?;
        let path = printed.trim();
        // Relative to the folder git ran in, or absolute: `join` takes either.
        (!path.is_empty()).then(|| dir.join(path))
    };
    let sources = Sources {
        version: variable("CARGO_PKG_VERSION"),
        env_git: variable("DIACRAFT_BUILD_GIT"),
        env_year: variable("DIACRAFT_BUILD_YEAR"),
        describe: git(&["describe", "--tags", "--always", "--dirty"]),
        commit_date: git(&["log", "-1", "--format=%cs"]),
        epoch: variable("SOURCE_DATE_EPOCH"),
    };
    let watched: Vec<PathBuf> = files_to_watch(&git_path, &read_start).into_iter().filter(|path| path.exists()).collect();
    (sources, watched)
}

/// The whole of a build script: finds the texts and prints the directives. Runs git in the crate's own folder, which is inside the checkout (or inside a linked worktree
/// of it); never fails.
#[allow(dead_code)]
pub fn emit() {
    let dir = std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let (sources, watched) = gather(&dir, &variable);
    for line in directives(&resolve(&sources), &watched) {
        println!("{line}");
    }
}
