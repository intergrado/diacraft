//! `cargo xtask help`: the checks of the help tree (`help/`), and what is generated from it.
//!
//! * `cargo xtask help check` (a step of `cargo xtask ci`) holds every file of `help/` to the rules of the format, with the same parser the program reads the topics with
//!   (`diacraft-help`): [`check`].
//! * `cargo xtask help --write` writes what is generated from the help tree. There is nothing generated yet: the site's help pages and guides and the What's New topic
//!   are written by later tasks, which add their files here, and the command checks the tree first so that nothing is generated from a tree that fails.

pub mod check;

use std::path::Path;

/// What `cargo xtask help` says when it is given nothing it knows.
const USAGE: &str = "usage: cargo xtask help check | cargo xtask help --write";

/// Runs `cargo xtask help <rest>` in the repository `root`.
pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    match rest {
        ["check"] => check::run(root),
        ["--write"] => {
            check::run(root)?;
            println!("help --write: nothing is generated from the help tree yet (the site's pages and What's New have their own tasks)");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
