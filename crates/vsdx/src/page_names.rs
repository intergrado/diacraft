//! The page names a VSDX file is written with.
//!
//! **A page name the program that opens a VSDX file would rename is never written.** That program renames a page called "page" and digits (`page1`: the name an `.odg` gives its pages) and
//! a page whose name another page has, and gives the renamed page's `Name` and `NameU` different numbers. A drawing can hold such names (one read from an `.odg`, a `.diacraft` file or a
//! file of another program, or one made by a build before this check), so every writer that makes a page row gives the page the default form instead, **`Page-N` for the lowest N that no
//! page has**, the same in `Name` and in `NameU`, and says so in the save's warnings. The three writers (a drawing never saved, the model writer and a save into a file) all ask
//! [`keep_page_names`] for the drawing they write.
//!
//! * **Names are compared without regard to case** ([`diacraft_doc::page_name_key`]): no specification says how the program compares them, so a repeat in another case is a repeat.
//! * **A page keeps its name when the name stands.** The first page of a name keeps it, and a later one with the same name is renamed. A renamed page never takes a name that a page
//!   of the list keeps, whatever the order, so a page called `Page-1` after one called `page1` is not renamed for it.
//! * **A name a file already holds is left as it is** by a save into that file: the splice writes only what an edit changed, and a page whose names are those of the file's own model (the
//!   baseline) is not looked at (the reader keeps whatever names a file has, and a save with no edit writes what it read). A page that is new or renamed is.
//! * **`NameU` is held to the same rules.** A page whose `Name` stands but whose `NameU` does not (a page a person renamed keeps its universal name) gets a new `NameU` only.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Document, Page, is_page_and_digits, page_name_key};

use crate::page_follow::follow_renames;
use crate::page_hrefs::follow_hrefs;

/// The most pages the warning of a save lists by place and name: the count says the rest.
const SHOWN: usize = 5;

/// One page that was given a new name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renamed {
    /// The place of the page in the list the file is written with, counting from 1.
    pub place: usize,
    /// The new `Name`: `None` when the page keeps its `Name` and only its `NameU` is new.
    pub name: Option<String>,
    /// The new `NameU`.
    pub name_u: String,
}

/// A drawing whose pages have the names the program keeps, and the pages that were given them.
#[derive(Clone, Debug)]
pub struct KeptNames {
    /// The drawing to write: the one given, with the pages that were renamed replaced (the others are shared, not copied).
    pub doc: Document,
    /// The pages that were given a new name in the file, in order. A page that the file the save goes into holds under this very name is not among them.
    pub renamed: Vec<Renamed>,
    /// How many formulas that named a renamed page by the name it had were rewritten to name it by its new name (`page_follow.rs`), with their values kept. A formula about a page the file holds under
    /// its new name already is counted too.
    pub formulas: usize,
    /// How many links to a renamed page by its name were rewritten to name its new name: the `SubAddress` of a `Hyperlink` row (`page_follow.rs`) and an `.odg`'s page link, an
    /// `xlink:href` of the form `#<page>` that the model keeps as markup (`page_hrefs.rs`).
    pub links: usize,
}

impl KeptNames {
    /// What the save says: how many pages were given a new name, why, and the first few by their place. **No name of the drawing is in it**, only places and the default names.
    pub fn words(&self) -> String {
        let count = self.renamed.len();
        let what = if count == 1 { "1 page was".to_owned() } else { format!("{count} pages were") };
        let listed: Vec<String> = self
            .renamed
            .iter()
            .take(SHOWN)
            .map(|r| match &r.name {
                Some(name) => format!("page {} is now \"{name}\"", r.place),
                None => format!("page {} has the universal name \"{}\"", r.place, r.name_u),
            })
            .collect();
        let more = count.saturating_sub(SHOWN);
        let rest = if more > 0 { format!(", and {more} more") } else { String::new() };
        // The formulas that named such a page follow it, and keep their values: the person did not choose the name, so nothing may read `#REF!` for it.
        let follow = match self.formulas {
            0 => String::new(),
            1 => ". 1 formula that named such a page by its old name now names it by its new name and keeps its value".to_owned(),
            n => format!(". {n} formulas that named such a page by their old name now name it by its new name and keep their values"),
        };
        // The links to such a page by its name follow it as well.
        let links = match self.links {
            0 => String::new(),
            1 => ". 1 hyperlink that named such a page now names it by its new name".to_owned(),
            n => format!(". {n} hyperlinks that named such a page now name it by its new name"),
        };
        format!(
            "{what} given a new name in the file, because programs that open it rename a page called \"page\" and digits (page1) and a name another page has: {}{rest}{follow}{links}",
            listed.join(", ")
        )
    }
}

/// The drawing `doc` with the page names the program that opens a VSDX file keeps, or `None` when every page of it has such names already (the common case, which allocates nothing).
///
/// `baseline` is the drawing a file was read as, for a save into that file: a page of `doc` whose `Name` and `NameU` are those of the baseline's page with the same `ID` is the file's own and
/// stands as it is, whatever its names are. With `None` (a drawing never saved, a model written from the model alone) every page is looked at.
pub fn keep_page_names(doc: &Document, baseline: Option<&Document>) -> Option<KeptNames> {
    // The file's own page of each `ID` (the first, as every lookup finds it).
    let mut of_file: HashMap<u32, &Page> = HashMap::new();
    if let Some(base) = baseline {
        for page in base.pages.iter() {
            of_file.entry(page.id).or_insert(page);
        }
    }
    let stands_as_filed = |page: &Page| of_file.get(&page.id).is_some_and(|filed| filed.name == page.name && filed.name_u == page.name_u);
    // The keys of the names that stand, in `Name` and in `NameU`.
    let mut names: HashSet<String> = HashSet::new();
    let mut universal: HashSet<String> = HashSet::new();
    for page in doc.pages.iter().filter(|page| stands_as_filed(page)) {
        names.insert(page_name_key(&page.name));
        if let Some(u) = &page.name_u {
            universal.insert(page_name_key(u));
        }
    }
    // First pass, in order: the pages whose names stand keep them, so that a page renamed in the second never takes one of them. (`insert` says whether the key is new.) A page whose `Name` does
    // not stand is renamed whole, and its `NameU` does not stand for another page's either way; one whose `Name` stands and `NameU` does not gets a `NameU` only.
    let mut to_name: Vec<(usize, bool)> = Vec::new();
    for (at, page) in doc.pages.iter().enumerate() {
        if stands_as_filed(page) {
            continue;
        }
        if is_page_and_digits(&page.name) || !names.insert(page_name_key(&page.name)) {
            to_name.push((at, true));
            continue;
        }
        if let Some(u) = page.name_u.as_deref()
            && (is_page_and_digits(u) || !universal.insert(page_name_key(u)))
        {
            to_name.push((at, false));
        }
    }
    if to_name.is_empty() {
        return None;
    }
    // Second pass: each such page takes the lowest `Page-N` that no page has. The numbers only go up, as the sets only grow.
    let mut out = doc.clone();
    let mut renamed = Vec::new();
    // The new universal name of each page that was renamed, by its place, for the formulas that name it.
    let mut new_names: HashMap<usize, Arc<str>> = HashMap::new();
    let mut number = 0u64;
    for (at, name_goes) in to_name {
        let fresh = loop {
            number = number.saturating_add(1);
            let candidate = format!("Page-{number}");
            let key = page_name_key(&candidate);
            if !names.contains(&key) && !universal.contains(&key) {
                names.insert(key.clone());
                universal.insert(key);
                break candidate;
            }
        };
        let Some(slot) = out.pages.get_mut(at) else { continue };
        let page = Arc::make_mut(slot);
        // A page whose `Name` is renamed has the new name in both; one whose only `NameU` is has that, and keeps what a person called it.
        if name_goes {
            page.name = Arc::from(fresh.as_str());
        }
        page.name_u = Some(Arc::from(fresh.as_str()));
        new_names.insert(at, Arc::from(fresh.as_str()));
        // The file already holds the page under these names (a session whose drawing keeps the names the file was given): nothing was written, and nothing is said.
        let as_filed =
            doc.pages.get(at).and_then(|own| of_file.get(&own.id)).is_some_and(|filed| filed.name == page.name && filed.name_u == page.name_u);
        if !as_filed {
            renamed.push(Renamed { place: at + 1, name: name_goes.then(|| fresh.clone()), name_u: fresh });
        }
    }
    // The formulas that named a renamed page by the name it had name it by its new name (`page_follow.rs`).
    let followed = follow_renames(&mut out, doc, &new_names);
    // And the links to it by its name: a link of the drawing (the `SubAddress` of a `Hyperlink` row, in `follow_renames`) and an `.odg`'s page link (`page_hrefs.rs`).
    let links = followed.links.saturating_add(follow_hrefs(&mut out, doc));
    // (When every renamed page is as the file holds it, `renamed` is empty: the drawing to write is the renamed one, and there is nothing to say.)
    Some(KeptNames { doc: out, renamed, formulas: followed.formulas, links })
}
