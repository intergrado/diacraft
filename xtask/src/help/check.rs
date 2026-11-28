//! `cargo xtask help check`: every file of the help tree held to the rules of the format.
//!
//! The topics are read by `diacraft_help::parse`, the very code the program reads them with, so a topic that passes is a topic the Help pane can draw. On top of
//! what one topic says about itself, the check holds the tree together: ids are unique and named by their file, none that was published is lost, every link goes
//! somewhere that exists, every topic is in a section or unlisted, the three tables name only topics, controls, commands and places that exist, the allowed web
//! prefixes of the program are the site's, and the words follow the project's rules about names (`cargo xtask brands`).
//!
//! What a control or a command is comes from the source code, by text: [`Known::scan`] reads the ribbon's declarations and the command registries, so that this tool
//! needs none of the window's or the engine's code. Where a set is [`None`] the check does not look at the names of that kind (the window's own coverage test holds
//! dialogs, panes and the File view's pages to the real ones).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use diacraft_help::tables::{Map, MapTarget, Place, Sections, Tips};
use diacraft_help::{Problem, Topic, host, parse};

use crate::brands;

/// The most bytes of any one file of the help tree that is read: a larger file is reported and not read.
const MAX_FILE_BYTES: usize = 1024 * 1024;
/// The most files of the help tree: more is reported and the rest are not read.
const MAX_FILES: usize = 10_000;
/// The deepest folder of the help tree that is read.
const MAX_DEPTH: usize = 8;

/// The files of the help tree, read.
#[derive(Debug, Default)]
pub struct Tree {
    /// The topics: path from the repository's root, and text.
    pub topics: Vec<(String, String)>,
    /// `help/tips.tsv`.
    pub tips: Option<String>,
    /// `help/map.tsv`.
    pub map: Option<String>,
    /// `help/sections.tsv`.
    pub sections: Option<String>,
    /// `help/published.txt`.
    pub published: Option<String>,
    /// Every file of the tree with its text, for the check of names.
    pub texts: Vec<(String, String)>,
    /// What could not be read (a file too large, not text, a link, a folder too deep or too many files), as `path: why`.
    pub unreadable: Vec<String>,
}

/// What exists in the window and the engine, by name: `None` where the check does not look.
#[derive(Debug, Default)]
pub struct Known {
    /// The ribbon's controls (menu items and launchers too).
    pub controls: Option<BTreeSet<String>>,
    /// The ribbon's groups.
    pub groups: Option<BTreeSet<String>>,
    /// The ribbon's tabs.
    pub tabs: Option<BTreeSet<String>>,
    /// The registered commands, the engine's and the window's.
    pub commands: Option<BTreeSet<String>>,
    /// The dialogs, `id` and `id/tab`.
    pub dialogs: Option<BTreeSet<String>>,
    /// The panes.
    pub panes: Option<BTreeSet<String>>,
    /// The pages of the File view.
    pub backstage: Option<BTreeSet<String>>,
}

/// What the site says that the program must agree with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteInfo {
    /// `domain` of `site/site.json`.
    pub domain: String,
    /// `app_url`.
    pub app_url: String,
    /// `repository`, `owner/name`.
    pub repository: String,
}

/// The ribbon's declarations, by text.
const RIBBON: &str = "crates/ui-egui/src/ribbon/spec.rs";
/// The window's commands.
const WINDOW_COMMANDS: &str = "crates/ui-egui/src/commands.rs";
/// The folder of the engine's commands.
const ENGINE_COMMANDS: &str = "crates/engine/src/cmd";

/// The fewest of each kind the scan of the source accepts: fewer means a declaration changed its shape and the scan sees nothing, which would pass every link.
const FLOOR_CONTROLS: usize = 200;
const FLOOR_GROUPS: usize = 20;
const FLOOR_TABS: usize = 10;
const FLOOR_COMMANDS: usize = 150;

/// `cargo xtask help check`.
pub fn run(root: &Path) -> Result<(), String> {
    let known = Known::scan(root)?;
    let tree = read_tree(root)?;
    let found = check(&tree, &known, &read_site(root)?);
    if found.is_empty() {
        let rows = |t: &Option<String>| t.as_deref().map_or(0, |t| t.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).count());
        println!(
            "help: ok ({} topics, {} sections rows, {} tips, {} map rows)",
            tree.topics.len(),
            rows(&tree.sections),
            rows(&tree.tips),
            rows(&tree.map)
        );
        return Ok(());
    }
    Err(format!("help: {} problem(s) in the help tree:\n  {}", found.len(), found.join("\n  ")))
}

// ------------------------------------------------------------------------------------------------ reading

/// Reads `help/` under `root`. The folder must be there, with its four files.
pub fn read_tree(root: &Path) -> Result<Tree, String> {
    let base = root.join(host::HELP_TREE);
    if !base.is_dir() {
        return Err(format!("help: the help tree `{}/` is missing", host::HELP_TREE));
    }
    let mut tree = Tree::default();
    let mut count = 0usize;
    let mut stack = vec![(base, 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| format!("help: cannot list {}: {e}", dir.display()))?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("help: cannot list {}: {e}", dir.display()))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("help: cannot read {rel}: {e}"))?;
            if meta.file_type().is_symlink() {
                tree.unreadable.push(format!("{rel}: is a link, which the help tree does not hold"));
            } else if meta.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    tree.unreadable.push(format!("{rel}: is more than {MAX_DEPTH} folders deep"));
                } else {
                    stack.push((path, depth + 1));
                }
            } else {
                count += 1;
                if count > MAX_FILES {
                    tree.unreadable.push(format!("{rel}: is past the {MAX_FILES} files of the help tree; the rest are not read"));
                    return Ok(tree);
                }
                read_file(&path, &rel, &mut tree);
            }
        }
    }
    tree.topics.sort();
    tree.texts.sort();
    Ok(tree)
}

/// Reads one file into `tree` by its place.
fn read_file(path: &Path, rel: &str, tree: &mut Tree) {
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes));
    if let Err(e) = read {
        tree.unreadable.push(format!("{rel}: cannot be read: {e}"));
        return;
    }
    if bytes.len() > MAX_FILE_BYTES {
        tree.unreadable.push(format!("{rel}: is more than {MAX_FILE_BYTES} bytes, more than any file of the help tree may be"));
        return;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        tree.unreadable.push(format!("{rel}: is not UTF-8 text"));
        return;
    };
    let name = rel.strip_prefix(&format!("{}/", host::HELP_TREE)).unwrap_or(rel);
    let topics = format!("{}/", host::TOPICS_DIR);
    match name {
        n if n == host::TIPS_FILE => tree.tips = Some(text.clone()),
        n if n == host::MAP_FILE => tree.map = Some(text.clone()),
        n if n == host::SECTIONS_FILE => tree.sections = Some(text.clone()),
        "published.txt" => tree.published = Some(text.clone()),
        n if n.starts_with(&topics) => tree.topics.push((rel.to_owned(), text.clone())),
        _ => {}
    }
    tree.texts.push((rel.to_owned(), text));
}

/// Reads `site/site.json`.
pub fn read_site(root: &Path) -> Result<SiteInfo, String> {
    let path = root.join("site/site.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("help: cannot read site/site.json: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("help: site/site.json is not JSON: {e}"))?;
    let field =
        |name: &str| json.get(name).and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| format!("help: site/site.json has no `{name}`"));
    Ok(SiteInfo { domain: field("domain")?, app_url: field("app_url")?, repository: field("repository")? })
}

// ------------------------------------------------------------------------------------------------ what exists, by text

impl Known {
    /// What the repository at `root` declares: the ribbon's controls, groups and tabs and the registered commands, read from the source as text. Dialogs, panes and
    /// the File view's pages are not scanned (their coverage test is the window's), so those sets are `None`.
    pub fn scan(root: &Path) -> Result<Known, String> {
        let ribbon = read_source(root, RIBBON)?;
        let mut commands: BTreeSet<String> = ids_after(&read_source(root, WINDOW_COMMANDS)?, "UiCommand {", Some("id:")).into_iter().collect();
        let engine = root.join(ENGINE_COMMANDS);
        let mut files: Vec<_> = std::fs::read_dir(&engine)
            .map_err(|e| format!("help: cannot list {ENGINE_COMMANDS}: {e}"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .collect();
        files.sort();
        for file in files {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("help: cannot read {}: {e}", file.display()))?;
            commands.extend(ids_after(&text, "cmd!(", None));
        }
        let controls: BTreeSet<String> = ids_after(&ribbon, "c(", None).into_iter().collect();
        let groups: BTreeSet<String> = ids_after(&ribbon, "g(", None).into_iter().collect();
        let tabs: BTreeSet<String> = ids_after(&ribbon, "Tab {", Some("id:")).into_iter().collect();
        for (what, count, floor) in [
            ("controls", controls.len(), FLOOR_CONTROLS),
            ("groups", groups.len(), FLOOR_GROUPS),
            ("tabs", tabs.len(), FLOOR_TABS),
            ("commands", commands.len(), FLOOR_COMMANDS),
        ] {
            if count < floor {
                return Err(format!(
                    "help: the scan of the source found only {count} {what} (at least {floor} are expected): a declaration changed its shape, so update the scan in xtask/src/help/check.rs"
                ));
            }
        }
        Ok(Known {
            controls: Some(controls),
            groups: Some(groups),
            tabs: Some(tabs),
            commands: Some(commands),
            dialogs: None,
            panes: None,
            backstage: None,
        })
    }
}

fn read_source(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("help: cannot read {rel}: {e}"))
}

/// The string literals that follow `opener` where it stands as a call or a struct: not inside a longer name and not after a dot. After the opener come white space,
/// then (with `field`) that field name and white space, then (for `cmd!`) the word `query`, then the literal.
fn ids_after(text: &str, opener: &str, field: Option<&str>) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find(opener)).map(|i| i + from) {
        from = at + opener.len();
        let before = at.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'"') {
            continue;
        }
        let mut i = from;
        i = skip_space(bytes, i);
        if let Some(field) = field {
            if !text.get(i..).is_some_and(|r| r.starts_with(field)) {
                continue;
            }
            i = skip_space(bytes, i + field.len());
        }
        if text.get(i..).is_some_and(|r| r.starts_with("query") && r.as_bytes().get(5).is_some_and(u8::is_ascii_whitespace)) {
            i = skip_space(bytes, i + 5);
        }
        if bytes.get(i) != Some(&b'"') {
            continue;
        }
        if let Some(len) = text.get(i + 1..).and_then(|r| r.find('"'))
            && let Some(id) = text.get(i + 1..i + 1 + len)
            && !id.is_empty()
            && id.len() <= 96
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            found.push(id.to_owned());
        }
    }
    found
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

// ------------------------------------------------------------------------------------------------ the check

/// A problem as a line: `path:line: words (text)`.
fn describe(path: &str, p: &Problem) -> String {
    let detail = if p.detail.is_empty() { String::new() } else { format!(" (`{}`)", p.detail) };
    if p.line > 0 { format!("{path}:{}: {}{detail}", p.line, p.code.words()) } else { format!("{path}: {}{detail}", p.code.words()) }
}

/// The name of a file without its folders and its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name)
}

/// One problem per line, each `path:line: what`, for the whole tree; empty when it is right.
pub fn check(tree: &Tree, known: &Known, site: &SiteInfo) -> Vec<String> {
    let mut out: Vec<String> = tree.unreadable.clone();
    let mut topics: Vec<(String, Topic)> = Vec::new();
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    for (path, text) in &tree.topics {
        if !path.ends_with(".md") {
            out.push(format!("{path}: a topic is a `.md` file"));
            continue;
        }
        let parsed = match parse(text) {
            Ok(parsed) => parsed,
            Err(p) => {
                out.push(describe(path, &p));
                continue;
            }
        };
        out.extend(parsed.problems.iter().map(|p| describe(path, p)));
        let id = parsed.topic.header.id.clone();
        if stem(path) != id {
            out.push(format!("{path}: a topic's file is named by its id: this is `{id}`, so the file is `{id}.md`"));
        }
        if let Some(&first) = by_id.get(&id) {
            let other = topics.get(first).map_or("another file", |(p, _)| p.as_str());
            out.push(format!("{path}: the id `{id}` is also the id of {other}"));
            continue;
        }
        by_id.insert(id, topics.len());
        topics.push((path.clone(), parsed.topic));
    }
    out.extend(missing_files(tree));
    check_links(&topics, &by_id, known, &mut out);
    check_published(tree, &by_id, &mut out);
    check_sections(tree, &topics, &by_id, &mut out);
    check_tips(tree, known, &mut out);
    check_map(tree, &topics, &by_id, known, &mut out);
    check_site(site, &mut out);
    check_names(tree, &mut out);
    out
}

/// The files the tree must have.
fn missing_files(tree: &Tree) -> Vec<String> {
    [(&tree.sections, host::SECTIONS_FILE), (&tree.tips, host::TIPS_FILE), (&tree.map, host::MAP_FILE), (&tree.published, "published.txt")]
        .iter()
        .filter(|(text, _)| text.is_none())
        .map(|(_, name)| format!("{}/{name}: the file is missing", host::HELP_TREE))
        .collect()
}

fn generated(id: &str) -> bool {
    host::GENERATED_TOPICS.contains(&id)
}

/// Every link of every topic goes to something that exists.
fn check_links(topics: &[(String, Topic)], by_id: &BTreeMap<String, usize>, known: &Known, out: &mut Vec<String>) {
    use diacraft_help::Target;
    for (path, topic) in topics {
        let own = topic.anchors();
        for (line, target) in topic.links() {
            let at = |what: String| format!("{path}:{line}: {what}");
            match target {
                Target::Topic { id, anchor } => {
                    if generated(id) {
                        if anchor.is_some() {
                            out.push(at(format!("`{id}` is made by the program and has no fixed anchors")));
                        }
                    } else if let Some((_, other)) = by_id.get(id).and_then(|&i| topics.get(i)) {
                        if let Some(anchor) = anchor
                            && !other.anchors().contains(&anchor.as_str())
                        {
                            out.push(at(format!("the topic `{id}` has no anchor `{anchor}`")));
                        }
                    } else {
                        out.push(at(format!("the link goes to the topic `{id}`, which does not exist")));
                    }
                }
                Target::Anchor(anchor) => {
                    if !own.contains(&anchor.as_str()) {
                        out.push(at(format!("this topic has no anchor `{anchor}`")));
                    }
                }
                Target::Control(id) => {
                    if known.controls.as_ref().is_some_and(|set| !set.contains(id)) {
                        out.push(at(format!("`ctl:{id}` is no control of the ribbon")));
                    }
                }
                Target::Command(id) => {
                    if known.commands.as_ref().is_some_and(|set| !set.contains(id)) {
                        out.push(at(format!("`cmd:{id}` is no registered command")));
                    }
                }
                Target::Key(_) | Target::Web(_) => {}
            }
        }
    }
}

/// An id that was published is still there.
fn check_published(tree: &Tree, by_id: &BTreeMap<String, usize>, out: &mut Vec<String>) {
    let Some(text) = &tree.published else { return };
    let path = format!("{}/published.txt", host::HELP_TREE);
    let mut seen = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        let id = line.trim();
        if id.is_empty() || id.starts_with('#') {
            continue;
        }
        if !seen.insert(id.to_owned()) {
            out.push(format!("{path}:{}: the id `{id}` is listed twice", n + 1));
        } else if !by_id.contains_key(id) && !generated(id) {
            out.push(format!("{path}:{}: the topic `{id}` was published and is gone: a published id never disappears, so restore the topic (it may move to another file)", n + 1));
        }
    }
}

/// The sections name topics that exist, and every topic is in a section or unlisted.
fn check_sections(tree: &Tree, topics: &[(String, Topic)], by_id: &BTreeMap<String, usize>, out: &mut Vec<String>) {
    let Some(text) = &tree.sections else { return };
    let path = format!("{}/{}", host::HELP_TREE, host::SECTIONS_FILE);
    let (sections, problems) = Sections::parse(text);
    out.extend(problems.iter().map(|p| describe(&path, p)));
    let mut listed = BTreeSet::new();
    for section in sections.sections() {
        for (id, line) in section.topics.iter().zip(&section.lines) {
            listed.insert(id.as_str());
            if !by_id.contains_key(id) && !generated(id) {
                out.push(format!("{path}:{line}: the section `{}` lists the topic `{id}`, which does not exist", section.title));
            }
        }
    }
    for (file, topic) in topics {
        if !listed.contains(topic.header.id.as_str()) && !topic.header.unlisted {
            out.push(format!(
                "{file}: the topic `{}` is in no section of {}: add a row there, or write `unlisted: yes` in its header",
                topic.header.id,
                host::SECTIONS_FILE
            ));
        }
    }
}

fn check_tips(tree: &Tree, known: &Known, out: &mut Vec<String>) {
    let Some(text) = &tree.tips else { return };
    let path = format!("{}/{}", host::HELP_TREE, host::TIPS_FILE);
    let (tips, problems) = Tips::parse(text);
    out.extend(problems.iter().map(|p| describe(&path, p)));
    let (Some(controls), Some(commands)) = (&known.controls, &known.commands) else { return };
    for tip in tips.rows() {
        if !controls.contains(&tip.key) && !commands.contains(&tip.key) {
            out.push(format!("{path}:{}: `{}` is no control of the ribbon and no registered command", tip.line, tip.key));
        }
    }
}

fn check_map(tree: &Tree, topics: &[(String, Topic)], by_id: &BTreeMap<String, usize>, known: &Known, out: &mut Vec<String>) {
    let Some(text) = &tree.map else { return };
    let path = format!("{}/{}", host::HELP_TREE, host::MAP_FILE);
    let (map, problems) = Map::parse(text);
    out.extend(problems.iter().map(|p| describe(&path, p)));
    let has = |set: &Option<BTreeSet<String>>, name: &str| set.as_ref().is_none_or(|s| s.contains(name));
    for row in map.rows() {
        let (known_place, label) = match &row.place {
            Place::Ribbon(id) => (has(&known.controls, id), "control of the ribbon"),
            Place::Group(id) => (has(&known.groups, id), "group of the ribbon"),
            Place::Tab(id) => (has(&known.tabs, id), "tab of the ribbon"),
            Place::Command(id) => (has(&known.commands, id), "registered command"),
            Place::Window(area) => (host::WINDOW_AREAS.contains(&area.as_str()), "area of the window"),
            Place::Dialog { id, tab } => {
                (has(&known.dialogs, id) && tab.as_ref().is_none_or(|t| has(&known.dialogs, &format!("{id}/{t}"))), "dialog or tab of a dialog")
            }
            Place::Pane(word) => (has(&known.panes, word), "pane"),
            Place::Backstage(page) => (has(&known.backstage, page), "page of the File view"),
        };
        if !known_place {
            out.push(format!("{path}:{}: the place `{}` is no {label}", row.line, place_text(&row.place)));
        }
        if let MapTarget::Topic { id, anchor } = &row.target {
            if generated(id) {
                if anchor.is_some() {
                    out.push(format!("{path}:{}: `{id}` is made by the program and has no fixed anchors", row.line));
                }
            } else if let Some((_, topic)) = by_id.get(id).and_then(|&i| topics.get(i)) {
                if let Some(anchor) = anchor
                    && !topic.anchors().contains(&anchor.as_str())
                {
                    out.push(format!("{path}:{}: the topic `{id}` has no anchor `{anchor}`", row.line));
                }
            } else {
                out.push(format!("{path}:{}: the topic `{id}` does not exist", row.line));
            }
        }
    }
}

/// A place as it is written in the map.
fn place_text(place: &Place) -> String {
    match place {
        Place::Ribbon(id) => format!("ribbon:{id}"),
        Place::Group(id) => format!("group:{id}"),
        Place::Tab(id) => format!("tab:{id}"),
        Place::Command(id) => format!("cmd:{id}"),
        Place::Window(area) => format!("window:{area}"),
        Place::Dialog { id, tab: Some(tab) } => format!("dialog:{id}/{tab}"),
        Place::Dialog { id, tab: None } => format!("dialog:{id}"),
        Place::Pane(word) => format!("pane:{word}"),
        Place::Backstage(page) => format!("backstage:{page}"),
    }
}

/// The sites a link may open are the site's own, and the feedback repository is its repository.
fn check_site(site: &SiteInfo, out: &mut Vec<String>) {
    let host_file = "crates/help/src/host.rs";
    let expected = [
        (host::WEB_PREFIXES.first().copied().unwrap_or_default().to_owned(), site.domain.trim_end_matches('/').to_owned(), "the website (`domain`)"),
        (host::WEB_PREFIXES.get(1).copied().unwrap_or_default().to_owned(), site.app_url.trim_end_matches('/').to_owned(), "the web app (`app_url`)"),
        (
            host::WEB_PREFIXES.get(2).copied().unwrap_or_default().to_owned(),
            format!("https://github.com/{}", site.repository),
            "the public repository (`repository`)",
        ),
        (host::FEEDBACK_REPOSITORY.to_owned(), site.repository.clone(), "the feedback repository (`repository`)"),
    ];
    for (ours, theirs, what) in expected {
        if ours != theirs {
            out.push(format!("{host_file}: {what} is `{ours}` here and `{theirs}` in site/site.json"));
        }
    }
}

/// The words of every file follow the rules about names.
fn check_names(tree: &Tree, out: &mut Vec<String>) {
    for (path, text) in &tree.texts {
        for hit in brands::scan_text(path, text) {
            out.push(format!("{path}:{}: names \"{}\", which the help may not name (the exceptions are an exact text of PRODUCT_USES, and the site-only header fields `description` and `seo_title`)", hit.line, hit.term));
        }
    }
}
