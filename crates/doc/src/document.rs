//! The document: what `visio/document.xml` holds and the parts it reaches.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use diacraft_color::{Rgba, fixed_color};

use crate::comments::Comments;
use crate::odf::{DefFamily, MAX_DEFINITIONS, OdfDoc};
use crate::package::{CustomProp, Ext, Kept, MAX_PARTS, Part, VsdxHeld};
use crate::part::{Master, Page, Style, Theme};
use crate::raw::{Passthrough, RawAttrs, RawElement};
use crate::shape::{MAX_SHAPE_DEPTH, ShapeList};
use crate::shared::SharedVec;
use crate::sheet::Sheet;
use crate::sheet_ref::{Visit, walk};
use crate::{
    Cell, CellName, DocError, MAX_COLOR_ENTRIES, MAX_MASTERS, MAX_PAGES, MAX_SHAPES_PER_PAGE, MAX_STYLES, Value, check_given_page_name,
    check_page_name, page_name_key,
};

/// One `ColorEntry` of the color table: a color index above 23 and the color it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorEntry {
    /// `IX`: at most 253 and unique in the table by MS-VSDX; whatever the file holds is kept.
    pub ix: u32,
    /// `RGB`, verbatim: `#RRGGBB`.
    pub rgb: Arc<str>,
}

/// One `FaceName` of the font table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceName {
    /// `NameU`: what a `Font`, `AsianFont` or `ComplexScriptFont` cell names.
    pub name_u: Arc<str>,
    /// `UnicodeRanges`, `CharSets`, `Panos`, `Panose`, `Flags` and any other attribute, verbatim.
    pub raw: RawAttrs,
    /// What this declaration holds beyond its name: an `.odg` face's attributes exactly (family, generic family, pitch, sources), and what a newer
    /// `.diacraft` added. Filled by the `.odg` reader; the VSDX writer writes the face from `name_u` and `raw`.
    pub ext: Ext<Kept>,
}

/// The document properties the formula functions read: the elements of `docProps/core.xml` and
/// `docProps/app.xml` that are not the title (which is [`Document::title`]). The parts themselves stay at the Bytes level in the
/// package; these are what the reader took from them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocProps {
    pub subject: Option<Arc<str>>,
    pub creator: Option<Arc<str>>,
    pub last_modified_by: Option<Arc<str>>,
    pub keywords: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub category: Option<Arc<str>>,
    pub company: Option<Arc<str>>,
    pub manager: Option<Arc<str>>,
    pub hyperlink_base: Option<Arc<str>>,
    pub app_version: Option<Arc<str>>,
    /// `created`, `modified` and `lastPrinted` of `docProps/core.xml`, as text (`2031-03-07T20:05:09Z`): what `DOCCREATION`, `DOCLASTSAVE`,
    /// `DOCLASTEDIT` and `DOCLASTPRINT` read (and its kin). The reader of a file fills them.
    pub created: Option<Arc<str>>,
    pub modified: Option<Arc<str>>,
    pub last_printed: Option<Arc<str>>,
    /// `language` of `docProps/core.xml`: the locale a string date is read in.
    pub language: Option<Arc<str>>,
    /// The user-defined properties (`custom.xml`'s, `meta:user-defined`'s): name, type and value each, in order. Filled by the VSDX reader and the `.odg`
    /// reader; written by the writer of the format each came from, and by the other where it has a place.
    pub custom: Vec<CustomProp>,
}

/// A document: pages, masters, styles, themes, the color and font tables and the document's own sheet.
///
/// Every list is a [`SharedVec`] of `Arc`s (the color and font tables are one `Arc` each), and a `SharedVec` is one pointer to its chunks:
/// a clone takes one reference count for each list and each text, a handful in all, and an edit copies the one chunk of the one list it
/// writes to. A document of 65,536 styles and 65,536 masters is cloned for those few counts, not for 131,072 (`lib.rs`, Sharing). The
/// text (the title, a page's names) is an `Arc<str>` too: a reader may hand over 16 MiB of it, and no edit may copy that.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The model version the document was made with.
    pub version: u32,
    /// The title: `docProps/core.xml`'s `title`.
    pub title: Arc<str>,
    pub props: Arc<DocProps>,
    /// The pages index, in order. At most [`MAX_PAGES`] at a door ([`Document::add_page`], [`Document::check_limits`]).
    pub pages: SharedVec<Arc<Page>>,
    /// At most [`MAX_MASTERS`] at a door.
    pub masters: SharedVec<Arc<Master>>,
    /// The style sheets. Style 0, `No Style`, is the root. At most [`MAX_STYLES`] at a door.
    pub styles: SharedVec<Arc<Style>>,
    /// The theme parts: a package may hold none, one or several.
    pub themes: SharedVec<Arc<Theme>>,
    pub colors: Arc<Vec<ColorEntry>>,
    pub fonts: Arc<Vec<FaceName>>,
    /// The `DocumentSheet`.
    pub sheet: Arc<Sheet>,
    /// Elements of `VisioDocument` the model has no field for: `DocumentSettings`, `EventList`, `HeaderFooter`,
    /// `DataTransferInfo`, `PublishSettings`, and anything in another namespace.
    pub passthrough: Passthrough,
    /// The `Data` element of the DiaCraft solution part (`SolutionXML Name="DiaCraft"`), as read: the application's own data,
    /// which has no cell in the ShapeSheet (the Snap & Glue options no specification gives a bit for). `None` for a file
    /// with no such part, which gets none until something is set. See [`Document::diacraft_child`].
    pub diacraft: Option<Arc<RawElement>>,
    /// The document's ODF data, and the VSDX elements and unknown `.diacraft` content the document element had no slot for. Empty for a drawing that
    /// came from neither. Filled by the `.odg` reader, the VSDX reader; see [`crate::package`].
    pub ext: Ext<OdfDoc>,
    /// The parts of the package that no model field reads, each tagged by its format: macro projects and embeddings as bytes, custom XML, the thumbnail,
    /// and the unknown-role parts of a newer `.diacraft` (tagged [`crate::package::Format::Native`]). At most [`MAX_PARTS`] at a door.
    pub parts: SharedVec<Arc<Part>>,
    /// What a VSDX package holds at the document level that nothing else here does (relationships, content-type rows, the rest of the properties
    /// parts, `windows.xml`). `None` for a document that came from no VSDX file, until its window settings are set: it then holds the windows part alone
    /// ([`VsdxHeld::holds_windows_alone`]).
    pub vsdx: Option<Arc<VsdxHeld>>,
    /// The comments of the drawing and their authors: VSDX's comments part, `.odg`'s `office:annotation`, and `.diacraft`'s own element. Each
    /// names its page and its shape by id, so no move of a shape parts them; the commands that take a shape or a page out take its comments. Empty for a
    /// drawing that holds none, and a document that holds none writes no comments part.
    pub comments: Comments,
}

impl Document {
    /// The model version [`Document::new_blank`] makes.
    pub const CURRENT_VERSION: u32 = 1;

    /// A document with nothing in it: no pages, no styles. What a reader fills and what a test builds on.
    pub fn empty() -> Document {
        Document {
            version: Document::CURRENT_VERSION,
            title: Arc::from(""),
            props: Arc::new(DocProps::default()),
            pages: SharedVec::new(),
            masters: SharedVec::new(),
            styles: SharedVec::new(),
            themes: SharedVec::new(),
            colors: Arc::new(Vec::new()),
            fonts: Arc::new(Vec::new()),
            sheet: Arc::new(Sheet::default()),
            passthrough: Passthrough::new(),
            diacraft: None,
            ext: Ext::default(),
            parts: SharedVec::new(),
            vsdx: None,
            comments: Comments::new(),
        }
    }

    /// A new document: titled "Untitled", with the root style and one page, "Page-1" (the name gives a new drawing's first page).
    pub fn new_blank() -> Document {
        let mut doc = Document::empty();
        doc.title = Arc::from("Untitled");
        doc.styles.push(Arc::new(root_style()));
        doc.pages.push(Arc::new(new_page(1, "Page-1")));
        doc
    }

    /// The page with `id`.
    pub fn page(&self, id: u32) -> Option<&Page> {
        self.pages.iter().map(Arc::as_ref).find(|p| p.id == id)
    }

    /// The master with `id`.
    pub fn master(&self, id: u32) -> Option<&Arc<Master>> {
        self.masters.iter().find(|m| m.id == id)
    }

    /// The theme part a `ThemeIndex` of `theme_index` selects: the first whose `vt:themeScheme` has that `schemeEnum`.
    /// A part's position in the list is never the index. `None` when no part has it; what a cell does then (E4 says: use the first part
    /// and warn) is for the caller, which knows the cell.
    pub fn theme_by_index(&self, theme_index: u32) -> Option<&Arc<Theme>> {
        self.themes.iter().find(|t| t.theme_scheme_id.as_ref().is_some_and(|id| id.scheme_enum == theme_index))
    }

    /// The style with `id`.
    pub fn style(&self, id: u32) -> Option<&Arc<Style>> {
        self.styles.iter().find(|s| s.id == id)
    }

    /// The color `index` names: the fixed palette for 0 to 23, else the color table's entry with that `IX`
    /// `None` for an index the table does not hold, or an `RGB` that is not a color: no color is
    /// invented, and what a missing one draws as is the caller's rule.
    pub fn color(&self, index: i64) -> Option<Rgba> {
        if let Some(fixed) = fixed_color(index) {
            return Some(fixed);
        }
        let ix = u32::try_from(index).ok()?;
        self.colors.iter().find(|e| e.ix == ix).and_then(|e| Rgba::from_hex(&e.rgb))
    }

    /// Adds a page after the others and returns its id: one more than the highest id in use. Without a `name` the page is called
    /// "Page-N" for its id N, or for the next number whose name no page has, in `Name` or in `NameU`, compared without regard to case ([`Document::default_page_name`]). A `name` that is
    /// given is the page's as it is, however many pages have it: the model takes the names of a file, and the commands hold a name to the rules of [`Document::check_free_page_name`]. The
    /// name is checked here, at the write that sets it. Nothing changes when this fails.
    ///
    /// The new page's sheet is the shared default (US Letter landscape, one to one), so adding a page copies nothing.
    pub fn add_page(&mut self, name: Option<&str>) -> Result<u32, DocError> {
        if let Some(name) = name {
            check_page_name(name)?;
        }
        if self.pages.len() >= MAX_PAGES {
            return Err(DocError::TooManyPages);
        }
        let id = self.pages.iter().map(|p| p.id).max().map_or(Some(1), |highest| highest.checked_add(1)).ok_or(DocError::IdsExhausted)?;
        let name = name.map_or_else(|| self.default_page_name(id), str::to_owned);
        self.pages.push(Arc::new(new_page(id, &name)));
        Ok(id)
    }

    /// The name a page added without one is given: `Page-N` for the number N of the id it will have, or for the next number after it whose name no page has (`Name` and `NameU` both
    /// looked at, without regard to case). A drawing read from an `.odg` numbers its pages from 0, so the second page's id 1 would call it `Page-1` beside the first.
    pub fn default_page_name(&self, id: u32) -> String {
        let taken: HashSet<String> =
            self.pages.iter().flat_map(|p| std::iter::once(&p.name).chain(p.name_u.as_ref())).map(|n| page_name_key(n)).collect();
        // Among as many numbers as there are names taken, one is free.
        let first = u64::from(id);
        (first..=first.saturating_add(taken.len() as u64))
            .map(|n| format!("Page-{n}"))
            .find(|name| !taken.contains(&page_name_key(name)))
            .unwrap_or_else(|| format!("Page-{id}"))
    }

    /// The page, other than the one with the id `except`, whose name is `name` **compared without regard to case** ([`crate::page_name_key`]): the first of them.
    pub fn page_named(&self, name: &str, except: Option<u32>) -> Option<&Arc<Page>> {
        let key = page_name_key(name);
        self.pages.iter().find(|p| Some(p.id) != except && page_name_key(&p.name) == key)
    }

    /// Checks a name a command gives to a page: the rules of [`check_given_page_name`] (not empty, at most 31 characters, no control character, not "page" and digits) and that **no
    /// other page has the name**, compared without regard to case. `except` is the id of the page being renamed, which may be given its own name again, in another case too.
    /// Nothing is changed. A reader does not ask this of the names of a file.
    pub fn check_free_page_name(&self, name: &str, except: Option<u32>) -> Result<(), DocError> {
        check_given_page_name(name)?;
        if self.page_named(name, except).is_some() {
            return Err(DocError::PageNameTaken);
        }
        Ok(())
    }

    /// Gives the page with `id` a new name. The name is checked here, at the write that changes it, and nothing else about the
    /// document is: a page whose imported name would not pass does not stop other edits. Nothing changes when this fails.
    /// Only the page is replaced: an undo step that shares it keeps its name.
    pub fn rename_page(&mut self, id: u32, name: &str) -> Result<(), DocError> {
        check_page_name(name)?;
        let at = self.pages.iter().position(|p| p.id == id).ok_or(DocError::NoSuchPage(id))?;
        let slot = self.pages.get_mut(at).ok_or(DocError::NoSuchPage(id))?;
        Arc::make_mut(slot).name = Arc::from(name);
        Ok(())
    }

    /// The page a command works on when it is not told which: **the first page that is not a background page**, and the first page there is when every page is one (`None` for a drawing
    /// with no page). A file lists its background pages first and a drawing a session makes keeps them so, so the first page of the list is a
    /// background page in any drawing that has one, where the page a person works on is the first the tabs show: the pages that are not backgrounds come before the backgrounds.
    pub fn first_foreground_page(&self) -> Option<&Arc<Page>> {
        self.pages.iter().find(|page| !page.background).or_else(|| self.pages.iter().next())
    }

    /// The drawing with its pages put in the order a file lists them: **every background page before every foreground page**, each kind in the order it has here (a stable partition),
    /// as all 70 files of the corpus that hold both kinds list them. `None` when a foreground page comes before no background page, which is to say
    /// when the pages are in that order already (a drawing made in a session keeps them so). Only the order of the list changes: the pages are the same pages (shared,
    /// not copied), with their `ID`s, names and `BackPage`s, and nothing else of the drawing is touched.
    pub fn pages_background_first(&self) -> Option<Document> {
        let mut seen_foreground = false;
        let out_of_order = self.pages.iter().any(|page| {
            if page.background {
                seen_foreground
            } else {
                seen_foreground = true;
                false
            }
        });
        if !out_of_order {
            return None;
        }
        let (back, front): (Vec<&Arc<Page>>, Vec<&Arc<Page>>) = self.pages.iter().partition(|page| page.background);
        let mut ordered = self.clone();
        ordered.pages = back.into_iter().chain(front).cloned().collect();
        Some(ordered)
    }

    /// The background pages of the page `id`, nearest first: its `BackPage`, that page's `BackPage`, and so on. A hostile file may
    /// make them a cycle or name a page that is not there; the chain ends at the first page it has already given and at the first
    /// missing one, so it is never longer than the document has pages.
    pub fn background_chain(&self, id: u32) -> Vec<&Arc<Page>> {
        // The first page with an `ID` is the one a lookup finds, as everywhere else.
        let mut by_id: HashMap<u32, &Arc<Page>> = HashMap::with_capacity(self.pages.len());
        for page in &self.pages {
            by_id.entry(page.id).or_insert(page);
        }
        let mut seen: HashSet<u32> = HashSet::from([id]);
        let mut chain = Vec::new();
        let mut at = by_id.get(&id).and_then(|p| p.back_page);
        while let Some(next) = at {
            if !seen.insert(next) {
                break;
            }
            let Some(page) = by_id.get(&next) else {
                break;
            };
            chain.push(*page);
            at = page.back_page;
        }
        chain
    }

    /// Adds a style after the others. At most [`MAX_STYLES`]: this is a door a document enters, and a style past the cap is refused
    /// and changes nothing. (A style with an `ID` already in use is kept, as a file may hold two, and the first is the one found.)
    pub fn add_style(&mut self, style: Style) -> Result<(), DocError> {
        if self.styles.len() >= MAX_STYLES {
            return Err(DocError::Limit { what: "styles", max: MAX_STYLES });
        }
        self.styles.push(Arc::new(style));
        Ok(())
    }

    /// Adds a master after the others. At most [`MAX_MASTERS`], as for [`Document::add_style`].
    pub fn add_master(&mut self, master: Master) -> Result<(), DocError> {
        if self.masters.len() >= MAX_MASTERS {
            return Err(DocError::Limit { what: "masters", max: MAX_MASTERS });
        }
        self.masters.push(Arc::new(master));
        Ok(())
    }

    /// Checks the document against the caps ([`MAX_PAGES`], [`MAX_STYLES`], [`MAX_MASTERS`], [`MAX_SHAPES_PER_PAGE`] a part,
    /// [`MAX_SHAPE_DEPTH`], and the limits of each sheet: [`Sheet::check_limits`]). It is the check at the door a file enters by: the
    /// reader runs it once on what it built, and refuses a file that fails. It costs a pass over every shape and cell, bounded by the caps
    /// themselves, so **no edit runs it** (gaps F2). A document built in memory by a test may be past a cap; this says so.
    pub fn check_limits(&self) -> Result<(), DocError> {
        // The held data first, before anything walks a page: the parts are a length, and the definitions are counted by family in one pass that ends at the
        // first item past a cap, so a hostile list costs at most 6 caps and one however long it is (and nothing when the document holds no ODF data).
        if self.parts.len() > MAX_PARTS {
            return Err(DocError::Limit { what: "parts", max: MAX_PARTS });
        }
        if let Some(data) = self.ext.get() {
            check_definitions(&data.odf)?;
        }
        if self.pages.len() > MAX_PAGES {
            return Err(DocError::TooManyPages);
        }
        if self.styles.len() > MAX_STYLES {
            return Err(DocError::Limit { what: "styles", max: MAX_STYLES });
        }
        if self.masters.len() > MAX_MASTERS {
            return Err(DocError::Limit { what: "masters", max: MAX_MASTERS });
        }
        if self.colors.len() > MAX_COLOR_ENTRIES {
            return Err(DocError::Limit { what: "colour table entries", max: MAX_COLOR_ENTRIES });
        }
        self.sheet.check_limits()?;
        for style in &self.styles {
            style.sheet.check_limits()?;
        }
        for master in &self.masters {
            master.sheet.check_limits()?;
            check_part(&master.shapes)?;
        }
        for page in &self.pages {
            page.sheet.check_limits()?;
            check_part(&page.shapes)?;
        }
        Ok(())
    }

    /// An audit of the page list: at most [`MAX_PAGES`] pages, each with a valid name and an id of its own. It costs a pass over
    /// the pages, and none over their shapes. It is for tests and for an import to report with; **no edit runs it**, because an
    /// imported document may hold a name that would not pass, and that must not stop every other edit. The names are checked at
    /// the writes that set them ([`Document::rename_page`], [`Document::add_page`]).
    pub fn validate(&self) -> Result<(), DocError> {
        if self.pages.len() > MAX_PAGES {
            return Err(DocError::TooManyPages);
        }
        let mut seen = HashSet::with_capacity(self.pages.len());
        for page in &self.pages {
            if !seen.insert(page.id) {
                return Err(DocError::DuplicatePageId(page.id));
            }
            check_page_name(&page.name)?;
        }
        Ok(())
    }
}

/// The held definitions against [`MAX_DEFINITIONS`] for each family. One pass, ending at the first definition that is past its family's cap.
fn check_definitions(odf: &OdfDoc) -> Result<(), DocError> {
    let mut seen = [0_usize; 6];
    for definition in &odf.definitions {
        crate::work::limit_item();
        let (at, what) = match definition.family {
            DefFamily::Marker => (0, "marker definitions"),
            DefFamily::Gradient => (1, "gradient definitions"),
            DefFamily::Opacity => (2, "opacity definitions"),
            DefFamily::Hatch => (3, "hatch definitions"),
            DefFamily::StrokeDash => (4, "stroke dash definitions"),
            DefFamily::FillImage => (5, "fill image definitions"),
        };
        if let Some(count) = seen.get_mut(at) {
            *count = count.saturating_add(1);
            if *count > MAX_DEFINITIONS {
                return Err(DocError::Limit { what, max: MAX_DEFINITIONS });
            }
        }
    }
    Ok(())
}

/// The shapes of one page or master part against the caps: how many, nested ones counted, how deep, and each shape's sheet.
fn check_part(shapes: &ShapeList) -> Result<(), DocError> {
    let mut count = 0_usize;
    let mut error = None;
    // Nothing matches, so the walk visits every shape to the depth limit; it stops at the first one that breaks a cap.
    walk(shapes, |shape, path| {
        count = count.saturating_add(1);
        if count > MAX_SHAPES_PER_PAGE {
            error = Some(DocError::Limit { what: "shapes in a part", max: MAX_SHAPES_PER_PAGE });
        } else if path.len() >= MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            error = Some(DocError::Limit { what: "levels of nested shapes", max: MAX_SHAPE_DEPTH });
        } else if let Err(e) = shape.sheet.check_limits() {
            error = Some(e);
        }
        if error.is_some() { Visit::Match } else { Visit::Descend }
    });
    error.map_or(Ok(()), Err)
}

/// A new page: the shared default sheet and no shapes.
fn new_page(id: u32, name: &str) -> Page {
    Page { sheet: Arc::clone(default_page_sheet()), ..Page::new(id, name) }
}

/// The sheet every new page starts from: US Letter landscape, drawn one to one, with no margins on the paper it prints on. The values are
/// ours to choose, and one `Arc` is shared by all pages made from it until one is written to.
///
/// **Our rule.** The four print margins are 0, where a file's root style holds a quarter of an inch: a margin is a blank
/// strip of the sheet, so a page of 11 by 8.5 inches inside quarter-inch margins would not fit the
/// 11 by 8.5 inch sheet it is held on and would print on four, and the page a new drawing starts with must print on one.
fn default_page_sheet() -> &'static Arc<Sheet> {
    static SHEET: OnceLock<Arc<Sheet>> = OnceLock::new();
    SHEET.get_or_init(|| {
        let mut sheet = Sheet::default();
        for (name, value) in [
            ("PageWidth", "11"),
            ("PageHeight", "8.5"),
            ("PageScale", "1"),
            ("DrawingScale", "1"),
            ("PageLeftMargin", "0"),
            ("PageRightMargin", "0"),
            ("PageTopMargin", "0"),
            ("PageBottomMargin", "0"),
        ] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
        }
        Arc::new(sheet)
    })
}

/// The root style of a new document, `No Style`: the three flags, the group and glue cells whose roots
/// reads off real files, and a basic line, fill and text look: a solid white fill, a black solid line one hundredth
/// of an inch wide, and 12 point black text in the middle of its block with a margin of 4 points. The values are ours to choose and are
/// not taken from a sample or a template. **`DropOnPageScale` is 1** (the ruling on F40), as the root holds it in every file of the corpus
/// A shape no drop scaled reads 1, and a formula of a master that names the cell is the master's own size
/// The root of a drawing read from a file is the file's, and is never given the cell.
///
/// **The root states every text cell the program assumes** ([`crate::root_text`]): the whole `Paragraph` row 0 (a paragraph with no `HorzAlign` on its way is
/// centred here, and a reader that finds none reads it as left), the rest of the text block and the cells of the first run, each with the value the renderer assumes.
fn root_style() -> Style {
    let mut sheet = Sheet::default();
    for (name, value) in [
        ("EnableLineProps", "1"),
        ("EnableFillProps", "1"),
        ("EnableTextProps", "1"),
        ("DropOnPageScale", "1"),
        ("SelectMode", "1"),
        ("DisplayMode", "2"),
        ("IsSnapTarget", "1"),
        ("IsTextEditTarget", "1"),
        ("FillPattern", "1"),
        ("FillForegnd", "1"),
        ("FillBkgnd", "0"),
        ("LinePattern", "1"),
        ("LineColor", "0"),
        ("LineWeight", "0.01"),
        ("LeftMargin", "0.05555556"),
        ("RightMargin", "0.05555556"),
        ("TopMargin", "0.05555556"),
        ("BottomMargin", "0.05555556"),
    ] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    let bare = Style { id: 0, name: Some(Arc::from("No Style")), name_u: Some(Arc::from("No Style")), sheet: Arc::new(sheet), ..Style::default() };
    // The vertical alignment, the rest of the text block, the first run (row 0 of `Character`) and the first paragraph (row 0 of `Paragraph`).
    crate::root_text::complete(&bare, crate::root_text::Scope::Whole).unwrap_or(bare)
}
