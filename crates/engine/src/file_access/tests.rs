//! The automation roots: what a root grants, what resolves where, and the work a hostile path costs (counted, not timed).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A fresh tree holding `in` (the read root, with `in/sub` and `in/a.vsdx`), `out` (the write root), `outside` (with `secret.vsdx`) and `in2`, and its roots.
struct Tree {
    base: PathBuf,
    roots: Arc<AutomationRoots>,
}

impl Tree {
    fn new() -> Tree {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!("diacraft-file-access-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&base);
        for dir in ["in/sub", "out", "outside", "in2"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        std::fs::write(base.join("in/a.vsdx"), "x").unwrap();
        std::fs::write(base.join("outside/secret.vsdx"), "x").unwrap();
        let roots = AutomationRoots::new(&[&base.join("in")], &[&base.join("out")]).unwrap().unwrap();
        Tree { base, roots }
    }

    fn p(&self, relative: &str) -> PathBuf {
        self.base.join(relative)
    }

    fn read(&self, relative: &str) -> Result<PathBuf, Refusal> {
        self.roots.check(&self.p(relative), Access::Read)
    }

    fn write(&self, relative: &str) -> Result<PathBuf, Refusal> {
        self.roots.check(&self.p(relative), Access::Write)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Makes a link at `link` to `target`; `false` where the system will not (Windows without Developer Mode): the test then has nothing to show.
fn link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        } else {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}

#[test]
fn paths_inside_the_roots_pass_and_others_do_not() {
    let t = Tree::new();
    assert!(t.read("in/a.vsdx").is_ok());
    assert!(t.read("in").is_ok(), "the root itself");
    assert!(t.read("in/sub/new.vsdx").is_ok(), "a file that is not there yet is judged by its folder");
    assert!(t.write("out/new/deeper/x.png").is_ok(), "and by its nearest folder that is there");
    assert_eq!(t.read("outside/secret.vsdx"), Err(Refusal::Outside(Access::Read)));
    assert_eq!(t.write("outside/x.png"), Err(Refusal::Outside(Access::Write)));
}

#[test]
fn a_root_of_one_kind_grants_only_that_kind() {
    let t = Tree::new();
    assert_eq!(t.write("in/a.vsdx"), Err(Refusal::Outside(Access::Write)), "a read root is no place to write");
    assert_eq!(t.read("out/x.png"), Err(Refusal::Outside(Access::Read)), "a write root is no place to read");
    let read_only = AutomationRoots::new(&[&t.p("in")], &[]).unwrap().unwrap();
    assert_eq!(read_only.check(&t.p("in/x.vsdx"), Access::Write), Err(Refusal::NotGranted(Access::Write)));
    let write_only = AutomationRoots::new(&[], &[&t.p("out")]).unwrap().unwrap();
    assert_eq!(write_only.check(&t.p("out/x.vsdx"), Access::Read), Err(Refusal::NotGranted(Access::Read)));
}

#[test]
fn the_answer_is_the_real_path_to_use() {
    let t = Tree::new();
    let real = t.read("in/sub/../a.vsdx").unwrap();
    assert_eq!(real, std::fs::canonicalize(t.p("in/a.vsdx")).unwrap(), "`..` taken, links followed");
    let new = t.write("out/new.png").unwrap();
    assert_eq!(new, std::fs::canonicalize(t.p("out")).unwrap().join("new.png"), "a file not there yet: its folder's real path and its name");
}

#[test]
fn a_sibling_that_shares_a_prefix_is_outside() {
    let t = Tree::new();
    std::fs::write(t.p("in2/b.vsdx"), "x").unwrap();
    assert_eq!(t.read("in2/b.vsdx"), Err(Refusal::Outside(Access::Read)), "in2 is not inside in");
    let spelled = format!("{}2/c.vsdx", t.p("in").display());
    assert_eq!(t.roots.check(Path::new(&spelled), Access::Read), Err(Refusal::Outside(Access::Read)));
}

#[test]
fn parent_steps_are_taken_before_the_comparison() {
    let t = Tree::new();
    let sep = std::path::MAIN_SEPARATOR;
    let up = format!("{}{sep}..{sep}outside{sep}secret.vsdx", t.p("in").display());
    assert_eq!(t.roots.check(Path::new(&up), Access::Read), Err(Refusal::Outside(Access::Read)), "`..` out of the root");
    let up_twice = format!("{}{sep}sub{sep}..{sep}..{sep}outside{sep}secret.vsdx", t.p("in").display());
    assert_eq!(t.roots.check(Path::new(&up_twice), Access::Read), Err(Refusal::Outside(Access::Read)));
    let back = format!("{}{sep}..{sep}a.vsdx", t.p("in/sub").display());
    assert!(t.roots.check(Path::new(&back), Access::Read).is_ok(), "`..` that stays inside");
    // A `..` below a folder that is not there: the system would fail it, so it is never trusted to land inside.
    let missing = format!("{}{sep}nope{sep}..{sep}..{sep}outside{sep}x.png", t.p("out").display());
    assert!(t.roots.check(Path::new(&missing), Access::Write).is_err());
    let missing_inside = format!("{}{sep}nope{sep}..{sep}x.png", t.p("out").display());
    assert!(t.roots.check(Path::new(&missing_inside), Access::Write).is_err(), "not trusted even where it would stay inside");
}

/// The rule that Windows needs written down (it folds `nope\..` away before the file system is asked), tried on every system with no file system: a `..` must step out of a
/// folder that is there, and each is asked of at most once in a row.
#[test]
fn a_parent_step_must_step_out_of_a_folder_that_is_there() {
    let base = if cfg!(windows) { PathBuf::from(r"C:\w") } else { PathBuf::from("/w") };
    let folders = [base.clone(), base.join("out"), base.join("out").join("sub")];
    let judge = |path: &Path| {
        let mut asked: Vec<PathBuf> = Vec::new();
        let ok = parent_steps_leave_folders(path, &mut |folder| {
            asked.push(folder.to_path_buf());
            folders.iter().any(|f| f == folder)
        });
        (ok, asked)
    };
    let out = base.join("out");
    let sub = out.join("sub");
    // Below a folder that is there.
    assert_eq!(judge(&sub.join("..").join("a.vsdx")), (true, vec![sub.clone()]));
    assert_eq!(judge(&out.join(".").join("sub").join(".").join("..").join("a.vsdx")), (true, vec![sub.clone()]), "a `.` is no part");
    assert_eq!(judge(&out.join("a.vsdx")), (true, vec![]), "no `..`: nothing is asked");
    // Below one that is not.
    assert_eq!(judge(&out.join("nope").join("..").join("x.png")), (false, vec![out.join("nope")]));
    assert_eq!(judge(&out.join("nope").join("..").join("..").join("outside").join("x.png")), (false, vec![out.join("nope")]));
    assert_eq!(judge(&sub.join("nope").join("..").join("..").join("x.png")), (false, vec![sub.join("nope")]));
    assert_eq!(judge(&out.join("f.png").join("..").join("x.png")), (false, vec![out.join("f.png")]), "a file is no folder to step out of");
    // The step after a step out of a folder that is there leaves the one above it, and that is asked too.
    assert_eq!(judge(&sub.join("..").join("..").join("a.vsdx")), (true, vec![sub.clone(), out.clone()]));
    assert_eq!(judge(&sub.join("..").join("..").join("..").join("a.vsdx")), (true, vec![sub.clone(), out.clone(), base.clone()]));
    assert_eq!(judge(&sub.join("..").join("nope").join("..").join("x.png")), (false, vec![sub.clone(), out.join("nope")]));
    // A chain that leaves one folder over and over asks of it once.
    let chain = (0..1000).fold(out.clone(), |p, _| p.join("sub").join(".."));
    assert_eq!(judge(&chain.join("a.vsdx")), (true, vec![sub.clone()]), "{} parts, one question", chain.components().count());
    // A `..` that has nothing to step out of is asked of nothing: at the start of a relative path, and at the root.
    assert_eq!(judge(Path::new("..").join("x.png").as_path()), (true, vec![]));
    assert_eq!(judge(Path::new("..").join("..").join("x.png").as_path()), (true, vec![]));
    assert_eq!(judge(Path::new("nope").join("..").join("x.png").as_path()), (false, vec![PathBuf::from("nope")]));
    assert_eq!(judge(Path::new("..").join("nope").join("..").join("x.png").as_path()), (false, vec![Path::new("..").join("nope")]));
    let root = base.ancestors().last().unwrap().to_path_buf();
    assert_eq!(judge(&root.join("..").join("w").join("x.png")), (true, vec![]), "the root has no folder above it");
}

/// What the rule above looks like on this system's own file system, through `check`: a `..` below a folder that is not there is refused, and below one that is there is taken.
#[test]
fn a_parent_step_below_a_missing_folder_is_refused_and_below_a_folder_that_is_there_is_taken() {
    let t = Tree::new();
    std::fs::create_dir_all(t.p("out/sub")).unwrap();
    let sep = std::path::MAIN_SEPARATOR;
    let out = t.p("out").display().to_string();
    let real = std::fs::canonicalize(t.p("out")).unwrap().join("x.png");
    assert_eq!(t.roots.check(Path::new(&format!("{out}{sep}sub{sep}..{sep}x.png")), Access::Write), Ok(real.clone()));
    assert_eq!(t.roots.check(Path::new(&format!("{out}{sep}sub{sep}..{sep}sub{sep}..{sep}x.png")), Access::Write), Ok(real));
    for spelled in [format!("{out}{sep}nope{sep}..{sep}x.png"), format!("{out}{sep}sub{sep}nope{sep}..{sep}..{sep}x.png")] {
        let answer = t.roots.check(Path::new(&spelled), Access::Write);
        assert_eq!(answer, Err(Refusal::Unresolved(Access::Write)), "{spelled}: the name is inside the root, and cannot be resolved");
    }
    // By its name it leaves the root, and is told as outside, as any path that does.
    let leaving = format!("{out}{sep}nope{sep}..{sep}..{sep}outside{sep}x.png");
    assert_eq!(t.roots.check(Path::new(&leaving), Access::Write), Err(Refusal::Outside(Access::Write)));
}

/// The name of a file to write is kept apart from the real path of its folder, and a link standing at the name is refused, where `check` would follow it.
#[test]
fn a_write_is_at_the_real_folder_and_the_name_and_a_link_at_the_name_is_refused() {
    let t = Tree::new();
    std::fs::create_dir_all(t.p("out/sub")).unwrap();
    let entry = |relative: &str| t.roots.check_entry(&t.p(relative), Access::Write);
    let folder = std::fs::canonicalize(t.p("out")).unwrap();
    assert_eq!(entry("out/new.vsdx"), Ok(folder.join("new.vsdx")), "a file that is not there");
    std::fs::write(t.p("out/a.vsdx"), "x").unwrap();
    assert_eq!(entry("out/a.vsdx"), Ok(folder.join("a.vsdx")), "a file that is");
    assert_eq!(entry("out/deeper/new.vsdx"), Ok(folder.join("deeper").join("new.vsdx")), "a folder that is not there is the write's to refuse");
    assert_eq!(entry("outside/secret.vsdx"), Err(Refusal::Outside(Access::Write)), "outside is outside, as `check` says");
    assert_eq!(t.roots.check_entry(&t.p("out/.."), Access::Write), Err(Refusal::Outside(Access::Write)), "a name that leads out");
    assert_eq!(entry("out/sub/.."), Err(Refusal::Unusable), "a path that ends in `..` has no name to write");
    assert_eq!(t.roots.check_entry(Path::new(""), Access::Write), Err(Refusal::Unusable));
    let read_only = AutomationRoots::new(&[&t.p("in")], &[]).unwrap().unwrap();
    assert_eq!(read_only.check_entry(&t.p("in/x.vsdx"), Access::Write), Err(Refusal::NotGranted(Access::Write)));
    if !link(&t.p("out/a.vsdx"), &t.p("out/ln.vsdx")) {
        return;
    }
    assert_eq!(
        t.write("out/ln.vsdx"),
        Ok(std::fs::canonicalize(t.p("out/a.vsdx")).unwrap()),
        "`check` follows a link inside the roots: the file it leads to"
    );
    assert_eq!(entry("out/ln.vsdx"), Err(Refusal::Swapped(Access::Write)), "and a write does not go through it");
    // A link that leads outside, or nowhere, is told as `check` tells it.
    assert!(link(&t.p("outside/secret.vsdx"), &t.p("out/leak.vsdx")));
    assert_eq!(entry("out/leak.vsdx"), Err(Refusal::Outside(Access::Write)));
    assert!(link(&t.p("out/gone.vsdx"), &t.p("out/dangling.vsdx")));
    assert_eq!(entry("out/dangling.vsdx"), Err(Refusal::Unresolved(Access::Write)));
    // A folder that is a link inside the roots is followed: the file is at the folder's real place.
    assert!(link(&t.p("out/sub"), &t.p("out/alias")));
    assert_eq!(entry("out/alias/b.vsdx"), Ok(folder.join("sub").join("b.vsdx")));
    // With no roots in force the path is as it is.
    let file = t.p("out/ln.vsdx");
    assert_eq!(allow_entry(&file, Access::Write), Ok(file.clone()));
    assert_eq!(confine(Some(&t.roots), || allow_entry(&file, Access::Write)), Err(Refusal::Swapped(Access::Write)));
}

#[test]
fn a_link_inside_a_root_that_leads_out_of_it_is_outside() {
    let t = Tree::new();
    if !link(&t.p("outside/secret.vsdx"), &t.p("in/leak.vsdx")) {
        return;
    }
    assert_eq!(t.read("in/leak.vsdx"), Err(Refusal::Outside(Access::Read)), "a file link out");
    assert!(link(&t.p("outside"), &t.p("in/away")));
    assert_eq!(t.read("in/away/secret.vsdx"), Err(Refusal::Outside(Access::Read)), "a folder link out");
    assert!(link(&t.p("outside"), &t.p("out/away")));
    assert_eq!(t.write("out/away/new.vsdx"), Err(Refusal::Outside(Access::Write)), "a save through a folder link would create the file outside");
    assert!(link(&t.p("outside/secret.vsdx"), &t.p("out/ln.vsdx")));
    assert_eq!(t.write("out/ln.vsdx"), Err(Refusal::Outside(Access::Write)), "a save through a file link would replace the file outside");
    // A link that leads nowhere would have a write create the file where it points.
    assert!(link(&t.p("outside/gone.vsdx"), &t.p("out/dangling.vsdx")));
    assert_eq!(t.write("out/dangling.vsdx"), Err(Refusal::Unresolved(Access::Write)), "named for what it is, since it stands inside the root");
    assert!(link(&t.p("outside/gone.vsdx"), &t.p("outside/dangling.vsdx")));
    assert_eq!(
        t.write("outside/dangling.vsdx"),
        Err(Refusal::Outside(Access::Write)),
        "outside the roots it is no more than outside: nothing is told of what is there"
    );
}

#[test]
fn a_link_that_stays_inside_the_root_is_followed() {
    let t = Tree::new();
    if !link(&t.p("in/sub"), &t.p("in/alias")) {
        return;
    }
    let real = t.read("in/alias/x.vsdx").unwrap();
    assert_eq!(real, std::fs::canonicalize(t.p("in/sub")).unwrap().join("x.vsdx"), "what is opened is where the link leads");
    std::fs::write(t.p("in2/b.vsdx"), "x").unwrap();
    assert!(link(&t.p("in2/b.vsdx"), &t.p("in/to-in2.vsdx")));
    assert_eq!(t.read("in/to-in2.vsdx"), Err(Refusal::Outside(Access::Read)), "a link into another folder is outside a root that does not hold it");
    let both = AutomationRoots::new(&[&t.p("in"), &t.p("in2")], &[]).unwrap().unwrap();
    assert!(both.check(&t.p("in/to-in2.vsdx"), Access::Read).is_ok(), "but inside when another root holds it");
}

#[test]
fn a_root_that_is_a_link_is_the_folder_it_leads_to() {
    let t = Tree::new();
    if !link(&t.p("in"), &t.p("in-link")) {
        return;
    }
    let linked = AutomationRoots::new(&[&t.p("in-link")], &[]).unwrap().unwrap();
    assert!(linked.check(&t.p("in/a.vsdx"), Access::Read).is_ok(), "the folder it leads to");
    assert!(linked.check(&t.p("in-link/a.vsdx"), Access::Read).is_ok(), "and through the link");
    assert_eq!(linked.check(&t.p("in2/b.vsdx"), Access::Read), Err(Refusal::Outside(Access::Read)));
    // A root that leads out of the folder it was named in is the folder it leads to, and its name grants nothing else.
    assert!(link(&t.p("outside"), &t.p("in/escape")));
    let through = AutomationRoots::new(&[&t.p("in/escape")], &[]).unwrap().unwrap();
    assert!(through.check(&t.p("outside/secret.vsdx"), Access::Read).is_ok());
    assert_eq!(through.check(&t.p("in/a.vsdx"), Access::Read), Err(Refusal::Outside(Access::Read)));
}

#[test]
fn a_case_variant_is_judged_by_the_real_folder() {
    let t = Tree::new();
    let variant = t.p("IN/A.VSDX");
    // Whether the file system ignores case is asked of it: a folder and its upper-case spelling are one folder on some (macOS, Windows) and two on others.
    let folds = std::fs::metadata(&variant).is_ok();
    let answer = t.roots.check(&variant, Access::Read);
    if folds {
        assert!(answer.is_ok(), "the file system says it is the same folder: {answer:?}");
        assert_eq!(answer.unwrap(), std::fs::canonicalize(t.p("in/a.vsdx")).unwrap(), "and the path to use has the real case");
    } else {
        assert_eq!(answer, Err(Refusal::Outside(Access::Read)), "another folder that is not there is not the root");
    }
    // A variant of the neighbour is never inside, whatever the file system does with case.
    assert!(t.roots.check(&t.p("IN2/b.vsdx"), Access::Read).is_err());
}

#[test]
fn only_a_drive_letter_ignores_case_in_a_component() {
    // `same_component_with`: the rule on its own, so that every system tests it. Folder names are compared as they are: the paths are real paths by then,
    // and a folder that differs in case alone may be another one (case-sensitive folders on Windows, volumes on macOS).
    let a = Path::new("/Work/Sub").components().collect::<Vec<_>>();
    let b = Path::new("/work/sub").components().collect::<Vec<_>>();
    assert!(!same_component_with(a[1], b[1], true));
    assert!(same_component_with(a[1], a[1], true));
    assert!(same_component_with(a[0], b[0], false));
}

/// Paths written the way another system writes them are parsed as this one parses them: on Unix a backslash is a letter, `C:` is a folder name and the
/// path is relative, and nothing escapes by the other system's separators.
#[cfg(unix)]
#[test]
fn windows_style_paths_are_plain_names_here() {
    let t = Tree::new();
    for text in [
        r"C:\Users\someone\secret.vsdx",
        r"\\server\share\secret.vsdx",
        r"\\?\C:\secret.vsdx",
        r"..\..\outside\secret.vsdx",
        r"in\..\..\outside\secret.vsdx",
        r"in\a.vsdx",
        "C:secret.vsdx",
    ] {
        // Relative to the working directory, none of them is inside a root.
        let relative = t.roots.check(Path::new(text), Access::Read);
        assert!(relative.is_err(), "{text}: {relative:?}");
        // Inside a root they are file names, however they look: never a way out.
        let inside = t.p("in").join(text);
        match t.roots.check(&inside, Access::Read) {
            Ok(real) => {
                let root = std::fs::canonicalize(t.p("in")).unwrap();
                assert_eq!(real.parent(), Some(root.as_path()), "{text} is one name inside the root: {real:?}");
            }
            Err(why) => panic!("{text}: a file name inside the root was refused: {why}"),
        }
    }
}

#[test]
fn a_chain_of_parent_steps_costs_one_resolution() {
    let t = Tree::new();
    let sep = std::path::MAIN_SEPARATOR;
    // `sub/..` a thousand times: the system folds it in one call, and it stays inside.
    let chain: String = std::iter::repeat_n(format!("sub{sep}..{sep}"), 1000).collect();
    let spelled = format!("{}{sep}{chain}a.vsdx", t.p("in").display());
    let before = probes_made();
    let answer = t.roots.check(Path::new(&spelled), Access::Read);
    let probes = probes_made() - before;
    assert!(probes <= 4, "{probes} probes for one chain");
    match answer {
        Ok(real) => assert_eq!(real, std::fs::canonicalize(t.p("in/a.vsdx")).unwrap()),
        // The system's own limit on a path's length is a refusal too.
        Err(why) => assert!(matches!(why, Refusal::Unresolved(_) | Refusal::Unusable), "{why:?}"),
    }
    // A chain that leaves the root by its end is outside, however deep.
    let leaving: String = std::iter::repeat_n(format!("sub{sep}..{sep}"), 800).collect();
    let spelled = format!("{}{sep}{leaving}..{sep}outside{sep}secret.vsdx", t.p("in").display());
    let before = probes_made();
    let answer = t.roots.check(Path::new(&spelled), Access::Read);
    assert!(probes_made() - before <= 4);
    assert!(matches!(answer, Err(Refusal::Outside(Access::Read) | Refusal::Unresolved(Access::Read) | Refusal::Unusable)), "{answer:?}");
    assert!(answer.is_err());
}

#[test]
fn a_chain_of_parent_steps_below_nothing_is_refused_in_a_bounded_walk() {
    let t = Tree::new();
    let sep = std::path::MAIN_SEPARATOR;
    // A hundred missing folders and then a hundred `..`: the walk back up is a probe or two for each part, never more.
    let down: String = (0..100).map(|n| format!("m{n}{sep}")).collect();
    let up: String = std::iter::repeat_n(format!("..{sep}"), 100).collect();
    let spelled = format!("{}{sep}{down}{up}x.png", t.p("out").display());
    let before = probes_made();
    let answer = t.roots.check(Path::new(&spelled), Access::Write);
    let probes = probes_made() - before;
    assert!(answer.is_err(), "{answer:?}");
    assert!(probes <= 2 * 201 + 2, "{probes} probes for 201 parts");
}

#[test]
fn a_path_of_64_kib_is_refused_before_the_file_system_is_asked_anything() {
    let t = Tree::new();
    let long = format!("{}/{}", t.p("in").display(), "a".repeat(64 * 1024));
    let before = probes_made();
    assert_eq!(t.roots.check(Path::new(&long), Access::Read), Err(Refusal::Unusable));
    let many = format!("{}/{}", t.p("in").display(), "a/".repeat(32 * 1024));
    assert_eq!(t.roots.check(Path::new(&many), Access::Read), Err(Refusal::Unusable));
    assert_eq!(probes_made() - before, 0, "no call was made for either");
    // A long path that is under the system's own limit is judged like any other, and the walk up from it is a probe or two for each part.
    let parts = "a/".repeat(1400);
    let spelled = format!("{}/{parts}x.png", t.p("out").display());
    let before = probes_made();
    let answer = t.roots.check(Path::new(&spelled), Access::Write);
    let probes = probes_made() - before;
    match answer {
        Ok(real) => {
            assert!(real.starts_with(std::fs::canonicalize(t.p("out")).unwrap()), "{real:?}");
            assert!((1400..=2 * 1401 + 1).contains(&probes), "{probes} probes for 1,401 missing parts");
        }
        // A system whose own limit on a path is shorter (macOS: 1,024 bytes) refuses it at the first call.
        Err(why) => assert_eq!((why, probes <= 4), (Refusal::Unresolved(Access::Write), true), "{probes} probes"),
    }
    // Near the cap on parts: the most it can cost is two probes for each part, whatever the system says of it.
    let parts = "a/".repeat(MOST_COMPONENTS - 100);
    let spelled = format!("{}/{parts}x.png", t.p("out").display());
    let before = probes_made();
    let answer = t.roots.check(Path::new(&spelled), Access::Write);
    assert!(matches!(answer, Ok(_) | Err(Refusal::Unresolved(Access::Write))), "{answer:?}");
    assert!(probes_made() - before <= 2 * u64::try_from(MOST_COMPONENTS).unwrap() + 2, "{} probes", probes_made() - before);
}

#[test]
fn an_empty_path_or_one_with_a_nul_is_not_a_path() {
    let t = Tree::new();
    assert_eq!(t.roots.check(Path::new(""), Access::Read), Err(Refusal::Unusable));
    let with_nul = format!("{}/a\0.vsdx", t.p("in").display());
    assert_eq!(t.roots.check(Path::new(&with_nul), Access::Read), Err(Refusal::Unusable));
}

/// A refusal says what the rule is and which flag grants the folder, and holds nothing of the path it was asked about or of what is there.
#[test]
fn a_refusal_names_the_rule_and_holds_nothing_of_the_path() {
    let t = Tree::new();
    let secret = t.p("outside/secret-name-xyzzy.vsdx");
    std::fs::write(&secret, "SECRET CONTENT PLUGH").unwrap();
    let cases = [
        (t.roots.check(&secret, Access::Read), "--automation-read-root", "reading"),
        (t.roots.check(&secret, Access::Write), "--automation-write-root", "writing"),
        (AutomationRoots::new(&[], &[&t.p("out")]).unwrap().unwrap().check(&secret, Access::Read), "--automation-read-root", "read"),
    ];
    for (answer, flag, word) in cases {
        let words = answer.unwrap_err().to_string();
        assert!(words.starts_with("automation "), "{words}");
        assert!(words.contains(flag) && words.contains(word), "{words}");
        assert!(!words.contains("xyzzy") && !words.contains("PLUGH") && !words.contains(&*t.base.to_string_lossy()), "{words}");
    }
    for refusal in [
        Refusal::NotGranted(Access::Read),
        Refusal::NotGranted(Access::Write),
        Refusal::Outside(Access::Read),
        Refusal::Outside(Access::Write),
        Refusal::Unresolved(Access::Read),
        Refusal::Unresolved(Access::Write),
        Refusal::Unusable,
        Refusal::Swapped(Access::Read),
        Refusal::Swapped(Access::Write),
    ] {
        let words = refusal.to_string();
        assert!(words.starts_with("automation ") && words.len() > 40 && !words.contains('\n'), "{refusal:?}: {words}");
    }
}

#[test]
fn roots_are_checked_when_they_are_given() {
    let t = Tree::new();
    let e = AutomationRoots::new(&[&t.p("nope")], &[]).unwrap_err();
    assert!(e.starts_with("cannot use the automation read root `") && e.contains("nope"), "{e}");
    let e = AutomationRoots::new(&[], &[&t.p("in/a.vsdx")]).unwrap_err();
    assert!(e.starts_with("cannot use the automation write root `") && e.contains("not a folder"), "{e}");
    assert!(AutomationRoots::new(&[Path::new("")], &[]).is_err());
    assert!(AutomationRoots::new(&[], &[]).unwrap().is_none(), "no roots is no confinement");
    // A bad one among good ones fails the lot: a root that did not take is never left out in silence.
    assert!(AutomationRoots::new(&[&t.p("in"), &t.p("nope")], &[]).is_err());
}

#[test]
fn several_roots_each_grant_their_kind_and_one_given_twice_counts_once() {
    let t = Tree::new();
    std::fs::create_dir_all(t.p("out2")).unwrap();
    std::fs::write(t.p("in2/b.vsdx"), "x").unwrap();
    let roots = AutomationRoots::new(&[&t.p("in"), &t.p("in2")], &[&t.p("out"), &t.p("out2")]).unwrap().unwrap();
    assert!(roots.check(&t.p("in/a.vsdx"), Access::Read).is_ok());
    assert!(roots.check(&t.p("in2/b.vsdx"), Access::Read).is_ok());
    assert!(roots.check(&t.p("out2/new/x.png"), Access::Write).is_ok());
    assert!(roots.check(&t.p("outside/secret.vsdx"), Access::Read).is_err());
    assert!(roots.check(&t.p("in2/b.vsdx"), Access::Write).is_err(), "read and write stay apart with several roots");
    let sep = std::path::MAIN_SEPARATOR;
    let up = format!("{}{sep}..{sep}outside{sep}secret.vsdx", t.p("in2").display());
    assert!(roots.check(Path::new(&up), Access::Read).is_err());
    let twice = AutomationRoots::new(&[&t.p("in"), &t.p("in/sub/..")], &[]).unwrap().unwrap();
    assert_eq!(twice.read_roots().len(), 1, "{:?}", twice.read_roots());
}

#[test]
fn there_is_a_most_to_the_roots_of_a_kind() {
    let t = Tree::new();
    let folders: Vec<PathBuf> = (0..=MOST_ROOTS)
        .map(|n| {
            let dir = t.p(&format!("many/{n}"));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        })
        .collect();
    let refs: Vec<&Path> = folders.iter().map(PathBuf::as_path).collect();
    let e = AutomationRoots::new(&refs, &[]).unwrap_err();
    assert!(e.contains("at most 64") && e.contains("read"), "{e}");
    let e = AutomationRoots::new(&[], &refs).unwrap_err();
    assert!(e.contains("at most 64") && e.contains("write"), "{e}");
    assert!(AutomationRoots::new(&refs[..MOST_ROOTS], &refs[..MOST_ROOTS]).unwrap().is_some(), "the most is allowed");
}

#[test]
fn the_roots_are_told_in_words_and_as_json() {
    let t = Tree::new();
    let shown_in = AutomationRoots::new(&[&t.p("in")], &[]).unwrap().unwrap();
    assert!(shown_in.describe().starts_with("read ") && shown_in.describe().ends_with("; write none"), "{}", shown_in.describe());
    let json = t.roots.to_json();
    assert_eq!(json["read"].as_array().map(Vec::len), Some(1), "{json}");
    assert_eq!(json["write"].as_array().map(Vec::len), Some(1), "{json}");
    assert!(json["read"][0].as_str().unwrap().ends_with("in"), "{json}");
    assert_eq!(t.roots.read_roots(), shown_in.read_roots());
}

#[test]
fn a_scope_is_in_force_inside_confine_and_put_back_after_it() {
    let t = Tree::new();
    let outside = t.p("outside/secret.vsdx");
    assert_eq!(allow(&outside, Access::Read), Ok(outside.clone()), "nothing is checked with no roots in force");
    confine(Some(&t.roots), || {
        assert_eq!(allow(&outside, Access::Read), Err(Refusal::Outside(Access::Read)));
        assert!(unconfined(|| allow(&outside, Access::Read)).is_ok(), "what the person does is not checked");
        assert!(allow(&outside, Access::Read).is_err(), "put back after it");
        assert!(allow(&t.p("in/a.vsdx"), Access::Read).is_ok());
    });
    assert!(allow(&outside, Access::Read).is_ok(), "put back after confine");
    assert!(confine(None, || allow(&outside, Access::Read)).is_ok(), "no roots to put in force is no scope");
    let panicked = std::panic::catch_unwind(|| confine(Some(&t.roots), || panic!("boom")));
    assert!(panicked.is_err());
    assert!(allow(&outside, Access::Read).is_ok(), "put back after a panic");
}

#[test]
fn a_scope_goes_to_work_handed_to_another_thread_only_when_it_is_carried() {
    let t = Tree::new();
    let outside = t.p("outside/secret.vsdx");
    confine(Some(&t.roots), || {
        let handle = ScopeHandle::capture();
        let (a, b) = (outside.clone(), outside.clone());
        let bare = std::thread::spawn(move || allow(&a, Access::Read).is_ok()).join().unwrap();
        let carried = std::thread::spawn(move || handle.run(|| allow(&b, Access::Read).is_ok())).join().unwrap();
        assert!(bare, "a thread that is not given the scope has none");
        assert!(!carried, "a thread that is given it is confined");
        let free = unconfined(ScopeHandle::capture);
        let b = outside.clone();
        assert!(std::thread::spawn(move || free.run(|| allow(&b, Access::Read).is_ok())).join().unwrap(), "an unconfined scope is carried as it is");
    });
}

/// Work that is started now and repeated later (a stencil reopened in a new drawing, a card answered a frame after it was raised) is repeated in the scope it was started in.
#[test]
fn work_that_is_repeated_later_runs_in_the_scope_it_was_started_in() {
    let t = Tree::new();
    let outside = t.p("outside/secret.vsdx");
    let (agent, person) = (confine(Some(&t.roots), ScopeHandle::capture), ScopeHandle::capture());
    // What a request started stays under its roots, on any thread and whatever scope the thread that repeats it holds.
    assert_eq!(unconfined(|| agent.clone().run_replay(|| allow(&outside, Access::Read))), Err(Refusal::Outside(Access::Read)));
    let (handle, path) = (agent.clone(), outside.clone());
    let on_a_thread = std::thread::spawn(move || handle.run_replay(|| allow(&path, Access::Read))).join().unwrap();
    assert_eq!(on_a_thread, Err(Refusal::Outside(Access::Read)));
    // What the person started stays the person's, even repeated inside a request's scope (a request that has a drawing read reopens the person's stencils).
    assert!(confine(Some(&t.roots), || person.clone().run_replay(|| allow(&outside, Access::Read))).is_ok());
}

/// A card answered after it was raised: its scope never loosens what the frame that answers it is under.
#[test]
fn a_carried_scope_never_loosens_what_is_in_force() {
    let t = Tree::new();
    let outside = t.p("outside/secret.vsdx");
    let (agent, person) = (confine(Some(&t.roots), ScopeHandle::capture), ScopeHandle::capture());
    // The person raised it, and a request's frame answers it: the frame's roots hold.
    assert_eq!(confine(Some(&t.roots), || person.clone().run_beneath(|| allow(&outside, Access::Read))), Err(Refusal::Outside(Access::Read)));
    // A request raised it, and the person answers it in a frame with no roots: the request's roots hold.
    assert_eq!(agent.clone().run_beneath(|| allow(&outside, Access::Read)), Err(Refusal::Outside(Access::Read)));
    // Nobody's roots anywhere: nothing is checked.
    assert!(person.run_beneath(|| allow(&outside, Access::Read)).is_ok());
}

/// The handle that was opened is looked at, and a file put where the checked one was between the check and the open is refused.
#[test]
fn a_file_swapped_for_a_link_after_the_check_is_refused_when_what_was_opened_is_looked_at() {
    let t = Tree::new();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/a.vsdx"), Access::Read)).unwrap();
    // As it was checked: the file opened is that file.
    let file = std::fs::File::open(&checked).unwrap();
    assert_eq!(confine(Some(&t.roots), || verify_opened(&file, &checked, Access::Read)), Ok(()));
    assert_eq!(verify_by_identity(&file, &checked), Ok(()));
    assert_eq!(verify_opened(&file, &checked, Access::Read), Ok(()), "nothing is looked at with no roots in force");
    // Swapped: a link to a file outside stands where it was, and the open follows it.
    std::fs::remove_file(&checked).unwrap();
    if !link(&t.p("outside/secret.vsdx"), &checked) {
        return;
    }
    let file = std::fs::File::open(&checked).unwrap();
    assert_eq!(confine(Some(&t.roots), || verify_opened(&file, &checked, Access::Read)), Err(Refusal::Swapped(Access::Read)));
    assert_eq!(verify_by_identity(&file, &checked), Err(Refusal::Swapped(Access::Read)), "the check that needs no /proc agrees");
    assert_eq!(verify_opened(&file, &checked, Access::Read), Ok(()), "with no roots in force nothing is refused");
}

/// The file that is there at the path now, and not the one opened: swapped back to a file inside after an outside one was opened.
#[test]
fn a_file_opened_outside_is_refused_even_when_the_path_is_put_back() {
    let t = Tree::new();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/a.vsdx"), Access::Read)).unwrap();
    std::fs::remove_file(&checked).unwrap();
    if !link(&t.p("outside/secret.vsdx"), &checked) {
        return;
    }
    let outside_handle = std::fs::File::open(&checked).unwrap();
    std::fs::remove_file(&checked).unwrap();
    std::fs::write(&checked, "back inside").unwrap();
    assert_eq!(verify_by_identity(&outside_handle, &checked), Err(Refusal::Swapped(Access::Read)), "another file than the one that is there now");
}

/// A file of `b.vsdx`'s size, and `stamp`, its modified time, made at `path`: another file that nothing but its identity tells from the one the test opened.
fn lookalike(path: &Path, size: u64, stamp: std::time::SystemTime) {
    std::fs::write(path, vec![b'Z'; usize::try_from(size).unwrap()]).unwrap();
    std::fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(stamp).unwrap();
    let meta = std::fs::metadata(path).unwrap();
    assert_eq!((meta.len(), meta.modified().unwrap()), (size, stamp), "it looks as the file that was opened did");
}

/// What the look at a handle read on Windows was its size and its modified time, which is less than an identity: a copy of the file, with its size and its modified time, passed
/// it. The file's own identity (device and inode on Unix, volume serial number and file index on Windows) tells a copy from the file, and the file under another name (a hard
/// link) from a copy.
#[test]
fn a_hard_link_is_the_file_that_was_opened_and_a_copy_of_the_same_size_and_time_is_not() {
    let t = Tree::new();
    std::fs::write(t.p("in/b.vsdx"), "bytes of b").unwrap();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/b.vsdx"), Access::Read)).unwrap();
    let opened = std::fs::File::open(&checked).unwrap();
    let (size, stamp) = (opened.metadata().unwrap().len(), opened.metadata().unwrap().modified().unwrap());
    std::fs::hard_link(&checked, t.p("in/b-link.vsdx")).unwrap();
    let linked = std::fs::canonicalize(t.p("in/b-link.vsdx")).unwrap();
    assert_eq!(verify_by_identity(&opened, &linked), Ok(()), "a hard link is the file that was opened");
    std::fs::copy(&checked, t.p("in/b-copy.vsdx")).unwrap();
    std::fs::OpenOptions::new().write(true).open(t.p("in/b-copy.vsdx")).unwrap().set_modified(stamp).unwrap();
    let copy = std::fs::canonicalize(t.p("in/b-copy.vsdx")).unwrap();
    let there = std::fs::metadata(&copy).unwrap();
    assert_eq!((there.len(), there.modified().unwrap()), (size, stamp), "nothing but its identity tells the copy from the file");
    assert_eq!(verify_by_identity(&opened, &copy), Err(Refusal::Swapped(Access::Read)), "the copy is not the file that was opened");
}

/// The race the look at a handle is for: after the file was opened, another file of the same size and modified time is put in the checked path's place (and then a folder,
/// and then nothing). Each is refused, with no roots' answer from the system to lean on.
#[test]
fn another_file_of_the_same_size_and_time_put_in_the_place_after_the_open_is_refused() {
    let t = Tree::new();
    std::fs::write(t.p("in/b.vsdx"), "bytes of b").unwrap();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/b.vsdx"), Access::Read)).unwrap();
    let opened = std::fs::File::open(&checked).unwrap();
    let (size, stamp) = (opened.metadata().unwrap().len(), opened.metadata().unwrap().modified().unwrap());
    assert_eq!(verify_by_identity(&opened, &checked), Ok(()), "as it was checked: the file opened is the file there");
    std::fs::remove_file(&checked).unwrap();
    lookalike(&checked, size, stamp);
    assert_eq!(verify_by_identity(&opened, &checked), Err(Refusal::Swapped(Access::Read)), "another file of that size and time is refused");
    // A folder put there is refused too, and is not looked into.
    std::fs::remove_file(&checked).unwrap();
    std::fs::create_dir(&checked).unwrap();
    assert_eq!(verify_by_identity(&opened, &checked), Err(Refusal::Swapped(Access::Read)), "a folder");
    // Nothing there is refused.
    std::fs::remove_dir(&checked).unwrap();
    assert_eq!(verify_by_identity(&opened, &checked), Err(Refusal::Swapped(Access::Read)), "nothing");
}

/// The look at the path's identity asks about what stands at the path and **never follows a link there**. `verify_by_identity` first sees that the path has no link on its way,
/// and a link put at the path after that look, leading to the file that was opened, would pass a look that followed it: the swap cannot be timed in a test, so the look is
/// tried on its own, with the link already in place.
#[test]
fn a_link_standing_at_the_path_is_not_followed_by_the_look_at_its_identity() {
    let t = Tree::new();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/a.vsdx"), Access::Read)).unwrap();
    // The file that was opened lies outside, as it would if a link had stood in the path when it was opened.
    let outside = std::fs::File::open(t.p("outside/secret.vsdx")).unwrap();
    assert!(!is_the_file_at(&outside, &checked), "another file stands at the path");
    assert!(is_the_file_at(&outside, &t.p("outside/secret.vsdx")), "the file that was opened, by its own name, is that file");
    // A link to it is put at the path: followed, it leads to the file that was opened.
    std::fs::remove_file(&checked).unwrap();
    if !link(&t.p("outside/secret.vsdx"), &checked) {
        return;
    }
    assert!(!is_the_file_at(&outside, &checked), "a link is looked at as itself, and is not the file it leads to");
    assert_eq!(verify_by_identity(&outside, &checked), Err(Refusal::Swapped(Access::Read)));
    // The same for a link to a file inside.
    std::fs::write(t.p("in/b.vsdx"), "x").unwrap();
    let b = std::fs::File::open(t.p("in/b.vsdx")).unwrap();
    assert!(link(&t.p("in/b.vsdx"), &t.p("in/b-link.vsdx")));
    assert!(!is_the_file_at(&b, &t.p("in/b-link.vsdx")), "a link to the file that was opened is not that file");
    assert!(is_the_file_at(&b, &t.p("in/b.vsdx")));
}

/// The identity is asked without opening the checked path a second time (where it is opened, a pipe waits for a writer): a pipe put in the place is refused, and the reader does
/// not wait. `file.rs` opens without waiting for the same reason.
#[cfg(unix)]
#[test]
fn a_pipe_put_in_the_place_is_refused_without_waiting_for_a_writer() {
    let t = Tree::new();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/a.vsdx"), Access::Read)).unwrap();
    let opened = std::fs::File::open(&checked).unwrap();
    std::fs::remove_file(&checked).unwrap();
    if !std::process::Command::new("mkfifo").arg(&checked).status().is_ok_and(|s| s.success()) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let at = checked.clone();
    std::thread::spawn(move || {
        let _ = tx.send(verify_by_identity(&opened, &at));
    });
    let answer = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("the check of a pipe's place waited for a writer: the path was opened");
    assert_eq!(answer, Err(Refusal::Swapped(Access::Read)));
}

/// What is looked at is real: the roots' own folders, and the folder a link was swapped for.
#[test]
fn a_folder_swapped_for_a_link_after_the_check_is_refused_at_the_open() {
    let t = Tree::new();
    std::fs::write(t.p("in/sub/b.vsdx"), "inside").unwrap();
    std::fs::write(t.p("outside/b.vsdx"), "outside").unwrap();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/sub/b.vsdx"), Access::Read)).unwrap();
    std::fs::remove_dir_all(t.p("in/sub")).unwrap();
    if !link(&t.p("outside"), &t.p("in/sub")) {
        return;
    }
    let file = std::fs::File::open(&checked).unwrap();
    assert_eq!(confine(Some(&t.roots), || verify_opened(&file, &checked, Access::Read)), Err(Refusal::Swapped(Access::Read)));
    assert_eq!(verify_by_identity(&file, &checked), Err(Refusal::Swapped(Access::Read)));
}

/// The system's own answer for a handle is the last word where it gives one: a handle it places outside every root is refused, though the file at the checked path has the
/// same identity (here a hard link; in a race, a folder that was a link when the file was opened and a folder again when it was looked at). The identity is two looks at two
/// paths, which a second swap can tell apart from the first; the system's answer is one look at the handle.
#[cfg(target_os = "linux")]
#[test]
fn a_handle_the_system_places_outside_the_roots_is_refused_though_the_checked_path_has_its_identity() {
    let t = Tree::new();
    std::fs::write(t.p("outside/b.vsdx"), "x").unwrap();
    std::fs::hard_link(t.p("outside/b.vsdx"), t.p("in/b.vsdx")).unwrap();
    let checked = confine(Some(&t.roots), || allow(&t.p("in/b.vsdx"), Access::Read)).unwrap();
    // Opened by its name outside the roots, as a swapped folder would have it opened.
    let opened = std::fs::File::open(t.p("outside/b.vsdx")).unwrap();
    if linux_real_path(&opened).is_none() {
        return; // no /proc here: the system gives no answer, and the identity is all there is
    }
    assert_eq!(verify_by_identity(&opened, &checked), Ok(()), "by its identity alone the handle is the file at the checked path");
    assert_eq!(
        confine(Some(&t.roots), || verify_opened(&opened, &checked, Access::Read)),
        Err(Refusal::Swapped(Access::Read)),
        "the system says the handle is outside: the identity does not overrule it"
    );
    // The same file opened by its name inside is the file that was checked.
    let inside = std::fs::File::open(&checked).unwrap();
    assert_eq!(confine(Some(&t.roots), || verify_opened(&inside, &checked, Access::Read)), Ok(()));
}

#[cfg(windows)]
#[test]
fn windows_refuses_device_names_and_judges_a_drive_letter_without_case() {
    let t = Tree::new();
    assert_eq!(t.roots.check(&t.p("out/NUL"), Access::Write), Err(Refusal::Unusable));
    assert_eq!(t.roots.check(&t.p("out/com1.txt"), Access::Write), Err(Refusal::Unusable));
    assert_eq!(t.roots.check(&t.p("out/CON.png"), Access::Write), Err(Refusal::Unusable));
    assert!(t.roots.check(&t.p("out/console.png"), Access::Write).is_ok());
    let text = t.p("in/a.vsdx").to_string_lossy().into_owned();
    let lower = format!("{}{}", text[..1].to_lowercase(), &text[1..]);
    assert!(t.roots.check(Path::new(&lower), Access::Read).is_ok(), "{lower}");
}

/// Windows folds `nope\..` away before the file system is asked (`std::path::absolute` does it, and so does the system when it opens a path), so a `..` below a folder that is
/// not there must be looked at first: it is refused as on every other system (`parent_steps_are_taken_before_the_comparison`), and one below a folder that is there is taken.
#[cfg(windows)]
#[test]
fn windows_refuses_a_parent_step_below_a_missing_folder_and_takes_one_below_a_folder_that_is_there() {
    let t = Tree::new();
    std::fs::create_dir_all(t.p("out/sub")).unwrap();
    std::fs::write(t.p("out/f.png"), "x").unwrap();
    let out = t.p("out").display().to_string();
    let write = |text: String| t.roots.check(Path::new(&text), Access::Write);
    assert!(write(format!(r"{out}\nope\..\x.png")).is_err(), "nope is not there");
    assert!(write(format!("{out}/nope/../x.png")).is_err(), "with the other separator");
    assert!(write(format!(r"{out}\nope\.\..\x.png")).is_err(), "with a `.` between");
    assert!(write(format!(r"{out}\sub\nope\..\..\x.png")).is_err(), "a missing folder below one that is there");
    assert!(write(format!(r"{out}\f.png\..\x.png")).is_err(), "a file is no folder to step out of");
    let real = std::fs::canonicalize(t.p("out")).unwrap().join("x.png");
    assert_eq!(write(format!(r"{out}\sub\..\x.png")), Ok(real.clone()), "sub is there");
    assert_eq!(write(format!(r"{out}\sub\..\sub\..\x.png")), Ok(real), "again and again");
    assert_eq!(t.read(r"in\sub\..\a.vsdx"), Ok(std::fs::canonicalize(t.p("in/a.vsdx")).unwrap()), "a step that stays inside, in a read root");
}

/// A junction (a folder link Windows makes without a privilege) is followed as a symbolic link is.
#[cfg(windows)]
#[test]
fn a_junction_that_leads_out_is_outside() {
    let t = Tree::new();
    // Both paths are written with the system's own separator: `mklink` takes a `/` in the link's path for the start of a switch, makes no junction ("Parameter format not
    // correct"), and this test used to return here, with nothing shown, on every Windows machine.
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(t.p("in").join("jump"))
        .arg(t.p("outside"))
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(made, "mklink /J made no junction (a junction needs no privilege, but a file system that is not NTFS has none)");
    assert_eq!(t.read("in/jump/secret.vsdx"), Err(Refusal::Outside(Access::Read)));
    assert_eq!(t.read("in/jump"), Err(Refusal::Outside(Access::Read)), "the junction itself leads out");
}

/// What a hostile path costs in time, on a real file system: a long chain of parent steps and a path of 64 KiB are each refused in far less than a second.
/// A time depends on the machine, so this is run on request (`--ignored`); the counted tests above are the ones that always run.
#[test]
#[ignore = "a wall-clock check: run it with --ignored"]
fn hostile_paths_are_refused_quickly() {
    let t = Tree::new();
    let sep = std::path::MAIN_SEPARATOR;
    let started = std::time::Instant::now();
    let chain: String = std::iter::repeat_n(format!("sub{sep}..{sep}"), 1000).collect();
    for _ in 0..100 {
        let _ = t.roots.check(Path::new(&format!("{}{sep}{chain}a.vsdx", t.p("in").display())), Access::Read);
        let _ = t.roots.check(Path::new(&format!("{}/{}", t.p("in").display(), "a".repeat(64 * 1024))), Access::Read);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
}
