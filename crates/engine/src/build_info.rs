//! What the build says of itself: the build line and the year of the commit it was made from.
//!
//! The engine does not find these out. **Each application takes them at build time, in a `build.rs` of its own** (`apps/diacraft/build_info.rs`: `git describe` and the workspace
//! version, which make the build line, the commit's date, or the variables `DIACRAFT_BUILD_GIT` and `DIACRAFT_BUILD_YEAR` for a tree with no `.git`), and hands them over once at its start
//! ([`set`]). So the engine, which is the layer under every application, is not recompiled when a commit changes (a commit would otherwise
//! rebuild it and everything above it), and nothing here runs git or reads the clock.
//!
//! They are for people: the About panel and the start-up splash of the window (`diacraft-ui-egui`), which show the line under the version, and the answer of `app.version`, which the
//! command line and the MCP server pass on. **The build line follows the version** (the plain `0.2.0` for a build of the tag `v0.2.0`, `0.2.0-dev+aab1664d` for a build made before that
//! tag exists, where `git describe` would say `v0.1.0-1555-gaab1664d`: `apps/diacraft/build_info.rs`, `identify`). Until an application sets them, the line reads `unknown` and the year
//! [`LAST_RESORT_YEAR`].

use std::sync::OnceLock;

use crate::text::shown;

/// What the build line reads when no application has said: a tree built with no `.git` and no `DIACRAFT_BUILD_GIT`.
pub const UNKNOWN_GIT: &str = "unknown";

/// The year of last resort, when there is no commit date, no `DIACRAFT_BUILD_YEAR` and no `SOURCE_DATE_EPOCH`: the year of the first public work.
pub const LAST_RESORT_YEAR: &str = "2026";

/// The most characters of either text. A line such as `v0.1.0-766-g106427ea-dirty` is 26 and `0.2.0-dev+aab1664d.dirty` is 24; the build script refuses anything longer than 48.
const MAX_CHARS: usize = 64;

/// The build line and the year of the build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    /// The build line, made at build time from `git describe --tags --always --dirty` and the workspace version (`0.2.0` for a build of its tag, `0.2.0-dev+aab1664d` before the tag
    /// exists, `0.2.0+12.aab1664d` twelve commits after it), or the text of `DIACRAFT_BUILD_GIT`, or `unknown`. The field keeps its first name, and so does the `git` of `app.version`.
    pub git: String,
    /// The year the build's commit was made, as four digits.
    pub year: String,
}

impl BuildInfo {
    /// A build whose line is `git`, made in `year`, escaped and cut short: they arrive from outside the engine, and one of them goes into a window and a JSON answer. A text that
    /// is empty once trimmed reads as the default.
    pub fn new(git: &str, year: &str) -> BuildInfo {
        let (git, year) = (git.trim(), year.trim());
        BuildInfo {
            git: if git.is_empty() { UNKNOWN_GIT.to_owned() } else { shown(git, MAX_CHARS) },
            year: if year.is_empty() { LAST_RESORT_YEAR.to_owned() } else { shown(year, MAX_CHARS) },
        }
    }
}

impl Default for BuildInfo {
    fn default() -> BuildInfo {
        BuildInfo { git: UNKNOWN_GIT.to_owned(), year: LAST_RESORT_YEAR.to_owned() }
    }
}

static BUILD: OnceLock<BuildInfo> = OnceLock::new();

/// Tells the engine what this build is. **The first call wins**: the build does not change while the process runs. → whether this call set it.
pub fn set(info: BuildInfo) -> bool {
    BUILD.set(info).is_ok()
}

/// What the application said ([`set`]), or [`BuildInfo::default`] if it said nothing.
pub fn get() -> BuildInfo {
    BUILD.get().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    #[test]
    fn a_build_that_says_nothing_is_unknown_and_the_last_resort_year() {
        assert_eq!(BuildInfo::default(), BuildInfo { git: "unknown".to_owned(), year: "2026".to_owned() });
        assert_eq!(BuildInfo::new("", "  "), BuildInfo::default(), "a text that is empty is the default");
    }

    #[test]
    fn a_text_is_trimmed_escaped_and_cut_short() {
        assert_eq!(BuildInfo::new(" v1.2.3-4-gabc1234 \n", "2031").git, "v1.2.3-4-gabc1234");
        let nasty = BuildInfo::new("v1\u{1b}[2J\u{202e}", "20\n31");
        assert!(!nasty.git.contains('\u{1b}') && !nasty.git.contains('\u{202e}'), "{:?}", nasty.git);
        assert!(!nasty.year.contains('\n'), "{:?}", nasty.year);
        assert!(
            BuildInfo::new(&"x".repeat(5000), "2026").git.chars().count() <= MAX_CHARS + 1,
            "the characters, and the ellipsis that says there were more"
        );
    }

    /// The one test of the library's own test binary that sets the build (the process-wide cell is set once): `app.version` then answers it, to the command line
    /// and the MCP server alike, and the first call is the one that counts.
    #[test]
    fn app_version_answers_what_the_application_set_and_the_first_call_wins() {
        assert!(set(BuildInfo::new("v9.8.7-12-gabc1234", "2031")), "the first call sets it");
        assert!(!set(BuildInfo::new("v0.0.1", "1999")), "a later call changes nothing");
        assert_eq!(get(), BuildInfo { git: "v9.8.7-12-gabc1234".to_owned(), year: "2031".to_owned() });
        let mut session = Session::new();
        let answer = session.execute("app.version", &json!({})).expect("app.version runs with no document");
        assert_eq!(answer, json!({"app": "diacraft", "version": env!("CARGO_PKG_VERSION"), "git": "v9.8.7-12-gabc1234", "year": "2031"}));
    }
}
