//! `cargo xtask help check`: every refusal of the design's section 2.5, over trees built in the test, and the real tree.

use std::collections::BTreeSet;
use std::process::Command;

use xtask::brands::DENY;
use xtask::help::check::{Known, SiteInfo, check, read_tree};

use crate::TempDir;

fn set(names: &[&str]) -> Option<BTreeSet<String>> {
    Some(names.iter().map(|n| (*n).to_owned()).collect())
}

fn known() -> Known {
    Known {
        controls: set(&["Alpha", "Beta"]),
        groups: set(&["GroupOne"]),
        tabs: set(&["TabOne"]),
        commands: set(&["view.fit", "page.rename"]),
        dialogs: None,
        panes: None,
        backstage: None,
    }
}

fn site() -> SiteInfo {
    SiteInfo { domain: "https://diacraft.dev".into(), app_url: "https://diacraft.app".into(), repository: "intergrado/diacraft".into() }
}

/// A topic file: its header (not published) and body.
fn topic(id: &str, extra: &str, body: &str) -> String {
    format!("---\nid: {id}\ntitle: Title of {id}\nsummary: About {id}.\nkeywords: {id}\nsite: no\n{extra}---\n{body}")
}

/// A tree that passes: a home that is unlisted, two topics in sections, and empty tables.
fn clean() -> TempDir {
    let dir = TempDir::new("help");
    dir.write("help/topics/start/home.md", &topic("home", "unlisted: yes\n", "Welcome. See [A](topic:alpha).\n"));
    dir.write("help/topics/start/alpha.md", &topic("alpha", "", "## First {#first}\n\nText with [Beta](ctl:Beta) and [fit](cmd:view.fit) and [Undo](key:Cmd+Z).\n\n## Second\n\nBack to [first](#first) and [home](topic:home).\n"));
    dir.write(
        "help/topics/start/beta.md",
        &topic("beta", "", "Link to [alpha](topic:alpha#second) and [web](https://diacraft.dev/guides/x) and [keys](topic:keyboard-shortcuts).\n"),
    );
    dir.write("help/sections.tsv", "# sections\nStart\talpha\nStart\tbeta\n");
    dir.write("help/tips.tsv", "# tips\nAlpha\tDoes alpha.\talpha first\nview.fit\tFits.\tfit\n");
    dir.write("help/map.tsv", "# map\nribbon:Alpha\talpha#first\t\ngroup:GroupOne\talpha\t\ntab:TabOne\tbeta\t\nwindow:canvas\tnone\tnot built\ncmd:view.fit\talpha\t\ndialog:anything/tab\talpha\t\n");
    dir.write("help/published.txt", "# published\nalpha\n");
    dir.write(
        "site/site.json",
        "{\"domain\":\"https://diacraft.dev\",\"app_url\":\"https://diacraft.app\",\"repository\":\"intergrado/diacraft\"}\n",
    );
    dir
}

fn problems(dir: &TempDir) -> Vec<String> {
    check(&read_tree(dir.path()).unwrap(), &known(), &site())
}

fn has(found: &[String], needles: &[&str]) -> bool {
    found.iter().any(|f| needles.iter().all(|n| f.contains(n)))
}

#[test]
fn a_clean_tree_passes() {
    assert_eq!(problems(&clean()), Vec::<String>::new());
}

#[test]
fn a_topic_that_breaks_a_rule_of_the_format_is_named_with_its_file_and_line() {
    let dir = clean();
    dir.write("help/topics/start/beta.md", &topic("beta", "colour: red\n", "fine\n\n<b>html</b>\n"));
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/beta.md:7:", "unknown header key"]), "{found:?}");
    assert!(has(&found, &["help/topics/start/beta.md:11:", "HTML is not allowed"]), "{found:?}");
}

#[test]
fn a_file_the_parser_refuses_whole_is_named_with_its_words() {
    let dir = clean();
    dir.write("help/topics/start/beta.md", "no header at all\n");
    assert!(has(&problems(&dir), &["help/topics/start/beta.md:1:", "does not start with a header"]));
}

#[test]
fn a_file_is_named_by_its_id_and_ids_are_unique() {
    let dir = clean();
    dir.write("help/topics/start/gamma.md", &topic("delta", "", "Text.\n"));
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/gamma.md", "named by its id", "delta.md"]), "{found:?}");
    let dir = clean();
    dir.write("help/topics/other/alpha.md", &topic("alpha", "", "Another.\n"));
    let found = problems(&dir);
    assert!(
        has(&found, &["help/topics/start/alpha.md", "is also the id of", "help/topics/other/alpha.md"])
            || has(&found, &["help/topics/other/alpha.md", "is also the id of", "help/topics/start/alpha.md"]),
        "{found:?}"
    );
}

#[test]
fn a_published_id_that_disappears_is_refused_and_one_that_moved_is_not() {
    let dir = clean();
    dir.write("help/published.txt", "# published\nalpha\nvanished\nkeyboard-shortcuts\n");
    let found = problems(&dir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(has(&found, &["help/published.txt:3:", "`vanished`", "was published and is gone"]), "{found:?}");
    // alpha moves to another folder and stays published
    let dir = clean();
    std::fs::rename(dir.path().join("help/topics/start/alpha.md"), {
        std::fs::create_dir_all(dir.path().join("help/topics/moved")).unwrap();
        dir.path().join("help/topics/moved/alpha.md")
    })
    .unwrap();
    assert_eq!(problems(&dir), Vec::<String>::new());
}

#[test]
fn a_link_to_a_topic_or_an_anchor_that_is_not_there_is_refused() {
    let dir = clean();
    dir.write(
        "help/topics/start/beta.md",
        &topic(
            "beta",
            "",
            "To [nowhere](topic:missing).\n\nTo [anchor](topic:alpha#absent).\n\nTo [own](#absent).\n\nTo [gen](topic:all-commands#x).\n",
        ),
    );
    let found = problems(&dir);
    assert!(has(&found, &["beta.md:8:", "the topic `missing`, which does not exist"]), "{found:?}");
    assert!(has(&found, &["beta.md:10:", "has no anchor `absent`", "alpha"]), "{found:?}");
    assert!(has(&found, &["beta.md:12:", "this topic has no anchor `absent`"]), "{found:?}");
    assert!(has(&found, &["beta.md:14:", "`all-commands` is made by the program"]), "{found:?}");
}

#[test]
fn a_link_to_a_control_or_a_command_that_does_not_exist_is_refused_and_unknown_sets_are_not_checked() {
    let dir = clean();
    dir.write("help/topics/start/beta.md", &topic("beta", "", "A [c](ctl:Gone) and a [d](cmd:no.such).\n"));
    let found = problems(&dir);
    assert!(has(&found, &["beta.md:8:", "`ctl:Gone` is no control"]), "{found:?}");
    assert!(has(&found, &["beta.md:8:", "`cmd:no.such` is no registered command"]), "{found:?}");
    let none = Known::default();
    assert!(check(&read_tree(dir.path()).unwrap(), &none, &site()).is_empty());
}

#[test]
fn a_key_that_is_no_key_and_a_web_address_off_the_sites_are_refused() {
    let dir = clean();
    dir.write(
        "help/topics/start/beta.md",
        &topic("beta", "", "A [k](key:Hyper+Z) and [w](https://example.org/x) and [x](https://diacraft.dev.evil/).\n"),
    );
    let found = problems(&dir);
    assert!(has(&found, &["beta.md:8:", "not a key"]) || has(&found, &["beta.md:8:", "key:"]), "{found:?}");
    assert_eq!(
        found.iter().filter(|f| f.contains("web address that is not on an allowed site")).count(),
        1,
        "the repeated problem of a line is said once: {found:?}"
    );
}

#[test]
fn every_topic_is_in_a_section_or_unlisted_and_a_section_names_topics_that_exist() {
    let dir = clean();
    dir.write("help/topics/start/lost.md", &topic("lost", "", "Not listed.\n"));
    dir.write("help/sections.tsv", "Start\talpha\nStart\tbeta\nStart\tabsent\n");
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/lost.md", "is in no section", "unlisted: yes"]), "{found:?}");
    assert!(has(&found, &["help/sections.tsv:3:", "`absent`, which does not exist"]), "{found:?}");
    dir.write("help/topics/start/lost.md", &topic("lost", "unlisted: yes\n", "Not listed.\n"));
    dir.write("help/sections.tsv", "Start\talpha\nStart\tbeta\nStart\tkeyboard-shortcuts\n");
    assert_eq!(problems(&dir), Vec::<String>::new());
}

#[test]
fn the_tips_table_names_only_controls_and_commands_that_exist_and_is_well_formed() {
    let dir = clean();
    dir.write("help/tips.tsv", "Alpha\tFine.\tk\nGone\tNo such.\tk\nbad row only\n");
    let found = problems(&dir);
    assert!(has(&found, &["help/tips.tsv:2:", "`Gone` is no control of the ribbon and no registered command"]), "{found:?}");
    assert!(has(&found, &["help/tips.tsv:3:", "wrong number of columns"]), "{found:?}");
}

#[test]
fn the_map_names_only_places_and_topics_that_exist() {
    let dir = clean();
    dir.write(
        "help/map.tsv",
        "ribbon:Nope\talpha\t\ngroup:Nope\talpha\t\ntab:Nope\talpha\t\ncmd:nope.cmd\talpha\t\nwindow:bogus\talpha\t\nribbon:Alpha\tmissing\t\nribbon:Beta\talpha#absent\t\ngroup:GroupOne\tkeyboard-shortcuts#x\t\ndialog:ok\talpha\t\n",
    );
    let found = problems(&dir);
    for (line, needle) in [
        (1, "no control of the ribbon"),
        (2, "no group of the ribbon"),
        (3, "no tab of the ribbon"),
        (4, "no registered command"),
        (5, "no area of the window"),
        (6, "the topic `missing` does not exist"),
        (7, "no anchor `absent`"),
        (8, "made by the program"),
    ] {
        assert!(has(&found, &[&format!("help/map.tsv:{line}:"), needle]), "line {line}: {found:?}");
    }
    assert!(!found.iter().any(|f| f.contains("map.tsv:9:")), "a dialog is not looked up when the set of dialogs is not given: {found:?}");
    // with the dialogs known, one that is not there is refused
    let with_dialogs = Known { dialogs: set(&["other", "other/tab"]), ..known() };
    let found = check(&read_tree(dir.path()).unwrap(), &with_dialogs, &site());
    assert!(has(&found, &["help/map.tsv:9:", "no dialog or tab of a dialog"]), "{found:?}");
}

#[test]
fn the_programs_sites_are_the_sites() {
    let dir = clean();
    let other = SiteInfo { domain: "https://elsewhere.example".into(), app_url: "https://diacraft.app/".into(), repository: "someone/else".into() };
    let found = check(&read_tree(dir.path()).unwrap(), &known(), &other);
    assert!(has(&found, &["crates/help/src/host.rs", "the website", "https://elsewhere.example"]), "{found:?}");
    assert!(has(&found, &["the public repository", "someone/else"]), "{found:?}");
    assert!(has(&found, &["the feedback repository", "someone/else"]), "{found:?}");
    assert!(!has(&found, &["the web app"]), "a trailing slash does not matter: {found:?}");
}

/// The product's name, made up at run time so that no line of this file names it (the export's list of lines that may is for the tests that need the literal).
fn product() -> String {
    ["Vi", "sio"].concat()
}

#[test]
fn the_product_name_is_refused_in_a_body_and_allowed_in_the_site_only_fields() {
    let (v, term) = (product(), DENY[1]);
    let dir = clean();
    let description = format!(
        "A guide to opening {v} drawings in DiaCraft: what opens, what is kept on a save, and where to look when something is missing from a drawing."
    );
    assert!((140..=160).contains(&description.chars().count()), "{}", description.chars().count());
    let bare = format!(
        "---\nid: beta\ntitle: Beta\nsummary: About beta.\nkeywords: beta\nsite: yes\ndescription: {description}\nseo_title: Open {v} drawings\n---\nBody with {v} in it.\n"
    );
    dir.write("help/topics/start/beta.md", &bare);
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/beta.md:10:", &format!("names \"{term}\"")]), "{found:?}");
    assert!(!found.iter().any(|f| f.contains("beta.md:7:") || f.contains("beta.md:8:")), "description and seo_title may name it: {found:?}");
    dir.write("help/topics/start/beta.md", &bare.replace("title: Beta", &format!("title: {v} help")));
    assert!(has(&problems(&dir), &["beta.md:3:", &format!("names \"{term}\"")]));
}

#[test]
fn the_other_files_of_the_tree_are_read_for_names_too() {
    let (v, term) = (product(), DENY[1]);
    let dir = clean();
    dir.write("help/README.md", "How to write about Microsoft products.\n");
    dir.write("help/tips.tsv", &format!("Alpha\tLike {v} does.\tk\n"));
    let found = problems(&dir);
    assert!(has(&found, &["help/README.md:1:", "names \"microsoft\""]), "{found:?}");
    assert!(has(&found, &["help/tips.tsv:1:", &format!("names \"{term}\"")]), "{found:?}");
}

#[test]
fn the_tree_must_be_there_with_its_four_files() {
    let dir = TempDir::new("help-none");
    assert!(read_tree(dir.path()).unwrap_err().contains("`help/` is missing"));
    let dir = clean();
    std::fs::remove_file(dir.path().join("help/map.tsv")).unwrap();
    std::fs::remove_file(dir.path().join("help/published.txt")).unwrap();
    let found = problems(&dir);
    assert!(has(&found, &["help/map.tsv: the file is missing"]), "{found:?}");
    assert!(has(&found, &["help/published.txt: the file is missing"]), "{found:?}");
}

#[test]
fn a_file_that_is_not_text_or_too_large_is_named_and_a_topic_folder_holds_topics_only() {
    let dir = clean();
    dir.write_bytes("help/topics/start/binary.md", &[0xff, 0xfe, 0x00]);
    dir.write("help/topics/start/readme.txt", "not a topic\n");
    dir.write("help/big.tsv", &"x".repeat(1024 * 1024 + 1));
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/binary.md", "is not UTF-8 text"]), "{found:?}");
    assert!(has(&found, &["help/big.tsv", "more than"]), "{found:?}");
    assert!(has(&found, &["help/topics/start/readme.txt", "a topic is a `.md` file"]), "{found:?}");
}

#[cfg(unix)]
#[test]
fn a_link_in_the_tree_is_named_and_not_followed() {
    let dir = clean();
    std::os::unix::fs::symlink("/etc/hostname", dir.path().join("help/topics/start/link.md")).unwrap();
    let found = problems(&dir);
    assert!(has(&found, &["help/topics/start/link.md", "is a link"]), "{found:?}");
}

// ---- the real tree

fn root() -> std::path::PathBuf {
    xtask::root()
}

#[test]
fn the_scan_of_the_source_finds_what_the_window_declares() {
    let known = Known::scan(&root()).unwrap();
    let controls = known.controls.unwrap();
    assert!(
        ["Help", "ContactUs", "ShowTraining", "WhatsNewRecentUpdates", "Paste", "_3DRotationGallery"].iter().all(|c| controls.contains(*c)),
        "{}",
        controls.len()
    );
    assert!(known.groups.unwrap().contains("GroupHelpAndSupport"));
    let tabs = known.tabs.unwrap();
    assert!(tabs.contains("HelpTab") && tabs.contains("TabHome"), "{tabs:?}");
    let commands = known.commands.unwrap();
    assert!(["file.new", "view.zoomIn", "window.find", "view.fitPage"].iter().all(|c| commands.contains(*c)), "{}", commands.len());
    assert!(known.dialogs.is_none() && known.panes.is_none() && known.backstage.is_none());
}

#[test]
fn the_scan_refuses_a_source_it_cannot_read_the_shape_of() {
    let dir = TempDir::new("help-scan");
    dir.write("crates/ui-egui/src/ribbon/spec.rs", "// no controls here\n");
    dir.write("crates/ui-egui/src/commands.rs", "// no commands here\n");
    dir.write("crates/engine/src/cmd/a.rs", "// none\n");
    let e = Known::scan(dir.path()).unwrap_err();
    assert!(e.contains("found only 0 controls") && e.contains("xtask/src/help/check.rs"), "{e}");
}

#[test]
fn the_real_help_tree_passes_the_check() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask")).args(["help", "check"]).output().unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("help: ok"), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn help_write_checks_first_and_says_it_has_nothing_to_write_yet() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask")).args(["help", "--write"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing is generated"), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn help_with_no_word_is_the_usage_of_the_whole_tool_and_with_another_word_is_refused_with_what_this_command_takes() {
    let alone = Command::new(env!("CARGO_BIN_EXE_xtask")).arg("help").output().unwrap();
    assert!(alone.status.success());
    assert!(String::from_utf8_lossy(&alone.stdout).contains("help check | help --write"));
    let other = Command::new(env!("CARGO_BIN_EXE_xtask")).args(["help", "frobnicate"]).output().unwrap();
    assert!(!other.status.success());
    assert!(String::from_utf8_lossy(&other.stderr).contains("cargo xtask help check"), "{}", String::from_utf8_lossy(&other.stderr));
}

#[test]
fn the_usage_names_the_help_commands() {
    let text = crate::usage::entry(&crate::usage::usage(), "help check");
    assert!(text.contains("help --write") && text.contains("file:line") && text.contains("`ctl:`"), "{text}");
    assert!(crate::usage::usage().contains("docs, help check,"), "the ci line names the step");
}
