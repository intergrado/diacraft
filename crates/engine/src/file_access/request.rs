//! What the command line and the environment ask of the roots: `--automation-read-root` and `--automation-write-root` (each given as many times as there are folders), and
//! `DIACRAFT_AUTOMATION_READ_ROOT` and `DIACRAFT_AUTOMATION_WRITE_ROOT` for a program started where its flags cannot be written (a desktop entry, an MCP client's settings).
//!
//! Both programs read them the same way, so the words and the rules are in one place: a flag is `--flag FOLDER` or `--flag=FOLDER`; the environment holds a list written as `PATH`
//! is (`:` between folders on Unix, `;` on Windows); **a flag of a kind wins over the environment for that kind**, and a kind that is given by neither has no root, which grants no access
//! of its kind. [`RootRequest::open`] makes the roots, and a folder that is not there is an error that stops the program before it serves anything.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{AutomationRoots, MOST_ROOTS};

/// The flag that names a folder automation may read.
pub const READ_FLAG: &str = "--automation-read-root";

/// The flag that names a folder automation may write.
pub const WRITE_FLAG: &str = "--automation-write-root";

/// The variable that lists the folders automation may read, when no [`READ_FLAG`] is given.
pub const READ_ENV: &str = "DIACRAFT_AUTOMATION_READ_ROOT";

/// The variable that lists the folders automation may write, when no [`WRITE_FLAG`] is given.
pub const WRITE_ENV: &str = "DIACRAFT_AUTOMATION_WRITE_ROOT";

/// The folders asked for, as they were given: not yet looked at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootRequest {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
}

impl RootRequest {
    /// Takes `arg` if it is one of the two flags, with its folder: `--flag FOLDER` (the folder is the next of `rest`) or `--flag=FOLDER`. `Ok(false)` when `arg` is not
    /// one of them. A flag with no folder, with an empty one or given more than [`MOST_ROOTS`] times is an error in words.
    pub fn flag(&mut self, arg: &str, rest: &mut std::slice::Iter<'_, String>) -> Result<bool, String> {
        for (name, list) in [(READ_FLAG, &mut self.read), (WRITE_FLAG, &mut self.write)] {
            let value = if arg == name {
                rest.next().map(String::as_str).ok_or_else(|| format!("{name} needs a folder"))?
            } else if let Some(value) = arg.strip_prefix(name).and_then(|tail| tail.strip_prefix('=')) {
                value
            } else {
                continue;
            };
            if value.is_empty() {
                return Err(format!("{name} needs a folder, and the one given is empty"));
            }
            if list.len() >= MOST_ROOTS {
                return Err(format!("{name} was given more than {MOST_ROOTS} times"));
            }
            list.push(PathBuf::from(value));
            return Ok(true);
        }
        Ok(false)
    }

    /// Whether no folder was asked for.
    pub fn is_empty(&self) -> bool {
        self.read.is_empty() && self.write.is_empty()
    }

    /// The folders asked for reading.
    pub fn read(&self) -> &[PathBuf] {
        &self.read
    }

    /// The folders asked for writing.
    pub fn write(&self) -> &[PathBuf] {
        &self.write
    }

    /// The request with the environment's folders for each kind that no flag gave. An empty entry in a list is left out: `PATH` reads it as the current folder, and a root
    /// must never be that by a slip of a colon.
    #[must_use]
    pub fn with_environment(mut self, read: Option<&OsStr>, write: Option<&OsStr>) -> RootRequest {
        if self.read.is_empty() {
            self.read = listed(read);
        }
        if self.write.is_empty() {
            self.write = listed(write);
        }
        self
    }

    /// The roots the request makes: each folder must be there ([`AutomationRoots::new`]); `None` when none were asked for.
    pub fn open(&self) -> Result<Option<Arc<AutomationRoots>>, String> {
        let read: Vec<&Path> = self.read.iter().map(PathBuf::as_path).collect();
        let write: Vec<&Path> = self.write.iter().map(PathBuf::as_path).collect();
        AutomationRoots::new(&read, &write)
    }
}

/// The folders of an environment list, at most one more than a kind may have (so that too many is told by [`RootRequest::open`] and not cut off in silence).
fn listed(value: Option<&OsStr>) -> Vec<PathBuf> {
    value.map(|v| std::env::split_paths(v).filter(|p| !p.as_os_str().is_empty()).take(MOST_ROOTS + 1).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// Reads `args` as a parser would: each argument is offered to the request, and what it does not take is returned.
    fn read(args: &[&str]) -> Result<(RootRequest, Vec<String>), String> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let (mut request, mut left, mut rest) = (RootRequest::default(), Vec::new(), args.iter());
        while let Some(arg) = rest.next() {
            if !request.flag(arg, &mut rest)? {
                left.push(arg.clone());
            }
        }
        Ok((request, left))
    }

    fn folders(list: &[PathBuf]) -> Vec<&str> {
        list.iter().map(|p| p.to_str().unwrap()).collect()
    }

    #[test]
    fn a_flag_is_written_with_its_folder_after_it_or_after_an_equals_sign_and_may_be_repeated() {
        let (request, left) =
            read(&["--automation-read-root", "a", "--automation-read-root=b", "--automation-write-root", "w", "--headless"]).unwrap();
        assert_eq!(folders(request.read()), ["a", "b"]);
        assert_eq!(folders(request.write()), ["w"]);
        assert_eq!(left, ["--headless"], "what is not a root flag is left to the parser");
        assert!(!request.is_empty());
        assert!(read(&["--headless"]).unwrap().0.is_empty());
        // A name that only starts like a flag is no flag.
        assert_eq!(read(&["--automation-read-roots", "a"]).unwrap().1, ["--automation-read-roots", "a"]);
    }

    #[test]
    fn a_flag_with_no_folder_an_empty_one_or_too_many_is_an_error_in_words() {
        for (args, words) in [
            (&["--automation-read-root"][..], "--automation-read-root needs a folder"),
            (&["--automation-write-root"][..], "--automation-write-root needs a folder"),
            (&["--automation-read-root="][..], "empty"),
            (&["--automation-write-root", ""][..], "empty"),
        ] {
            let error = read(args).unwrap_err();
            assert!(error.contains(words), "{args:?}: {error}");
        }
        let many: Vec<String> = (0..=MOST_ROOTS).flat_map(|n| ["--automation-read-root".to_owned(), format!("d{n}")]).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let error = read(&refs).unwrap_err();
        assert!(error.contains("more than 64 times"), "{error}");
    }

    #[test]
    fn the_environment_fills_a_kind_that_no_flag_gave() {
        let list = |names: &[&str]| std::env::join_paths(names).unwrap();
        let (read_env, write_env) = (list(&["e1", "e2"]), list(&["w1"]));
        let flags = read(&["--automation-read-root", "f"]).unwrap().0;
        let request = flags.with_environment(Some(&read_env), Some(&write_env));
        assert_eq!(folders(request.read()), ["f"], "a flag of the kind wins over the environment");
        assert_eq!(folders(request.write()), ["w1"], "and the environment gives the kind the flags did not");
        let from_environment = RootRequest::default().with_environment(Some(&read_env), None);
        assert_eq!(folders(from_environment.read()), ["e1", "e2"]);
        assert!(from_environment.write().is_empty());
        assert!(RootRequest::default().with_environment(None, None).is_empty());
    }

    #[test]
    fn an_empty_entry_in_a_list_is_no_folder() {
        // `PATH` reads an empty entry as the current folder; a root is never that by a slip of a colon.
        let with_gaps = std::env::join_paths(["a", "", "b", ""]).unwrap_or_else(|_| OsString::from("a::b:"));
        let request = RootRequest::default().with_environment(Some(&with_gaps), Some(OsStr::new("")));
        assert_eq!(folders(request.read()), ["a", "b"]);
        assert!(request.write().is_empty(), "an empty variable is no variable");
    }

    #[test]
    fn the_roots_are_made_when_the_folders_are_there_and_none_is_none() {
        assert!(RootRequest::default().open().unwrap().is_none());
        let here = std::env::temp_dir();
        let mut request = RootRequest::default();
        request.read.push(here.clone());
        request.write.push(here);
        let roots = request.open().unwrap().unwrap();
        assert_eq!((roots.read_roots().len(), roots.write_roots().len()), (1, 1));
        request.read.push(PathBuf::from("/no/such/folder/anywhere"));
        let error = request.open().unwrap_err();
        assert!(error.starts_with("cannot use the automation read root"), "{error}");
        // Too many from the environment is told, and not cut.
        let names: Vec<String> = (0..=MOST_ROOTS + 5).map(|n| format!("e{n}")).collect();
        let long = std::env::join_paths(&names).unwrap();
        let request = RootRequest::default().with_environment(Some(&long), None);
        assert_eq!(request.read().len(), MOST_ROOTS + 1);
        assert!(request.open().unwrap_err().contains("at most 64"), "told as too many");
    }

    #[test]
    fn the_flags_and_variables_are_the_documented_ones() {
        assert_eq!((READ_FLAG, WRITE_FLAG), ("--automation-read-root", "--automation-write-root"));
        assert_eq!((READ_ENV, WRITE_ENV), ("DIACRAFT_AUTOMATION_READ_ROOT", "DIACRAFT_AUTOMATION_WRITE_ROOT"));
    }
}
