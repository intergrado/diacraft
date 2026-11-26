//! DiaCraft's document-level file (design decisions 6 and 7; the proposal): the one file in the package, listed in the
//! manifest and in no other slot, that holds what is on no single shape: the document's own cells, the page cells the file's markup cannot say, the
//! masters with their bodies, the themes, the colour table, the Snap & Glue words of `DocumentSettings` and DiaCraft's own data (the Snap & Glue switches, the stencil
//! list).
//!
//! **Where it is.** `DiaCraft/document.xml`, media type `application/vnd.diacraft.document+xml`. A LibreOffice save drops it with its manifest row
//! So a reader that finds none, or one that is stale or does not read, reads the drawing from its native markup and says nothing is
//! wrong (tier 3 is).
//!
//! **What is in it.** One root, `diacraft:document` with `version="2"` ([`VERSION`]), and children, each in DiaCraft's namespace: `data` (the element of DiaCraft's own data
//! the model keeps, as it was), `windows` (the windows part the model holds the window settings in, the grid, the rulers, the guides and the connection points,
//! whole, as a VSDX file's `windows.xml` holds them; a drawing whose settings were never set holds none, and opens as a new drawing shows), `words` (the four Snap & Glue words of `DocumentSettings`, as attributes `glue`, `snap`, `extensions` and
//! `dynamic-grid`, each present when the document has the word), `docsheet` and `page` (**only what differs from what the reader rebuilds**: cells and whole sections, and the names of what the
//! model lacks), `blob` (a picture a shape of a master shows: its number and its bytes as base64, once however many shapes show it), `master` (every master, with its body, in the
//! vocabulary of the whole-model codec: `body.rs`), `theme` (a part name and the bytes, base64) and `colors`. A child this build does not know,
//! or any other text of the file, is kept as it is on a save.
//!
//! **Versions**. Version 1 wrote the masters in the vocabulary of the `body.rs` before the codec (a `shapes` wrapper, a picture as base64 in its own
//! element, a kept element as literal XML) and the sheets of the entries in the sheet codec's own. No release wrote one. A file of version 1, or of any lower number, is **kept and not
//! read**: the drawing reads from its markup with one note (`diacraft_file_old`), an unedited save gives the file back byte for byte, and a save that has something to write
//! writes a new file of this version, which holds only what the model holds (what the old file held was never read). A file of a higher version has the names this build knows read,
//! with one note (`diacraft_version`), as it was.
//!
//! **Pages are found by name.** A `page` holds the page's `draw:name` and is put on the page of that name: a page a user renamed in another program loses
//! what the file says of it (and is read from its markup), and no page is given what another's file entry says.
//!
//! **A save writes no file the reader would refuse** (the report, concern 5). The file made is held to the parser's limits (the depth, the nodes of a tree and the widest
//! attribute, which a test compares with the parser at each boundary), to the bytes of a part of the package (64 MiB, and the container's cap when a caller lowers it), and **it is read as the
//! reader reads it before it is written** ([`ReadCheck`]): the reader's own functions charge its bytes, read its entries for groups and read the rest over the pages and style sheets the
//! markup gives after the save, with the masters read, charged and let go one at a time, within the items the rest of the file leaves the reader (its items for a drawing less what
//! the read of the file as it is cost without the document file, `ReadWork::docfile_items`, and the manifest row that lists a first document file). So it is refused, in words and with nothing written, where the reader would refuse it and for the
//! reason it would give: its items, its shapes and cells, its pictures' bytes, and more masters (65,536), themes (256) or colours (4,096) than a drawing holds, the last three said before a tree is made.

use std::sync::Arc;

use diacraft_container::XmlLimits;
use diacraft_container::xml::{El, Node};
use diacraft_doc::{ColorEntry, Document, Page, SharedVec, Sheet, SnapWords, Style, Theme};
use diacraft_sheetxml::model::BlobId;
use diacraft_sheetxml::model::MemBlobs;
use diacraft_sheetxml::passthrough::canonical_u32;
use diacraft_sheetxml::sheet_delta;

use super::body::{self, Sc};
use super::names::{self, Gone};
use crate::error::{LimitKind, OdgError};
use crate::package::Package;
use crate::read::Reader;
use crate::write::{Structure, WriteError, WriteWork};

/// The file's place in the package.
pub(crate) const PATH: &str = "DiaCraft/document.xml";
/// The media type the manifest gives it.
pub(crate) const MEDIA_TYPE: &str = "application/vnd.diacraft.document+xml";

/// The version of the document file this build writes and reads (its root's `version`): 2, the vocabulary of the whole-model codec for the masters and the sheets of the
/// entries. It is no tier 2's `version` (`names::VERSION`, which stays 1): that is the version of the attributes on a shape's style.
pub(crate) const VERSION: &str = "2";

/// How a document file's `version` stands to this build's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Age {
    /// The version this build writes.
    Current,
    /// A lower number: a vocabulary this build no longer reads.
    Old(u32),
    /// A higher number, or a word that is no number: the names this build knows are read.
    Other,
}

impl Age {
    /// The age of the version `root` states: a canonical decimal number below [`VERSION`] is old, [`VERSION`] itself is current, and anything else is another.
    pub(crate) fn of(root: &El) -> Age {
        let Some(version) = root.attr("version") else { return Age::Other };
        if version == VERSION {
            return Age::Current;
        }
        match (canonical_u32(version), canonical_u32(VERSION)) {
            (Some(n), Some(now)) if n < now => Age::Old(n),
            _ => Age::Other,
        }
    }
}

/// The limits the document file is parsed at, in the reader and in the writer's look at the file it replaces: the caller's, with the depth the codec needs. A master of the
/// codec holds its shapes nested to the model's cap and a kept element as deep as the codec writes one beside them (at most 407 elements, the design's NP6), which the
/// container's default of 200 refuses: the file is read at the container's ceiling, the deepest it reads at all, and never below what the caller allows.
pub(crate) fn parse_limits(limits: &crate::Limits) -> XmlLimits {
    let mut xml = limits.container.xml.clone();
    xml.max_depth = xml.max_depth.max(diacraft_container::limits::XML_DEPTH_CEILING);
    xml
}

/// The most themes a document file holds: the number the VSDX reader keeps (`vsdx::MAX_THEMES`) and the engine adds no more than. A file of more is refused where the
/// theme past the most stands, before it is read (a count of its own: until a later change only the file's size held them).
pub(crate) const MAX_THEMES: usize = 256;

/// The most tokens of a `gone` list of a page or the document that are read: four for each attribute a sheet may hold sections' worth of (the figure; tier 3 widened
/// a style's own cap for the fingerprints, not this one).
const MOST_GONE_TOKENS: usize = 4 * (diacraft_doc::MAX_SECTIONS_PER_SHEET + 16);

/// The children of the root this build writes.
const KNOWN: [&str; 12] = ["data", "windows", "words", "docsheet", "page", "group", "style", "blob", "master", "theme", "colors", super::items::ITEM];

/// A page as the file's native markup alone gives it: what the reader built before it put the document file over it.
#[derive(Clone, Debug, Default)]
pub(crate) struct NativePage {
    pub id: u32,
    pub name_u: Option<Arc<str>>,
    pub sheet: Arc<Sheet>,
}

/// A style sheet as the file's native markup alone gives it: the name the file holds it by (the reader's `NameU`) and its sheet, with the
/// links its parent gives it and the cells its properties give, and none of what the document file puts over it.
#[derive(Clone, Debug, Default)]
pub(crate) struct NativeStyle {
    pub name_u: Option<Arc<str>>,
    pub sheet: Arc<Sheet>,
}

/// What the document holds that the reader rebuilds from the native parts: the document's sheet, each page's id, name and sheet, and each style sheet's. The writer
/// compares the model with it to say what the document file holds.
#[derive(Clone, Debug, Default)]
pub(crate) struct Native {
    pub sheet: Arc<Sheet>,
    pub pages: Vec<NativePage>,
    pub styles: Vec<NativeStyle>,
    /// The `Layer` section a page that has no layer set of its own reads: the document's (`None` when it holds no row a page reads). A page the writer adds is written with no
    /// layer set, so this is the section its markup gives it, whatever the pages the file has read.
    pub new_page_layers: Option<Arc<diacraft_doc::Section>>,
}

// A read report is compared as a value in tests; this bookkeeping is no part of what the read found.
impl PartialEq for Native {
    fn eq(&self, _: &Native) -> bool {
        true
    }
}

impl Eq for Native {}

impl Native {
    /// The native markup of the document as it will be after a save that writes the model's pages' native cells (a page's size, margins and orientation are
    /// written into the layout, so the file's markup then gives the model's values), in the order of the model's pages: each page of the model is the page of
    /// the file it was (`st`), or a page the save adds, which takes what the first page of the file has beside its layout cells. A page is read under the `ID` of its place
    /// (the pages that are not backgrounds from 0, the background pages after them) and a page that is not a background under its `draw:name` as its universal name.
    pub(crate) fn after(&self, doc: &Document, st: &Structure) -> Native {
        let foreground = u32::try_from(doc.pages.iter().filter(|p| !p.background).count()).unwrap_or(u32::MAX);
        let (mut next_foreground, mut next_background) = (0u32, 0u32);
        let pages = doc
            .pages
            .iter()
            .enumerate()
            .map(|(at, model)| {
                let from = st.matched.get(at).copied().flatten().or(st.template).and_then(|h| self.pages.get(h));
                let id = if model.background {
                    next_background = next_background.saturating_add(1);
                    foreground.saturating_add(next_background - 1)
                } else {
                    next_foreground = next_foreground.saturating_add(1);
                    next_foreground - 1
                };
                let mut sheet = from.map(|p| Arc::clone(&p.sheet)).unwrap_or_default();
                // A page the save adds has no layer set of its own: its markup gives it the rows of the document's, and not those the first page of the file reads (which
                // are its own when it has a layer set of its own).
                if st.matched.get(at).copied().flatten().is_none() {
                    let key = diacraft_doc::SectionKey::new("Layer", None);
                    if sheet.section(&key) != self.new_page_layers.as_ref() {
                        let s = Arc::make_mut(&mut sheet);
                        s.sections.retain(|sec| !(sec.name == diacraft_doc::SectionName::new("Layer") && sec.ix.is_none()));
                        if let Some(rows) = &self.new_page_layers {
                            s.sections.push(Arc::clone(rows));
                        }
                    }
                }
                for name in crate::write::PAGE_CELL_NAMES {
                    let now = model.sheet.cells.get(name);
                    if sheet.cells.get(name) != now {
                        let s = Arc::make_mut(&mut sheet);
                        match now {
                            Some(c) => {
                                s.cells.insert(diacraft_doc::CellName::new(name), c.clone());
                            }
                            None => {
                                s.cells.remove(name);
                            }
                        }
                    }
                }
                let name_u = if model.background { from.and_then(|p| p.name_u.clone()) } else { Some(Arc::clone(&model.name)) };
                NativePage { id, name_u, sheet }
            })
            .collect();
        // A style sheet the model has that the file lacks is written as an empty common style (`write/newstyles.rs`): the file's markup alone gives it its links, and
        // the document file carries the rest of the sheet.
        let mut styles = self.styles.clone();
        // A style sheet of the file that the model no longer has is taken out of the file by the save (`write/newstyles.rs`): the markup after the save does not hold it.
        styles.truncate(doc.styles.len());
        // The names the new ones are written under (the model writer's, which are the name of the `NameU` where that is free and another where it is not) and the links the markup gives them: a style
        // whose links the markup can say (one style before it) has them as they are, and any other has the parent of its first link, the true ones riding in the entry.
        let from = styles.len();
        let mut taken: std::collections::HashSet<String> = styles.iter().filter_map(|s| s.name_u.as_deref().map(str::to_owned)).collect();
        // (The probes were charged by the plan, which made the same names.)
        let names = crate::write::newstyles::model_names(doc, from, &mut taken, &mut 0);
        // (A model whose `ID`s cannot be mapped is refused by the plan of the styles, which asks for the same map.)
        let places = crate::write::newstyles::places_of(doc).unwrap_or_default();
        for (offset, (at, style)) in doc.styles.iter().enumerate().skip(from).enumerate() {
            let name = style.name_u.as_deref().and_then(|_| names.get(offset)).cloned();
            let sheet = Sheet { links: crate::write::newstyles::markup_links(doc, at, &places), ..Sheet::default() };
            styles.push(NativeStyle { name_u: name.map(|n| Arc::from(n.as_str())), sheet: Arc::new(sheet) });
        }
        Native { sheet: Arc::clone(&self.sheet), pages, styles, new_page_layers: self.new_page_layers.clone() }
    }
}

fn el(name: &str, attrs: Vec<(String, String)>, children: Vec<Node>) -> El {
    El { name: format!("{}:{name}", body::P), attrs, children }
}

// ------------------------------------------------------------------------------------------------------------------ reading

/// Parses the document file of `package`, if it has one, **before the pages are read** (a group's data is applied when the group is built, so the file is
/// read ahead of the pages and kept for the apply that follows). The file's size is charged here, once, as a backstop under the charge of each string. A file that
/// is not there, is not XML or is not a document of this namespace gives `None`, and is no error: the drawing is read from its markup (a warning says so when the
/// file is there).
pub(crate) fn parse(r: &mut Reader<'_>, package: &Package) -> Result<Option<El>, OdgError> {
    counted(r, |r| parse_part(r, package))
}

/// Runs `read`, and counts what it charged the reader as what DiaCraft's document file cost it ([`crate::report::ReadWork::docfile_items`]): the writer asks for it, to know what
/// the rest of the file leaves of the reader's items for a document file it is about to write.
fn counted<T>(r: &mut Reader<'_>, read: impl FnOnce(&mut Reader<'_>) -> T) -> T {
    let before = r.report.work.items;
    let out = read(r);
    r.report.work.docfile_items = r.report.work.docfile_items.saturating_add(r.report.work.items.saturating_sub(before));
    out
}

fn parse_part(r: &mut Reader<'_>, package: &Package) -> Result<Option<El>, OdgError> {
    let Some(bytes) = package.part(PATH) else { return Ok(None) };
    // A backstop under the charge of each string: whatever a branch of the reader forgets to charge, the file is not past what its own size says, and the
    // budget it takes is no less than the bytes the file has.
    r.take_model_bytes(bytes.len())?;
    let mut limits = parse_limits(r.limits);
    limits.max_nodes = limits.max_nodes.min(r.limits.max_read_xml_nodes);
    let mut work = diacraft_container::Work::default();
    let parsed = diacraft_container::xml::parse_counted(bytes, &limits, &mut work);
    r.report.work.package.xml_events = r.report.work.package.xml_events.saturating_add(work.xml_events);
    let Ok(root) = parsed else {
        r.warn("diacraft_file_unreadable", || "DiaCraft's document file is not XML, and is left unread".to_owned());
        return Ok(None);
    };
    let sc = Sc::empty().enter(&root);
    if !sc.is(&root, "document") {
        r.warn("diacraft_file_unreadable", || "DiaCraft's document file is not a document of DiaCraft's namespace, and is left unread".to_owned());
        return Ok(None);
    }
    match Age::of(&root) {
        Age::Current => {}
        // A vocabulary this build no longer reads: the file is kept as it is, and the drawing reads from its markup.
        Age::Old(n) => {
            r.warn("diacraft_file_old", || {
                format!("DiaCraft's document file is of version {n}, older than version {VERSION}, the one this build reads: it is kept as it is and left unread")
            });
            return Ok(None);
        }
        Age::Other => r.warn_once("diacraft_version", || {
            "DiaCraft's document file is of a version this build does not read: the names it knows are read".to_owned()
        }),
    }
    Ok(Some(root))
}

/// What the document file says of one group: its `ID` when it keeps one, its links to a master (a group dropped from a master of several shapes is an instance
/// of it, and a group inside one stands for a shape of it), and the cells, whole sections and removals that are put over what the markup gave.
pub(crate) struct GroupEntry {
    pub id: Option<u32>,
    pub master: Option<u32>,
    pub master_shape: Option<u32>,
    pub cells: diacraft_doc::Cells,
    pub sections: Vec<Arc<diacraft_doc::Section>>,
    pub gone: Vec<Gone>,
    /// The VSDX data the group carries: its shell and the stamp it is judged by.
    pub item: Option<super::items::GroupItem>,
    /// Whether a `group` element made the entry; false for one made for an item alone.
    pub entry: bool,
}

impl GroupEntry {
    /// Whether the entry holds nothing but the VSDX data of a group (it was made for an item and no entry of the file's own named the group): an item whose group is gone is dropped silently.
    pub(crate) fn is_item_only(&self) -> bool {
        !self.entry
    }
}

/// The `group` entries of the file, by the page they are for and then by the group (Spec question 1, ruled): a page is found by
/// its `draw:name` and its number among the pages of that name, a group by its `draw:name` (empty for none) and, **when the entry has an `nth`**, its number among the
/// groups of that name on the page, counted in the pre-order of the page, members of groups included. **An entry with no `nth` is keyed by the name alone, which the
/// writer saw unique on the page: it binds to a group only while exactly one group of the page has that name** ([`Binding`]; the review's reader side).
/// **The first entry for a key stands**; the rest are read, charged, and left out.
#[derive(Default)]
pub(crate) struct Groups {
    pages: std::collections::HashMap<(String, u32), PageGroups>,
    /// How many pages of each name have been read, so that the next one is told apart from them.
    seen: std::collections::HashMap<String, u32>,
    /// How many entries of the pages read no group of the page took (a group renamed or taken away by another program): fix round.
    left_over: usize,
}

/// The entries one page's groups are looked up in: by the group's name and the `nth` its entry has (`None` for an entry keyed by its name alone).
pub(crate) type PageGroups = std::collections::HashMap<(String, Option<u32>), GroupEntry>;

/// How a group of a page finds its entry in [`PageGroups`]: by its number among the groups of its name (an entry the writer made for a group with no key of its own, which
/// has `nth` always), by its name alone (an entry with no `nth`, which binds only while the name is unique on the page), or not at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    No,
    Nth(u32),
    Name,
}

impl Binding {
    /// How the group called `name`, the `nth` of its name on the page, finds an entry in `groups`; `unique` is whether no other group of the page has the name. An entry
    /// with an `nth` is found first, then an entry with none, and that one only for a group that holds its name alone.
    pub(crate) fn of(groups: &PageGroups, name: &str, nth: u32, unique: bool) -> Binding {
        if groups.is_empty() {
            return Binding::No;
        }
        if groups.contains_key(&(name.to_owned(), Some(nth))) {
            Binding::Nth(nth)
        } else if unique && groups.contains_key(&(name.to_owned(), None)) {
            Binding::Name
        } else {
            Binding::No
        }
    }

    /// The key of the entry in `groups` for the group called `name`.
    fn key(self, name: &str) -> Option<(String, Option<u32>)> {
        match self {
            Binding::No => None,
            Binding::Nth(n) => Some((name.to_owned(), Some(n))),
            Binding::Name => Some((name.to_owned(), None)),
        }
    }

    /// The entry the group called `name` is bound to, still in `groups`.
    pub(crate) fn get<'g>(self, groups: &'g PageGroups, name: &str) -> Option<&'g GroupEntry> {
        self.key(name).and_then(|key| groups.get(&key))
    }

    /// The entry the group called `name` is bound to, taken out of `groups` (an entry is for one group).
    pub(crate) fn take(self, groups: &mut PageGroups, name: &str) -> Option<GroupEntry> {
        self.key(name).and_then(|key| groups.remove(&key))
    }
}

impl Groups {
    /// Takes the entries for the next page called `name`: it is the first of that name, or the one after those taken before it.
    pub(crate) fn take_page(&mut self, name: &str) -> PageGroups {
        let count = self.seen.entry(name.to_owned()).or_insert(0);
        *count = count.saturating_add(1);
        self.pages.remove(&(name.to_owned(), *count)).unwrap_or_default()
    }

    /// Notes that `n` entries of a page just read were taken by no group of it.
    pub(crate) fn leave(&mut self, n: usize) {
        self.left_over = self.left_over.saturating_add(n);
    }

    /// How many entries of the pages read no group took.
    pub(crate) fn left_over(&self) -> usize {
        self.left_over
    }

    /// How many entries are for pages the drawing does not have (those not taken).
    pub(crate) fn unclaimed(&self) -> usize {
        self.pages.values().flat_map(std::collections::HashMap::values).filter(|e| !e.is_item_only()).count()
    }
}

/// Reads the `group` entries of the file's root into [`Groups`], each charged as it is read: the entry's elements and attributes, the bytes of its two names, and what
/// its sheet holds (`delta_of`).
pub(crate) fn read_groups(r: &mut Reader<'_>, root: &El) -> Result<Groups, OdgError> {
    counted(r, |r| read_group_entries(r, root))
}

fn read_group_entries(r: &mut Reader<'_>, root: &El) -> Result<Groups, OdgError> {
    let mut groups = Groups::default();
    let sc = Sc::empty().enter(root);
    let mut repeated = false;
    for child in root.elements() {
        let csc = sc.enter(child);
        if !csc.is(child, "group") {
            continue;
        }
        r.saw_element();
        r.take_items(1 + child.attrs.len())?;
        let (page, name) = (child.attr("page").unwrap_or_default(), child.attr("name").unwrap_or_default());
        r.take_bytes(page.len().saturating_add(name.len()))?;
        let number = |attr: &str| child.attr(attr).and_then(diacraft_sheetxml::passthrough::canonical_u32).unwrap_or(1).max(1);
        // An entry with no `nth` is keyed by the name alone (the writer saw it unique); one with an `nth` that is no number is the first of its name, as it always was.
        let nth = child.attr("nth").map(|_| number("nth"));
        let (page_key, key) = ((page.to_owned(), number("page-nth")), (name.to_owned(), nth));
        let (cells, sections, gone) = delta_of(r, child, &csc)?;
        let number_of = |attr: &str| child.attr(attr).and_then(diacraft_sheetxml::passthrough::canonical_u32);
        let entry = GroupEntry {
            id: number_of("id"),
            master: number_of("master"),
            master_shape: number_of("master-shape"),
            cells,
            sections,
            gone,
            item: None,
            entry: true,
        };
        match groups.pages.entry(page_key).or_default().entry(key) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
            std::collections::hash_map::Entry::Occupied(_) => repeated = true,
        }
    }
    if repeated {
        r.warn_once("diacraft_group_repeated", || "DiaCraft's document file holds two entries for one group: the first is read".to_owned());
    }
    // The items of the groups: the VSDX data of a group, found by the same key as its entry; an item for a key with no entry makes an entry that holds nothing else.
    for child in root.elements() {
        let csc = sc.enter(child);
        if !csc.is(child, super::items::ITEM) || child.attr("of") != Some("group") {
            continue;
        }
        r.saw_element();
        r.take_items(1 + child.attrs.len())?;
        let (page, name) = (child.attr("page").unwrap_or_default(), child.attr("name").unwrap_or_default());
        r.take_bytes(page.len().saturating_add(name.len()))?;
        let number = |attr: &str| child.attr(attr).and_then(diacraft_sheetxml::passthrough::canonical_u32).unwrap_or(1).max(1);
        let nth = child.attr("nth").map(|_| number("nth"));
        let (page_key, key) = ((page.to_owned(), number("page-nth")), (name.to_owned(), nth));
        let Some(item) = super::items::read_group_item(r, child)? else { continue };
        let entry = groups.pages.entry(page_key).or_default().entry(key).or_insert_with(|| GroupEntry {
            id: None,
            master: None,
            master_shape: None,
            cells: diacraft_doc::Cells::new(),
            sections: Vec::new(),
            gone: Vec::new(),
            item: None,
            entry: false,
        });
        if entry.item.is_none() {
            entry.item = Some(item);
        } else {
            r.report.carry.left_out = r.report.carry.left_out.saturating_add(1);
            r.warn_once("diacraft_item_ambiguous", || "DiaCraft's document file has two items for one group: the first is read".to_owned());
        }
    }
    Ok(groups)
}

/// Reads the document file over `doc` (what `root` holds, parsed by [`parse`]): and gives what the document was before (the native markup's own). The `group`
/// entries were read before the pages ([`read_groups`]) and are none of this.
pub(crate) fn apply(r: &mut Reader<'_>, doc: &mut Document, root: Option<&mut El>, package: Option<&Package>) -> Result<Native, OdgError> {
    counted(r, |r| apply_to(r, doc, root, package, true))
}

/// What a `style` entry says of the style sheet's identity besides its cells: the `NameU` (`name-u`), the shown name of the root (`display`) and the links the markup does not give (a style whose
/// three links name different style sheets: `line-style`, `fill-style` and `text-style`, `-` for none, else the number of the style). Each string is charged before it is kept.
fn put_style_identity(r: &mut Reader<'_>, style: &mut diacraft_doc::Style, child: &El) -> Result<(), OdgError> {
    if let Some(name_u) = child.attr("name-u") {
        r.take_bytes(name_u.len())?;
        style.name_u = Some(Arc::from(name_u));
    }
    if let Some(shown) = child.attr("display") {
        r.take_bytes(shown.len())?;
        style.name = Some(Arc::from(shown));
    }
    let link = |key: &str, now: Option<u32>| match child.attr(key) {
        Some("-") => None,
        Some(text) => diacraft_sheetxml::passthrough::canonical_u32(text).or(now),
        None => now,
    };
    let links = style.sheet.links;
    let wanted =
        diacraft_doc::Links { line: link("line-style", links.line), fill: link("fill-style", links.fill), text: link("text-style", links.text) };
    if wanted != links {
        Arc::make_mut(&mut style.sheet).links = wanted;
    }
    Ok(())
}

/// [`apply`], and `keep` says whether the masters it reads are put in the document: a writer that only asks what the reader would charge for a file does not keep them (they are read, charged, and let go
/// one at a time), so that what it holds beside the model it saves is one master, and not a second copy of all of them.
fn apply_to(r: &mut Reader<'_>, doc: &mut Document, root: Option<&mut El>, package: Option<&Package>, keep: bool) -> Result<Native, OdgError> {
    // The `ID`s of the style sheets, before anything is read over them: from here on every number of the model is an `ID`, and what the markup alone gives (`native`) holds them.
    // `keep` is false for the document the writer reads a file over, which holds the `ID`s already.
    // The style sheets an entry put an earlier ODF version's forms back on: a second entry for one is let be.
    let mut older_done: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut style_at: Option<std::collections::HashMap<String, usize>> = match root.as_deref() {
        Some(root) => styleids::restore(r, doc, root, keep)?,
        None => None,
    };
    let native = Native {
        sheet: Arc::clone(&doc.sheet),
        pages: doc.pages.iter().map(|p| NativePage { id: p.id, name_u: p.name_u.clone(), sheet: Arc::clone(&p.sheet) }).collect(),
        styles: doc.styles.iter().map(|s| NativeStyle { name_u: s.name_u.clone(), sheet: Arc::clone(&s.sheet) }).collect(),
        new_page_layers: None,
    };
    let Some(root) = root else { return Ok(native) };
    let sc = Sc::empty().enter(root);
    // The pictures first, whatever their place among the children: a master names the one it shows by its number.
    let pictures = body::read_pictures(r, root, &sc)?;
    let mut page_at: Option<std::collections::HashMap<String, Vec<usize>>> = None;
    let mut masters = doc.masters.len();
    // The children are visited by `iter_mut`: a master is given the declaration of the namespace in place, when it is reached (`body::read_master`).
    for node in &mut root.children {
        let Node::El(child) = node else { continue };
        let csc = sc.enter(child);
        // A blob was read, and charged, with the list of them.
        if csc.is(child, "blob") {
            continue;
        }
        r.saw_element();
        r.take_items(1 + child.attrs.len())?;
        if csc.is(child, "data") {
            if let Some(inner) = child.elements().next() {
                // The nodes of the subtree, and the bytes of the names, values and texts in it, before it is copied.
                r.take_items(diacraft_sheetxml::passthrough::element_cost(inner))?;
                r.take_bytes(diacraft_sheetxml::passthrough::element_bytes(inner))?;
                let mut data = diacraft_sheetxml::passthrough::raw_element(inner);
                // Data a build made before the id moved to `diacraft.app` wrote is held as the data this build writes (APPID): the namespace it declares is the current one.
                diacraft_doc::use_current_namespace(&mut data);
                doc.diacraft = Some(Arc::new(data));
            }
        } else if csc.is(child, "windows") {
            // The window settings: the windows part, whole, charged as `data` is before it is copied, and held where the model keeps it.
            if let Some(inner) = child.elements().next() {
                r.take_items(diacraft_sheetxml::passthrough::element_cost(inner))?;
                r.take_bytes(diacraft_sheetxml::passthrough::element_bytes(inner))?;
                let windows = Arc::new(diacraft_sheetxml::passthrough::raw_element(inner));
                Arc::make_mut(doc.vsdx.get_or_insert_with(Default::default)).windows = Some(windows);
            }
        } else if csc.is(child, "words") {
            // The Snap & Glue words of `DocumentSettings` (the flags no item of LibreOffice holds): each is text, charged by its bytes before it is copied, and put
            // where the model keeps them (`Document::set_snap_words`). A word a later element names again replaces the earlier one.
            let mut words = doc.snap_words();
            for (name, slot) in [
                ("glue", &mut words.glue),
                ("snap", &mut words.snap),
                ("extensions", &mut words.extensions),
                ("dynamic-grid", &mut words.dynamic_grid),
            ] {
                if let Some(text) = child.attr(name) {
                    r.take_bytes(text.len())?;
                    *slot = Some(Arc::from(text));
                }
            }
            doc.set_snap_words(&words);
        } else if csc.is(child, "docsheet") {
            let (cells, sections, gone) = delta_of(r, child, &csc)?;
            let sheet = Arc::make_mut(&mut doc.sheet);
            charge_overlay(r, sheet, &sections, &gone)?;
            super::payload::overlay(sheet, cells, sections, &gone)?;
        } else if csc.is(child, "page") {
            let name = child.attr("name").unwrap_or_default();
            // The pages by their names, indexed once (the places of the pages of one name, in order), so that an entry for each of many pages is no search of them
            // all for each entry: the index is charged for what it holds, before it is made. An entry is for the `nth` page of its name (the first when it
            // says no more), which is how two pages of one name are told apart.
            if page_at.is_none() {
                r.take_items(doc.pages.len())?;
            }
            let index = page_at.get_or_insert_with(|| {
                let mut by_name: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
                for (at, p) in doc.pages.iter().enumerate() {
                    by_name.entry(p.name.to_string()).or_default().push(at);
                }
                by_name
            });
            let nth = child.attr("nth").and_then(diacraft_sheetxml::passthrough::canonical_u32).unwrap_or(1);
            let at = index.get(name).and_then(|places| places.get((nth as usize).saturating_sub(1))).copied();
            let (cells, sections, gone) = delta_of(r, child, &csc)?;
            match at {
                Some(at) => {
                    if let Some(page) = doc.pages.get_mut(at).map(Arc::make_mut) {
                        if let Some(id) = child.attr("id").and_then(diacraft_sheetxml::passthrough::canonical_u32) {
                            page.id = id;
                        }
                        if let Some(name_u) = child.attr("name-u") {
                            r.take_bytes(name_u.len())?;
                            page.name_u = Some(Arc::from(name_u));
                        }
                        let sheet = Arc::make_mut(&mut page.sheet);
                        charge_overlay(r, sheet, &sections, &gone)?;
                        super::payload::overlay(sheet, cells, sections, &gone)?;
                    }
                }
                None => r.warn("diacraft_page_unknown", || {
                    "DiaCraft's document file holds data for a page the drawing no longer has by that name".to_owned()
                }),
            }
        } else if csc.is(child, "style") {
            // A style sheet's entry: what the sheet holds that the file's common style of that name does not say, put over the sheet the reader
            // built from it. The styles are indexed by their names once, charged for what the index holds, before it is made; an entry names its style by the name the
            // file holds it by (the reader's `NameU`), and gives the `NameU` back when it is another. The root is none of them (it is the default styles').
            if style_at.is_none() {
                r.take_items(doc.styles.len())?;
            }
            let index = style_at.get_or_insert_with(|| {
                let mut by_name: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for (at, s) in doc.styles.iter().enumerate().skip(1) {
                    if let Some(name) = s.name_u.as_deref() {
                        by_name.entry(name.to_owned()).or_insert(at);
                    }
                }
                by_name
            });
            // The root is keyed by its place (`root="1"`, the model writer carries its sheet), any other style by the name the file holds it by.
            let at = if child.attr("root").is_some() { Some(0) } else { child.attr("name").and_then(|name| index.get(name)).copied() };
            let (cells, sections, gone) = delta_of(r, child, &csc)?;
            match at.and_then(|at| doc.styles.get_mut(at)).map(Arc::make_mut) {
                Some(style) => {
                    put_style_identity(r, style, child)?;
                    put_older(r, style, child, &csc, at.unwrap_or_default(), &mut older_done)?;
                    let sheet = Arc::make_mut(&mut style.sheet);
                    charge_overlay(r, sheet, &sections, &gone)?;
                    super::payload::overlay(sheet, cells, sections, &gone)?;
                }
                None => r.warn("diacraft_style_unknown", || {
                    "DiaCraft's document file holds data for a style sheet the drawing no longer has by that name".to_owned()
                }),
            }
        } else if csc.is(child, "master") {
            if masters >= diacraft_doc::MAX_MASTERS {
                return Err(OdgError::TooComplex { what: "masters", max: diacraft_doc::MAX_MASTERS as u64 });
            }
            if let Some(master) = body::read_master(r, child, &pictures)? {
                masters += 1;
                if keep {
                    doc.masters.push(master);
                }
            }
        } else if csc.is(child, "theme") {
            if doc.themes.len() >= MAX_THEMES {
                return Err(OdgError::TooComplex { what: "themes", max: MAX_THEMES as u64 });
            }
            let encoded = child.text();
            r.take_model_bytes(encoded.len())?;
            match body::base64_decoded(&encoded) {
                Some(bytes) => {
                    let part = child.attr("part").unwrap_or_default();
                    r.take_bytes(part.len())?;
                    doc.themes.push(Arc::new(Theme { part: Arc::from(part), bytes: Arc::from(bytes), ..Theme::default() }));
                }
                None => r.warn("diacraft_file_unreadable", || "a theme in DiaCraft's document file is not base64, and is left out".to_owned()),
            }
        } else if csc.is(child, "colors") {
            let mut colors: Vec<ColorEntry> = Vec::new();
            for c in child.elements() {
                if let (Some(ix), Some(rgb)) = (c.attr("ix").and_then(diacraft_sheetxml::passthrough::canonical_u32), c.attr("rgb")) {
                    if colors.len() >= diacraft_doc::MAX_COLOR_ENTRIES {
                        return Err(OdgError::TooComplex { what: "colours", max: diacraft_doc::MAX_COLOR_ENTRIES as u64 });
                    }
                    r.take_items(2)?;
                    r.take_bytes(rgb.len())?;
                    colors.push(ColorEntry { ix, rgb: Arc::from(rgb) });
                }
            }
            doc.colors = Arc::new(colors);
        }
    }
    renumber_back_pages(r, doc, &native)?;
    // The items of the pages' and the styles' VSDX data: judged against the pages and styles the entries above made. The document's and its parts' are read after its properties are.
    let _ = (package, keep);
    super::items::read_early(r, doc, root)?;
    Ok(native)
}

/// A page names its background page by that page's `ID` (`BackPage`), which the reader numbered from the places the pages have; an entry of the file may give a page another
/// `ID` (a page taken away before it, a page put in another place), and the page that names it is then to name it by the `ID` it has now.
fn renumber_back_pages(r: &mut Reader<'_>, doc: &mut Document, native: &Native) -> Result<(), OdgError> {
    if !doc.pages.iter().zip(native.pages.iter()).any(|(p, n)| p.id != n.id) {
        return Ok(());
    }
    r.take_items(doc.pages.len().saturating_mul(2))?;
    let now: std::collections::HashMap<u32, u32> = native.pages.iter().zip(doc.pages.iter()).map(|(n, p)| (n.id, p.id)).collect();
    for at in 0..doc.pages.len() {
        let Some(back) = doc.pages.get(at).and_then(|p| p.back_page) else { continue };
        let Some(&id) = now.get(&back) else { continue };
        if id != back
            && let Some(page) = doc.pages.get_mut(at)
        {
            Arc::make_mut(page).back_page = Some(id);
        }
    }
    Ok(())
}

/// Charges an overlay by the work it walks, before it walks it: the sections that are gone are taken out in one pass over the
/// sheet's sections, and the sections that are put are found by an index of the sheet's keys made in another, so each pass is one item for each section the sheet
/// holds. What an entry puts is charged as it is read, and a page that many entries name is walked once for each: without this charge a file inside every cap
/// made work in the square of what it counted.
pub(crate) fn charge_overlay(r: &mut Reader<'_>, sheet: &Sheet, sections: &[Arc<diacraft_doc::Section>], gone: &[Gone]) -> Result<(), OdgError> {
    let passes = usize::from(gone.iter().any(|g| matches!(g, Gone::Section(_)))) + usize::from(!sections.is_empty());
    r.take_items(sheet.sections.len().saturating_mul(passes))
}

/// What an element of the document file holds of a sheet: its cells, its sections whole, and what is gone.
type Held = (diacraft_doc::Cells, Vec<Arc<diacraft_doc::Section>>, Vec<Gone>);

/// The cells, sections and removals an element holds: its `sheet` child and its `gone` attribute. **A page cell is none of them**:
/// `PageWidth` and the rest of the page's layout are what the file's markup says (a save writes them into the layout, from the model), so an entry of the document
/// file that names one, or says it gone, is ignored: read, it made the model say a size the layout did not, and an untouched save was an edit that dropped the file.
fn delta_of(r: &mut Reader<'_>, el: &El, sc: &Sc) -> Result<Held, OdgError> {
    let mut sheet = Sheet::default();
    for child in el.elements() {
        let csc = sc.enter(child);
        // A sheet the codec finds damaged is left out with a word, and the entry holds what its attributes say.
        if csc.is(child, "sheet")
            && let Some(read) = body::read_sheet(r, child)?
        {
            sheet = read;
        }
    }
    let tokens = el.attr("gone").unwrap_or_default();
    r.take_items(tokens.split_ascii_whitespace().count().min(MOST_GONE_TOKENS))?;
    let gone = tokens
        .split_ascii_whitespace()
        .take(MOST_GONE_TOKENS)
        .filter_map(Gone::parse)
        .filter(|g| !matches!(g, Gone::Cell(name) if crate::write::PAGE_CELL_NAMES.contains(&name.as_str())))
        .collect();
    let mut cells = sheet.cells;
    for name in crate::write::PAGE_CELL_NAMES {
        cells.remove(name);
    }
    Ok((cells, sheet.sections, gone))
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// The words of the refusal of a save that would hold more memory than it may for the trees of the document file.
const WORDS_MEMORY: &str = "bytes of memory a save holds (the trees of DiaCraft's document file, counted from the models before they are made)";

/// The memory a save holds for the trees the document file's writer makes: each tree's is taken from the save's room ([`WriteWork::hold`]) **before** the tree is made, and
/// all of it is given back together once the file's bytes are made. A comparison of two models by the elements they would make holds nothing, since it saves nothing
/// ([`Gate::comparing`]).
struct Gate<'w> {
    work: Option<&'w mut WriteWork>,
    taken: u64,
    /// The comparison of two models ([`Gate::comparing`]) takes the sections of a sheet in the order of their names, which no sheet's meaning depends on.
    by_name: bool,
}

impl<'w> Gate<'w> {
    fn new(work: &'w mut WriteWork) -> Gate<'w> {
        Gate { work: Some(work), taken: 0, by_name: false }
    }

    /// A gate that holds nothing and compares nothing: what a test writes a tree with (a comparison is [`Gate::comparing`]).
    #[cfg(test)]
    fn none() -> Gate<'static> {
        Gate { work: None, taken: 0, by_name: false }
    }

    /// The gate of the comparison of two models by the elements they would make (`same`): it holds nothing, and **it takes a sheet's sections in the order of their names**:
    /// a sheet is matched by section key and never by place, and the order of the sections of a sheet is the file's own (a VSDX file's page sheet with a `User` section before its `Layer` section, which
    /// the reader of the `.odg` the save wrote gives back with the `Layer` section first). Compared in the order each model has them, an unedited save of such a drawing was an edit.
    fn comparing() -> Gate<'static> {
        Gate { work: None, taken: 0, by_name: true }
    }

    /// Takes `bytes` of the room a save has left, or refuses the save in words.
    fn take(&mut self, bytes: u64) -> Result<(), WriteError> {
        if let Some(work) = self.work.as_deref_mut() {
            work.hold(bytes, WORDS_MEMORY)?;
            self.taken = self.taken.saturating_add(bytes);
            work.docfile_held = work.docfile_held.max(self.taken);
        }
        Ok(())
    }

    /// Whether the caller chose to leave out the macro projects of the other formats (`WriteOptions::drop_macros`).
    fn drop_macros(&self) -> bool {
        self.work.as_deref().is_some_and(|w| w.drop_macros)
    }

    /// Whether the save is the model writer's (a drawing with no file of its own, written from the model alone).
    fn model_path(&self) -> bool {
        self.work.as_deref().is_some_and(|w| w.model_path)
    }

    /// What the model writer made of the whole model's data, when the model this save is given is one it took that data out of.
    fn built(&self) -> Option<Arc<super::items::Built>> {
        self.work.as_deref().and_then(|w| w.built.clone())
    }

    /// Keeps the entries the document file's items name, for the save to put in the package.
    fn carried(&mut self, blobs: Vec<super::items::Blob>, built: &super::items::Built) {
        if let Some(work) = self.work.as_deref_mut() {
            work.carried = blobs;
            work.carried_classes = built.classes.clone();
            work.carried_macros = built.macros;
            work.carried_objects = built.objects;
            work.macros_dropped.clone_from(&built.macros_dropped);
        }
    }

    /// One more tree of a master was made.
    fn made(&mut self) {
        if let Some(work) = self.work.as_deref_mut() {
            work.docfile_trees = work.docfile_trees.saturating_add(1);
        }
    }

    /// Gives back what was taken.
    fn release(self) {
        if let Some(work) = self.work {
            work.release(self.taken);
        }
    }
}

/// The element that holds what `model` has that `native` lacks, under `attrs`, or `None` when it holds nothing and `attrs` say nothing.
fn delta_el(name: &str, mut attrs: Vec<(String, String)>, model: &Sheet, native: &Sheet, gate: &mut Gate<'_>) -> Result<Option<El>, WriteError> {
    let d = sheet_delta(model, native);
    let mut gone: Vec<Gone> = d.cells.removed.iter().map(|n| Gone::Cell(n.as_str().to_owned())).collect();
    gone.extend(d.removed_sections.iter().cloned().map(Gone::Section));
    gone.sort_by_cached_key(Gone::token);
    gone.dedup();
    let cells: diacraft_doc::Cells = d.cells.changed.iter().map(|(n, c)| ((*n).clone(), (*c).clone())).collect();
    let mut sections: Vec<Arc<diacraft_doc::Section>> = d.sections.iter().map(|s| Arc::new(s.section.clone())).collect();
    if gate.by_name {
        // (A stable sort: two sections of one key keep the order they have, which is how they are told apart.)
        sections.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()).then(a.ix.cmp(&b.ix)));
    }
    if cells.is_empty() && sections.is_empty() && gone.is_empty() && attrs.is_empty() {
        return Ok(None);
    }
    if !gone.is_empty() {
        attrs.push(("gone".to_owned(), gone.iter().map(Gone::token).collect::<Vec<_>>().join(" ")));
    }
    let mut children = Vec::new();
    if !cells.is_empty() || !sections.is_empty() {
        let delta = Sheet { cells, sections, ..Sheet::default() };
        // The tree of the sheet is held before it is made: its cells and sections, and its own element.
        gate.take(body::tree_bytes(&crate::figures::of_sheet(&delta), 1))?;
        children.push(Node::El(body::sheet_node(&delta)?));
    }
    Ok(Some(el(name, attrs, children)))
}

/// What the document file holds of `doc`, as the children of the root in the order written, given the native markup `native`. `Err` for what the file
/// cannot hold, in words. **The memory of each tree is held before the tree is made** ([`Gate`]): the masters' from the figures of their models, a sheet's from its own.
fn children(doc: &Document, native: &Native, steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Vec<El>, WriteError> {
    if doc.themes.len() > MAX_THEMES {
        return Err(WriteError::Unsupported("more themes than DiaCraft's document file holds (256, the most the VSDX reader keeps)"));
    }
    // The counts the reader refuses a file for (`apply`), said before a tree of the file is made: a model of more masters or colours than a drawing holds is no file.
    if doc.masters.len() > diacraft_doc::MAX_MASTERS {
        return Err(WriteError::Limit {
            what: "masters in DiaCraft's document file (the most a drawing holds, and the most the reader reads)",
            max: diacraft_doc::MAX_MASTERS as u64,
        });
    }
    if doc.colors.len() > diacraft_doc::MAX_COLOR_ENTRIES {
        return Err(WriteError::Limit {
            what: "colours in DiaCraft's document file (the most a drawing holds, and the most the reader reads)",
            max: diacraft_doc::MAX_COLOR_ENTRIES as u64,
        });
    }
    let mut out: Vec<El> = Vec::new();
    if let Some(data) = &doc.diacraft {
        out.push(el("data", Vec::new(), vec![Node::El(diacraft_sheetxml::passthrough::to_tree(data))]));
    }
    // The window settings: the windows part the model holds them in, whole, as `data` holds DiaCraft's own data. No item of `settings.xml` is read for
    // them yet (leaves `GridIsVisible` to a later task), so this file is their place in an `.odg`.
    if let Some(windows) = doc.vsdx.as_ref().and_then(|held| held.windows.as_deref()) {
        out.push(el("windows", Vec::new(), vec![Node::El(diacraft_sheetxml::passthrough::to_tree(windows))]));
    }
    out.extend(words_el(&doc.snap_words()));
    out.extend(small(doc, native, steps, gate)?);
    out.extend(style_entries(doc, native, gate)?);
    // The VSDX data an `.odg` has no place for, as items, and the entries of its bytes.
    let mut built = match gate.built() {
        Some(done) => (*done).clone(),
        None => super::items::build(doc, gate.drop_macros())?,
    };
    out.append(&mut built.elements);
    let blobs = std::mem::take(&mut built.blobs);
    gate.carried(blobs, &built);
    // The masters: their pictures first, as a list of the file's own, and then each master as the codec's element. The trees of all of them are held at once, so the
    // memory of all of them is held before the first is made.
    let figures = crate::figures::of_masters(doc.masters.iter());
    if !steps.take(figures.steps) {
        return Err(WriteError::Unsupported(WORDS_STEPS));
    }
    gate.take(body::tree_bytes(&figures, doc.masters.len() as u64))?;
    let mut pictures = MemBlobs::new();
    let mut masters: Vec<El> = Vec::new();
    // What the trees made will be as bytes is counted as each is made, and the loop stops at the cap of the file: a file past it is refused, so the trees of the masters
    // after the one that took it past are never made (a model of 12 masters of 100 MB each made 12 trees before the file was refused).
    let mut written = 0u64;
    for master in doc.masters.iter() {
        let tree = body::master_el(master, &mut pictures)?;
        written = written.saturating_add(el_bytes(&tree));
        if written > MOST_BYTES as u64 {
            return Err(WriteError::Unsupported(WORDS_MORE_THAN_THE_FILE_HOLDS));
        }
        masters.push(tree);
        gate.made();
    }
    // The pictures are base64 text in the file, once each: their size is known before the text is made.
    let picture_bytes = (0..pictures.len())
        .filter_map(|at| pictures.entry(BlobId(u32::try_from(at).ok()?)))
        .fold(0u64, |sum, (bytes, _)| sum.saturating_add((bytes.len() as u64).div_ceil(3).saturating_mul(4)));
    if written.saturating_add(picture_bytes) > MOST_BYTES as u64 {
        return Err(WriteError::Unsupported(WORDS_MORE_THAN_THE_FILE_HOLDS));
    }
    out.extend(body::blob_els(&pictures));
    out.extend(masters);
    for theme in doc.themes.iter() {
        out.push(el("theme", vec![("part".to_owned(), theme.part.to_string())], vec![Node::Text(body::base64_text(&theme.bytes))]));
    }
    if !doc.colors.is_empty() {
        let rows = doc
            .colors
            .iter()
            .map(|c| Node::El(el("color", vec![("ix".to_owned(), c.ix.to_string()), ("rgb".to_owned(), c.rgb.to_string())], Vec::new())));
        out.push(el("colors", Vec::new(), rows.collect()));
    }
    Ok(out)
}

/// The element that holds the Snap & Glue words of `DocumentSettings`: an attribute for each word the document has, and none when it has none.
fn words_el(words: &SnapWords) -> Option<El> {
    let attrs: Vec<(String, String)> =
        [("glue", &words.glue), ("snap", &words.snap), ("extensions", &words.extensions), ("dynamic-grid", &words.dynamic_grid)]
            .into_iter()
            .filter_map(|(name, word)| word.as_ref().map(|w| (name.to_owned(), w.to_string())))
            .collect();
    (!attrs.is_empty()).then(|| el("words", attrs, Vec::new()))
}

/// The entries for the style sheets whose sheet differs from the one the file's own markup gives: the cells and sections that differ, whole, and the names of
/// those the model lacks. A style is keyed by the name the file holds it by, and gives its `NameU` when that is another. **The root is keyed by `root="1"`** (the model
/// writer carries the root's sheet and names, since the package it makes holds the root the reader makes of the file's defaults), and **a style whose links are not the ones the markup gives** (a
/// style whose three links name different style sheets, which an OpenDocument style cannot say) says its own in `line-style`, `fill-style` and `text-style` (`-` for none, else the number).
/// **A style whose `ID` is not its place says its `ID` in `id`**: the file numbers the style sheets by their places, and the links of the model (the entry's own, a shape's, a
/// style's) are `ID`s, which the reader gives back.
fn style_entries(doc: &Document, native: &Native, gate: &mut Gate<'_>) -> Result<Vec<El>, WriteError> {
    let mut out: Vec<El> = Vec::new();
    for (at, style) in doc.styles.iter().enumerate() {
        let Some(n) = native.styles.get(at) else { continue };
        let root = at == 0;
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut entry = false;
        if root {
            // The root has no style element: it is keyed by its place. What it says of itself that the reader's root does not (the `NameU`, the shown name) is carried.
            attrs.push(("root".to_owned(), "1".to_owned()));
            if style.name_u != n.name_u
                && let Some(name_u) = &style.name_u
            {
                attrs.push(("name-u".to_owned(), name_u.to_string()));
                entry = true;
            }
            // (The reader's root shows the name it is called by.)
            if style.name.as_deref() != n.name_u.as_deref()
                && let Some(shown) = &style.name
            {
                attrs.push(("display".to_owned(), shown.to_string()));
                entry = true;
            }
        } else {
            let Some(name) = n.name_u.as_deref() else { continue };
            attrs.push(("name".to_owned(), name.to_owned()));
            entry = style.name_u != n.name_u;
            if entry && let Some(name_u) = &style.name_u {
                attrs.push(("name-u".to_owned(), name_u.to_string()));
            }
            // The `ID` of a style sheet that is not at the place the file numbers it by: the reader gives the style at place *p* the `ID` *p*, and this is the `ID` it had.
            if u32::try_from(at).ok() != Some(style.id) {
                attrs.push(("id".to_owned(), style.id.to_string()));
                entry = true;
            }
        }
        // The links the markup does not give (the root's are none, and the reader's root holds none).
        for (key, now, then) in [
            ("line-style", style.sheet.links.line, n.sheet.links.line),
            ("fill-style", style.sheet.links.fill, n.sheet.links.fill),
            ("text-style", style.sheet.links.text, n.sheet.links.text),
        ] {
            if now != then {
                attrs.push((key.to_owned(), now.map_or_else(|| "-".to_owned(), |link| link.to_string())));
                entry = true;
            }
        }
        // What the style held in an earlier ODF version's form, which the file says in 1.4's (`odf14`): the source's form, for a read to give back.
        // (Only the model writer writes a style in 1.4's form: a save into the file writes its styles as the file has them.)
        let model_path = gate.work.as_ref().is_some_and(|w| w.model_path);
        let form = match (gate.work.as_deref(), style.ext.get()) {
            (Some(work), Some(data)) if model_path && !root => super::odf14::cached(work, at, &data.odf),
            _ => None,
        };
        let older = form.as_ref().map(|f| &f.older);
        entry |= older.is_some_and(|o| !o.is_empty());
        // A size of 0 is written as the least the file takes (`runs::LEAST_FONT_SIZE`), which the markup then gives: the style's `Character` rows ride, with the 0.
        let mut native_sheet = Arc::clone(&n.sheet);
        if model_path && crate::write::runs::holds_zero_size(&style.sheet) {
            Arc::make_mut(&mut native_sheet).sections.retain(|s| s.name.as_str() != "Character");
        }
        if let Some(mut e) = delta_el("style", attrs, &style.sheet, &native_sheet, gate)?
            && (entry || e.children.iter().any(|c| matches!(c, Node::El(_))) || e.attr("gone").is_some())
        {
            if let Some(older) = older {
                e.children.extend(older_els(older).into_iter().map(Node::El));
            }
            out.push(e);
        }
    }
    Ok(out)
}

/// The `older` children of a style's entry: one for each property left out (`key`, `value`), each list style named (`named`), and each label width brought to 0 (`list`, `level`, `width`).
fn older_els(older: &super::odf14::Older) -> Vec<El> {
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    let mut out: Vec<El> = older.dropped.iter().map(|(k, v)| el("older", vec![pair("key", k), pair("value", v)], Vec::new())).collect();
    out.extend(older.named.iter().map(|n| el("older", vec![pair("named", n)], Vec::new())));
    out.extend(older.labels.iter().map(|(l, lv, w)| el("older", vec![pair("list", l), pair("level", lv), pair("width", w)], Vec::new())));
    out
}

/// The style's ODF data in the form of the earlier version it was read in, where its entry (`child`) says it: once for the style at `at` (a second entry for it is let be, with a
/// word). `done` holds the places of the styles put back so far.
fn put_older(
    r: &mut Reader<'_>,
    style: &mut diacraft_doc::Style,
    child: &El,
    sc: &Sc,
    at: usize,
    done: &mut std::collections::HashSet<usize>,
) -> Result<(), OdgError> {
    let index = read_older(r, child, sc)?;
    if index.is_empty() || style.ext.get().is_none() {
        return Ok(());
    }
    if done.insert(at) {
        super::odf14::restore(&mut style.ext.make_mut().odf, &index, |n| r.take_items(n))
    } else {
        r.warn_once("diacraft_older_twice", || {
            "DiaCraft's document file holds two entries of an earlier ODF version's forms for one style; the first is read".to_owned()
        });
        Ok(())
    }
}

/// The `older` children of a style's entry, read: see [`older_els`]. **Each takes an item before it is read and its strings their bytes, and at most `odf14::MOST_RECORDS` are read** (a writer
/// writes no more; the rest are left with a word). The index keeps only a property ODF 1.4 refuses and the first record of a key. A child of none of the three forms is no part of it.
fn read_older(r: &mut Reader<'_>, entry: &El, sc: &Sc) -> Result<super::odf14::Index, OdgError> {
    let mut index = super::odf14::Index::default();
    let mut records = 0usize;
    for child in entry.elements() {
        let csc = sc.enter(child);
        if !csc.is(child, "older") {
            continue;
        }
        if records >= super::odf14::MOST_RECORDS {
            r.warn_once("diacraft_older_too_many", || {
                "a style's entry in DiaCraft's document file holds more records of an earlier ODF version's forms than DiaCraft writes; the rest are left out".to_owned()
            });
            break;
        }
        records += 1;
        r.take_items(1)?;
        for value in child.attrs.iter().map(|(_, v)| v) {
            r.take_bytes(value.len())?;
        }
        let text = |name: &str| child.attr(name).map(str::to_owned);
        if let (Some(key), Some(value)) = (text("key"), text("value")) {
            index.dropped(key, value);
        } else if let Some(named) = text("named") {
            index.named(named);
        } else if let (Some(list), Some(level), Some(width)) = (text("list"), text("level"), text("width")) {
            index.label(list, level, width);
        }
    }
    Ok(index)
}

/// The entries for the document's own sheet and for the pages: what differs from the native markup, which is small (a few cells and sections), so that two
/// models are compared by the elements they make and no tree of the masters is built to find out that nothing changed.
fn small(doc: &Document, native: &Native, steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Vec<El>, WriteError> {
    let mut out: Vec<El> = Vec::new();
    if let Some(e) = delta_el("docsheet", Vec::new(), &doc.sheet, &native.sheet, gate)? {
        out.push(e);
    }
    let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for (at, page) in doc.pages.iter().enumerate() {
        let Some(n) = native.pages.get(at) else { return Err(WriteError::Unsupported("a page the file does not hold")) };
        let mut attrs: Vec<(String, String)> = vec![("name".to_owned(), page.name.to_string())];
        // A page whose name an earlier page has is told apart by its number among the pages of the name.
        let nth = {
            let count = seen.entry(&*page.name).or_insert(0);
            *count = count.saturating_add(1);
            *count
        };
        if nth > 1 {
            attrs.push(("nth".to_owned(), nth.to_string()));
        }
        let mut entry = false;
        if page.id != n.id {
            attrs.push(("id".to_owned(), page.id.to_string()));
            entry = true;
        }
        if page.name_u != n.name_u {
            if let Some(v) = &page.name_u {
                attrs.push(("name-u".to_owned(), v.to_string()));
            }
            entry = true;
        }
        let delta = delta_el("page", attrs, &page.sheet, &n.sheet, gate)?;
        if let Some(e) = delta
            && (entry || e.children.iter().any(|c| matches!(c, Node::El(_))) || e.attr("gone").is_some())
        {
            out.push(e);
        }
        out.extend(group_entries(page, nth, steps, gate)?);
    }
    Ok(out)
}

/// The cells that make a group's frame and pose: the reader rebuilds them from the box of the members and the markup holds them in its members' places, so a
/// group's entry never carries them.
const GROUP_FRAME: [&str; 9] = ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];

/// Whether `rows` are the four side-middle rows the reader gives every shape it reads, by the formulas of each row's place.
fn standard_four(rows: &[Arc<diacraft_doc::Row>]) -> bool {
    const PLACES: [(Option<&str>, Option<&str>); 4] =
        [(Some("Width*0.5"), Some("Height")), (None, Some("Height*0.5")), (Some("Width*0.5"), None), (Some("Width"), Some("Height*0.5"))];
    rows.len() == PLACES.len()
        && rows.iter().zip(PLACES).enumerate().all(|(at, (row, (x, y)))| {
            row.ix.is_none_or(|ix| usize::try_from(ix).is_ok_and(|ix| ix == at))
                && row.cells.get("X").is_some_and(|c| c.formula.text() == x)
                && row.cells.get("Y").is_some_and(|c| c.formula.text() == y)
        })
}

/// Whether a `Connection` section is the four side-middle rows the reader gives every shape it reads, by the formulas of each row's
/// place: the rows a group read from a file holds, which are no data. (A group that holds none, as a group a person makes and a VSDX file's group do, says so in its
/// entry: [`group_native`].)
fn is_standard_connection(section: &diacraft_doc::Section) -> bool {
    standard_four(&section.rows)
}

/// Whether every row of `rows` is one a `draw:glue-point` of the markup made and that is unchanged since: the element held beside the row (`HeldGroup::Glue`) has the row's stamp
/// (the row whole, [`diacraft_doc::stamp_row`]), so that the model writer writes the element back as it was read and the reader reads the same row again with the same element held.
pub(crate) fn held_glue_rows(rows: &[Arc<diacraft_doc::Row>]) -> bool {
    !rows.is_empty()
        && rows.iter().all(|row| {
            row.ext
                .get()
                .and_then(|e| e.odf.held.as_ref())
                .is_some_and(|h| h.group == diacraft_doc::HeldGroup::Glue && h.stamp == Some(diacraft_doc::stamp_row(row)))
        })
}

/// Whether a `Connection` section is what the markup alone gives a group **on the model path**: the four side-middle rows, then the rows of the group's own
/// `draw:glue-point` elements that are unchanged since they were read ([`held_glue_rows`]). The model writer writes those elements with the group and the reader reads the same
/// rows again with the same elements held, so the group's entry would carry them twice; and an entry of a group with no key of its own is refused (it would be found by a place another
/// program can change), so a group read from a file, with glue points of its own, that stands among the members of a group a save does not rename, was refused.
fn is_rebuilt_connection(section: &diacraft_doc::Section) -> bool {
    let (standard, held) = section.rows.split_at(section.rows.len().min(4));
    standard_four(standard) && held_glue_rows(held)
}

/// What a group's sheet holds that the reader would not rebuild: its cells but the frame and the pose, and its sections but the standard `Connection` rows. The
/// sheet the reader rebuilds, for the comparison that makes the entry.
///
/// **The reader gives every group the standard `Connection` rows** (groups included), and a `draw:g` has no style to say that it has none, as
/// another shape's tier 2 does: so for a group that holds no `Connection` section and is `keyed` (its name is its own on the page) the sheet the reader
/// rebuilds holds one, and the entry says it is gone (`s.Connection._`). Without it, a group of a VSDX drawing, or one a person made, was read back with four connection
/// points it did not have, and the check after the save named them. **A group with no key of its own is not given an entry for that alone**: its entry would be found by
/// a place another program can change, and is written only when the file holds it already, so a save would be refused for connection points the drawing shows nowhere
/// (the engine names every group it writes anew, `Group_<ID>`, so a group a person makes has a key).
fn group_native(sheet: &Sheet, keyed: bool, model_path: bool) -> Sheet {
    let mut native = Sheet::default();
    for name in GROUP_FRAME {
        if let Some(cell) = sheet.cells.get(name) {
            native.cells.insert(diacraft_doc::CellName::new(name), cell.clone());
        }
    }
    let mut connection = false;
    for section in &sheet.sections {
        if section.name.as_str() == "Connection" {
            connection = true;
            if is_standard_connection(section) || (model_path && is_rebuilt_connection(section)) {
                native.sections.push(Arc::clone(section));
            }
        }
    }
    // The section the reader would give the group, which only its key stands for here: the comparison finds it gone from the group's sheet.
    if !connection && keyed {
        native.sections.push(Arc::new(diacraft_doc::Section::new("Connection", None)));
    }
    native
}

/// [`group_native`] of a group `shape` of the model path, with the cells of the group's own automatic style (the model writer writes a style for a group the file read with one, and
/// the reader reads the same cells back from it, so they are the markup's and need no entry).
fn group_native_of(shape: &diacraft_doc::Shape, keyed: bool, model_path: bool) -> Sheet {
    let mut native = group_native(&shape.sheet, keyed, model_path);
    if model_path {
        for name in crate::write::groups::style_cells(shape) {
            if let Some(cell) = shape.sheet.cells.get(name) {
                native.cells.insert(diacraft_doc::CellName::new(name), cell.clone());
            }
        }
    }
    native
}

/// The steps a walk of the model for the document file may still take: charged **before** the work is done, and held to what the save's step cap has left, so that a
/// walk stops at the cap and does not run on to the end of a hostile model (the cap is consulted by the caller, which refuses the save when [`Steps::over`]).
pub(crate) struct Steps {
    room: u64,
    left: u64,
    over: bool,
}

impl Steps {
    /// A count with `room` steps to take.
    pub(crate) fn new(room: u64) -> Steps {
        Steps { room, left: room, over: false }
    }

    /// Takes `n` steps, charged before the work: `false` when they are more than are left, and from then on (the walk stops).
    pub(crate) fn take(&mut self, n: u64) -> bool {
        if self.over || n > self.left {
            self.over = true;
            self.left = 0;
            return false;
        }
        self.left -= n;
        true
    }

    /// The steps taken (all the room, once over).
    pub(crate) fn used(&self) -> u64 {
        self.room - self.left
    }

    /// Whether a walk asked for more than the room.
    pub(crate) fn over(&self) -> bool {
        self.over
    }
}

/// Charges what the walks of `sheet` cost, before they are made: each cell, each section, each row of a section (all of them before any is walked) and each cell of each
/// row (a row at a time, so that the count stops at the cap). `false` when the room is not enough.
fn charge_sheet(steps: &mut Steps, sheet: &Sheet) -> bool {
    if !steps.take(1_u64.saturating_add(sheet.cells.len() as u64).saturating_add(sheet.sections.len() as u64)) {
        return false;
    }
    for section in &sheet.sections {
        if !steps.take(1_u64.saturating_add(section.cells.len() as u64).saturating_add(section.rows.len() as u64)) {
            return false;
        }
        for row in section.rows.iter() {
            if !steps.take(row.cells.len() as u64) {
                return false;
            }
        }
    }
    true
}

/// Puts into `named` each `ID` of `groups` that `text` names as `Sheet.<ID>` (in any case): a scan of the bytes, with no copy of the text, and at most ten digits of a
/// number.
fn collect_named(text: &str, groups: &std::collections::HashSet<u32>, named: &mut std::collections::HashSet<u32>) {
    let bytes = text.as_bytes();
    let mut at: usize = 0;
    while let Some(window) = bytes.get(at..at.saturating_add(6)) {
        if window.eq_ignore_ascii_case(b"sheet.") {
            let digits = bytes.get(at + 6..).unwrap_or_default().iter().take(10).take_while(|b| b.is_ascii_digit()).count();
            if let Some(id) = text.get(at + 6..at + 6 + digits).and_then(|d| d.parse::<u32>().ok())
                && groups.contains(&id)
            {
                named.insert(id);
            }
            at += 6;
        } else {
            at += 1;
        }
    }
}

/// The `ID`s of the groups of a page that something names: a `Connect` row, or a formula of any cell of any shape (`Sheet.<ID>`). A group that is named keeps its `ID` in
/// the file (an id is written when another shape names the shape); one that is not takes the number the reader gives it. **Only the `ID`s of groups
/// are kept**, so the set is no bigger than the page's groups whatever the formulas name; every shape, cell, row and `Connect` row is a step, each formula adds a step for
/// each 64 bytes of its text (the scan of it), all charged before the work.
fn named_ids(page: &diacraft_doc::Page, steps: &mut Steps) -> std::collections::HashSet<u32> {
    let mut groups: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut named: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        if !steps.take(1) {
            return named;
        }
        if !shape.children.is_empty() {
            groups.insert(shape.id);
            stack.extend(shape.children.iter());
        }
    }
    for c in page.connects.iter() {
        if !steps.take(1) {
            return named;
        }
        if groups.contains(&c.to_sheet) {
            named.insert(c.to_sheet);
        }
    }
    let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        if !charge_sheet(steps, &shape.sheet) {
            return named;
        }
        let sections = shape.sheet.sections.iter();
        let cells = shape.sheet.cells.values().chain(sections.flat_map(|s| s.cells.values().chain(s.rows.iter().flat_map(|r| r.cells.values()))));
        for cell in cells {
            if let Some(text) = cell.formula.text() {
                if !steps.take((text.len() / 64) as u64) {
                    return named;
                }
                collect_named(text, &groups, &mut named);
            }
        }
        stack.extend(shape.children.iter());
    }
    named
}

/// How many groups of the page have each name (the empty one is a group with none), in one walk of the page, each group a step.
fn group_name_counts<'a>(page: &'a diacraft_doc::Page, steps: &mut Steps) -> std::collections::HashMap<&'a str, u32> {
    let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().collect();
    while let Some(shape) = stack.pop() {
        if !steps.take(1) {
            break;
        }
        if shape.children.is_empty() {
            continue;
        }
        let count = counts.entry(shape.name_u.as_deref().unwrap_or_default()).or_insert(0);
        *count = count.saturating_add(1);
        stack.extend(shape.children.iter());
    }
    counts
}

/// The `group` entries of one page of `doc`, in the pre-order of the page: a group is found by the page's name (and `nth`, its number among the pages of that name,
/// when it is not the first) and its own `draw:name`. An entry is made **only for a group that holds data of its own, whose `ID` is named by another shape, or that has a
/// link to a master**; the rest are rebuilt by the reader from the markup alone (Spec question 1).
///
/// **The key of an entry is the group's name, which is unique among the page's groups** (review: an entry found by the place of a
/// group among those of its name went to another group when another program reordered, added or took away groups). A group with a name that no other group of the page
/// has is keyed by it and the entry has no `nth`; **a group with no name, or one that another group has, has no key of its own**: its entry has `nth` always (its place
/// among the groups of the name, the old key, which is the marker too), is made only when it holds data or a link to a master (an `ID` alone is not kept for it), and is
/// written by [`tree`] only when the file holds it already, so that a save never makes an entry that another program can bind to another group (and left out when its data is
/// what the file's markup gives the group, which the reader builds again with no entry: [`from_markup`]).
///
/// **Where to change the rule** if an entry should be made for every group: the test for `named` below. **Cost**: every shape of a page that holds a group is a step; a
/// group's own sheet is charged by its cells, sections, rows and the cells of its rows, twice (the frame is taken out of it, and what is left is compared), before either
/// is walked; all in `steps`, which stops at the save's step cap.
fn group_entries(page: &diacraft_doc::Page, nth: u32, steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Vec<El>, WriteError> {
    Ok(group_entries_of(page, nth, steps, gate)?.into_iter().map(|(_, entry)| entry).collect())
}

/// [`group_entries`], each with the `ID` of the group it is for: the item of the group's VSDX data and then its `group` entry, either or both.
fn group_entries_of(page: &diacraft_doc::Page, nth: u32, steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Vec<(u32, El)>, WriteError> {
    // A group is a shape with members: nested only inside another, so a page whose top level has none holds none.
    if page.shapes.iter().all(|s| s.children.is_empty()) {
        return Ok(Vec::new());
    }
    let named = named_ids(page, steps);
    let totals = group_name_counts(page, steps);
    let mut out: Vec<(u32, El)> = Vec::new();
    let mut by_name: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    // The shapes still to look at, the last first, so that the pre-order of the file is the order of the groups.
    let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().rev().collect();
    while let Some(shape) = stack.pop() {
        if !steps.take(1) {
            return Ok(out);
        }
        if shape.children.is_empty() {
            continue;
        }
        if !charge_sheet(steps, &shape.sheet) || !charge_sheet(steps, &shape.sheet) {
            return Ok(out);
        }
        let name = shape.name_u.as_deref().unwrap_or_default();
        let count = by_name.entry(name).or_insert(0);
        *count = count.saturating_add(1);
        let mut attrs: Vec<(String, String)> = vec![("page".to_owned(), page.name.to_string())];
        if nth > 1 {
            attrs.push(("page-nth".to_owned(), nth.to_string()));
        }
        attrs.push(("name".to_owned(), name.to_owned()));
        let keyed = !name.is_empty() && totals.get(name) == Some(&1);
        if !keyed {
            attrs.push(("nth".to_owned(), count.to_string()));
        }
        // The links to a master: a group has no style of its own to carry them, so its entry does. **A group that is an instance of a master, or stands for a shape of
        // one, keeps its `ID` too**: it has an entry for its links whatever else it holds, so the `ID` costs it no entry, and a trip through an `.odg` gives it the number it
        // had, as it gives a master's instance that is no group.
        let linked = shape.master.is_some() || shape.master_shape.is_some();
        if named.contains(&shape.id) || linked {
            attrs.push(("id".to_owned(), shape.id.to_string()));
        }
        if let Some(master) = shape.master {
            attrs.push(("master".to_owned(), master.to_string()));
        }
        if let Some(master_shape) = shape.master_shape {
            attrs.push(("master-shape".to_owned(), master_shape.to_string()));
        }
        // The VSDX data of the group, keyed as its entry is and stamped by its members' names. The item is paired with the group's `ID`, as the group's entry is.
        if let Some(shell) = super::shell::shape_shell(shape, &diacraft_doc::ShapeType::Group) {
            let keys: Vec<(String, String)> =
                attrs.iter().filter(|(k, _)| matches!(k.as_str(), "page" | "page-nth" | "name" | "nth")).cloned().collect();
            let members = shape.children.iter().map(|c| c.name_u.as_deref().map(str::to_owned));
            out.push((shape.id, super::items::group_item(keys, &shell, members)?));
        }
        // `delta_el` makes the element when it holds data or the attributes say something: a group with neither is none.
        let data = delta_el("group", Vec::new(), &shape.sheet, &group_native_of(shape, keyed, gate.model_path()), gate)?;
        // A group with no key of its own keeps an `ID` that a shape names no longer (it would be found by its place): its entry is for data and a link to a master.
        let wanted = if keyed { data.is_some() || named.contains(&shape.id) || linked } else { data.is_some() || linked };
        if wanted {
            let mut entry = data.unwrap_or_else(|| el("group", Vec::new(), Vec::new()));
            attrs.append(&mut entry.attrs);
            entry.attrs = attrs;
            out.push((shape.id, entry));
        }
        stack.extend(shape.children.iter().rev());
    }
    Ok(out)
}

/// Which groups of a page DiaCraft's document file carries data for ([`groups_needing_a_key`]).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GroupKeys {
    /// The `ID`s of the groups that [`group_entries_of`] makes an entry for, or the item of their VSDX data: **a group with data of its own** (a cell or a section the file's markup does not give
    /// a group, on the model path), a link to a master, or an `ID` that another shape names.
    pub ids: std::collections::HashSet<u32>,
    /// The steps the walk took.
    pub steps: u64,
    /// The walk was asked for more steps than the room it was given, and stopped: `ids` is then not all of them.
    pub over: bool,
}

/// The groups of `page` that DiaCraft's document file carries data for, and so need a key of their own, a name no other group of the page has. **The
/// rule is the one [`group_entries_of`] makes an entry by** (the test of its `wanted`, for a group that is named): the engine names a new group only when it needs one, so that a group that
/// holds no data (a `draw:g` the file had with no name, which only its members fill, and that holds the four connection points the reader gives every shape) stays as the file had it. **A
/// group that holds no connection points is data**: the reader gives it the four, and its entry says it has none (a group of a VSDX drawing, a group made in DiaCraft). Every shape, cell,
/// row and formula of the page is a step, charged before the work, within `room`.
pub fn groups_needing_a_key(page: &diacraft_doc::Page, room: u64) -> GroupKeys {
    let mut steps = Steps::new(room);
    let mut ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // A page whose top level holds no group holds none.
    if !page.shapes.iter().all(|s| s.children.is_empty()) {
        let named = named_ids(page, &mut steps);
        let mut stack: Vec<&Arc<diacraft_doc::Shape>> = page.shapes.iter().rev().collect();
        while let Some(shape) = stack.pop() {
            if !steps.take(1) {
                break;
            }
            if shape.children.is_empty() {
                continue;
            }
            stack.extend(shape.children.iter().rev());
            if !charge_sheet(&mut steps, &shape.sheet) {
                break;
            }
            let linked = shape.master.is_some() || shape.master_shape.is_some();
            let carried = super::shell::shape_shell(shape, &diacraft_doc::ShapeType::Group).is_some();
            // As the group will be keyed once it is named: a group with no `Connection` section is given the standard one by the reader, and its entry says it has none.
            let data = !sheet_delta(&shape.sheet, &group_native_of(shape, true, true)).is_empty();
            if linked || carried || data || named.contains(&shape.id) {
                ids.insert(shape.id);
            }
        }
    }
    GroupKeys { ids, steps: steps.used(), over: steps.over() }
}

/// Whether an element this build made for a group is the group's `group` entry, and not the item of its VSDX data, which [`group_entries_of`] makes beside it.
pub(crate) fn is_group_entry(entry: &El) -> bool {
    entry.name.ends_with(":group")
}

/// The number of each page of `doc` among the pages of its name, as the entries say it (`page-nth`, [`small`]).
fn page_nths(doc: &Document) -> Vec<u32> {
    let mut seen: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    doc.pages
        .iter()
        .map(|page| {
            let count = seen.entry(&*page.name).or_insert(0);
            *count = count.saturating_add(1);
            *count
        })
        .collect()
}

/// The entries with no key of their own (`nth`) that `doc` makes for groups whose data is **what the file's markup gives them**: the drawing as it was read
/// (`base`) has the group on the page of the same `ID`, under the same `ID`, the entry `base` makes for it carries what this one does (its attributes but the key, `page`, `page-nth`,
/// `name` and `nth`, and its children), and no entry of the file as it is (`old`) has the group's name, so none gave it data on the read. Such data is no data of DiaCraft's: the
/// graphic style of a group's `draw:style-name` and the glue points of its own `draw:glue-point`s, which are no frame and no standard `Connection` row ([`group_native`]). The reader
/// builds it again from the markup, so the group needs no entry, and [`tree`] leaves its entry out where it would refuse a new entry with no key: a save that rewrites the file for
/// something else (a window setting turned) is no refusal. A group whose data was changed in DiaCraft is none of these, and its entry is refused as before.
///
/// **Only `group` entries are looked at**: the item of a group's VSDX data, paired with the group's `ID` as its entry is, holds what no markup
/// says, and is never one of these, so a save never leaves it out.
fn from_markup(doc: &Document, base: &Document, old: Option<&El>, steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Vec<El>, WriteError> {
    const KEY: [&str; 4] = ["page", "page-nth", "name", "nth"];
    // The `group` entries of a page's groups: the items beside them are left alone.
    let entries_of = |page: &diacraft_doc::Page, nth: u32, steps: &mut Steps, gate: &mut Gate<'_>| -> Result<Vec<(u32, El)>, WriteError> {
        let mut entries = group_entries_of(page, nth, steps, gate)?;
        entries.retain(|(_, entry)| is_group_entry(entry));
        Ok(entries)
    };
    let mut out: Vec<El> = Vec::new();
    // A drawing with no group has no entry to look at.
    if doc.pages.iter().all(|page| page.shapes.iter().all(|s| s.children.is_empty())) {
        return Ok(out);
    }
    let sc = Sc::empty();
    // The names the file's own entries are for: a group of such a name may have had its data from the file.
    let taken: std::collections::HashSet<&str> = old.map_or_else(Default::default, |o| {
        let sc = sc.enter(o);
        o.elements().filter(|e| sc.enter(e).is(e, "group")).filter_map(|e| e.attr("name")).collect()
    });
    let data = |e: &El| -> Vec<(String, String)> { e.attrs.iter().filter(|(k, _)| !KEY.contains(&k.as_str())).cloned().collect() };
    let (nths, base_nths) = (page_nths(doc), page_nths(base));
    let base_pages: std::collections::HashMap<u32, usize> = base.pages.iter().enumerate().map(|(at, page)| (page.id, at)).collect();
    for (at, page) in doc.pages.iter().enumerate() {
        if !steps.take(1) {
            return Ok(out);
        }
        let Some((base_page, base_nth)) =
            base_pages.get(&page.id).and_then(|&was| Some((base.pages.get(was)?, base_nths.get(was).copied().unwrap_or(1))))
        else {
            continue;
        };
        let made = entries_of(page, nths.get(at).copied().unwrap_or(1), steps, gate)?;
        if made.iter().all(|(_, entry)| entry.attr("nth").is_none()) {
            continue;
        }
        let read = entries_of(base_page, base_nth, steps, gate)?;
        for (id, entry) in made {
            if entry.attr("nth").is_none() || entry.attr("name").is_some_and(|name| taken.contains(name)) {
                continue;
            }
            if !steps.take(1 + read.len() as u64) {
                return Ok(out);
            }
            let carried = data(&entry);
            if read.iter().any(|(was, r)| *was == id && r.children == entry.children && data(r) == carried) {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

/// What a save does about the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// The file's new bytes (the file is new, or its content changed).
    Write(Vec<u8>),
    /// The model holds nothing the file carried, and nothing else is in it: the file and its row go.
    Remove,
}

/// The tree of the file for `doc`: `old` with the children this build writes taken out and the ones for `doc` put in, so that what `old` holds that this
/// build does not know is kept where it was. `None` when there is nothing to write and nothing was kept. `markup` holds the entries with no key whose data the file's markup gives
/// ([`from_markup`]).
fn tree(old: Option<&El>, doc: &Document, native: &Native, markup: &[El], steps: &mut Steps, gate: &mut Gate<'_>) -> Result<Option<El>, WriteError> {
    let all = children(doc, native, steps, gate)?;
    let sc = Sc::empty();
    // An entry with no key of its own (`nth` says so: [`group_entries`]) is written only when the file holds it already: a new one would be found by the place of its group. One whose
    // data the markup gives and that the file does not hold is left out: the reader builds that data again with no entry. The item of a group's VSDX data
    // is no `group` entry, and is written whenever it is made.
    let held: Vec<&El> = old.map_or_else(Vec::new, |o| {
        let sc = sc.enter(o);
        o.elements().filter(|e| sc.enter(e).is(e, "group") && e.attr("nth").is_some()).collect()
    });
    let mut wanted: Vec<El> = Vec::new();
    for entry in all {
        if entry.attr("nth").is_some() && entry.attr("page").is_some() && is_group_entry(&entry) {
            if !steps.take(1 + held.len() as u64 + markup.len() as u64) {
                return Err(WriteError::Unsupported(WORDS_STEPS));
            }
            let same = |h: &El| h.attrs == entry.attrs && h.children == entry.children;
            if !held.iter().any(|h| same(h)) {
                if markup.iter().any(same) {
                    continue;
                }
                // The refusal names the group, so that a person can find it and name it.
                let number = |attr: &str| entry.attr(attr).and_then(diacraft_sheetxml::passthrough::canonical_u32).unwrap_or(1);
                return Err(WriteError::Group {
                    page: entry.attr("page").unwrap_or_default().to_owned(),
                    page_nth: number("page-nth"),
                    nth: number("nth"),
                    why: WORDS_UNKEYED_ENTRY,
                });
            }
        }
        wanted.push(entry);
    }
    let mut root = match old {
        // The root says the version of what is written: the masters and the sheets of the entries below it are this build's, whatever number the file had (a file of a newer
        // minor keeps the children this build does not know, as the file had them, under the number of the version that holds them now).
        Some(old) => {
            let mut root = old.clone();
            for (k, v) in &mut root.attrs {
                if k == "version" {
                    VERSION.clone_into(v);
                } else if (k == "xmlns" || k.starts_with("xmlns:")) && v == diacraft_doc::DIACRAFT_URN_LEGACY {
                    // The file was written by a build before the id moved to `diacraft.app` (APPID): what is written now is in the namespace this build writes, whose declaration the root makes.
                    diacraft_doc::DIACRAFT_URN.clone_into(v);
                }
            }
            root
        }
        None => El {
            name: format!("{}:document", body::P),
            attrs: vec![(format!("xmlns:{}", body::P), names::URN.to_owned()), ("version".to_owned(), VERSION.to_owned())],
            children: Vec::new(),
        },
    };
    let sc = sc.enter(&root);
    let first_known = root.children.iter().position(|n| matches!(n, Node::El(e) if is_known(&sc, e)));
    root.children.retain(|n| !matches!(n, Node::El(e) if is_known(&sc, e)));
    let at = first_known.unwrap_or(root.children.len()).min(root.children.len());
    let tail = root.children.split_off(at);
    root.children.extend(wanted.into_iter().map(Node::El));
    root.children.extend(tail);
    let holds = root.children.iter().any(|n| matches!(n, Node::El(_)));
    Ok(holds.then_some(root))
}

/// The words of the refusal of a document file past [`MOST_BYTES`], said before the trees that would make it are all made.
const WORDS_MORE_THAN_THE_FILE_HOLDS: &str = "a document file of more than 64 MiB";

/// What the element `root` is as bytes in the file, at the least: the names, the attributes' names and values and the text in it, and the marks round them, walked with a stack
/// of its own. It is a bound for a loop that stops when the file would be past [`MOST_BYTES`], not the file's exact size (which [`written`] makes and checks).
fn el_bytes(root: &El) -> u64 {
    let mut bytes = 0u64;
    let mut stack: Vec<&El> = vec![root];
    while let Some(e) = stack.pop() {
        // `<name` and `/>`, and a space, `=""` and the quotes for each attribute.
        bytes = bytes.saturating_add(e.name.len() as u64 + 3);
        for (k, v) in &e.attrs {
            bytes = bytes.saturating_add(k.len() as u64 + v.len() as u64 + 4);
        }
        for n in &e.children {
            match n {
                Node::El(child) => stack.push(child),
                Node::Text(t) | Node::CData(t) | Node::Comment(t) => bytes = bytes.saturating_add(t.len() as u64),
                Node::Pi { target, data } => bytes = bytes.saturating_add(target.len() as u64 + data.len() as u64),
            }
        }
    }
    bytes
}

/// The words of the refusal of the data of a group that has no name of its own on its page.
const WORDS_UNKEYED_ENTRY: &str = "a group with no name that is unique among the page's groups is given data of its own or a link to a master (its entry in DiaCraft's document file is found by its name, and another program can change the order of unnamed groups: name the group, or put the data on a shape beside it)";

/// The words of the refusal of a save past the step cap in the check of the entries of unnamed groups.
const WORDS_STEPS: &str = "more steps than a save may take, in the entries for the groups of the drawing";

fn is_known(sc: &Sc, e: &El) -> bool {
    let csc = sc.enter(e);
    KNOWN.iter().any(|k| csc.is(e, k))
}

/// The most the document file may be, inflated, for a save to rewrite it: a file past it is not read again and written by the writer, which builds the file's
/// tree beside the model's (the memory a save holds is the gate's, and this is the file's own).
pub(crate) const MOST_BYTES: usize = 64 << 20;

/// Whether the model holds what the baseline held of everything the file carries. **No tree is built to find out**: the masters, themes, colours and data are
/// compared as models (a value comparison that costs no allocation), the entries for the sheets and the pages as the small elements they are.
fn same(doc: &Document, base: &Document, native: &Native, after: &Native, drop_macros: bool, steps: &mut Steps) -> bool {
    let themes = |d: &Document| -> Vec<(Arc<str>, Arc<[u8]>)> { d.themes.iter().map(|t| (Arc::clone(&t.part), Arc::clone(&t.bytes))).collect() };
    doc.masters == base.masters
        && doc.colors == base.colors
        && doc.diacraft == base.diacraft
        && windows_of(doc) == windows_of(base)
        && doc.snap_words() == base.snap_words()
        && crate::write::newstyles::styles_same(doc, base)
        && themes(doc) == themes(base)
        && super::items::same(doc, base, drop_macros)
        && small(doc, after, steps, &mut Gate::comparing()).ok() == small(base, native, steps, &mut Gate::comparing()).ok()
}

/// Whether a style sheet of `doc` holds ODF data in a form of an earlier version that 1.4 refuses (`odf14`, the forms the save worked out before it wrote anything) or a font size of 0: the model
/// writer writes it in 1.4's form, and carries the source's.
fn holds_older(doc: &Document, work: &crate::write::WriteWork) -> bool {
    work.odf14.iter().any(Option::is_some) || doc.styles.iter().any(|s| crate::write::runs::holds_zero_size(&s.sheet))
}

/// The windows part a model holds, which the file carries whole.
fn windows_of(doc: &Document) -> Option<&diacraft_doc::RawElement> {
    doc.vsdx.as_ref().and_then(|held| held.windows.as_deref())
}

/// Adds what a walk of the model took to the save's steps, and refuses the save when the walk asked for more than the cap had left (it stopped there).
fn charge(work: &mut crate::write::WriteWork, steps: &Steps) -> Result<(), crate::write::WriteError> {
    work.cells_compared = work.cells_compared.saturating_add(steps.used());
    if steps.over() {
        // One step past the cap, so that the save's own check says it in its own words.
        work.cells_compared = work.cells_compared.saturating_add(1);
    }
    work.check_steps()
}

/// The change a save makes to the file, or `None` when the model holds what the baseline held. `old` is the file as the package has it, `base` the model
/// the package reads as, and `native` what its native markup alone gives. **The cost is counted**: the bytes of the file read, and of the file made, are
/// charged to `work` (the save's own caps), and a file past [`MOST_BYTES`] is refused before it is parsed.
pub(crate) fn plan(
    doc: &Document,
    base: &Document,
    native: &Native,
    st: &Structure,
    old: Option<&[u8]>,
    limits: &crate::Limits,
    work: &mut crate::write::WriteWork,
) -> Result<Option<Change>, crate::write::WriteError> {
    let after = native.after(doc, st);
    let mut steps = Steps::new(work.step_cap.saturating_sub(work.steps_used()));
    // A style of an earlier ODF version's form is written in 1.4's on the model path, and its source's form rides here, whatever else the document file holds.
    let unchanged = same(doc, base, native, &after, work.drop_macros, &mut steps) && !(work.model_path && holds_older(doc, work));
    charge(work, &steps)?;
    if unchanged {
        return Ok(None);
    }
    let words = WriteError::Unsupported;
    if old.is_some_and(|b| b.len() > MOST_BYTES) {
        return Err(words("a document file of more than 64 MiB (it is not written again)"));
    }
    if let Some(bytes) = old {
        work.charge_copy(bytes.len(), limits)?;
    }
    let old_tree = old.and_then(|bytes| {
        let mut scan = diacraft_container::Work::default();
        let parsed = diacraft_container::xml::parse_counted(bytes, &parse_limits(limits), &mut scan).ok()?;
        let sc = Sc::empty().enter(&parsed);
        // A file of an older version is not written over child by child: what it holds was never read, and under this version's name it would be read as another thing. A
        // save that has something to write makes a new file, and one that has none takes the old one out; either way what the old file held is gone, and the save says so.
        if sc.is(&parsed, "document") && matches!(Age::of(&parsed), Age::Old(_)) {
            work.docfile_old_dropped = true;
            return None;
        }
        sc.is(&parsed, "document").then_some(parsed)
    });
    let mut steps = Steps::new(work.step_cap.saturating_sub(work.steps_used()));
    // What a read of the file has left of its items for the document file: the reader's, less what the rest of the file costs it (what the read of the file as it is cost, less what its
    // document file cost). The file made is read, as the reader reads it, within that.
    let mut rest = work.baseline_read.items.saturating_sub(work.baseline_read.docfile_items);
    // A package that has no document file gains the manifest row that lists it, which the reader keeps in the model and charges.
    if old.is_none() {
        rest = rest.saturating_add(MANIFEST_ROW_ITEMS);
    }
    // The walk that maps the links to the style sheets when one is not at its place (`styleids.rs`): a visit for each page and each shape, which a document of the markup
    // alone (the check's) has not got to charge.
    let moved = doc.styles.iter().enumerate().any(|(at, s)| u32::try_from(at).ok() != Some(s.id));
    let walk = if moved { (doc.pages.len() as u64).saturating_add(work.model_shapes) } else { 0 };
    let check = ReadCheck { doc, after: &after, room: (limits.max_items as u64).saturating_sub(rest), walk };
    // The trees are made, and written, while the save holds the memory they take; it is given back when the bytes are made.
    let mut gate = Gate::new(work);
    // The entries with no key whose data is the markup's own, which the file leaves out, are found first.
    let made = from_markup(doc, base, old_tree.as_ref(), &mut steps, &mut gate)
        .and_then(|markup| tree(old_tree.as_ref(), doc, &after, &markup, &mut steps, &mut gate))
        .and_then(|now| now.map(|root| written(root, limits, &check)).transpose());
    gate.release();
    charge(work, &steps)?;
    match made? {
        None => Ok(old.is_some().then_some(Change::Remove)),
        Some((bytes, nodes)) => {
            work.docfile_items = nodes;
            work.charge_copy(bytes.len(), limits)?;
            Ok(Some(Change::Write(bytes)))
        }
    }
}

/// The file for the tree `root`: its bytes, and the items of the tree. **A file the reader would refuse is refused in words and never written**: no value past what the
/// parser holds in an attribute, no more nodes than the reader's trees may have, no deeper than the reader parses the file, no more bytes than a part of the package may be, and no
/// more items than the reader has left to read it with ([`ReadCheck`]).
fn written(mut root: El, limits: &crate::Limits, check: &ReadCheck<'_>) -> Result<(Vec<u8>, u64), WriteError> {
    let words = WriteError::Unsupported;
    let (widest, nodes, depth) = measure(&root);
    if widest > limits.container.xml.max_attr_bytes {
        return Err(words("a document file with a name or a value too long for an attribute of the file"));
    }
    if nodes > limits.max_read_xml_nodes.min(limits.container.xml.max_nodes) {
        return Err(words("a document file with more elements than the reader holds"));
    }
    if depth > parse_limits(limits).max_depth.min(diacraft_container::limits::XML_DEPTH_CEILING) {
        return Err(words("a document file nested deeper than the reader parses it"));
    }
    let bytes = diacraft_container::xml::to_bytes(&root, &[], &[], true).map_err(|_| words("a document file with a character XML cannot hold"))?;
    if bytes.len() > MOST_BYTES {
        return Err(words(WORDS_MORE_THAN_THE_FILE_HOLDS));
    }
    // The package the reader reads holds no part of more bytes than the container's cap, which a caller may lower.
    if bytes.len() as u64 > limits.container.max_part_bytes {
        return Err(WriteError::Limit {
            what: "bytes in DiaCraft's document file (the most a part of the package may be)",
            max: limits.container.max_part_bytes,
        });
    }
    check.reads(&mut root, bytes.len(), limits)?;
    Ok((bytes, nodes as u64))
}

/// What the reader says to the file a save is about to write, asked before it is written (the report, concern 5: a model of masters that passed the writer's
/// node check was a file the reader refused for its items, so a save could write what DiaCraft could not open again).
///
/// **The reader's own functions read it**: the bytes are charged, the entries for groups are read, and the rest is read over a document of the pages and styles the file's markup gives
/// ([`Native::after`]), by [`apply_to`] without keeping the masters, so that a master is read, charged and let go, one at a time, and the file is refused where the reader would
/// refuse it and for the reason it would give: the reader's items (what is left of them once the rest of the file has had its own, `room`), the shapes and the cells it may count,
/// the pictures' bytes, and the number of masters, themes and colours. A change to what the reader charges is a change to this check, and the two cannot differ.
struct ReadCheck<'a> {
    doc: &'a Document,
    after: &'a Native,
    /// The items a read of the file has left for the document file.
    room: u64,
    /// The items the reader charges for the walk of the pages' shapes that maps their links to the style sheets, which the document of the markup alone does not hold the shapes of.
    walk: u64,
}

/// The most items the reader charges for the manifest row of the document file (two attributes and a place in a list; it was 26 when merged): a figure above it, so that
/// a save that adds the first document file to a package errs towards refusing. A package that has one already has the row, and its cost is in what the read of it cost.
const MANIFEST_ROW_ITEMS: u64 = 64;

/// The words of the refusal of a file that costs the reader more items than it has.
const WORDS_READER_ITEMS: &str =
    "items to read DiaCraft's document file with (the reader's items for a drawing, less what the rest of the file takes)";
/// The words of the refusal of a file the reader refuses for another cap.
const WORDS_READER_REFUSES: &str = "a document file that the reader would refuse";

impl ReadCheck<'_> {
    /// The document the reader has when it reads the file over the markup: the pages and the style sheets as the markup gives them after the save, and nothing else.
    fn scratch(&self) -> Document {
        let mut scratch = Document::empty();
        scratch.sheet = Arc::clone(&self.after.sheet);
        scratch.pages = SharedVec::from_vec(
            self.doc
                .pages
                .iter()
                .zip(self.after.pages.iter())
                .map(|(model, native)| {
                    let mut page = Page::new(native.id, &model.name);
                    page.name_u.clone_from(&native.name_u);
                    page.sheet = Arc::clone(&native.sheet);
                    page.back_page = model.back_page;
                    page.background = model.background;
                    Arc::new(page)
                })
                .collect(),
        );
        scratch.styles = SharedVec::from_vec(
            self.doc
                .styles
                .iter()
                .zip(self.after.styles.iter())
                .map(|(model, native)| {
                    Arc::new(Style {
                        id: model.id,
                        name: model.name.clone(),
                        name_u: native.name_u.clone(),
                        sheet: Arc::clone(&native.sheet),
                        ..Style::default()
                    })
                })
                .collect(),
        );
        scratch
    }

    /// Reads the file of `len` bytes whose tree is `root` as the reader reads it, within the items `room` leaves it: refused in words where the reader refuses it.
    fn reads(&self, root: &mut El, len: usize, limits: &crate::Limits) -> Result<(), WriteError> {
        let scoped = crate::Limits { max_items: usize::try_from(self.room).unwrap_or(usize::MAX), ..limits.clone() };
        let mut r = Reader::new(&scoped);
        let refused = |e: OdgError| match e {
            OdgError::Limit { kind: LimitKind::Items, .. } => WriteError::Limit { what: WORDS_READER_ITEMS, max: self.room },
            OdgError::TooComplex { what, max } => WriteError::Limit { what, max },
            _ => WriteError::Unsupported(WORDS_READER_REFUSES),
        };
        r.take_model_bytes(len).map_err(refused)?;
        r.take_items(usize::try_from(self.walk).unwrap_or(usize::MAX)).map_err(refused)?;
        drop(read_group_entries(&mut r, root).map_err(refused)?);
        let mut scratch = self.scratch();
        apply_to(&mut r, &mut scratch, Some(root), None, false).map_err(refused)?;
        Ok(())
    }
}

/// The longest attribute name or value, in bytes, the number of items (elements, attributes and text nodes) of a tree, and its depth (the root is 1), walked with a stack of
/// its own.
fn measure(root: &El) -> (usize, usize, usize) {
    let (mut widest, mut nodes, mut deepest) = (0usize, 0usize, 0usize);
    let mut stack: Vec<(&El, usize)> = vec![(root, 1)];
    while let Some((e, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        nodes = nodes.saturating_add(1).saturating_add(e.attrs.len());
        for (k, v) in &e.attrs {
            widest = widest.max(k.len()).max(v.len());
        }
        for n in &e.children {
            match n {
                Node::El(child) => stack.push((child, depth.saturating_add(1))),
                _ => nodes = nodes.saturating_add(1),
            }
        }
    }
    (widest, nodes, deepest)
}

mod styleids;
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use styleids::map_links;

#[cfg(test)]
mod codec_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_doc::{Cell, CellName, Page, Value};

    /// The entries of a page's groups, made with no memory held (a test of the entries is no save).
    fn groups_of(page: &Page, nth: u32, steps: &mut Steps) -> Vec<El> {
        group_entries(page, nth, steps, &mut Gate::none()).unwrap()
    }

    fn constant(text: &str) -> Cell {
        Cell::constant(Value::from_text(text))
    }

    fn sheet_with(cells: &[(&str, &str)]) -> Arc<Sheet> {
        let mut s = Sheet::default();
        for (name, text) in cells {
            s.cells.insert(CellName::new(name), constant(text));
        }
        Arc::new(s)
    }

    fn native_page(id: u32, sheet: Arc<Sheet>) -> NativePage {
        NativePage { id, name_u: None, sheet }
    }

    /// A document of one page, called `P`, with `id`, `name_u` and `sheet`.
    fn document(id: u32, name_u: Option<&str>, sheet: Arc<Sheet>) -> Document {
        let mut doc = Document::empty();
        let mut page = Page::new(id, "P");
        page.name_u = name_u.map(Arc::from);
        page.sheet = sheet;
        doc.pages.push(Arc::new(page));
        doc
    }

    /// What the reader rebuilt is bookkeeping: two reports are equal whatever it was.
    #[test]
    fn what_the_reader_rebuilt_is_no_part_of_what_two_reads_are_compared_by() {
        let one = Native::default();
        let other =
            Native { sheet: sheet_with(&[("A", "1")]), pages: vec![native_page(4, sheet_with(&[]))], styles: Vec::new(), ..Native::default() };
        assert_eq!(one, other);
    }

    /// The markup after a save has the style sheets the model added last, each as an empty common style gives it (its links, the name R11 writes, and
    /// no cell: the sheet is the document file's), after the file's own, which stay as they were.
    #[test]
    fn the_native_markup_after_a_save_has_the_style_sheets_added_last_as_empty_common_styles() {
        let style = |id: u32, name: &str, link: u32| {
            let sheet = Sheet {
                links: diacraft_doc::Links { line: Some(link), fill: Some(link), text: Some(link) },
                cells: [(CellName::new("ThemeIndex"), constant("65534"))].into_iter().collect(),
                ..Sheet::default()
            };
            Arc::new(diacraft_doc::Style {
                id,
                name: Some(Arc::from(name)),
                name_u: Some(Arc::from(name)),
                sheet: Arc::new(sheet),
                ..diacraft_doc::Style::default()
            })
        };
        let native = Native {
            sheet: Arc::default(),
            pages: Vec::new(),
            styles: vec![NativeStyle { name_u: Some(Arc::from("No Style")), sheet: Arc::default() }],
            ..Native::default()
        };
        let mut doc = Document::empty();
        doc.styles = diacraft_doc::SharedVec::from_vec(vec![style(0, "No Style", 0), style(1, "My Theme", 0), style(2, "Next", 1)]);
        let after = native.after(&doc, &Structure::identity(doc.pages.len()));
        assert_eq!(after.styles.len(), 3);
        assert_eq!(after.styles[0].name_u.as_deref(), Some("No Style"), "the file's own is as it was");
        assert_eq!(after.styles[1].name_u.as_deref(), Some("My_20_Theme"), "a new one is held by the name R11 writes");
        assert_eq!(after.styles[2].name_u.as_deref(), Some("Next"));
        assert_eq!(after.styles[1].sheet.links, doc.styles.get(1).unwrap().sheet.links);
        assert_eq!(
            after.styles[2].sheet.links,
            diacraft_doc::Links { line: Some(1), fill: Some(1), text: Some(1) },
            "the links of an empty common style with a parent"
        );
        assert!(after.styles[1].sheet.cells.is_empty() && after.styles[2].sheet.cells.is_empty(), "and no cell: the document file carries them");
        // What the entries of the document file then say is the whole sheet of each new style, under the name R11 writes, and the NameU where it is another.
        let entries = style_entries(&doc, &after, &mut Gate::none()).unwrap();
        // (The root has an entry of its own, keyed by `root="1"`, where the model's root says more than the package's: the model writer's. A save into a file never gets here with one, its plan
        // refuses a change of the root.)
        let names: Vec<(Option<&str>, Option<&str>)> =
            entries.iter().filter(|e| e.attr("root").is_none()).map(|e| (e.attr("name"), e.attr("name-u"))).collect();
        assert_eq!(names, [(Some("My_20_Theme"), Some("My Theme")), (Some("Next"), None)]);
        assert_eq!(
            entries.iter().filter(|e| e.attr("root").is_some()).count(),
            1,
            "the root's sheet differs from the package's here, and is carried"
        );
    }

    /// The markup after a save has the model's value of each cell of the page's layout, a cell the model lacks taken out and one it has put in, and the
    /// cells that are not the layout's as they were.
    #[test]
    fn the_native_markup_after_a_save_has_the_models_page_layout_cells_and_leaves_the_others() {
        let native = Native {
            sheet: Arc::default(),
            pages: vec![native_page(1, sheet_with(&[("PageWidth", "1"), ("PageHeight", "2"), ("DrawingScale", "5")]))],
            styles: Vec::new(),
            ..Native::default()
        };
        let doc = document(1, None, sheet_with(&[("PageWidth", "9"), ("PageLeftMargin", "3"), ("DrawingScale", "7")]));
        let after = native.after(&doc, &Structure::identity(1));
        let cells = &after.pages[0].sheet.cells;
        assert_eq!(cells.get("PageWidth"), Some(&constant("9")), "a changed layout cell is the model's");
        assert_eq!(cells.get("PageHeight"), None, "a layout cell the model lacks is gone");
        assert_eq!(cells.get("PageLeftMargin"), Some(&constant("3")), "a layout cell the model has is there");
        assert_eq!(cells.get("DrawingScale"), Some(&constant("5")), "a cell that is not the layout's is the markup's");
        // The same model makes no change, and the page keeps its id and universal name.
        let same = document(1, None, native.pages[0].sheet.clone());
        let kept = native.after(&same, &Structure::identity(1));
        assert_eq!(kept.pages[0].sheet, native.pages[0].sheet);
        // The page is read under the `ID` of its place, from 0, and under its name as its universal name.
        assert_eq!((kept.pages[0].id, kept.pages[0].name_u.clone()), (0, Some(Arc::from("P"))));
    }

    /// A page the save adds has no layer set of its own: it is read with the `Layer` rows of the document's, and with the other sections of the markup of the first page, and
    /// not with the first page's own layer rows.
    #[test]
    fn a_page_added_is_read_with_the_documents_layer_rows_and_the_other_sections_of_the_first_page() {
        let section = |name: &str, del: bool| {
            let mut s = diacraft_doc::Section::new(name, None);
            s.del = del;
            Arc::new(s)
        };
        let key = |name: &str| diacraft_doc::SectionKey::new(name, None);
        // The first page: a section of another name, and layer rows of its own (`del` stands for which rows they are).
        let first = Arc::new(Sheet { sections: vec![section("User", false), section("Layer", false)], ..Sheet::default() });
        let mut native = Native { pages: vec![native_page(0, first)], ..Native::default() };
        let mut doc = document(0, None, sheet_with(&[]));
        doc.pages.push(Arc::new(Page::new(5, "New")));
        let structure = Structure { matched: vec![Some(0), None], template: Some(0), ..Structure::default() };
        // The document's layer rows differ from the first page's.
        native.new_page_layers = Some(section("Layer", true));
        let after = native.after(&doc, &structure);
        let sheet = &after.pages[1].sheet;
        assert_eq!(sheet.section(&key("Layer")), native.new_page_layers.as_ref(), "the page added has the document's rows");
        assert_eq!(sheet.section(&key("User")), Some(&section("User", false)), "and the other sections the first page's markup gives");
        assert_eq!(sheet.sections.len(), 2);
        assert_eq!(after.pages[0].sheet.section(&key("Layer")), Some(&section("Layer", false)), "the page that stays has its own");
        // A document whose layer set holds no row: the page added has none.
        native.new_page_layers = None;
        let after = native.after(&doc, &structure);
        assert_eq!(after.pages[1].sheet.sections, vec![section("User", false)]);
        // The same rows as the first page's: nothing to change.
        native.new_page_layers = Some(section("Layer", false));
        let after = native.after(&doc, &structure);
        assert_eq!(after.pages[1].sheet.sections, vec![section("User", false), section("Layer", false)]);
    }

    /// A page is an entry of the file when what the model holds differs from the markup's by a cell it has, by a cell it lacks, by its id or by its universal name,
    /// and is none when it holds nothing the markup does not.
    #[test]
    fn a_page_is_an_entry_for_a_cell_it_has_or_lacks_for_its_id_or_universal_name_and_for_nothing_else() {
        let native =
            Native { sheet: Arc::default(), pages: vec![native_page(1, sheet_with(&[("PageWidth", "1")]))], styles: Vec::new(), ..Native::default() };
        let entries = |doc: &Document| small(doc, &native, &mut Steps::new(u64::MAX), &mut Gate::none()).unwrap();
        assert!(entries(&document(1, None, sheet_with(&[("PageWidth", "1")]))).is_empty(), "the same page is no entry");
        // A cell the markup lacks: an entry with the cell and no `gone`.
        let out = entries(&document(1, None, sheet_with(&[("PageWidth", "1"), ("UserCell", "2")])));
        assert_eq!(out.len(), 1);
        assert!(out[0].attr("gone").is_none() && out[0].elements().count() == 1, "{:?}", out[0]);
        // A cell the markup has that the model lacks: an entry with `gone` alone.
        let out = entries(&document(1, None, sheet_with(&[])));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].attr("gone"), Some("c.PageWidth"));
        assert_eq!(out[0].elements().count(), 0, "{:?}", out[0]);
        // The id and the universal name: an entry with that attribute alone.
        let out = entries(&document(7, None, sheet_with(&[("PageWidth", "1")])));
        assert_eq!((out.len(), out[0].attr("id")), (1, Some("7")));
        let out = entries(&document(1, Some("U.1"), sheet_with(&[("PageWidth", "1")])));
        assert_eq!((out.len(), out[0].attr("name-u")), (1, Some("U.1")));
    }

    /// A document that holds one master called `name`, against one that holds nothing: a save has a file to write.
    fn with_master(name: &str) -> Document {
        let mut doc = Document::empty();
        doc.masters.push(Arc::new(diacraft_doc::Master { id: 1, name: Some(Arc::from(name)), ..diacraft_doc::Master::default() }));
        doc
    }

    fn planned(doc: &Document, old: Option<&[u8]>, limits: &crate::Limits) -> Result<Option<Change>, crate::write::WriteError> {
        plan(doc, &Document::empty(), &Native::default(), &Structure::identity(doc.pages.len()), old, limits, &mut crate::write::WriteWork::default())
    }

    fn refused_for(r: &Result<Option<Change>, crate::write::WriteError>, words: &str) -> bool {
        matches!(r, Err(crate::write::WriteError::Unsupported(w)) if w.contains(words))
    }

    /// The items of a tree: each element, each attribute and each text node of it, and the longest name or value of an attribute, with children of children.
    #[test]
    fn a_tree_is_measured_by_its_items_and_its_widest_attribute() {
        let el = |name: &str, attrs: &[(&str, &str)], children: Vec<Node>| El {
            name: name.to_owned(),
            attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            children,
        };
        let tree = el(
            "a",
            &[("x", "12345"), ("yy", "1")],
            vec![
                Node::El(el("b", &[("z", "123456789")], vec![Node::El(el("d", &[], Vec::new()))])),
                Node::Text("text".to_owned()),
                Node::El(el("c", &[], Vec::new())),
            ],
        );
        // a: 1 + 2 attributes, a text node, b: 1 + 1 attribute, d: 1, c: 1.
        assert_eq!(measure(&tree), (9, 3 + 1 + 2 + 1 + 1, 3));
        assert_eq!(measure(&el("only", &[], Vec::new())), (0, 1, 1));
        assert_eq!(measure(&el("long", &[("a-name-of-eleven", "v")], Vec::new())), (16, 2, 1));
    }

    /// The old file is read when it is no bigger than 64 MiB and is refused, in words, past it (checked before it is parsed).
    #[test]
    fn an_old_file_of_the_most_bytes_is_read_and_one_byte_more_is_refused() {
        let limits = crate::Limits { max_write_bytes: u64::MAX, ..crate::Limits::default() };
        let doc = with_master("m");
        let exact = vec![0u8; MOST_BYTES];
        let r = planned(&doc, Some(&exact), &limits);
        assert!(matches!(r, Ok(Some(Change::Write(_)))), "{r:?}");
        let over = vec![0u8; MOST_BYTES + 1];
        let r = planned(&doc, Some(&over), &limits);
        assert!(refused_for(&r, "more than 64 MiB (it is not written again)"), "{r:?}");
    }

    /// An attribute holds what the parser's cap says and no more: a master's name of that many bytes is written and one byte more is refused in words.
    #[test]
    fn a_value_of_the_attribute_cap_is_written_and_one_byte_more_is_refused() {
        let mut limits = crate::Limits::default();
        limits.container.xml.max_attr_bytes = 200;
        let r = planned(&with_master(&"n".repeat(200)), None, &limits);
        assert!(matches!(r, Ok(Some(Change::Write(_)))), "{r:?}");
        let r = planned(&with_master(&"n".repeat(201)), None, &limits);
        assert!(refused_for(&r, "too long for an attribute"), "{r:?}");
    }

    /// The file made holds the nodes the reader's trees may have and no more.
    #[test]
    fn a_file_of_the_most_nodes_is_written_and_one_node_more_is_refused() {
        let doc = with_master("m");
        let tree = tree(
            None,
            &doc,
            &Native::default().after(&doc, &Structure::identity(doc.pages.len())),
            &[],
            &mut Steps::new(u64::MAX),
            &mut Gate::none(),
        )
        .unwrap()
        .unwrap();
        let (_, nodes, _) = measure(&tree);
        let limits = crate::Limits { max_read_xml_nodes: nodes, ..crate::Limits::default() };
        let r = planned(&doc, None, &limits);
        assert!(matches!(r, Ok(Some(Change::Write(_)))), "{r:?}");
        let limits = crate::Limits { max_read_xml_nodes: nodes - 1, ..crate::Limits::default() };
        let r = planned(&doc, None, &limits);
        assert!(refused_for(&r, "more elements than the reader holds"), "{r:?}");
    }

    /// Whether the reader's parser takes the file made of `root` at `limits`: the limits `parse` reads the document file at.
    fn reader_parses(root: &El, limits: &crate::Limits) -> bool {
        let bytes = diacraft_container::xml::to_bytes(root, &[], &[], true).unwrap();
        let mut parse_at = parse_limits(limits);
        parse_at.max_nodes = parse_at.max_nodes.min(limits.max_read_xml_nodes);
        diacraft_container::xml::parse_counted(&bytes, &parse_at, &mut diacraft_container::Work::default()).is_ok()
    }

    /// Whether the writer makes a file of the tree `root` at `limits`.
    fn writer_writes(root: El, limits: &crate::Limits) -> bool {
        let doc = Document::empty();
        let after = Native::default().after(&doc, &Structure::identity(doc.pages.len()));
        written(root, limits, &ReadCheck { doc: &doc, after: &after, room: u64::MAX, walk: 0 }).is_ok()
    }

    fn element(name: &str, attrs: Vec<(String, String)>, children: Vec<Node>) -> El {
        El { name: name.to_owned(), attrs, children }
    }

    /// Item 3: the writer takes the file where the reader's parser takes it, at the depth, the nodes and the attribute width, one step either side of each: a file
    /// nested to the reader's depth is written and one level more is refused, as the parser refuses it (the depth had no test); and the same for the nodes and the widest value.
    #[test]
    fn the_writer_takes_a_file_where_the_readers_parser_takes_it_at_the_depth_the_nodes_and_the_widest_value() {
        let chain = |depth: usize| {
            let mut tree = element("diacraft:x", Vec::new(), Vec::new());
            for _ in 1..depth {
                tree = element("diacraft:x", Vec::new(), vec![Node::El(tree)]);
            }
            tree
        };
        let limits = crate::Limits::default();
        let ceiling = diacraft_container::limits::XML_DEPTH_CEILING;
        for depth in [ceiling - 1, ceiling, ceiling + 1, ceiling + 2] {
            let (writes, parses) = (writer_writes(chain(depth), &limits), reader_parses(&chain(depth), &limits));
            assert_eq!(writes, parses, "depth {depth}: the writer says {writes}, the reader's parser {parses}");
            assert_eq!(writes, depth <= ceiling, "depth {depth}");
        }
        // The nodes: a root with `k` empty children is `k + 1` of them. At `max_read_xml_nodes` of exactly that, and one either side.
        let fan = |k: usize| element("diacraft:x", Vec::new(), (0..k).map(|_| Node::El(element("diacraft:y", Vec::new(), Vec::new()))).collect());
        for nodes in [49usize, 50, 51, 52] {
            let limits = crate::Limits { max_read_xml_nodes: nodes, ..crate::Limits::default() };
            let (writes, parses) = (writer_writes(fan(50), &limits), reader_parses(&fan(50), &limits));
            assert_eq!(writes, parses, "{nodes} nodes for a tree of 51: the writer says {writes}, the reader's parser {parses}");
            assert_eq!(writes, nodes >= 51, "{nodes} nodes for a tree of 51");
        }
        // The widest value of an attribute.
        let wide = |n: usize| element("diacraft:x", vec![("v".to_owned(), "w".repeat(n))], Vec::new());
        let mut limits = crate::Limits::default();
        limits.container.xml.max_attr_bytes = 100;
        for n in [99usize, 100, 101] {
            let (writes, parses) = (writer_writes(wide(n), &limits), reader_parses(&wide(n), &limits));
            assert_eq!(writes, parses, "a value of {n} bytes at 100: the writer says {writes}, the reader's parser {parses}");
            assert_eq!(writes, n <= 100, "a value of {n} bytes at 100");
        }
    }

    /// The most themes a file holds are written and one more is refused in words: the reader refuses a file of more.
    #[test]
    fn the_most_themes_are_written_and_one_more_is_refused() {
        let themes = |n: usize| {
            let mut doc = Document::empty();
            for k in 0..n {
                doc.themes.push(Arc::new(Theme { part: Arc::from(format!("t{k}").as_str()), bytes: Arc::from(vec![7u8; 3]), ..Theme::default() }));
            }
            doc
        };
        let limits = crate::Limits::default();
        let r = planned(&themes(MAX_THEMES), None, &limits);
        assert!(matches!(r, Ok(Some(Change::Write(_)))), "{:?}", r.map(|c| c.map(|_| ())));
        let r = planned(&themes(MAX_THEMES + 1), None, &limits);
        assert!(refused_for(&r, "more themes than DiaCraft's document file holds"), "{:?}", r.map(|c| c.map(|_| ())));
    }

    /// The file made is no more than 64 MiB: one of exactly that many bytes is written and one byte more is refused. The size is made by a theme of many bytes and a
    /// part name of the length that is left over.
    #[test]
    fn a_file_made_of_the_most_bytes_is_written_and_one_byte_more_is_refused() {
        let theme = |bytes: usize, part: usize| {
            let mut doc = Document::empty();
            doc.themes.push(Arc::new(Theme { part: Arc::from("p".repeat(part).as_str()), bytes: Arc::from(vec![7u8; bytes]), ..Theme::default() }));
            doc
        };
        let limits = crate::Limits { max_write_bytes: u64::MAX, ..crate::Limits::default() };
        // What the file holds besides the text of the theme and the part's name: made by a theme of three bytes (four characters) and a part of one.
        let small = match planned(&theme(3, 1), None, &limits) {
            Ok(Some(Change::Write(b))) => b.len(),
            other => panic!("{other:?}"),
        };
        let fixed = small - 4 - 1;
        let bytes = 50_331_300usize;
        let text = 4 * bytes.div_ceil(3);
        let part = MOST_BYTES - fixed - text;
        assert!(part > 0 && part < 512 * 1024, "{part}");
        match planned(&theme(bytes, part), None, &limits) {
            Ok(Some(Change::Write(b))) => assert_eq!(b.len(), MOST_BYTES),
            other => panic!("a file of exactly the most is written: {:?}", other.map(|c| c.map(|_| ()))),
        }
        let r = planned(&theme(bytes, part + 1), None, &limits);
        assert!(refused_for(&r, "a document file of more than 64 MiB"), "{:?}", r.map(|c| c.map(|_| ())));
    }

    /// A row of a `Connection` section whose `X` and `Y` have the formulas `x` and `y` (none for a constant), numbered `ix`.
    fn glue_row(ix: Option<u32>, x: Option<&str>, y: Option<&str>) -> Arc<diacraft_doc::Row> {
        let mut row = diacraft_doc::Row { ix, ..diacraft_doc::Row::default() };
        for (name, formula) in [("X", x), ("Y", y)] {
            let cell = match formula {
                Some(f) => Cell::with_formula(f, Some(Value::from_text("1"))),
                None => constant("0"),
            };
            row.cells.insert(CellName::new(name), cell);
        }
        Arc::new(row)
    }

    /// The four rows the reader gives every shape, numbered 0 to 3, in a section named `name`.
    fn four_rows(name: &str) -> diacraft_doc::Section {
        let mut section = diacraft_doc::Section::new(name, None);
        for (ix, (x, y)) in
            [(Some("Width*0.5"), Some("Height")), (None, Some("Height*0.5")), (Some("Width*0.5"), None), (Some("Width"), Some("Height*0.5"))]
                .into_iter()
                .enumerate()
        {
            section.rows.push(glue_row(Some(ix as u32), x, y));
        }
        section
    }

    /// The standard `Connection` rows are no data of a group, and nothing else is: the count of rows, each row's number and the formulas of each row's place are all part
    /// of it (a section the reader would not rebuild is the group's data).
    #[test]
    fn a_connection_section_is_the_standard_one_only_when_every_row_is() {
        assert!(is_standard_connection(&four_rows("Connection")));
        // A row number out of place, a row with no number (the reader numbers it), a missing row, an extra row.
        let mut section = four_rows("Connection");
        section.rows.get_mut(2).map(Arc::make_mut).unwrap().ix = Some(7);
        assert!(!is_standard_connection(&section), "a row at another place");
        let mut section = four_rows("Connection");
        section.rows.get_mut(2).map(Arc::make_mut).unwrap().ix = None;
        assert!(is_standard_connection(&section), "a row with no number is numbered by its place");
        let mut section = four_rows("Connection");
        section.rows.pop();
        assert!(!is_standard_connection(&section), "three rows");
        let mut section = four_rows("Connection");
        section.rows.push(glue_row(Some(4), Some("Width*0.5"), Some("Height")));
        assert!(!is_standard_connection(&section), "five rows");
        // Each of the two formulas of each row, taken one at a time.
        for at in 0..4 {
            let places =
                [(Some("Width*0.5"), Some("Height")), (None, Some("Height*0.5")), (Some("Width*0.5"), None), (Some("Width"), Some("Height*0.5"))];
            let (x, y) = places[at];
            let mut wrong_x = four_rows("Connection");
            *wrong_x.rows.get_mut(at).map(Arc::make_mut).unwrap() = Arc::unwrap_or_clone(glue_row(Some(at as u32), Some("Width*0.25"), y));
            assert!(!is_standard_connection(&wrong_x), "row {at}: another X");
            let mut wrong_y = four_rows("Connection");
            *wrong_y.rows.get_mut(at).map(Arc::make_mut).unwrap() = Arc::unwrap_or_clone(glue_row(Some(at as u32), x, Some("Height*0.25")));
            assert!(!is_standard_connection(&wrong_y), "row {at}: another Y");
        }
    }

    /// What a group's own sheet holds that the reader rebuilds: the frame, the pose and the standard `Connection` rows only, and no other section even if its rows are
    /// the standard ones; a `Connection` section that is not standard is data.
    #[test]
    fn what_the_reader_rebuilds_of_a_group_is_its_frame_and_the_standard_connection_rows_and_nothing_else() {
        let mut sheet = Sheet::default();
        for name in ["PinX", "Width", "Angle", "Cost"] {
            sheet.cells.insert(CellName::new(name), constant("1"));
        }
        sheet.sections.push(Arc::new(four_rows("Connection")));
        sheet.sections.push(Arc::new(four_rows("User")));
        let native = group_native(&sheet, true, false);
        assert_eq!(native.cells.len(), 3, "the frame and the pose, and not `Cost`");
        assert!(!native.cells.contains_key("Cost") && native.cells.contains_key("Angle"));
        assert_eq!(native.sections.len(), 1, "the standard `Connection` rows, and not a `User` section that looks like them");
        assert_eq!(native.sections[0].name.as_str(), "Connection");
        let mut odd = Sheet::default();
        let mut section = four_rows("Connection");
        section.rows.pop();
        odd.sections.push(Arc::new(section));
        assert!(group_native(&odd, true, false).sections.is_empty(), "a `Connection` section of three rows is the group's data");
        // A group with no `Connection` section: the reader gives it the standard one, so the sheet it rebuilds holds one.
        let bare = group_native(&Sheet::default(), true, false);
        assert_eq!(bare.sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Connection"]);
        assert!(group_native(&Sheet::default(), false, false).sections.is_empty(), "a group with no key of its own is given no entry for that alone");
    }

    /// A glue point row a `draw:glue-point` of the markup made: a row at `ix` whose element is held with the stamp of the row when it is `valid`, and with another when it is not.
    fn held_glue_row(ix: u32, valid: bool) -> Arc<diacraft_doc::Row> {
        let mut row = Arc::unwrap_or_clone(glue_row(Some(ix), Some("Width*0.25"), Some("Height*0.75")));
        let stamp = diacraft_doc::stamp_row(&row);
        let held = diacraft_doc::Held {
            group: diacraft_doc::HeldGroup::Glue,
            stamp: Some(if valid { stamp } else { diacraft_doc::stamp_row(&diacraft_doc::Row::default()) }),
            attrs: diacraft_doc::RawAttrs::from_pairs([("draw:id", ix.to_string().as_str())]),
            kept: diacraft_doc::Passthrough::new(),
        };
        row.ext.make_mut().odf.held = Some(held);
        Arc::new(row)
    }

    /// **On the model path the rows of a group's own `draw:glue-point` elements are what the markup gives** (the four standard rows, then rows whose element is held with the
    /// row's stamp), so a group that holds nothing else has no entry, and with no key of its own is not refused for them; a row whose stamp no longer holds (the point was moved), or
    /// a section that is not the standard rows then held ones, is the group's data as before. Off the model path the rows are data, as they were.
    #[test]
    fn a_groups_own_glue_points_are_what_the_markup_gives_on_the_model_path_only() {
        let sheet_of = |rows: &[Arc<diacraft_doc::Row>]| {
            let mut section = four_rows("Connection");
            section.rows.extend(rows.iter().cloned());
            let mut sheet = Sheet::default();
            sheet.sections.push(Arc::new(section));
            sheet
        };
        let held = sheet_of(&[held_glue_row(4, true), held_glue_row(5, true)]);
        assert_eq!(group_native(&held, true, true).sections.len(), 1, "the model path: the section is the one the markup gives");
        assert!(sheet_sections_equal(&group_native(&held, false, true), &held), "also for a group with no key of its own");
        assert!(group_native(&held, true, false).sections.is_empty(), "off the model path the rows are the group's data");
        let moved = sheet_of(&[held_glue_row(4, true), held_glue_row(5, false)]);
        assert!(group_native(&moved, true, true).sections.is_empty(), "a row whose stamp does not hold is data");
        let no_element = sheet_of(&[glue_row(Some(4), Some("Width*0.25"), Some("Height*0.75"))]);
        assert!(group_native(&no_element, true, true).sections.is_empty(), "a row with no element held is data");
        // Held rows after fewer than the four standard ones are no rebuilt section.
        let mut short = four_rows("Connection");
        short.rows.pop();
        short.rows.push(held_glue_row(4, true));
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(short));
        assert!(group_native(&sheet, true, true).sections.is_empty(), "three standard rows and a held one");
    }

    /// Whether the sections of two sheets are the same (the `Arc`s need not be).
    fn sheet_sections_equal(a: &Sheet, b: &Sheet) -> bool {
        a.sections.len() == b.sections.len()
            && a.sections.iter().zip(b.sections.iter()).all(|(x, y)| x.rows.len() == y.rows.len() && x.name == y.name)
    }

    /// An entry is made for a group with a `Connection` section that is not the standard one, and for none that has the standard one. **A group with a name of its own
    /// and no `Connection` section has an entry that says the section is gone**: the reader gives every group the standard rows, so without it the group
    /// came back with four connection points it did not have. A group with no key of its own is given none for that alone.
    #[test]
    fn a_group_whose_connection_rows_are_the_standard_ones_has_no_entry_and_one_whose_are_not_has() {
        let page_of = |section: Option<diacraft_doc::Section>| {
            let mut group = diacraft_doc::Shape::new(1);
            group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(diacraft_doc::Shape::new(2))]);
            group.kind = diacraft_doc::ShapeType::Group;
            if let Some(section) = section {
                Arc::make_mut(&mut group.sheet).sections.push(Arc::new(section));
            }
            let mut page = Page::new(1, "P");
            page.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(group)]);
            page
        };
        let entries = |section| groups_of(&page_of(section), 1, &mut Steps::new(u64::MAX)).len();
        assert_eq!(entries(None), 0, "a group with no key of its own is given no entry for having no connection points");
        let mut named = page_of(None);
        Arc::make_mut(named.shapes.get_mut(0).unwrap()).name_u = Some(Arc::from("G"));
        let none = groups_of(&named, 1, &mut Steps::new(u64::MAX));
        assert_eq!(none.len(), 1, "a group with a name of its own and no connection points says so: {none:?}");
        assert_eq!(none[0].attr("gone"), Some("s.Connection._"), "{:?}", none[0]);
        assert_eq!(none[0].elements().count(), 0, "and holds nothing else: {:?}", none[0]);
        assert_eq!(entries(Some(four_rows("Connection"))), 0, "the standard rows are rebuilt by the reader");
        let mut three = four_rows("Connection");
        three.rows.pop();
        assert_eq!(entries(Some(three)), 1, "other rows are the group's data");
        assert_eq!(entries(Some(four_rows("User"))), 1, "a section of another name is data");
    }

    /// A page of one group (`1`, one member `2`) that holds the cell `Cost`, so that it has an entry; with the `Connect` rows `connects` (each names the shape `to`).
    fn page_with_a_group(connects: &[u32]) -> Page {
        let mut group = diacraft_doc::Shape::new(1);
        group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(diacraft_doc::Shape::new(2))]);
        group.kind = diacraft_doc::ShapeType::Group;
        Arc::make_mut(&mut group.sheet).cells.insert(CellName::new("Cost"), constant("3"));
        let mut page = Page::new(1, "P");
        page.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(group)]);
        let rows: Vec<diacraft_doc::Connect> =
            connects.iter().map(|to| diacraft_doc::Connect { from_sheet: 2, to_sheet: *to, ..diacraft_doc::Connect::default() }).collect();
        page.connects = diacraft_doc::Connects::from_vec(rows);
        page
    }

    /// A page of groups, one for each of `names` (a group of id 10, 20, ... holding a shape of the next id), each with the cell `Cost` when `cost` says so.
    fn page_of_groups(names: &[Option<&str>], cost: Option<&str>) -> Page {
        let groups: Vec<Arc<diacraft_doc::Shape>> = names
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let id = 10 * (u32::try_from(k).unwrap() + 1);
                let mut g = diacraft_doc::Shape::new(id);
                g.name_u = name.map(Arc::from);
                g.name = g.name_u.clone();
                g.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(diacraft_doc::Shape::new(id + 1))]);
                g.kind = diacraft_doc::ShapeType::Group;
                if let Some(c) = cost {
                    Arc::make_mut(&mut g.sheet).cells.insert(CellName::new("Cost"), constant(c));
                }
                Arc::new(g)
            })
            .collect();
        let mut page = Page::new(1, "P");
        page.shapes = diacraft_doc::ShapeList::from_vec(groups);
        page
    }

    /// The review: an entry is keyed by the name of its group when no other group of the page has it (no `nth`); one whose group has no name, an empty
    /// one or one another group has says its place among the groups of the name in `nth`, always (even the first), which is the mark of an entry with no key.
    #[test]
    fn an_entry_is_keyed_by_a_name_unique_on_the_page_and_the_others_say_their_number() {
        let entries = |names: &[Option<&str>]| -> Vec<(String, Option<String>)> {
            groups_of(&page_of_groups(names, Some("1")), 1, &mut Steps::new(u64::MAX))
                .iter()
                .map(|e| (e.attr("name").unwrap().to_owned(), e.attr("nth").map(str::to_owned)))
                .collect()
        };
        let pair = |a: &str, n: Option<&str>| (a.to_owned(), n.map(str::to_owned));
        assert_eq!(entries(&[Some("A"), Some("B")]), [pair("A", None), pair("B", None)]);
        assert_eq!(entries(&[Some("T"), Some("T")]), [pair("T", Some("1")), pair("T", Some("2"))]);
        assert_eq!(entries(&[None]), [pair("", Some("1"))]);
        assert_eq!(entries(&[Some("")]), [pair("", Some("1"))], "an empty name is none");
        assert_eq!(entries(&[None, None]), [pair("", Some("1")), pair("", Some("2"))]);
        assert_eq!(entries(&[Some("A"), Some("T"), Some("T")]), [pair("A", None), pair("T", Some("1")), pair("T", Some("2"))]);
        // A group inside a group counts for its name too.
        let mut page = page_of_groups(&[Some("A"), Some("B")], Some("1"));
        let inner = Arc::make_mut(Arc::make_mut(page.shapes.get_mut(0).unwrap()).children.get_mut(0).unwrap());
        inner.name_u = Some(Arc::from("B"));
        inner.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(diacraft_doc::Shape::new(99))]);
        inner.kind = diacraft_doc::ShapeType::Group;
        Arc::make_mut(&mut inner.sheet).cells.insert(CellName::new("Cost"), constant("1"));
        let nested: Vec<(String, Option<String>)> = groups_of(&page, 1, &mut Steps::new(u64::MAX))
            .iter()
            .map(|e| (e.attr("name").unwrap().to_owned(), e.attr("nth").map(str::to_owned)))
            .collect();
        assert_eq!(
            nested,
            [pair("A", None), pair("B", Some("1")), pair("B", Some("2"))],
            "the inner group has the name of the second: neither is keyed"
        );
    }

    /// The groups of a page that need a key are the ones [`group_entries_of`] makes an entry for: one with a cell beyond its frame, one that holds no connection points, one linked
    /// to a master, one a `Connect` row names; a group that holds none of these (the four standard connection points the reader gives a group a file had, and its members) needs none, and the
    /// walk costs a step for each shape and each cell, and stops past its room.
    #[test]
    fn a_group_needs_a_key_when_it_holds_data_no_connection_points_a_link_to_a_master_or_an_id_another_shape_names() {
        // The groups of the page as a file gives them: each with the four standard connection points.
        let as_read = |names: &[Option<&str>], cost: Option<&str>| {
            let mut page = page_of_groups(names, cost);
            for at in 0..page.shapes.len() {
                let group = Arc::make_mut(page.shapes.get_mut(at).unwrap());
                Arc::make_mut(&mut group.sheet).sections.push(Arc::new(four_rows("Connection")));
            }
            page
        };
        let ids = |page: &Page| {
            let keys = groups_needing_a_key(page, u64::MAX);
            assert!(!keys.over);
            let mut ids: Vec<u32> = keys.ids.into_iter().collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(ids(&as_read(&[None, Some("A")], None)), Vec::<u32>::new(), "nothing on them but the four points");
        assert_eq!(ids(&as_read(&[None, Some("A")], Some("1"))), [10, 20], "a cell of their own");
        assert_eq!(ids(&page_of_groups(&[None, Some("A")], None)), [10, 20], "no connection points: the entry says so");
        let mut linked = as_read(&[None, None], None);
        Arc::make_mut(linked.shapes.get_mut(1).unwrap()).master_shape = Some(4);
        assert_eq!(ids(&linked), [20], "a link to a master's shape");
        let mut named = as_read(&[None, None], None);
        named.connects =
            diacraft_doc::Connects::from_vec(vec![diacraft_doc::Connect { from_sheet: 11, to_sheet: 10, ..diacraft_doc::Connect::default() }]);
        assert_eq!(ids(&named), [10], "an `ID` a connection names");
        // A page that holds no group costs nothing but the look at its top level, and a walk past its room stops and says so.
        let plain = Page::new(1, "P");
        assert_eq!(groups_needing_a_key(&plain, 0), GroupKeys::default());
        let many = as_read(&[None; 50], Some("1"));
        let full = groups_needing_a_key(&many, u64::MAX);
        let short = groups_needing_a_key(&many, full.steps / 2);
        assert!(short.over && short.ids.len() < full.ids.len() && short.steps <= full.steps / 2, "{short:?} {full:?}");
    }

    /// An `ID` that a `Connect` row names is kept for a group that has a key, and is not kept for one with none (it would be found by its place): the entry is for data and
    /// for a link to a master then.
    #[test]
    fn an_id_alone_is_kept_for_a_keyed_group_and_not_for_one_with_no_key() {
        let connect = |to: u32| diacraft_doc::Connect { from_sheet: 11, to_sheet: to, ..diacraft_doc::Connect::default() };
        let with_connect = |names: &[Option<&str>]| {
            let mut page = page_of_groups(names, None);
            page.connects = diacraft_doc::Connects::from_vec(vec![connect(10)]);
            groups_of(&page, 1, &mut Steps::new(u64::MAX))
        };
        let keyed = with_connect(&[Some("A")]);
        assert_eq!((keyed.len(), keyed[0].attr("id"), keyed[0].attr("nth")), (1, Some("10"), None), "a keyed group keeps the `ID` a row names");
        assert!(with_connect(&[None]).is_empty(), "a group with no name keeps none");
        assert!(with_connect(&[Some("T"), Some("T")]).is_empty(), "nor does one whose name another group has");
        // With data, or a link to a master, the unkeyed group has an entry that says so, and the `ID` is in it when a row names it.
        let mut page = page_of_groups(&[None], Some("1"));
        page.connects = diacraft_doc::Connects::from_vec(vec![connect(10)]);
        let held = groups_of(&page, 1, &mut Steps::new(u64::MAX));
        assert_eq!((held.len(), held[0].attr("id"), held[0].attr("nth")), (1, Some("10"), Some("1")));
        let mut linked = page_of_groups(&[None], None);
        Arc::make_mut(linked.shapes.get_mut(0).unwrap()).master = Some(4);
        let held = groups_of(&linked, 1, &mut Steps::new(u64::MAX));
        assert_eq!((held.len(), held[0].attr("master"), held[0].attr("nth")), (1, Some("4"), Some("1")));
    }

    /// The file written has an entry with no key only when the file holds that entry already: a new one is refused in words, an unchanged one is kept, one that changed is
    /// refused, and an entry with a key is written whenever it is wanted.
    #[test]
    fn an_entry_with_no_key_is_written_only_when_the_file_holds_it_already() {
        let native = |doc: &Document| {
            Native { sheet: Arc::default(), pages: vec![native_page(1, Arc::default())], styles: Vec::new(), ..Native::default() }
                .after(doc, &Structure::identity(doc.pages.len()))
        };
        let doc_of = |page: Page| {
            let mut doc = Document::empty();
            doc.pages = diacraft_doc::SharedVec::from_vec(vec![Arc::new(page)]);
            doc
        };
        let tree_of = |old: Option<&El>, doc: &Document| tree(old, doc, &native(doc), &[], &mut Steps::new(u64::MAX), &mut Gate::none());
        // A group with a name and data: written, and the entry has no `nth`.
        let named = doc_of(page_of_groups(&[Some("A")], Some("1")));
        let written = tree_of(None, &named).unwrap().expect("a file for the entry");
        assert!(written.elements().all(|e| e.attr("nth").is_none()));
        // One with no name: refused when the file holds none, and kept when it holds the same entry.
        let unnamed = doc_of(page_of_groups(&[None], Some("1")));
        assert!(matches!(tree_of(None, &unnamed), Err(WriteError::Group { why: WORDS_UNKEYED_ENTRY, .. })));
        let held = El {
            name: format!("{}:document", body::P),
            attrs: vec![(format!("xmlns:{}", body::P), names::URN.to_owned())],
            children: groups_of(unnamed.pages.get(0).unwrap(), 1, &mut Steps::new(u64::MAX)).into_iter().map(Node::El).collect(),
        };
        assert!(tree_of(Some(&held), &unnamed).unwrap().is_some(), "the entry the file holds is kept");
        let changed = doc_of(page_of_groups(&[None], Some("2")));
        assert!(
            matches!(tree_of(Some(&held), &changed), Err(WriteError::Group { why: WORDS_UNKEYED_ENTRY, .. })),
            "an entry that changed is a new one"
        );
        // No data and nothing to keep: no entry, and nothing is refused.
        let none = tree_of(None, &doc_of(page_of_groups(&[None], None))).unwrap();
        assert!(none.is_none_or(|t| t.elements().all(|e| !e.name.ends_with(":group"))), "no entry for a group that holds nothing");
        // A group with a name of its own and no connection points has an entry that says so; one with no key has none for that alone (above), and is not refused.
        let named = tree_of(None, &doc_of(page_of_groups(&[Some("A")], None))).unwrap().expect("a file for the entry");
        let entries: Vec<Option<&str>> = named.elements().filter(|e| e.name.ends_with(":group")).map(|e| e.attr("gone")).collect();
        assert_eq!(entries, [Some("s.Connection._")]);
    }

    /// The merge: the item of a group's VSDX data is paired with the group's `ID`, as the group's entry is, and comes
    /// before it; **an item is never among the entries whose data the markup gives** ([`from_markup`]), so [`tree`] writes it where it leaves the group's entry out. A group with
    /// no name, unchanged since the read, that holds data the markup gives (the drawing as read is the drawing) and carries VSDX data of its own.
    #[test]
    fn the_item_of_an_unnamed_group_is_paired_with_its_id_and_never_left_out_as_the_markups() {
        let mut page = page_of_groups(&[None], Some("1"));
        Arc::make_mut(page.shapes.get_mut(0).unwrap()).raw = diacraft_doc::RawAttrs::from_pairs([("UniqueID", "{G}")]);
        let mut doc = Document::empty();
        doc.pages = diacraft_doc::SharedVec::from_vec(vec![Arc::new(page)]);
        let made = group_entries_of(doc.pages.get(0).unwrap(), 1, &mut Steps::new(u64::MAX), &mut Gate::none()).unwrap();
        let kinds: Vec<(u32, &str, Option<&str>, Option<&str>)> =
            made.iter().map(|(id, e)| (*id, e.name.rsplit(':').next().unwrap_or_default(), e.attr("of"), e.attr("nth"))).collect();
        assert_eq!(
            kinds,
            [(10, crate::ext::items::ITEM, Some("group"), Some("1")), (10, "group", None, Some("1"))],
            "the item, then the entry, each with the group's ID"
        );
        // The drawing as read is this one: the group's entry is the markup's own, and the item is not.
        let markup = from_markup(&doc, &doc, None, &mut Steps::new(u64::MAX), &mut Gate::none()).unwrap();
        let names: Vec<&str> = markup.iter().map(|e| e.name.rsplit(':').next().unwrap_or_default()).collect();
        assert_eq!(names, ["group"], "only the group's entry is the markup's: {markup:?}");
        // The file: the item, and no entry for the group; nothing refused.
        let native = Native { sheet: Arc::default(), pages: vec![native_page(1, Arc::default())], styles: Vec::new(), ..Native::default() }
            .after(&doc, &Structure::identity(doc.pages.len()));
        let written = tree(None, &doc, &native, &markup, &mut Steps::new(u64::MAX), &mut Gate::none()).unwrap().expect("a file for the item");
        let items: Vec<&El> =
            written.elements().filter(|e| e.name.ends_with(&format!(":{}", crate::ext::items::ITEM)) && e.attr("of") == Some("group")).collect();
        assert_eq!(items.len(), 1, "the group's item is written: {written:?}");
        assert_eq!((items[0].attr("name"), items[0].attr("nth")), (Some(""), Some("1")), "keyed by its place among the groups with no name");
        assert!(written.elements().all(|e| !e.name.ends_with(":group")), "the group's entry is left out: {written:?}");
        // The item is no `group` entry whatever the markup holds: a markup list that has the item itself leaves it in the file.
        let with_item: Vec<El> = made.into_iter().map(|(_, e)| e).collect();
        let written = tree(None, &doc, &native, &with_item, &mut Steps::new(u64::MAX), &mut Gate::none()).unwrap().expect("a file for the item");
        assert_eq!(written.elements().filter(|e| e.attr("of") == Some("group")).count(), 1, "the item is written");
    }

    /// The survivors of the mutation run: what the walks of a sheet cost, exactly: a step for the sheet and each of its cells and sections, for each section
    /// and each of its cells and rows (an empty section is one), and for each cell of each row (a row with none is none); and the walk stops where the room ends.
    #[test]
    fn a_sheet_is_charged_for_each_cell_section_row_and_row_cell_and_the_charge_stops_at_the_room() {
        let mut sheet = Sheet::default();
        for name in ["A", "B"] {
            sheet.cells.insert(CellName::new(name), constant("1"));
        }
        let mut user = diacraft_doc::Section::new("User", None);
        for name in ["X", "Y", "Z"] {
            user.cells.insert(CellName::new(name), constant("1"));
        }
        let mut four = diacraft_doc::Row { name: Some(Arc::from("r1")), ..diacraft_doc::Row::default() };
        for name in ["C0", "C1", "C2", "C3"] {
            four.cells.insert(CellName::new(name), constant("1"));
        }
        user.rows.push(Arc::new(four));
        user.rows.push(Arc::new(diacraft_doc::Row { name: Some(Arc::from("r2")), ..diacraft_doc::Row::default() }));
        sheet.sections.push(Arc::new(user));
        sheet.sections.push(Arc::new(diacraft_doc::Section::new("Prop", None)));
        // The sheet: 1 + 2 cells + 2 sections; the first section: 1 + 3 cells + 2 rows, and 4 and 0 cells of its rows; the empty one: 1.
        let cost = 1 + 2 + 2 + (1 + 3 + 2) + 4 + 1;
        let mut all = Steps::new(u64::MAX);
        assert!(charge_sheet(&mut all, &sheet));
        assert_eq!(all.used(), cost);
        let enough = |room: u64| charge_sheet(&mut Steps::new(room), &sheet);
        assert!(enough(cost), "exactly the room");
        assert!(!enough(cost - 1), "one step short: the empty section is not charged");
        assert!(!enough(5 + 6 + 3), "short in the cells of the rows");
        assert!(enough(5 + 6 + 4 + 1), "the first row's four cells and the empty section fit exactly");
        assert!(!enough(5 + 5), "short in the first section: its header, cells and rows");
        assert!(!enough(4), "short in the sheet's own");
        assert!(enough(5 + 6 + 4 + 1) && !enough(5 + 6 + 4), "an empty section costs the step of its own");
        // A sheet with nothing in it is one step.
        let mut one = Steps::new(u64::MAX);
        assert!(charge_sheet(&mut one, &Sheet::default()));
        assert_eq!(one.used(), 1);
    }

    /// The set of the `ID`s a page's shapes name is the groups' and no more: a `Connect` row that names a shape that is no group, and a formula that reads one, name none;
    /// a row and a formula that name a group do; a formula that reads an `ID` of no shape does not (the filters to groups were not pinned).
    #[test]
    fn the_ids_a_page_names_are_those_of_its_groups_and_no_other_shape() {
        let mut group = diacraft_doc::Shape::new(10);
        group.children = diacraft_doc::ShapeList::from_vec(vec![Arc::new(diacraft_doc::Shape::new(11))]);
        group.kind = diacraft_doc::ShapeType::Group;
        let mut reader = diacraft_doc::Shape::new(30);
        Arc::make_mut(&mut reader.sheet)
            .cells
            .insert(CellName::new("Cost"), Cell::with_formula("Sheet.30!X+Sheet.20!X+Sheet.77!X", Some(Value::from_text("1"))));
        let plain = diacraft_doc::Shape::new(20);
        let mut page = Page::new(1, "P");
        page.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(group), Arc::new(plain), Arc::new(reader)]);
        page.connects = diacraft_doc::Connects::from_vec(vec![
            diacraft_doc::Connect { from_sheet: 30, to_sheet: 20, ..diacraft_doc::Connect::default() },
            diacraft_doc::Connect { from_sheet: 30, to_sheet: 11, ..diacraft_doc::Connect::default() },
        ]);
        let named = |p: &Page| -> Vec<u32> {
            let mut ids: Vec<u32> = named_ids(p, &mut Steps::new(u64::MAX)).into_iter().collect();
            ids.sort_unstable();
            ids
        };
        assert!(named(&page).is_empty(), "rows and formulas that name shapes that are no group name none");
        let mut by_row = page.clone();
        by_row.connects =
            diacraft_doc::Connects::from_vec(vec![diacraft_doc::Connect { from_sheet: 30, to_sheet: 10, ..diacraft_doc::Connect::default() }]);
        assert_eq!(named(&by_row), [10], "a row that names the group");
        let mut by_formula = page.clone();
        let reader = Arc::make_mut(by_formula.shapes.get_mut(2).unwrap());
        Arc::make_mut(&mut reader.sheet)
            .cells
            .insert(CellName::new("Cost"), Cell::with_formula("Sheet.10!X+Sheet.20!X", Some(Value::from_text("1"))));
        assert_eq!(named(&by_formula), [10], "a formula that reads the group");
    }

    /// The survivors of the mutation run of the fix round: the check of the entries with no key costs a step and one more for each entry the file holds already, for each
    /// such entry the save would write, charged before it is looked at, and the check stops at the room.
    #[test]
    fn the_check_of_the_entries_with_no_key_costs_a_step_for_each_with_the_entries_the_file_holds() {
        let doc = {
            let mut doc = Document::empty();
            doc.pages = diacraft_doc::SharedVec::from_vec(vec![Arc::new(page_of_groups(&[None, None], Some("1")))]);
            doc
        };
        let native = Native { sheet: Arc::default(), pages: vec![native_page(1, Arc::default())], styles: Vec::new(), ..Native::default() }
            .after(&doc, &Structure::identity(doc.pages.len()));
        let held = El {
            name: format!("{}:document", body::P),
            attrs: vec![(format!("xmlns:{}", body::P), names::URN.to_owned())],
            children: groups_of(doc.pages.get(0).unwrap(), 1, &mut Steps::new(u64::MAX)).into_iter().map(Node::El).collect(),
        };
        assert_eq!(held.elements().count(), 2, "two entries with no key are held");
        let mut base = Steps::new(u64::MAX);
        children(&doc, &native, &mut base, &mut Gate::none()).unwrap();
        // Two entries wanted, two held: a step and the two held for each of the two.
        let mut all = Steps::new(u64::MAX);
        assert!(tree(Some(&held), &doc, &native, &[], &mut all, &mut Gate::none()).unwrap().is_some());
        assert_eq!(all.used(), base.used() + 2 * (1 + 2));
        // The room of the children and of the first entry and not of the second: refused in words and stays over.
        let mut short = Steps::new(base.used() + 3 + 2);
        assert_eq!(tree(Some(&held), &doc, &native, &[], &mut short, &mut Gate::none()), Err(WriteError::Unsupported(WORDS_STEPS)));
        assert!(short.over());
        let mut exact = Steps::new(base.used() + 6);
        assert!(tree(Some(&held), &doc, &native, &[], &mut exact, &mut Gate::none()).unwrap().is_some(), "exactly the room is enough");
    }

    /// A group with a link to a master has an entry for it whatever else it holds (a group has no style of its own to carry the link): the
    /// attributes `master` and `master-shape` are written for the link each has, and a group with neither and nothing else has none.
    #[test]
    fn a_group_linked_to_a_master_has_an_entry_that_names_the_link() {
        let entry_of = |master: Option<u32>, master_shape: Option<u32>| {
            let mut page = page_with_a_group(&[]);
            let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
            Arc::make_mut(&mut group.sheet).cells.clear();
            (group.master, group.master_shape) = (master, master_shape);
            groups_of(&page, 1, &mut Steps::new(u64::MAX))
        };
        assert!(entry_of(None, None).is_empty(), "no data, no name and no link: no entry");
        let both = entry_of(Some(4), Some(9));
        assert_eq!(both.len(), 1);
        assert_eq!((both[0].attr("master"), both[0].attr("master-shape")), (Some("4"), Some("9")));
        let only_master = entry_of(Some(4), None);
        assert_eq!((only_master.len(), only_master[0].attr("master"), only_master[0].attr("master-shape")), (1, Some("4"), None));
        let only_shape = entry_of(None, Some(9));
        assert_eq!((only_shape.len(), only_shape[0].attr("master"), only_shape[0].attr("master-shape")), (1, None, Some("9")));
    }

    /// A group that a `Connect` row names (a connector glued to it) keeps its `ID` in the entry, and so does one a formula names; one that nothing names does not. Each
    /// `Connect` row is a step, charged before it is looked at.
    #[test]
    fn a_group_that_a_connect_row_names_keeps_its_id_and_each_row_is_a_step() {
        let entry = |connects: &[u32]| {
            let mut steps = Steps::new(u64::MAX);
            let out = groups_of(&page_with_a_group(connects), 1, &mut steps);
            (out.first().and_then(|e| e.attr("id").map(str::to_owned)), steps.used())
        };
        let (none, base) = entry(&[]);
        assert_eq!(none, None, "nothing names it");
        assert_eq!(entry(&[7]).0, None, "a row that names another shape");
        let (named, with_one) = entry(&[1]);
        assert_eq!(named.as_deref(), Some("1"), "a row that names the group");
        assert_eq!(with_one, base + 1, "one row is one step");
        assert_eq!(entry(&[7, 1, 7, 7]).1, base + 4, "four rows are four steps, whatever they name");
        assert_eq!(entry(&[7, 1]).0.as_deref(), Some("1"), "the first row does not end the look");
    }

    /// The steps a page of one group costs, exactly: `named_ids` takes a step for each of the two shapes to find the groups (2), then charges each shape's sheet (the
    /// group's: one and its one cell; the member's: one), `group_name_counts` takes a step for each of the two shapes (2), and `group_entries` takes a step for each of the
    /// two shapes and charges the group's sheet twice (2 each).
    #[test]
    fn the_steps_of_a_page_of_one_group_are_counted_to_the_step() {
        let mut steps = Steps::new(u64::MAX);
        let out = groups_of(&page_with_a_group(&[]), 1, &mut steps);
        assert_eq!(out.len(), 1);
        assert_eq!(steps.used(), 2 + (2 + 1) + 2 + (1 + 2 * 2 + 1), "{}", steps.used());
    }

    /// A page that is not the first of its name says so in each entry (`page-nth`), and the first does not.
    #[test]
    fn the_entries_of_a_page_that_is_not_the_first_of_its_name_say_which_it_is() {
        let entries = |nth: u32| groups_of(&page_with_a_group(&[]), nth, &mut Steps::new(u64::MAX));
        assert_eq!(entries(1)[0].attr("page-nth"), None);
        assert_eq!(entries(2)[0].attr("page-nth"), Some("2"));
        assert_eq!(entries(5)[0].attr("page-nth"), Some("5"));
        assert_eq!(entries(1)[0].attr("page"), Some("P"));
    }

    /// A count of steps is charged before the work: exactly the room is taken, one more is refused and stays refused, and what was used is all the room once over.
    #[test]
    fn steps_take_the_room_and_no_more_and_say_so() {
        let mut steps = Steps::new(5);
        assert!(!steps.over() && steps.used() == 0);
        assert!(steps.take(3) && steps.take(2), "exactly the room is taken");
        assert!(!steps.over() && steps.used() == 5, "and that is not over");
        assert!(!steps.take(1), "one more is refused");
        assert!(steps.over() && steps.used() == 5, "all the room is used once over");
        assert!(!steps.take(0), "and it stays refused");
        // A request of more than is left at once is refused, whole.
        let mut steps = Steps::new(5);
        assert!(!steps.take(6) && steps.over() && steps.used() == 5);
        // Nothing asked of nothing is no refusal.
        let mut none = Steps::new(0);
        assert!(none.take(0) && !none.over());
    }
}
