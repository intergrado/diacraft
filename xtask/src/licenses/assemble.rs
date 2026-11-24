//! Turning what cargo says (which crates each program is built from, on each platform) and what is on disk (each crate's licence files) into the entries and
//! texts of the notices, and into the list of problems the check fails on. No command is run here, so the tests can feed it a made-up registry.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::classify::{class_of_id, classes, squash, standard_text, title};
use super::files::{Found, Place, discover, is_notice_name, read_listed};
use super::policy::{LISTED_FILES, NATIVE_CODE, REVIEWED_FILES, SKIPPED_FILES};
use super::spdx::{self, Expr};

/// The name in `Found::rel` of a text that no file of the crate carries: the standard text of the licence.
pub const STANDARD: &str = "standard text";

/// One package of the lock, as `cargo metadata` describes it.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    /// The licence expression of its manifest.
    pub license: Option<String>,
    pub authors: Vec<String>,
    pub repository: Option<String>,
    /// `registry+...` or `git+...`; none for a crate of this workspace or another one on a path.
    pub source: Option<String>,
    /// The folder of its manifest.
    pub dir: PathBuf,
    /// Part of this workspace: DiaCraft's own, under its own licence.
    pub own: bool,
    /// It has a build dependency on `cc` or `cmake`: it compiles C or C++.
    pub native: bool,
}

/// What `deny.toml` allows: the licences of everything, and more for a named crate.
#[derive(Debug, Clone, Default)]
pub struct Allowed {
    pub global: BTreeSet<String>,
    pub per_crate: BTreeMap<String, BTreeSet<String>>,
}

impl Allowed {
    /// Whether `krate` may be used under `id`.
    pub fn permits(&self, krate: &str, id: &str) -> bool {
        self.global.contains(id) || self.per_crate.get(krate).is_some_and(|set| set.contains(id))
    }
}

/// The crates one platform's programs are built from.
#[derive(Debug, Clone)]
pub struct Build {
    pub triple: String,
    pub label: String,
    /// For each program, its crates as (name, version).
    pub programs: Vec<(String, BTreeSet<(String, String)>)>,
    /// The crates of all the programs built together, which is what `cargo build -p a -p b` compiles.
    pub together: BTreeSet<(String, String)>,
}

/// Everything the notices are made from.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub packages: Vec<Package>,
    pub builds: Vec<Build>,
    pub allowed: Allowed,
}

impl Inputs {
    /// The inputs of one file of notices: the same packages and licences, and only the builds of the given target triples (the web build has a
    /// file of its own).
    pub fn only(&self, triples: &[&str]) -> Inputs {
        Inputs {
            packages: self.packages.clone(),
            builds: self.builds.iter().filter(|b| triples.contains(&b.triple.as_str())).cloned().collect(),
            allowed: self.allowed.clone(),
        }
    }
}

/// A crate in the notices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub expression: String,
    /// The licences DiaCraft uses it under.
    pub chosen: Vec<String>,
    pub link: String,
    pub notes: Vec<String>,
    /// The texts it is listed with, by their place in [`Assembly::texts`].
    pub texts: Vec<usize>,
    /// For each program, the platforms (places in [`Inputs::builds`]) whose build has the crate.
    pub used_in: Vec<(String, Vec<usize>)>,
}

/// A licence or notice text, once, with the crates that carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub title: String,
    pub body: String,
    /// The crates, as `name version`.
    pub users: BTreeSet<String>,
    /// The files it was read from (by their names in the crates), or [`STANDARD`] when no file was.
    pub files: BTreeSet<String>,
}

/// The result: what the file says, and what is wrong.
#[derive(Debug, Clone, Default)]
pub struct Assembly {
    pub entries: Vec<Entry>,
    pub texts: Vec<Text>,
    /// DiaCraft's own crates in the build, which have no entry (their licence is DiaCraft's).
    pub own: Vec<String>,
    /// What the check fails on.
    pub problems: Vec<String>,
    /// What is worth a look and does not fail the check.
    pub warnings: Vec<String>,
}

/// A text of a crate that was kept, with the licences it was found to be.
struct Kept {
    found: Found,
    classes: Vec<String>,
}

impl Kept {
    /// A licence text that no signature of this tool matches (a licence it has no words for, or a short note): it is listed as it is.
    fn is_unrecognised(&self) -> bool {
        self.classes.is_empty() && !is_notice_name(self.found.rel.rsplit('/').next().unwrap_or(&self.found.rel))
    }
}

/// Builds the entries and the texts, and checks the lists of `policy.rs` against them.
pub fn assemble(inputs: &Inputs) -> Assembly {
    assemble_with(inputs, true)
}

/// Builds the entries and the texts. With `check_lists` false, an entry of a list of `policy.rs` that matches nothing here is not called stale: the file this
/// is for covers only some of the builds, and the entry may belong to another (a crate that compiles C code into the desktop programs only). A crate that
/// compiles C code and is not named in `NATIVE_CODE` is a problem either way.
pub fn assemble_with(inputs: &Inputs, check_lists: bool) -> Assembly {
    let mut b = Builder { inputs, out: Assembly::default(), keys: BTreeMap::new(), policy_used: BTreeSet::new() };
    let needed: BTreeSet<&(String, String)> = inputs.builds.iter().flat_map(|build| build.together.iter()).collect();
    let known: BTreeSet<(&str, &str)> = inputs.packages.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect();
    for (name, version) in &needed {
        if !known.contains(&(name.as_str(), version.as_str())) {
            b.out.problems.push(format!("{name} {version}: `cargo tree` lists it and `cargo metadata` does not"));
        }
    }
    let mut packages: Vec<&Package> = inputs.packages.iter().filter(|p| needed.contains(&(p.name.clone(), p.version.clone()))).collect();
    packages.sort_by(|a, c| (&a.name, &a.version).cmp(&(&c.name, &c.version)));
    let mut native_seen: BTreeSet<String> = BTreeSet::new();
    for pkg in packages {
        if pkg.own {
            b.out.own.push(pkg.name.clone());
            continue;
        }
        if pkg.native {
            native_seen.insert(pkg.name.clone());
        }
        if let Some(entry) = b.entry_of(pkg) {
            b.out.entries.push(entry);
        }
    }
    b.check_policy_lists(&native_seen, check_lists);
    sort_texts(&mut b.out);
    b.out
}

/// Puts the texts in an order that does not depend on which crate was read first: by title, then by the first crate that carries each. The entries' places follow.
fn sort_texts(out: &mut Assembly) {
    let mut order: Vec<usize> = (0..out.texts.len()).collect();
    let key = |at: &usize| out.texts.get(*at).map(|t| (t.title.clone(), t.users.iter().next().cloned(), t.body.clone()));
    order.sort_by_key(key);
    let mut place = vec![0usize; order.len()];
    for (new, old) in order.iter().enumerate() {
        if let Some(slot) = place.get_mut(*old) {
            *slot = new;
        }
    }
    let sorted: Vec<Text> = order.iter().filter_map(|old| out.texts.get(*old).cloned()).collect();
    out.texts = sorted;
    for entry in &mut out.entries {
        entry.texts = entry.texts.iter().map(|old| place.get(*old).copied().unwrap_or(*old)).collect();
        entry.texts.sort_unstable();
    }
}

struct Builder<'a> {
    inputs: &'a Inputs,
    out: Assembly,
    /// The squashed text of each registered text, to its place in `out.texts`.
    keys: BTreeMap<String, usize>,
    /// The entries of the skip and review lists that matched a file.
    policy_used: BTreeSet<(String, String)>,
}

impl Builder<'_> {
    /// One crate: its licences chosen, its files found and kept, its texts registered.
    fn entry_of(&mut self, pkg: &Package) -> Option<Entry> {
        let who = format!("{} {}", pkg.name, pkg.version);
        let Some(expression) = pkg.license.clone() else {
            self.out.problems.push(format!("{who}: its manifest has no licence expression"));
            return None;
        };
        let expr = match spdx::parse(&expression) {
            Ok(expr) => expr,
            Err(e) => {
                self.out.problems.push(format!("{who}: {e}"));
                return None;
            }
        };
        let allowed = &self.inputs.allowed;
        let chosen = match spdx::choose(&expr, &|id| allowed.permits(&pkg.name, id)) {
            Ok(chosen) => chosen,
            Err(refused) => {
                let refused: BTreeSet<String> = refused.into_iter().collect();
                self.out.problems.push(format!(
                    "{who}: its licence `{expression}` offers nothing that deny.toml allows (not allowed: {})",
                    refused.into_iter().collect::<Vec<_>>().join(", ")
                ));
                return None;
            }
        };
        let files = self.files_of(pkg, &who)?;
        let mut kept = self.select(pkg, &who, &expr, &chosen, files);
        self.add_standard_texts(pkg, &who, &chosen, &mut kept);
        let mut texts: Vec<usize> = kept.into_iter().map(|k| self.register(&who, k, &chosen)).collect();
        texts.sort_unstable();
        texts.dedup();
        Some(Entry {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            expression,
            notes: notes_of(pkg, &chosen, &self.policy_used),
            chosen,
            link: link_of(pkg),
            texts,
            used_in: used_in(pkg, &self.inputs.builds),
        })
    }

    /// The licence files on disk for a crate, with the ones its policy lists added. A folder that cannot be read is a problem.
    fn files_of(&mut self, pkg: &Package, who: &str) -> Option<Vec<Found>> {
        let stop = checkout_root(pkg);
        let mut files = match discover(&pkg.dir, stop.as_deref()) {
            Ok(files) => files,
            Err(e) => {
                self.out.problems.push(format!("{who}: {e}"));
                return None;
            }
        };
        if let Some((_, rels)) = LISTED_FILES.iter().find(|(name, _)| *name == pkg.name) {
            match read_listed(&pkg.dir, rels) {
                Ok(listed) => files.extend(listed),
                Err(e) => {
                    self.out.problems.push(format!("{who}: {e}"));
                    return None;
                }
            }
        }
        Some(files)
    }

    /// Which of the files go in the notices.
    fn select(&mut self, pkg: &Package, who: &str, expr: &Expr, chosen: &[String], files: Vec<Found>) -> Vec<Kept> {
        let chosen_classes: BTreeSet<String> = chosen.iter().map(|id| class_of_id(id)).collect();
        let offered: BTreeSet<String> = spdx::ids(expr).iter().map(|id| class_of_id(id)).collect();
        let mut kept = Vec::new();
        for found in files {
            let name = found.rel.rsplit('/').next().unwrap_or(&found.rel).to_owned();
            let policy_key = (pkg.name.clone(), found.rel.clone());
            if SKIPPED_FILES.iter().any(|(krate, rel, _)| *krate == pkg.name && *rel == found.rel) {
                self.policy_used.insert(policy_key);
                continue;
            }
            if is_notice_name(&name) {
                // A NOTICE file is part of the Apache License's terms (4d), and the way a crate says what it has adapted from others. One of the repository that
                // holds the crate, for all its crates, is not part of the terms a crate is used under unless that is the Apache License.
                if found.place != Place::Above || chosen_classes.contains("Apache-2.0") {
                    kept.push(Kept { classes: Vec::new(), found });
                }
                continue;
            }
            let cs: Vec<String> = classes(&found.text).into_iter().map(str::to_owned).collect();
            if cs.is_empty() {
                self.out.warnings.push(format!("{who}: {} is a licence text this tool does not recognise; it is listed as it is", found.rel));
                kept.push(Kept { classes: cs, found });
            } else if cs.iter().any(|c| chosen_classes.contains(c)) {
                kept.push(Kept { classes: cs, found });
            } else if cs.iter().all(|c| offered.contains(c)) {
                // A licence the crate offers and DiaCraft does not use (the other half of `MIT OR GPL-2.0`).
            } else if cs.iter().any(|c| self.inputs.allowed.permits(&pkg.name, c)) {
                kept.push(Kept { classes: cs, found });
            } else if REVIEWED_FILES.iter().any(|(krate, rel, _)| *krate == pkg.name && *rel == found.rel) {
                self.policy_used.insert(policy_key);
                kept.push(Kept { classes: cs, found });
            } else {
                self.out.problems.push(format!(
                    "{who}: {} is {} text, which deny.toml does not allow (if it is not compiled into a program, or is reviewed, say so in xtask/src/licenses/policy.rs)",
                    found.rel,
                    cs.iter().map(|c| title(c)).collect::<Vec<_>>().join(" and ")
                ));
            }
        }
        kept
    }

    /// The text of each licence a crate is used under that none of its files carries: the standard text, or a problem.
    fn add_standard_texts(&mut self, pkg: &Package, who: &str, chosen: &[String], kept: &mut Vec<Kept>) {
        for id in chosen {
            let class = class_of_id(id);
            if kept.iter().any(|k| k.classes.contains(&class)) {
                continue;
            }
            match standard_text(&class, &holders_of(pkg)) {
                Some(text) => kept.push(Kept { classes: vec![class], found: Found { rel: STANDARD.to_owned(), text, place: Place::Own } }),
                // A licence this tool has no words for, and a licence file it could not tell: the file is listed, and the warning says so.
                None if kept.iter().any(Kept::is_unrecognised) => {}
                None => self.out.problems.push(format!("{who}: it ships no text of {id}, and there is no standard text of it to use in its place")),
            }
        }
    }

    /// Puts a text in the list once, and returns its place.
    fn register(&mut self, who: &str, kept: Kept, chosen: &[String]) -> usize {
        let name = kept.found.rel.rsplit('/').next().unwrap_or(&kept.found.rel).to_owned();
        let file = if kept.found.rel == STANDARD { STANDARD.to_owned() } else { name };
        let notice = is_notice_name(&file);
        let key = squash(&kept.found.text);
        let at = match self.keys.get(&key) {
            Some(at) => *at,
            None => {
                let class = kept.classes.iter().find(|c| chosen.iter().any(|id| class_of_id(id) == **c)).or_else(|| kept.classes.first());
                let heading = if notice { "Notice" } else { class.map_or("Licence text", |c| title(c)) };
                self.out.texts.push(Text {
                    title: heading.to_owned(),
                    body: kept.found.text.trim().to_owned(),
                    users: BTreeSet::new(),
                    files: BTreeSet::new(),
                });
                self.keys.insert(key, self.out.texts.len() - 1);
                self.out.texts.len() - 1
            }
        };
        if let Some(text) = self.out.texts.get_mut(at) {
            text.users.insert(who.to_owned());
            text.files.insert(file);
        }
        at
    }

    /// The lists of `policy.rs`: a native-code crate in a build must be listed, and an entry of a list that matched nothing is stale.
    fn check_policy_lists(&mut self, native_seen: &BTreeSet<String>, check_stale: bool) {
        for name in native_seen {
            if !NATIVE_CODE.iter().any(|(krate, _)| krate == name) {
                self.out.problems.push(format!(
                    "{name}: it compiles C or C++ code into a program; say what it compiles and where its notice is in NATIVE_CODE (xtask/src/licenses/policy.rs)"
                ));
            }
        }
        if !check_stale {
            return;
        }
        let in_build = |entries: &[Entry], name: &str| entries.iter().any(|e| e.name == name);
        for (krate, _) in NATIVE_CODE {
            if in_build(&self.out.entries, krate) && !native_seen.contains(*krate) {
                self.out.problems.push(format!("NATIVE_CODE lists {krate}, which has no build dependency on cc or cmake: remove the entry"));
            }
        }
        for (krate, rel, _) in SKIPPED_FILES.iter().chain(REVIEWED_FILES.iter()) {
            if in_build(&self.out.entries, krate) && !self.policy_used.contains(&((*krate).to_owned(), (*rel).to_owned())) {
                self.out.problems.push(format!("policy.rs lists {krate}/{rel}, which the crate does not have: remove the entry"));
            }
        }
    }
}

/// For a crate in a git checkout (`.../checkouts/<repository>/<revision>/crates/x`), the root of the checkout: its licence is kept there.
fn checkout_root(pkg: &Package) -> Option<PathBuf> {
    if !pkg.source.as_deref().is_some_and(|s| s.starts_with("git+")) {
        return None;
    }
    pkg.dir.ancestors().find(|a| a.parent().and_then(|p| p.parent()).and_then(|g| g.file_name()).is_some_and(|n| n == "checkouts")).map(PathBuf::from)
}

/// The copyright holders written on a standard text: the authors of the crate's manifest, without their addresses.
fn holders_of(pkg: &Package) -> String {
    let names: Vec<String> = pkg.authors.iter().map(|a| a.split('<').next().unwrap_or(a).trim().to_owned()).filter(|a| !a.is_empty()).collect();
    if names.is_empty() { format!("the authors of {}", pkg.name) } else { names.join(", ") }
}

/// What is said about a crate besides its licence.
fn notes_of(pkg: &Package, chosen: &[String], policy_used: &BTreeSet<(String, String)>) -> Vec<String> {
    let mut notes = Vec::new();
    if chosen.iter().any(|id| id == "MPL-2.0") {
        notes.push(format!(
            "Mozilla Public License 2.0: this crate is used as published, unmodified. Its source code is available from {}.",
            source_link(pkg)
        ));
    }
    if let Some((_, what)) = NATIVE_CODE.iter().find(|(name, _)| *name == pkg.name) {
        notes.push(format!("C code: it {what}."));
    }
    for (krate, rel, why) in REVIEWED_FILES {
        if *krate == pkg.name && policy_used.contains(&(pkg.name.clone(), (*rel).to_owned())) {
            notes.push(format!("{rel}: {why}."));
        }
    }
    notes
}

/// Where the source of a crate is: its page on crates.io, or its git address.
fn source_link(pkg: &Package) -> String {
    match pkg.source.as_deref() {
        Some(s) if s.starts_with("registry+") => format!("https://crates.io/crates/{}/{}", pkg.name, pkg.version),
        Some(s) if s.starts_with("git+") => s.trim_start_matches("git+").split(['?', '#']).next().unwrap_or(s).to_owned(),
        _ => pkg.repository.clone().unwrap_or_else(|| format!("https://crates.io/crates/{}", pkg.name)),
    }
}

/// The address listed beside a crate: its repository, else where its source is.
fn link_of(pkg: &Package) -> String {
    pkg.repository.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| source_link(pkg))
}

fn used_in(pkg: &Package, builds: &[Build]) -> Vec<(String, Vec<usize>)> {
    let key = (pkg.name.clone(), pkg.version.clone());
    // The programs in the order the first build lists them, so that the line does not depend on which build is read first.
    let order: Vec<&String> = builds.first().map(|b| b.programs.iter().map(|(p, _)| p).collect()).unwrap_or_default();
    let mut programs: Vec<(String, Vec<usize>)> = Vec::new();
    for program in order {
        let places: Vec<usize> = builds
            .iter()
            .enumerate()
            .filter(|(_, build)| build.programs.iter().any(|(p, crates)| p == program && crates.contains(&key)))
            .map(|(at, _)| at)
            .collect();
        if !places.is_empty() {
            programs.push((program.clone(), places));
        }
    }
    programs
}
