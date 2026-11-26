//! The copy of the drawing an `.odg` save hands the writer: **each shape the file does not hold yet is flattened to what a new shape of an `.odg`
//! is**.
//!
//! The writer (`diacraft_odg::write`) writes a new shape as a native element and nothing else: a plain shape with cells of its own, its outline and its
//! four side connection points, plain text, and the style cells it has a style attribute for. The engine's new shapes are not that: `shape.drop` makes an
//! instance of a master that holds only its pin, a stencil shape links to its style sheet, a text box holds the text-block cells, a shape whose text was
//! formatted holds `Character` and `Paragraph` rows, and `connector.add` writes neither of the two cells the writer reads to tell a connector from a line. So
//! the save works on a **copy** of the document in which each such shape is replaced by its flattened form, and the engine's model is not changed.
//!
//! **What a flattened shape is.**
//!
//! * **Its links to its master kept**, no style-sheet link (the three links are the root style's, 0), and no ids of the file's (`xml:id` and `draw:id`
//!   of a shape's `raw` are never copied: an id names one element). A group's members are copied in the same way.
//! * The frame cells (`PinX` to `Height`, `Angle`, the flips when they are not 0, and the ends of a 1-D shape), and the style cells the writer has an
//!   attribute for (the fill, the line, the vertical alignment and the margins), **each as a constant of the value the shape resolves to**: found by
//!   inheritance through the master and the styles, a themed colour worked out, a palette index turned into its RGB (`formatting::look`, as the
//!   renderer reads them).
//! * The `Geometry` sections, and for a 2-D shape the `Connection` section, **merged with the master's**, each cell with the
//!   formula it resolves to and the value this shape holds: the writer recognises a basic shape by the forms `Width*0.5` and `Height`.
//! * A connector (a 1-D shape that is glued, that holds the route marker `connector.add` writes, or that holds a route cell already) has
//!   `ShapeRouteStyle` and `ConLineRouteExt` as the route style the engine draws it in (`draw::path_style`), which is what the writer reads to write
//!   a `draw:connector` and not a `draw:line`; and its marker row of `User`.
//! * Its plain text, `Data1` to `Data3`, foreign data and the attributes and elements of the file's own are as the shape has them: the writer says what
//!   it does not write of the last three ([`diacraft_odg::SaveNote::NewShapeUnwritten`]). A group is copied with its frame cells as constants, its own
//!   other cells and sections as they are (its data, which the document file carries), and each member flattened as a shape of the page is.
//!
//! **Nothing on a new shape is dropped without a word, and almost nothing is dropped at all.** The writer carries what a native
//! element cannot say in DiaCraft's own data on the shape's style (tier 2: every cell and section of the shape's own sheet, and its whole text), so the copy **carries
//! everything the shape holds itself** as it is: rounded corners, a shadow (a shape that has one from a style gets the cells as constants), the text block that is not the whole
//! shape, shape data (`Property` and `User` rows), the layer, the text's background and direction, the `Character`, `Paragraph` and `Tabs` rows, the markers and fields of
//! the text, and any other cell or section. A cell is **carried**, or **inert** (a cell the engine writes by itself and the file does not need: `ResizeMode`, the place of a
//! label on a connector with no text). **Two kinds of loss remain, and are classified and said** ([`KINDS`]): the text formatting a **style sheet** gives the text of a
//! shape or of its master's shape (the copy has no link to the style sheet; writes a shape's text from its own rows, but the common style a style sheet becomes holds graphic properties only, so a sheet's `Character` and `Paragraph` rows have no place in the file yet), and the link to the `Theme` style of
//! a shape whose three links are not that one style.
//!
//! **Which shapes.** A shape the index the engine holds does not know (it is new), and each shape an earlier save flattened whose copy is no longer what a
//! flattening of the model's shape gives (the writer compares the model with the pages as the save before had them, and those pages hold the flattened form):
//! a shape an earlier save flattened that **has not changed since** (the very shape of the model it was made from, in a drawing whose styles, themes and
//! colours are the very ones, whose master is the very master and whose connector is glued and drawn as it was) is given the copy that save made, at a step, and
//! is not flattened again (the review: a drawing of 45,000 new shapes saved, then 10,000 more, was refused, since every save flattened every
//! shape it had ever flattened). A shape of the file as it was read is never touched but for the colours a person chose (`constants`). **With no file** (`Basis::Model`), a member of a
//! new group that holds a file's data is such a shape too: a `draw:g` with no attribute holds no data of its own and is new to the save, and its members, flattened with it,
//! lost their links to their style sheets and their styles' parents.
//!
//! **Which masters** (then). The writer writes every master the model holds into DiaCraft's document file, and the copy
//! holds **every master of the model**, the file's own and the ones a drop brought, so that nothing is left out of a save. The copy of a shape **keeps its links to
//! its master** (`master` and `master_shape`, which the writer carries on the shape's own style, and for a group in its entry of the document file), and a file read
//! again has the shape linked to the master the file carries. (Until a later change a drop's masters were left out and the copies linked to none; before a change every
//! master was taken out of every save, the file's own included.)
//!
//! **Cost.** Each shape visited, cell resolved, cell audited, row and cell of a section is a step, held to [`MAX_STEPS`] before the work is done; a section of
//! more than [`MAX_ROWS`] rows or a shape of more than [`MAX_SECTIONS`] sections of one name is refused in words. **Each copy is charged to the memory a save may
//! hold before it is made** ([`Work::charge`]: the shape the model keeps and the copy the save holds, each at what the shape holds, `hold::shape_bytes`). Lookups
//! of a shape's place in the index and in what was flattened are by `ID` in a map. The copy of the document shares everything the save does not change.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, ColorEntry, Document, FormulaSlot, Links, MAX_SHAPE_DEPTH, Master, Owner, RawAttrs, Resolved, Row, RowKey, Section,
    Shape, ShapeList, ShapeRef, SharedVec, Sheet, SheetRef, Source, Style, Theme, Value,
};
use diacraft_odg::{ShapeIndex, TEXT_OBJECT_ROW};

use super::hold::Sizer;
use crate::draw::{PathStyle, Reader as DrawReader, page_mut, path_style};
use crate::formatting::{LookReader, LookVal, look_eval_ctx};
use crate::stencil::number_text;

/// The words of the refusal of a connector of the file glued to a shape the file lacks that name how the shape came back; `save.rs` gives that refusal the way
/// out that works (a redo), since the undo that put the shape back is the change, and undoing again does not save.
pub(crate) const PUT_BACK: &str = "put back by an undo";

/// The name of the `User` row the reader gives an object DiaCraft cannot draw and has no picture of (`crates/render/src/placeholder.rs`).
const OBJECT_ROW: &str = "ODFObject";

/// The local names of the elements that make a frame an object: the reader keeps them among the children of the shape it gives the frame.
const OBJECT_ELEMENTS: [&str; 6] = ["object", "object-ole", "applet", "plugin", "floating-frame", "table"];

/// The words of the refusal of **an object that the file does not hold**: a copy of one, or one an undo brought back after a save that deleted it. Whether the object has a
/// replacement picture the shape shows or only a box that says what it is, its element and the files that go with it are not copied yet,
/// so a save could only write a plain shape that has lost them: a box that draws nothing when the file is opened again, or a picture that is no longer the object (
/// for the first, B1 for the second). **A text box that holds the frame of an object** is the same case, since the model keeps nothing of the
/// frame. Said before any copy of the drawing is made; no name from the file is in it.
pub(crate) const OBJECT_NOT_IN_FILE: &str = "saving does not yet write this change: an object (an embedded object, a table, a 3-D scene or the like, which DiaCraft shows as its picture or as a labelled box), or a text box that holds one, that is not in the file, a copy of one or one an undo brought back: its element and the files that go with it are not copied yet. Delete it to save";

/// Whether the element called `name` (as the file wrote it, prefix and all) is one that makes a frame an object. **The local name decides, and no namespace**: the model
/// keeps an element's name as written and has lost the declarations of the ancestors that bound its prefix, so a frame that binds `draw` to another prefix is told by what
/// follows the colon. That errs toward refusing: a picture frame that keeps a child of another namespace with one of these local names is refused too.
fn is_object_element(name: &str) -> bool {
    let local = name.rsplit_once(':').map_or(name, |(_, local)| local);
    OBJECT_ELEMENTS.contains(&local)
}

/// Whether `shape` is an **object**: a foreign shape (a picture frame) that the reader read from a frame that holds an object, or any shape whose text holds the frame of one
/// A picture frame with **no picture** has the `User` row `ODFObject`, which names it for the box that stands for it; one **with a picture** (its replacement,
/// which the shape shows) has no row, and its object's element is among the children the shape keeps; a shape whose **text holds the frame of an object** has the row
/// [`TEXT_OBJECT_ROW`], which the reader gives it because the model keeps nothing of that frame. Only the shape's own sections and (for a picture frame) top-level kept
/// children are looked at, each charged a step in `work` before it is looked at, so that many copies of a shape that holds a great deal are refused by the cap and are not
/// walked one by one without a limit. **A shape that is not a picture frame is charged its sections** and the rows of its `User` section, since any shape's `User` section can
/// hold the row of a text.
pub(crate) fn is_object(shape: &Shape, work: &mut Work) -> Result<bool, String> {
    let foreign = shape.kind == diacraft_doc::ShapeType::Foreign;
    for section in shape.sheet.sections.iter() {
        work.tick(1)?;
        if section.name.as_str() != "User" {
            continue;
        }
        work.tick(section.rows.len() as u64)?;
        let marks = |name: &str| name == TEXT_OBJECT_ROW || (foreign && name == OBJECT_ROW);
        if section.rows.iter().any(|r| r.name.as_deref().is_some_and(marks)) {
            return Ok(true);
        }
    }
    if !foreign {
        return Ok(false);
    }
    // The children the `.odg` reader keeps are in the `odf` slot of the shape, and what the model made itself may be in `passthrough`: both are looked at.
    let odf = shape.ext.get().map(|d| &d.odf.kept.kept);
    work.tick((shape.passthrough.len() + odf.map_or(0, |k| k.len())) as u64)?;
    let is_object = |p: &diacraft_doc::Placed| matches!(&p.node, diacraft_doc::RawNode::Element(e) if is_object_element(&e.name));
    Ok(shape.passthrough.iter().any(is_object) || odf.is_some_and(|k| k.iter().any(is_object)))
}

/// The most steps one flattening may take (shapes visited, cells resolved and audited, rows and their cells copied).
pub(crate) const MAX_STEPS: u64 = 6_000_000;

/// The most rows of one section a shape is flattened with: the writer holds a custom shape's path to what a part may hold, and this is past the rows
/// of any shape a person draws.
pub(crate) const MAX_ROWS: usize = 20_000;

/// The most sections of one name (the `Geometry` sections) a shape is flattened with.
pub(crate) const MAX_SECTIONS: usize = 256;

/// The cells that make a shape's frame, which every shape has.
const FRAME: [&str; 6] = ["PinX", "PinY", "LocPinX", "LocPinY", "Width", "Height"];

/// The frame cells that are there only when they are not 0.
const FRAME_IF_SET: [&str; 3] = ["Angle", "FlipX", "FlipY"];

/// The ends of a 1-D shape.
const ENDS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// The four margins of the text block, written together.
const MARGINS: [&str; 4] = ["TopMargin", "BottomMargin", "LeftMargin", "RightMargin"];

/// The cells of the fill and the line the writer has an attribute for, besides the ones a 1-D shape does not have.
const STYLE_CELLS: [&str; 10] = [
    "FillPattern",
    "FillForegnd",
    "FillForegndTrans",
    "FillGradientEnabled",
    "LinePattern",
    "LineWeight",
    "LineColor",
    "LineColorTrans",
    "LineCap",
    "VerticalAlign",
];

/// The cells of a connector that the writer reads or the reader gives back: the route, and the triggers of its two ends.
const ROUTE_CELLS: [&str; 4] = ["ShapeRouteStyle", "ConLineRouteExt", "BegTrigger", "EndTrigger"];

// The kinds of what a new shape holds that the writer does not carry: a bit each, in what a flattened shape remembers of what a save said of it. (Bits 1 and 2 were the line ends,
// which the writer writes; bits 4, 8, 32, 128, 256 and 512 were rounded corners, a shadow, a text block that is not the whole shape, shape data, the layer and any other cell
// or section, which the copy carries: the writer carries them in DiaCraft's own data on the shape's style.)
pub(crate) const TEXT_FORMAT: Lost = 16;
pub(crate) const THEME_LINK: Lost = 1024;

/// What the writer does not carry, by kind: the bit and the words a warning of the save uses. A kind of loss that is not here is not said.
pub(crate) const KINDS: [(Lost, &str); 2] = [
    (TEXT_FORMAT, "text formatting (the character and paragraph settings a style sheet gives the text)"),
    (
        THEME_LINK,
        "the link to the Theme style (the file says one parent, so only a shape whose three links are that style keeps it: this one follows no theme when the file is opened again)",
    ),
];

/// The cells of a shadow, which a shape that has one from a style sheet is given as constants (the copy has no link to the sheet): the writer carries them.
const SHADOW_CELLS: [&str; 14] = [
    "ShdwPattern",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShdwOffsetX",
    "ShdwOffsetY",
    "ShdwType",
    "ShdwScaleFactor",
    "ShdwBlur",
    "ShdwObliqueAngle",
    "ShapeShdwType",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwScaleFactor",
    "ShapeShdwBlur",
];

/// The cells of a line end, which the writer writes for a 1-D shape: the number and the size class of each end.
const ARROW_CELLS: [(&str, &str); 2] = [("BeginArrow", "BeginArrowSize"), ("EndArrow", "EndArrowSize")];

/// Cells that are no loss: the engine writes them by itself (`ResizeMode`), and the file does not need them.
const INERT: [&str; 1] = ["ResizeMode"];

/// The most style sheets along a `TextStyle` chain the audit follows (a chain longer than that is a cycle).
pub(crate) const MOST_STYLE_STEPS: usize = 64;

/// The cells that qualify the text, which the writer has no attribute for.
const TEXT_FORMAT_CELLS: [&str; 3] = ["TextBkgnd", "TextBkgndTrans", "TextDirection"];

/// The cells that place the text block and qualify the text, which a shape that shows a text takes from its master when it holds none of its own.
const TEXT_BLOCK_CELLS: [&str; 10] =
    ["TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtAngle", "TxtWidth", "TxtHeight", "TextBkgnd", "TextBkgndTrans", "TextDirection"];

/// What the shapes an earlier save flattened are, to be given again: for each by its page's `ID` and its own, the shape of the model it was made from, the
/// copy, what it holds that is not written, and what the copy was made under. See the module documentation ("Which shapes").
#[derive(Clone, Debug, Default)]
pub(crate) struct Flattened {
    entries: BTreeMap<(u32, u32), Made>,
    context: Option<Context>,
}

impl Flattened {
    /// How many shapes.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, page: u32, shape: u32) -> bool {
        self.entries.contains_key(&(page, shape))
    }

    /// The bits of what the shape's copy lost, if it was flattened.
    pub(crate) fn lost(&self, page: u32, shape: u32) -> Option<Lost> {
        self.entries.get(&(page, shape)).map(|m| m.lost)
    }

    /// The shapes of the model the copies were made from, which the engine holds beside the model.
    pub(crate) fn sources(&self) -> Vec<Arc<Shape>> {
        self.entries.values().map(|m| Arc::clone(&m.source)).collect()
    }
}

/// One shape flattened by a save.
#[derive(Clone, Debug)]
struct Made {
    source: Arc<Shape>,
    flat: Arc<Shape>,
    lost: Lost,
    master: Option<Arc<Master>>,
    glued: bool,
    style: Option<PathStyle>,
}

/// What a copy depends on besides the shape and its master: the parts a cell is resolved through. A copy is given again only where these are the very ones.
#[derive(Clone, Debug)]
struct Context {
    styles: SharedVec<Arc<Style>>,
    themes: SharedVec<Arc<Theme>>,
    colors: Arc<Vec<ColorEntry>>,
    sheet: Arc<Sheet>,
}

impl Context {
    fn of(doc: &Document) -> Context {
        Context { styles: doc.styles.clone(), themes: doc.themes.clone(), colors: Arc::clone(&doc.colors), sheet: Arc::clone(&doc.sheet) }
    }

    fn is(&self, doc: &Document) -> bool {
        self.styles.is_same_list_as(&doc.styles)
            && self.themes.is_same_list_as(&doc.themes)
            && Arc::ptr_eq(&self.colors, &doc.colors)
            && Arc::ptr_eq(&self.sheet, &doc.sheet)
    }
}

/// What a flattening made.
#[derive(Debug, Default)]
pub(crate) struct Flat {
    /// The document the writer is given: the model with the shapes flattened. `None` when no shape needed it (the model itself is given).
    pub doc: Option<Document>,
    /// The shapes flattened, which the next save may give again.
    pub made: Flattened,
    /// What is not carried, in words, for the warnings of the save.
    pub notes: Vec<String>,
    /// What it cost: the shapes it visited, the shapes it flattened, the copies it gave again and the steps it took.
    pub visited: u64,
    pub flattened: u64,
    pub reused: u64,
    pub steps: u64,
    /// The bytes of the shapes of the model that the copy replaced: still in the model, so beside what the writer counts.
    pub replaced: u64,
}

/// What a shape shows that it may not hold itself: its text and its picture (the master's, when the shape holds none).
type Shown = (Option<Arc<diacraft_doc::Text>>, Option<Arc<diacraft_doc::ForeignData>>);

/// Shapes by their place on a page, each to take the place of the one there.
type Placed = Vec<(usize, Arc<Shape>)>;

/// The shapes of one page that are replaced by others (by their place on it), and the page's place in the document.
type Swaps = (usize, Placed);

/// What a flattening counts, and refuses past: the steps, and the memory the copies may take.
pub(crate) struct Work {
    pub steps: u64,
    pub cap: u64,
    /// The formulas of the shapes' own cells the copy holds as their values, which a file read again does not give back: said in the save's words.
    pub formulas_lost: u64,
    /// The bytes of the shapes the copy replaces and of the copies, and what they may come to together (the memory a save may hold, less what the engine
    /// holds beside the drawing and what a save of this file costs on its own).
    pub charged: u64,
    pub room: u64,
    /// The bytes of the shapes of the model that the copy replaced.
    pub replaced: u64,
    /// What prices the shapes: a formula that many copies share (they share the master's) is priced once.
    sizer: Sizer,
}

impl Work {
    /// A work with the step cap and as much memory as it asks.
    pub(crate) fn new(cap: u64, room: u64) -> Work {
        Work { steps: 0, cap, formulas_lost: 0, charged: 0, room, replaced: 0, sizer: Sizer::default() }
    }

    /// What a shape holds, by the measure of `hold.rs`, with the formulas already priced here left out.
    fn price(&mut self, shape: &Shape) -> u64 {
        self.sizer.shape(shape)
    }

    /// `n` steps, charged before the work: refused in words past the cap.
    fn tick(&mut self, n: u64) -> Result<(), String> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.cap {
            return Err(format!(
                "the new shapes are too many to write to an .odg in one save (more than {} cells and rows to copy): undo some of them to save, then add the rest and save again",
                self.cap
            ));
        }
        Ok(())
    }

    /// The memory of a shape of the model that a copy replaces (`replaced`, which it keeps) and of the copy (`copy`), before the copy is made.
    pub(crate) fn charge(&mut self, replaced: u64, copy: u64) -> Result<(), String> {
        self.replaced = self.replaced.saturating_add(replaced);
        self.charged = self.charged.saturating_add(replaced).saturating_add(copy);
        if self.charged > self.room {
            return Err(
                "the copy of the drawing a save makes would hold more memory than a save may (the shapes the save changes are too many for what the drawing, the earlier versions undo keeps and the file take): undo some of the changes to save"
                    .to_owned(),
            );
        }
        Ok(())
    }
}

/// What says which shapes of a drawing are the file's, and so are not flattened.
#[derive(Clone, Copy)]
pub(crate) enum Basis<'a> {
    /// **The file the drawing stands on** (a save into an `.odg`): the index maps the shapes of the file, and every other shape is new. The flag says the file is a flat one (`.fodg`), which has no
    /// document file of DiaCraft's to carry a page cell no attribute holds.
    File(&'a ShapeIndex, bool),
    /// **No file** (a save from the model alone, `diacraft_odg::write_with(doc, None, ..)`): every shape is new to the package that is made, so the file's own are told by
    /// what they hold. **A shape that holds ODF data is in the form a file gives** (it was read from an `.odg`, directly or through a `.diacraft` file: explicit geometry, constants, and
    /// the held names, values and units the writer writes back while their stamps hold), and is left as it is but for the colours a person chose; a shape that holds none was made by
    /// the engine, and is flattened as a new shape of a file is.
    Model,
}

impl<'a> Basis<'a> {
    /// What the basis says of the page at `at`: `None` for a page the file's index does not hold (the page is left as it is).
    fn of_page(self, at: usize) -> Option<PageBasis<'a>> {
        match self {
            Basis::File(index, _) => index.page(at).map(PageBasis::Index),
            Basis::Model => Some(PageBasis::Model),
        }
    }
}

/// What the basis says of one page.
#[derive(Clone, Copy)]
enum PageBasis<'a> {
    Index(&'a diacraft_odg::PageShapes),
    Model,
}

impl PageBasis<'_> {
    /// A connector the file holds, glued to a shape it does not (a new shape, or one an undo put back after a save that deleted it): the writer refuses it too (`compare.rs`, `unglued`), and this
    /// says so before a copy is made. It is refused in the writer's manner until a later change writes it. (A save from the model alone has no file, and nothing of it is the file's.)
    fn check_glue(&self, page: &diacraft_doc::Page, work: &mut Work) -> Result<(), String> {
        let PageBasis::Index(page_held) = self else { return Ok(()) };
        for connect in page.connects.iter() {
            work.tick(1)?;
            if page_held.ordinal(connect.from_sheet).is_some() && page_held.ordinal(connect.to_sheet).is_none() {
                return Err(String::from(
                    "saving does not yet write this change: a connector of the file glued to a shape that is not in the file (a shape added, or put back by an undo: a later version writes it)",
                ));
            }
        }
        Ok(())
    }

    /// Whether the shape is the file's: the index holds it, or (with no file) it holds the data of one.
    fn holds(&self, shape: &Shape) -> bool {
        match self {
            PageBasis::Index(page) => page.ordinal(shape.id).is_some(),
            PageBasis::Model => shape.ext.get().is_some_and(|data| data.odf != diacraft_doc::OdfShape::default()),
        }
    }
}

/// Flattens the shapes of `doc` that the file `held` does not hold, and those in `before` that changed. See the module documentation. `room` is the memory
/// the copies may take, and `flat_file` says the file is a flat one (`.fodg`).
pub(crate) fn flatten(doc: &Document, held: &ShapeIndex, before: &Flattened, room: u64, flat_file: bool) -> Result<Flat, String> {
    flatten_on(doc, Basis::File(held, flat_file), before, &mut Work::new(MAX_STEPS, room))
}

/// [`flatten`] into a package with a [`Work`] of the caller's: the steps are counted in it whether the flattening ends or is refused, and a cap of its own holds. The
/// tests of what a flattening costs use it.
#[cfg(test)]
pub(crate) fn flatten_counted(doc: &Document, held: &ShapeIndex, before: &Flattened, work: &mut Work) -> Result<Flat, String> {
    flatten_on(doc, Basis::File(held, false), before, work)
}

/// [`flatten`] for a save from the model alone ([`Basis::Model`]): `before` is what an earlier save flattened (none for a drawing no `.odg` has stood on). **`DrawingSizeType` is kept**: the
/// model writer carries a page cell no attribute holds in DiaCraft's own document file, and a drawing that comes back from the file has its preset size.
pub(crate) fn flatten_model(doc: &Document, before: &Flattened, room: u64) -> Result<Flat, String> {
    flatten_on(doc, Basis::Model, before, &mut Work::new(MAX_STEPS, room))
}

/// The flattening, for either [`Basis`].
pub(crate) fn flatten_on(doc: &Document, held: Basis<'_>, before: &Flattened, work: &mut Work) -> Result<Flat, String> {
    let mut flat = Flat::default();
    let starved = || "there is not enough memory to save the new shapes".to_owned();
    let context = Context::of(doc);
    let same_context = before.context.as_ref().is_some_and(|c| c.is(doc));
    let masters: HashMap<u32, &Arc<Master>> = doc.masters.iter().map(|m| (m.id, m)).collect();
    // Which shapes: by place on the page, one walk of each page's top level, a look in two maps for each. A shape that is not flattened is looked at for
    // the colours a person chose, which are `RGB(r,g,b)` in the model and constants in a file (`constants`).
    let mut picks: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut gives: Vec<Swaps> = Vec::new();
    let mut swaps: Vec<Swaps> = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    let mut kept: BTreeMap<(u32, u32), Made> = BTreeMap::new();
    let mut eval = look_eval_ctx();
    let look = LookReader::new(doc, &mut eval);
    let draw = DrawReader::new(doc);
    for (at, page) in doc.pages.iter().enumerate() {
        // `DrawingSizeType` (1 for a preset size, 3 for a custom one) is the engine's own mark of how a page was sized: no attribute of an `.odg` holds it, the reader makes
        // none, and the size it classifies is written as the page's size. **The model writer carries it, and so does a save into a package** (a page cell with no attribute goes
        // into DiaCraft's own document file, `model_kinds::a_page_cell_no_attribute_of_the_file_holds_survives_a_model_save`); a **flat** drawing has no document file, and its writer
        // refuses a page cell it has no attribute for (`odg_flat`), so the copy a save into a flat file is given has none. (A save into a package dropped it too, until
        // A drawing the model writer had saved with the cell, saved again with no edit, then wrote the document file again without it, an edit and a loss.)
        if matches!(held, Basis::File(_, true)) && page.sheet.cell("DrawingSizeType").is_some() {
            sizes.try_reserve(1).map_err(|_| starved())?;
            sizes.push(at);
        }
        let Some(page_held) = held.of_page(at) else { continue };
        page_held.check_glue(page, work)?;
        let glued: HashSet<u32> = page.connects.iter().map(|c| c.from_sheet).collect();
        let (mut here, mut given, mut swapped): (Vec<usize>, Placed, Placed) = (Vec::new(), Vec::new(), Vec::new());
        for (pos, shape) in page.shapes.iter().enumerate() {
            flat.visited = flat.visited.saturating_add(1);
            work.tick(1)?;
            let flattened_before = before.entries.get(&(page.id, shape.id));
            if !page_held.holds(shape) {
                // An object the file does not hold (a copy, or one an undo brought back) is refused, with a picture or without: what the file has of an object is not
                // copied. Only a foreign shape is looked into, so a drawing of plain shapes costs no step more.
                if is_object(shape, work)? {
                    return Err(OBJECT_NOT_IN_FILE.to_owned());
                }
                here.try_reserve(1).map_err(|_| starved())?;
                here.push(pos);
            } else if let Some(made) = flattened_before {
                // Flattened by an earlier save: the same copy again if nothing it was made from has changed, else flattened anew.
                let current_master = shape.master.and_then(|id| masters.get(&id).copied());
                let master_same = match (&made.master, current_master) {
                    (None, None) => true,
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    _ => false,
                };
                let mut same = same_context && Arc::ptr_eq(&made.source, shape) && master_same && made.glued == glued.contains(&shape.id);
                if same && let Some(style) = made.style {
                    work.tick(1)?;
                    let at = SheetRef::Shape(ShapeRef::top(
                        Owner::Page(page.id),
                        u32::try_from(pos).map_err(|_| "a page has too many shapes".to_owned())?,
                    ));
                    same = path_style(&draw, page.id, &at) == style;
                }
                if same {
                    given.try_reserve(1).map_err(|_| starved())?;
                    given.push((pos, Arc::clone(&made.flat)));
                    kept.insert((page.id, shape.id), made.clone());
                    flat.reused = flat.reused.saturating_add(1);
                } else {
                    here.try_reserve(1).map_err(|_| starved())?;
                    here.push(pos);
                }
            } else if let Some(new) = constants(shape, 1, work)? {
                swapped.try_reserve(1).map_err(|_| starved())?;
                swapped.push((pos, new));
            }
        }
        if !here.is_empty() {
            picks.push((at, here));
        }
        if !given.is_empty() {
            gives.push((at, given));
        }
        if !swapped.is_empty() {
            swaps.push((at, swapped));
        }
    }
    // Every master the model holds is the copy's too, the file's own and the ones a drop brought: the writer writes them into DiaCraft's
    // document file, and the copies of the shapes made from them keep their links, so that a file read again has the shape linked to its master.
    if picks.is_empty() && gives.is_empty() && swaps.is_empty() && sizes.is_empty() {
        flat.steps = work.steps;
        flat.replaced = work.replaced;
        return Ok(flat);
    }
    let mut out = doc.clone();
    for at in sizes {
        let Some(page) = doc.pages.get(at) else { continue };
        if let Some(target) = page_mut(&mut out, page.id) {
            Arc::make_mut(&mut target.sheet).cells.remove("DrawingSizeType");
        }
    }
    for (at, swapped) in swaps.into_iter().chain(gives) {
        let Some(page) = doc.pages.get(at) else { continue };
        if let Some(target) = page_mut(&mut out, page.id) {
            for (pos, shape) in swapped {
                if let Some(slot) = target.shapes.get_mut(pos) {
                    *slot = shape;
                }
            }
        }
    }
    // The style sheets by `ID`, for the text a shape reaches through its `TextStyle` link (`Flattener::text_style_loss`): a table made once, and only when a
    // shape is flattened (a drawing that flattens nothing pays nothing for it), charged a step for each sheet before it is made.
    let mut styles: HashMap<u32, &Arc<Style>> = HashMap::new();
    if picks.iter().any(|(_, positions)| !positions.is_empty()) {
        styles.try_reserve(doc.styles.len()).map_err(|_| starved())?;
        work.tick(doc.styles.len() as u64)?;
        styles.extend(doc.styles.iter().map(|s| (s.id, s)));
    }
    // The `Theme` style, which a shape that follows it keeps the links to: the root is no style a shape follows.
    let theme = crate::theme::theme_style_id(doc).filter(|id| *id != 0);
    // What is newly not carried, by kind: how many shapes, and the first.
    let mut dropped: HashMap<Lost, (u64, (u32, u32))> = HashMap::new();
    let from_vsdx = doc.vsdx.as_ref().is_some_and(|held| !held.rels.is_empty());
    for (at, positions) in picks {
        let Some(page) = doc.pages.get(at) else { continue };
        let Some(page_held) = held.of_page(at) else { continue };
        let glued: HashSet<u32> = page.connects.iter().map(|c| c.from_sheet).collect();
        let mut made: Vec<(usize, Arc<Shape>)> = Vec::new();
        made.try_reserve_exact(positions.len()).map_err(|_| "there is not enough memory to save the new shapes".to_owned())?;
        let group_names = new_group_names(page, &positions, work)?;
        for pos in positions {
            let Some(shape) = page.shapes.get(pos) else { continue };
            let one = Flattener {
                look: &look,
                draw: &draw,
                page: page.id,
                pos,
                glued: &glued,
                masters: &masters,
                styles: &styles,
                theme,
                group_names: &group_names,
                page_held,
                from_vsdx,
            };
            let before_shape = work.price(shape);
            // The shape of the model stays beside its copy: charged before the copy is made, the copy at what the original holds as an upper bound of a
            // shape that is flattened and is then charged again at what it came to.
            work.charge(before_shape, before_shape)?;
            let entry = one.shape(shape, work)?;
            let made_bytes = work.price(&entry.flat);
            work.charge(0, made_bytes.saturating_sub(before_shape))?;
            let previous = before.lost(page.id, shape.id).unwrap_or(0);
            // Said once: what the save before already said of this shape is not said again.
            let fresh = entry.lost & !previous;
            for (bit, _) in KINDS {
                if fresh & bit != 0 {
                    let slot = dropped.entry(bit).or_insert((0, (page.id, shape.id)));
                    slot.0 = slot.0.saturating_add(1);
                }
            }
            flat.flattened = flat.flattened.saturating_add(1);
            made.push((pos, Arc::clone(&entry.flat)));
            kept.insert((page.id, shape.id), entry.made(shape, one.master_of(shape), glued.contains(&shape.id)));
        }
        if let Some(target) = page_mut(&mut out, page.id) {
            for (pos, shape) in made {
                if let Some(slot) = target.shapes.get_mut(pos) {
                    *slot = shape;
                }
            }
        }
    }
    flat.notes = lost_notes(&dropped);
    flat.notes.extend(formulas_lost_words(work.formulas_lost));
    flat.made.entries = kept;
    flat.made.context = Some(context);
    flat.steps = work.steps;
    flat.replaced = work.replaced;
    flat.doc = Some(out);
    Ok(flat)
}

/// The names the new groups of a page are given (review): a group's data, `ID` and link to a master are found in DiaCraft's
/// document file by the group's `draw:name`, which must be unique among the page's groups, so each new group (those under a shape of `positions`, the top-level shapes
/// the copy makes) that has no name, or has one another group of the page has, **and that DiaCraft's document file carries data for** (a cell or a section of its own that
/// the file's markup does not give a group, a link to a master, or an `ID` another shape names, `diacraft_odg::groups_needing_a_key`) is named `Group_<ID>` (`Group_<ID>_2` and so on when
/// a group of the page has that name already). **A group that holds no data stays as it is**: a `draw:g` the file had with no name, which only its members fill, is written with none, as the
/// file had it, and a group a person makes with nothing on it needs no key either. A group with a name of its own that is unique keeps it. Nothing is done for a page with no new group; each
/// shape of the page is a step, and so is each of the walk that finds the groups that need a name.
fn new_group_names(page: &diacraft_doc::Page, positions: &[usize], work: &mut Work) -> Result<HashMap<u32, Arc<str>>, String> {
    let starved = || "there is not enough memory to save the new shapes".to_owned();
    let mut names: HashMap<u32, Arc<str>> = HashMap::new();
    if !positions.iter().any(|p| page.shapes.get(*p).is_some_and(|s| !s.children.is_empty())) {
        return Ok(names);
    }
    let new_tops: HashSet<usize> = positions.iter().copied().collect();
    // Every group of the page in pre-order, with whether it is new (under a new shape of the top level), and how many groups have each name.
    let mut counts: HashMap<&str, u32> = HashMap::new();
    let mut groups: Vec<(u32, Option<&str>, bool)> = Vec::new();
    let mut stack: Vec<(&Arc<Shape>, bool)> = Vec::new();
    stack.try_reserve(page.shapes.len()).map_err(|_| starved())?;
    stack.extend(page.shapes.iter().enumerate().rev().map(|(at, s)| (s, new_tops.contains(&at))));
    while let Some((shape, new)) = stack.pop() {
        work.tick(1)?;
        if shape.children.is_empty() {
            continue;
        }
        let name = shape.name_u.as_deref();
        let count = counts.entry(name.unwrap_or_default()).or_insert(0);
        *count = count.saturating_add(1);
        groups.try_reserve(1).map_err(|_| starved())?;
        groups.push((shape.id, name, new));
        stack.try_reserve(shape.children.len()).map_err(|_| starved())?;
        stack.extend(shape.children.iter().rev().map(|c| (c, new)));
    }
    // Which groups need a key at all: one that holds no data is left as it is. The walk is charged here, in the steps of this copy, and past the room it is refused as any other is.
    let keys = diacraft_odg::groups_needing_a_key(page, work.cap.saturating_sub(work.steps));
    work.tick(keys.steps)?;
    if keys.over {
        work.tick(u64::MAX)?;
    }
    let mut taken: HashSet<String> = HashSet::new();
    for (id, name, new) in groups {
        let unique = name.is_some_and(|n| !n.is_empty() && counts.get(n) == Some(&1));
        if !new || unique || !keys.ids.contains(&id) {
            continue;
        }
        let mut candidate = format!("Group_{id}");
        let mut again = 1u32;
        while counts.contains_key(candidate.as_str()) || taken.contains(&candidate) {
            again = again.saturating_add(1);
            candidate = format!("Group_{id}_{again}");
        }
        taken.insert(candidate.clone());
        names.insert(id, Arc::from(candidate.as_str()));
    }
    Ok(names)
}

/// The copy of a guide.
///
/// **A guide** holds every cell as a constant of its own (its style links are the root's), so there is nothing to resolve, and the cells it holds
/// (`NonPrinting`, `ShapePermeable...`, the `InfiniteLine` row) are not ones this audit knows and would be said lost: the writer carries them in DiaCraft's
/// extension, with its kind. In the file it is a shape of no size, which another program would draw as a dot with the root's line: **the copy gives it no
/// fill and no line**, so that the file draws nothing (the guide holds none of the two cells, and so draws with the root style's, in no program that draws guides).
fn guide_copy(shape: &Arc<Shape>) -> Entry {
    let mut copy = (**shape).clone();
    let sheet = Arc::make_mut(&mut copy.sheet);
    for name in ["FillPattern", "LinePattern"] {
        if sheet.cell(name).is_none() {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text("0")));
        }
    }
    Entry { flat: Arc::new(copy), lost: 0, style: None }
}

/// The warnings of what new shapes lost that the writer does not carry, by kind: how many shapes lost it, and the first.
/// The words of the formulas the copy writes as their values ([`Work::formulas_lost`]): none for none.
fn formulas_lost_words(n: u64) -> Option<String> {
    let one = n == 1;
    (n > 0).then(|| {
        format!(
            "{n} formula{} of the shapes' own cells (the place, size or turn of a member of a group or of a line) {} written as the value{} {} give{}: the file read again holds the value{}, not the formula{}",
            if one { "" } else { "s" },
            if one { "is" } else { "are" },
            if one { "" } else { "s" },
            if one { "it" } else { "they" },
            if one { "s" } else { "" },
            if one { "" } else { "s" },
            if one { "" } else { "s" },
        )
    })
}

fn lost_notes(dropped: &HashMap<Lost, (u64, (u32, u32))>) -> Vec<String> {
    let mut notes = Vec::new();
    for (bit, what) in KINDS {
        let Some((count, (page, shape))) = dropped.get(&bit).copied() else { continue };
        let (shapes, were) = if count == 1 { ("new shape", "was") } else { ("new shapes", "were") };
        notes.push(format!(
            "{what} of {count} {shapes} {were} not written to the file, which will be missing when it is opened again (the first is shape {shape} on page {page}): the .odg save has no attribute for it yet"
        ));
    }
    notes
}

/// What the flattening of one shape gave: the copy and what it lost.
struct Entry {
    flat: Arc<Shape>,
    lost: Lost,
    style: Option<PathStyle>,
}

impl Entry {
    fn made(&self, source: &Arc<Shape>, master: Option<Arc<Master>>, glued: bool) -> Made {
        Made { source: Arc::clone(source), flat: Arc::clone(&self.flat), lost: self.lost, master, glued, style: self.style }
    }
}

/// What flattens one shape: the readers of the document, and where the shape is.
struct Flattener<'a, 'd, 'c> {
    look: &'a LookReader<'d, 'c>,
    draw: &'a DrawReader<'d>,
    page: u32,
    pos: usize,
    /// The shapes of the page that are the `FromSheet` of a `Connect` row: the glued ones.
    glued: &'a HashSet<u32>,
    masters: &'a HashMap<u32, &'d Arc<Master>>,
    /// The document's style sheets by `ID`.
    styles: &'a HashMap<u32, &'d Arc<Style>>,
    /// The `ID` of the `Theme` style, when the document has one: a new shape whose three links name it keeps them.
    theme: Option<u32>,
    /// The names the page's new groups are given, by the group's `ID` ([`new_group_names`]).
    group_names: &'a HashMap<u32, Arc<str>>,
    /// What the file holds of the page: a member of a new group that it holds is a shape of the file put into the group (the writer refuses that in its own words).
    page_held: PageBasis<'a>,
    /// The drawing came from a VSDX file (it holds the package data of one): the shapes' own formulas are kept ([`Flattener::own_formulas`]).
    from_vsdx: bool,
}

/// Whether a formula is one the theme answers: a call of `THEMEGUARD` (a colour picked from the theme) or of `THEMEVAL` (Use Default), at its start.
fn is_theme_formula(text: &str) -> bool {
    let word = text.trim_start();
    ["THEMEGUARD(", "THEMEVAL("].iter().any(|call| word.get(..call.len()).is_some_and(|head| head.eq_ignore_ascii_case(call)))
}

/// The bits (`KINDS`) of what a shape holds that the writer has no attribute for.
pub(crate) type Lost = u16;

/// Whether the copy is given the cell `name` by what the shape **resolves** to, and not as the shape holds it (the frame, the ends of a segment, the fill, the line, the margins,
/// the line ends and the route's kind): the first pass of [`Flattener::shape_at`] decided whether the copy holds it, and what the shape holds of its own is not put over that.
fn is_resolved(name: &str) -> bool {
    let named = |list: &[&str]| list.contains(&name);
    named(&FRAME)
        || named(&FRAME_IF_SET)
        || named(&ENDS)
        || named(&MARGINS)
        || named(&STYLE_CELLS)
        || named(&ROUTE_CELLS)
        || ARROW_CELLS.iter().any(|(a, s)| name == *a || name == *s)
}

impl Flattener<'_, '_, '_> {
    /// The master an instance has, as the model holds it.
    fn master_of(&self, shape: &Shape) -> Option<Arc<Master>> {
        shape.master.and_then(|id| self.masters.get(&id)).map(|m| Arc::clone(m))
    }

    /// The style the copy of `shape` keeps its three links to: the `Theme` style, when the three links of the shape name it, and none otherwise.
    fn carried_link(&self, shape: &Shape) -> Option<u32> {
        let theme = self.theme?;
        let l = shape.sheet.links;
        (l.line == Some(theme) && l.fill == Some(theme) && l.text == Some(theme)).then_some(theme)
    }

    /// Whether the copy holds the cell `name` as a constant: it does unless the shape keeps its links (`carried`) and **the cell is the style's**, found along them and
    /// not in the shape or in the shape its master holds, in which case the link gives it, now and when the file is read again, and a constant would be the shape's own.
    fn own(&self, at: &SheetRef, carried: Option<u32>, name: &str, work: &mut Work) -> Result<bool, String> {
        if carried.is_none() {
            return Ok(true);
        }
        work.tick(1)?;
        let found = self.look.res.resolve(at, &CellAddr::direct(name));
        Ok(!found.is_some_and(|r| matches!(r.formula.source, Source::Style { .. }) && matches!(r.value.source, Source::Style { .. })))
    }

    /// A number a cell resolves to, as a constant with its unit; `None` when it holds none. **The value itself, not rounded**: the writer writes a length as the
    /// shortest decimal that reads back to it (`diacraft_odg` `lengths.rs`, which takes arithmetic noise for the value), and a value rounded to nine places of an inch
    /// is not the length a metric file gave (2cm came back 2.0000000005cm after a trip through a `.diacraft` drawing).
    fn number(&self, at: &SheetRef, name: &str, work: &mut Work) -> Result<Option<Cell>, String> {
        work.tick(1)?;
        let addr = CellAddr::direct(name);
        Ok(match self.look.val(at, &addr) {
            LookVal::Num(n) if n.is_finite() => {
                let Some(value) = Value::from_number(n) else { return Ok(None) };
                let mut cell = Cell::constant(value);
                cell.unit = self.look.res.resolve(at, &addr).and_then(|r| unit_of(&r));
                Some(cell)
            }
            _ => None,
        })
    }

    /// The cell of a line end that is a custom one (254) with the `USE("<master>")` it resolves to, and the value 254: the writer draws a marker of a master it knows by its name (DiaCraft's
    /// filled diamond, `diacraft_odg` `markers.rs`), and a file read again has the formula on the shape as it has it on its master, so that the end is still drawn from the master
    /// and not as number 4. Any other cell, a formula that is no `USE` and one longer than a name needs are the constant they were.
    fn with_use(&self, at: &SheetRef, name: &str, cell: Cell) -> Cell {
        const LONGEST_USE: usize = 512;
        if cell.value.as_ref().and_then(Value::number) != Some(254.0) {
            return cell;
        }
        let Some(found) = self.look.res.resolve(at, &CellAddr::direct(name)) else { return cell };
        let Some(FormulaSlot::Local(formula)) = found.formula.cell.map(|c| &c.formula) else { return cell };
        let source = formula.source.trim_start();
        let is_use = source.len() <= LONGEST_USE && source.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("USE("));
        if is_use { Cell::read(Some(&formula.source), Some("254"), cell.unit.as_deref(), None, &mut 64) } else { cell }
    }

    /// The pin of a shape that turns about its centre, as the formula the reader gives it (`Width*0.5`, `Height*0.5`): the copy holds it so, and the writer finds the
    /// pin what the file will give back and carries nothing of it in DiaCraft's extension. A pin anywhere else stays the constant it is.
    fn centred(&self, at: &SheetRef, cell: Cell, size: &str, work: &mut Work) -> Result<Cell, String> {
        let whole = self.num(at, size, work)?;
        let at_centre = cell.value.as_ref().and_then(Value::number).is_some_and(|pin| (pin - whole / 2.0).abs() <= 1e-9 * (1.0 + whole.abs()));
        match (&cell.value, at_centre) {
            (Some(value), true) => Ok(Cell::read(Some(&format!("{size}*0.5")), Some(value.text()), cell.unit.as_deref(), None, &mut 64)),
            _ => Ok(cell),
        }
    }

    /// The number a cell resolves to, 0 when it holds none.
    fn num(&self, at: &SheetRef, name: &str, work: &mut Work) -> Result<f64, String> {
        work.tick(1)?;
        Ok(self.look.num(at, &CellAddr::direct(name), 0.0))
    }

    /// A colour a cell resolves to, as the `#rrggbb` the cell holds.
    fn colour(&self, at: &SheetRef, name: &str, work: &mut Work) -> Result<Option<Cell>, String> {
        work.tick(1)?;
        Ok(self.look.colour(at, &CellAddr::direct(name)).map(|c| Cell::constant(Value::from_text(&format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)))))
    }

    /// The sheet of the master shape `shape` is an instance of, when the master holds one shape.
    fn master_sheet(&self, shape: &Shape) -> Option<&Sheet> {
        let master = shape.master.and_then(|id| self.masters.get(&id))?;
        let mut tops = master.shapes.iter();
        match (tops.next(), tops.next()) {
            (Some(only), None) => Some(&*only.sheet),
            _ => None,
        }
    }

    /// Whether the cell `name` of `shape` is the master's and not its own: the copy keeps the link and the master is written into DiaCraft's document file, so a file
    /// read again has the cell through the link and it is no loss. A cell the shape holds itself, or one that comes from a style, is not.
    fn through_link(&self, shape: &Shape, name: &str) -> bool {
        !shape.sheet.cells.contains_key(name) && self.master_sheet(shape).is_some_and(|m| m.cells.contains_key(name))
    }

    /// The style cells of the copy (the fill, the line, the line ends of a 1-D shape, the vertical alignment and the margins of a 2-D one), each a constant of what the shape resolves
    /// to: left out when the copy keeps its links (`carried`) and the style is where the cell is found ([`Self::own`]).
    fn style_cells(&self, at: &SheetRef, carried: Option<u32>, one_d: bool, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        let mut put = |name: &str, cell: Option<Cell>| {
            if let Some(cell) = cell {
                sheet.cells.insert(CellName::new(name), cell);
            }
        };
        if !one_d {
            if self.own(at, carried, "FillPattern", work)? {
                put("FillPattern", self.number(at, "FillPattern", work)?);
            }
            if self.own(at, carried, "FillForegnd", work)? {
                put("FillForegnd", self.colour(at, "FillForegnd", work)?);
            }
            if self.own(at, carried, "FillForegndTrans", work)? {
                put("FillForegndTrans", self.number(at, "FillForegndTrans", work)?);
            }
            if self.num(at, "FillGradientEnabled", work)? != 0.0 && self.own(at, carried, "FillGradientEnabled", work)? {
                put("FillGradientEnabled", self.number(at, "FillGradientEnabled", work)?);
            }
        }
        for (name, colour) in [("LinePattern", false), ("LineWeight", false), ("LineColor", true), ("LineColorTrans", false), ("LineCap", false)] {
            if self.own(at, carried, name, work)? {
                put(name, if colour { self.colour(at, name, work)? } else { self.number(at, name, work)? });
            }
        }
        if one_d {
            // A line end the shape resolves to is written, with its size class (the writer draws its own marker for the number and refuses a kind it has no
            // art for, in words): no end is no cell.
            for (arrow, size) in ARROW_CELLS {
                if self.num(at, arrow, work)? != 0.0 {
                    if self.own(at, carried, arrow, work)? {
                        put(arrow, self.number(at, arrow, work)?.map(|cell| self.with_use(at, arrow, cell)));
                    }
                    if self.own(at, carried, size, work)? {
                        put(size, self.number(at, size, work)?);
                    }
                }
            }
        } else {
            if self.own(at, carried, "VerticalAlign", work)? {
                put("VerticalAlign", self.number(at, "VerticalAlign", work)?);
            }
            // The four margins are written together: the shape has them from the style alone, or it holds one of them (or its master does) and all four are written.
            let mut any = false;
            for name in MARGINS {
                if matches!(self.look.val(at, &CellAddr::direct(name)), LookVal::Num(_)) && self.own(at, carried, name, work)? {
                    any = true;
                }
            }
            if any {
                for name in MARGINS {
                    let cell = self.number(at, name, work)?.unwrap_or_else(|| Cell::constant(Value::from_text("0")).with_unit("IN"));
                    put(name, Some(cell));
                }
            }
        }
        Ok(())
    }

    /// The place of the label of a line or a connector (the engine writes it when it routes one: a label stays upright, so `TxtAngle` turns against the line) is the
    /// shape's own, with its formulas, and it is carried: the file's text has no cell for it, and read without them the label would be drawn along the line (a
    /// Data Flow's label).
    fn label_cells(shape: &Shape, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        for name in LABEL_CELLS {
            work.tick(1)?;
            if let Some(cell) = shape.sheet.cell(name) {
                sheet.cells.insert(CellName::new(name), cell.clone());
            }
        }
        Ok(())
    }

    /// The shape, flattened, and what it had that is not carried.
    fn shape(&self, shape: &Arc<Shape>, work: &mut Work) -> Result<Entry, String> {
        let top = ShapeRef::top(Owner::Page(self.page), u32::try_from(self.pos).map_err(|_| "a page has too many shapes".to_owned())?);
        self.shape_at(shape, top, work)
    }

    /// A group: its own frame as the shape holds it and its members, each flattened where it is in the group (the copy of a member has its cells in the group's frame,
    /// as the model has them, with the constants and the formulas of a shape of the file). What a member lost is the group's (a group of new shapes is written
    /// as a `draw:g` holding them).
    fn group_at(&self, shape: &Arc<Shape>, at: &ShapeRef, work: &mut Work) -> Result<Entry, String> {
        if at.path.len() > MAX_SHAPE_DEPTH {
            return Err("shapes are nested deeper than the model allows".to_owned());
        }
        let own = SheetRef::Shape(at.clone());
        let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
        for name in FRAME {
            let cell = self.number(&own, name, work)?;
            let cell = match (name, cell) {
                ("LocPinX", Some(c)) => Some(self.centred(&own, c, "Width", work)?),
                ("LocPinY", Some(c)) => Some(self.centred(&own, c, "Height", work)?),
                (_, c) => c,
            };
            if let Some(cell) = cell {
                sheet.cells.insert(CellName::new(name), cell);
            }
        }
        for name in FRAME_IF_SET {
            if self.num(&own, name, work)? != 0.0
                && let Some(cell) = self.number(&own, name, work)?
            {
                sheet.cells.insert(CellName::new(name), cell);
            }
        }
        // What else the group's own sheet holds (a cell of its own, a section) is the group's data: kept as it is, for the writer to carry or refuse.
        for (name, cell) in shape.sheet.cells.iter() {
            work.tick(1)?;
            if !FRAME.contains(&name.as_str()) && !FRAME_IF_SET.contains(&name.as_str()) {
                sheet.cells.insert(name.clone(), cell.clone());
            }
        }
        for section in &shape.sheet.sections {
            work.tick(1)?;
            sheet.sections.push(Arc::clone(section));
        }
        sheet.raw = without_ids(&shape.sheet.raw);
        sheet.passthrough = shape.sheet.passthrough.clone();
        let (mut members, mut lost): (Vec<Arc<Shape>>, Lost) = (Vec::new(), 0);
        members.try_reserve_exact(shape.children.len()).map_err(|_| "there is not enough memory to save the new shapes".to_owned())?;
        for (place, member) in shape.children.iter().enumerate() {
            work.tick(1)?;
            let member_at = at.child(u32::try_from(place).map_err(|_| "a group has too many members".to_owned())?);
            // A member the file does not hold is as new as a shape of the page's top level that it does not hold: an object among them (a copy of a group that holds one)
            // is refused as one is there (B1), under the same `Work`. A member the file holds is a shape put into the group, which the writer refuses.
            if !self.page_held.holds(member) && is_object(member, work)? {
                return Err(OBJECT_NOT_IN_FILE.to_owned());
            }
            // **With no file (`Basis::Model`), a member that holds what a file gave it is left as it is** but for the colours a person chose, as a shape of the page's top level that holds
            // it is: a group that holds no data of a file's of its own (a `draw:g` with no attribute) is new to the save, and its members were flattened with it, which took each
            // off its style sheet, and a member's style off the parent its file gave it (the fill, the line and the shadow `standard` gives went with it).
            let entry = if matches!(self.page_held, PageBasis::Model) && self.page_held.holds(member) {
                let kept = constants(member, at.path.len().saturating_add(1), work)?.unwrap_or_else(|| Arc::clone(member));
                Entry { flat: kept, lost: 0, style: None }
            } else {
                self.shape_at(member, member_at, work)?
            };
            lost |= entry.lost;
            members.push(entry.flat);
        }
        // A new group is named after its `ID` when it has no name unique among the page's groups: the key its entry in DiaCraft's document file is found by.
        let given = self.group_names.get(&shape.id);
        let copy = Shape {
            id: shape.id,
            name: given.map_or_else(|| shape.name.clone(), |n| Some(Arc::clone(n))),
            name_u: given.map_or_else(|| shape.name_u.clone(), |n| Some(Arc::clone(n))),
            kind: shape.kind.clone(),
            master: shape.master,
            master_shape: shape.master_shape,
            del: false,
            sheet: Arc::new(sheet),
            // A group's own text is carried as it is, for the writer to refuse in words (R45 writes it as one more member of the group: not written yet): it is never
            // dropped without a word (review).
            text: shape.text.clone(),
            data1: shape.data1.clone(),
            data2: shape.data2.clone(),
            data3: shape.data3.clone(),
            foreign: shape.foreign.clone(),
            children: ShapeList::from_vec(members),
            raw: without_ids(&shape.raw),
            passthrough: shape.passthrough.clone(),
            // The slot goes with the copy, as `raw` and `passthrough` do, but for the ids of the file's own.
            ext: ext_without_ids(&shape.ext),
        };
        Ok(Entry { flat: Arc::new(copy), lost, style: None })
    }

    /// The copy of `shape`, which is at `at` in the document (a shape of the page's top level, or a member of a group).
    fn shape_at(&self, shape: &Arc<Shape>, at: ShapeRef, work: &mut Work) -> Result<Entry, String> {
        if !shape.children.is_empty() {
            return self.group_at(shape, &at, work);
        }
        if shape.kind == diacraft_doc::ShapeType::Guide {
            return Ok(guide_copy(shape));
        }
        let shape_ref = at.clone();
        let at = SheetRef::Shape(at);
        let one_d = self.look.res.resolve(&at, &CellAddr::direct("BeginX")).is_some_and(|r| r.value.cell.is_some());
        let marked =
            shape.sheet.sections.iter().any(|s| s.name.as_str() == "User" && s.rows.iter().any(|r| r.name.as_deref() == Some("DiaCraftRoute")));
        let routed = ["ShapeRouteStyle", "ConLineRouteExt"].iter().any(|n| shape.sheet.cell(n).is_some());
        let connector = one_d && (self.glued.contains(&shape.id) || marked || routed);
        // **The link to the `Theme` style is carried**: a shape whose three links name it is written as a shape whose automatic style has it as its parent
        // And so the shape that was dropped after `theme.apply` follows `theme.variant` when the file is read again (the copy of it had the root's links
        // and the theme's look as constants, which no variant moves). Then a cell the shape has from the style, and from no cell of its own or of its master's, is left to the
        // link: a constant would be the look of the theme as it was, and would be the shape's own after the file is read again. Links of any other kind are the root's.
        let carried = self.carried_link(shape);
        let link = carried.unwrap_or(0);
        let mut sheet = Sheet { links: Links { line: Some(link), fill: Some(link), text: Some(link) }, ..Sheet::default() };
        let mut put = |name: &str, cell: Option<Cell>| {
            if let Some(cell) = cell {
                sheet.cells.insert(CellName::new(name), cell);
            }
        };
        for name in FRAME {
            let cell = self.number(&at, name, work)?;
            let cell = match (name, cell) {
                ("LocPinX", Some(c)) => Some(self.centred(&at, c, "Width", work)?),
                ("LocPinY", Some(c)) => Some(self.centred(&at, c, "Height", work)?),
                (_, c) => c,
            };
            put(name, cell);
        }
        for name in FRAME_IF_SET {
            if self.num(&at, name, work)? != 0.0 {
                put(name, self.number(&at, name, work)?);
            }
        }
        if one_d {
            for name in ENDS {
                put(name, self.number(&at, name, work)?);
            }
            // The frame of a segment is worked out from its ends by the formulas the reader gives it: the copy holds them so, when what the shape resolves to is what
            // they give, and the writer finds the frame what the file gives back and carries nothing of it in DiaCraft's extension.
            let (bx, by) = (self.num(&at, "BeginX", work)?, self.num(&at, "BeginY", work)?);
            let (ex, ey) = (self.num(&at, "EndX", work)?, self.num(&at, "EndY", work)?);
            let frame = [
                ("PinX", "(BeginX+EndX)/2", (bx + ex) / 2.0),
                ("PinY", "(BeginY+EndY)/2", (by + ey) / 2.0),
                ("Width", "SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)", (ex - bx).hypot(ey - by)),
                ("Angle", "ATAN2(EndY-BeginY,EndX-BeginX)", (ey - by).atan2(ex - bx)),
            ];
            for (name, formula, value) in frame {
                if let Some(held) = self.number(&at, name, work)?
                    && held.value.as_ref().and_then(Value::number).is_some_and(|n| (n - value).abs() <= 1e-9 * (1.0 + value.abs()))
                    && let Some(text) = held.value.as_ref().map(Value::text)
                {
                    put(name, Some(Cell::read(Some(formula), Some(text), held.unit.as_deref(), None, &mut 64)));
                }
            }
        }
        // The frame's own formulas come back for a 2-D shape at the top of the page; a member of a group has its place worked out from the group's frame by the reader, and a line its frame
        // from its ends, so theirs are said lost.
        let frame_too = !one_d && matches!(&at, SheetRef::Shape(r) if r.path.len() == 1);
        self.style_cells(&at, carried, one_d, &mut sheet, work)?;
        // A drawing from a VSDX file keeps all the shape's own formulas; any other keeps the **theme formulas** a palette pick writes (`THEMEGUARD(...)`, and `THEMEVAL()` of Use
        // Default), so that a colour picked from the theme is a value in the file's attribute with its formula carried, and still follows the theme when the file is read again.
        Self::own_formulas(shape, frame_too, self.glued.contains(&shape.id), self.from_vsdx, &mut sheet, work)?;
        if one_d && shape.text.is_some() {
            Self::label_cells(shape, &mut sheet, work)?;
        }
        let mut style = None;
        if connector {
            let found = path_style(self.draw, self.page, &at);
            style = Some(found);
            let (kind, appearance) = match found {
                PathStyle::RightAngle => (1.0, 1.0),
                PathStyle::Straight => (2.0, 1.0),
                PathStyle::Curved => (1.0, 2.0),
                PathStyle::CentreToCentre => (16.0, 1.0),
            };
            // A route cell the shape holds already (a copy of a connector of the file) is kept as the shape resolves it.
            let held = |name: &str, otherwise: f64, work: &mut Work| -> Result<Cell, String> {
                Ok(self
                    .number(&at, name, work)?
                    .filter(|_| shape.sheet.cell(name).is_some())
                    .unwrap_or_else(|| Cell::constant(Value::from_text(&number_text(otherwise)))))
            };
            let (kind, appearance) = if routed {
                (held("ShapeRouteStyle", kind, work)?, held("ConLineRouteExt", appearance, work)?)
            } else {
                (Cell::constant(Value::from_text(&number_text(kind))), Cell::constant(Value::from_text(&number_text(appearance))))
            };
            sheet.cells.insert(CellName::new("ShapeRouteStyle"), kind);
            sheet.cells.insert(CellName::new("ConLineRouteExt"), appearance);
        }
        // The sections: the geometry, and for a 2-D shape the connection points, merged with the master's.
        self.sections(&at, "Geometry", &mut sheet, work)?;
        if !one_d {
            self.sections(&at, "Connection", &mut sheet, work)?;
        }
        // **What the shape holds of its own that is carried as it is** (the marker of a route that is the engine's to make again, `User` rows, the layer, rounded corners, a moved text
        // block, any other cell or section) and what a style gives it that has no attribute of a file (rounded corners and a shadow).
        Self::own_rest(shape, one_d, &mut sheet, work)?;
        self.style_look(&at, shape, &mut sheet, work)?;
        sheet.raw = without_ids(&shape.sheet.raw);
        sheet.passthrough = shape.sheet.passthrough.clone();
        // The text, whole: its characters, the markers of its runs, paragraphs and tabs, and its fields (the writer carries what a text holds beyond its characters in DiaCraft's
        // own data on the shape's style). Only the formatting a style sheet gives it is lost, and said. **What the shape shows through its master is the shape's own in the file**:
        // another program has no master to ask.
        let (text, foreign) = self.through_master(&shape_ref, shape, &mut sheet, work)?;
        let mut lost = self.audit(shape, work)?;
        // A link to the `Theme` style the copy does not carry (the three links are not that one style: a master that links a style of its own for a family) is dropped:
        // said, as every other thing a new shape has that the file does not hold is.
        if carried.is_none()
            && self.theme.is_some_and(|t| [shape.sheet.links.line, shape.sheet.links.fill, shape.sheet.links.text].contains(&Some(t)))
        {
            lost |= THEME_LINK;
        }
        let copy = Shape {
            id: shape.id,
            name: shape.name.clone(),
            name_u: shape.name_u.clone(),
            kind: shape.kind.clone(),
            master: shape.master,
            master_shape: shape.master_shape,
            del: false,
            sheet: Arc::new(sheet),
            text,
            data1: shape.data1.clone(),
            data2: shape.data2.clone(),
            data3: shape.data3.clone(),
            foreign,
            children: ShapeList::default(),
            raw: without_ids(&shape.raw),
            passthrough: shape.passthrough.clone(),
            // As for a group above.
            ext: ext_without_ids(&shape.ext),
        };
        Ok(Entry { flat: Arc::new(copy), lost, style })
    }

    /// **What the shape shows that its master gives it and it does not hold**: its text (a shape with no `Text` of its own shows its master shape's: the renderer reads it so), the rows that
    /// format the text (a row of `Character`, `Paragraph`, `Tabs` or `Field` that the shape does not hold is its master's, cell by cell), the cells that place the text block, and the picture of a `Foreign` shape
    /// that holds none. The copy holds them as its own, as it holds the geometry it shows: a file read by another program has no master for it to ask, and a DiaCraft read of it has the same shape
    /// by value (the link to the master is kept, and the master rides in DiaCraft's document file). The shape's own text and its own picture are never replaced. Each section and row looked at is a step,
    /// charged before it is merged; a section the master holds more of than a shape may is refused in words, as the geometry's is.
    fn through_master(&self, at: &ShapeRef, shape: &Shape, sheet: &mut Sheet, work: &mut Work) -> Result<Shown, String> {
        let own = (shape.text.clone(), shape.foreign.clone());
        let Ok(Some(step)) = self.look.res.master_step(at) else { return Ok(own) };
        let Some(master) = self.look.res.shape(&step.shape) else { return Ok(own) };
        work.tick(1)?;
        let text = shape.text.clone().or_else(|| master.text.clone());
        let foreign =
            if shape.foreign.is_none() && shape.kind == diacraft_doc::ShapeType::Foreign { master.foreign.clone() } else { shape.foreign.clone() };
        if text.is_none() {
            return Ok((text, foreign));
        }
        self.text_block_through_master(at, shape, master, sheet, work)?;
        // The rows the text names: a row of the master that no marker of the text reaches shows nothing, and the link to the master has it for the shape that is read again. Row 0 of the character and
        // paragraph rows is the one a text with no marker of its own is formatted by.
        let shown = text.as_deref().map_or(&[][..], |t| &t.parts[..]);
        work.tick(shown.len() as u64)?;
        let (mut cp, mut pp, mut tp, mut fld): (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<u32>, BTreeSet<u32>) = Default::default();
        cp.insert(0);
        pp.insert(0);
        for part in shown {
            match part {
                diacraft_doc::TextPart::Cp(ix) => cp.insert(*ix),
                diacraft_doc::TextPart::Pp(ix) => pp.insert(*ix),
                diacraft_doc::TextPart::Tp(ix) => tp.insert(*ix),
                diacraft_doc::TextPart::Fld { ix, .. } => fld.insert(*ix),
                _ => false,
            };
        }
        for (name, wanted) in [("Character", &cp), ("Paragraph", &pp), ("Tabs", &tp), ("Field", &fld)] {
            work.tick(master.sheet.sections.len() as u64)?;
            let inherited: Vec<&Arc<Section>> = master.sheet.sections.iter().filter(|s| s.name.as_str() == name).collect();
            if inherited.is_empty() {
                continue;
            }
            if inherited.len() > MAX_SECTIONS {
                return Err(format!("a new shape has {} {name} sections, and an .odg save writes at most {MAX_SECTIONS}", inherited.len()));
            }
            // The sections of the copy that are the shape's own, taken out and put back with the rows of the master's that the text names laid under them.
            let mut kept: Vec<Arc<Section>> = Vec::new();
            let mut own_sections: Vec<Arc<Section>> = Vec::new();
            work.tick(sheet.sections.len() as u64)?;
            for section in std::mem::take(&mut sheet.sections) {
                if section.name.as_str() == name {
                    own_sections.push(section);
                } else {
                    kept.push(section);
                }
            }
            if own_sections.len() > MAX_SECTIONS {
                return Err(format!("a new shape has {} {name} sections, and an .odg save writes at most {MAX_SECTIONS}", own_sections.len()));
            }
            sheet.sections = kept;
            let by_ix: HashMap<Option<u32>, usize> = own_sections.iter().enumerate().rev().map(|(at, s)| (s.ix, at)).collect();
            let mut used = vec![false; own_sections.len()];
            for master_section in inherited {
                // Each row the text names is one look in the master's section (by key, through its table): the rows of the master that nothing names are not walked.
                work.tick(wanted.len() as u64)?;
                let mut picked = Section::new(name, master_section.ix);
                work.tick(master_section.cells.len() as u64)?;
                picked.cells = master_section.cells.clone();
                for ix in wanted {
                    if let Some(row) = master_section.row(&RowKey::Ix(*ix)) {
                        picked.rows.push(Arc::clone(row));
                    }
                }
                let own = by_ix.get(&master_section.ix).and_then(|at| own_sections.get(*at).map(|own| (*at, own)));
                match own {
                    Some((at, own)) => {
                        if let Some(flag) = used.get_mut(at) {
                            *flag = true;
                        }
                        sheet.sections.push(Arc::new(section_over(&picked, own, work)?));
                    }
                    None if picked.rows.is_empty() && picked.cells.is_empty() => {}
                    None => sheet.sections.push(Arc::new(picked)),
                }
            }
            for (section, used) in own_sections.into_iter().zip(used) {
                if !used {
                    sheet.sections.push(section);
                }
            }
        }
        Ok((text, foreign))
    }

    /// The cells of the text block that the master shape holds and the shape does not, put into the copy **when the block they give is not the whole shape** (the default of every shape, which a file says by
    /// having none: a master that holds `TxtPinX` as `Width*0.5` is no block of its own, and an instance of it is given none, so that no instance is given data the file's markup says without it). The block
    /// is judged by what the cells resolve to for this shape, each against its default: the pin at the middle, the size of the shape, the text's pin at the middle of the block, no turn, no background.
    fn text_block_through_master(&self, at: &ShapeRef, shape: &Shape, master: &Shape, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        let here = SheetRef::Shape(at.clone());
        let inherited: Vec<(&str, &Cell)> = TEXT_BLOCK_CELLS
            .iter()
            .filter(|name| !sheet.cells.contains_key(**name) && !shape.sheet.cells.contains_key(**name))
            .filter_map(|name| master.sheet.cells.get(*name).map(|cell| (*name, cell)))
            .collect();
        work.tick(TEXT_BLOCK_CELLS.len() as u64)?;
        if inherited.is_empty() {
            return Ok(());
        }
        let (width, height) = (self.num(&here, "Width", work)?, self.num(&here, "Height", work)?);
        let (block_width, block_height) = (self.num(&here, "TxtWidth", work)?, self.num(&here, "TxtHeight", work)?);
        // A block cell the shape resolves to nothing for reads as 0 here, so the width and height of a block that is not stated are the shape's.
        let (block_width, block_height) =
            (if block_width == 0.0 { width } else { block_width }, if block_height == 0.0 { height } else { block_height });
        let mut differs = false;
        for (name, _) in &inherited {
            let default = match *name {
                "TxtPinX" => width / 2.0,
                "TxtPinY" => height / 2.0,
                "TxtWidth" => width,
                "TxtHeight" => height,
                "TxtLocPinX" => block_width / 2.0,
                "TxtLocPinY" => block_height / 2.0,
                _ => 0.0,
            };
            let now = self.num(&here, name, work)?;
            differs |= block_differs(now, default);
        }
        if differs {
            for (name, cell) in inherited {
                sheet.cells.insert(CellName::new(name), cell.clone());
            }
        }
        Ok(())
    }

    /// **The shape's own formulas stay** (a formula that does not come back is a loss, even when its value does). Each cell of the frame and the style the copy holds as
    /// the constant the shape resolves to (so that another program draws the shape as the drawing has it), and that the shape holds itself with a formula of its own, is the shape's cell, with the value
    /// worked out where the shape's own is another (a colour told only by its case is the same): the writer writes the value in the file's attribute and carries the formula in DiaCraft's data on the
    /// shape's style. An evidence run found 1,808 of 13,267 formulas of VSDX drawings lost on a trip through an `.odg` (`RGB()`, `GUARD`, `Sheet.N!` and the shape's own cells), where the copy held
    /// constants.
    fn own_formulas(shape: &Shape, frame_too: bool, glued: bool, all: bool, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        for (name, cell) in sheet.cells.iter_mut() {
            let Some(own) = shape.sheet.cells.get(name.as_str()) else { continue };
            if !matches!(own.formula, FormulaSlot::Local(_)) || own.formula.text() == cell.formula.text() {
                continue;
            }
            // Outside a drawing from a VSDX file, only the formulas of the theme (a palette pick, Use Default).
            if !all && !own.formula.text().is_some_and(is_theme_formula) {
                continue;
            }
            work.tick(1)?;
            // The ends of a glued line the reader gives back from the file's glue, with the formula it writes for them; any other frame cell of a member or a line is written as its value.
            if glued && ENDS.contains(&name.as_str()) {
                continue;
            }
            if !frame_too && (FRAME.contains(&name.as_str()) || FRAME_IF_SET.contains(&name.as_str()) || ENDS.contains(&name.as_str())) {
                work.formulas_lost = work.formulas_lost.saturating_add(1);
                continue;
            }
            let same = match (own.value.as_ref(), cell.value.as_ref()) {
                (Some(a), Some(b)) => match (a.number(), b.number()) {
                    (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * (1.0 + y.abs()),
                    _ => a.text().eq_ignore_ascii_case(b.text()),
                },
                _ => false,
            };
            let mut kept = own.clone();
            if !same {
                kept.value = cell.value.clone();
                kept.unit = cell.unit.clone();
            }
            *cell = kept;
        }
        Ok(())
    }

    /// What the shape holds in its own sheet that no cell above carries, as it is (a cell with its formula, a section whole): the cells the copy was not given by name
    /// (the layer, the text block, the rounded corners, a shadow, the text's background and direction, a user cell) and the sections that are not merged with the master's
    /// (`Property`, `User`, the text's rows, any other). **The cells and sections of the shape's master are not copied**: the copy keeps the link and the writer writes the master
    /// into DiaCraft's document file. The place of a label on a line or a connector that holds no text is the engine's own and is left out: carried, it is written
    /// into the style of every connector (`diacraft:cells`) and read back as the connector's own, which a save of a connector of the file takes out again (`constants`), so
    /// the second save of the same drawing would be an edit (measured: `odg_newsave_more`). It stays the one silent drop of the copy.
    fn own_rest(shape: &Shape, one_d: bool, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        let labelless = one_d && shape.text.is_none();
        for (name, cell) in shape.sheet.cells.iter() {
            work.tick(1)?;
            let n = name.as_str();
            if INERT.contains(&n) || sheet.cells.contains_key(name) || is_resolved(n) || (labelless && LABEL_CELLS.contains(&n)) {
                continue;
            }
            sheet.cells.insert(name.clone(), cell.clone());
        }
        for section in &shape.sheet.sections {
            work.tick(1)?;
            if matches!(section.name.as_str(), "Geometry" | "Connection") {
                continue;
            }
            sheet.sections.try_reserve(1).map_err(|_| "there is not enough memory to save the new shapes".to_owned())?;
            sheet.sections.push(Arc::clone(section));
        }
        Ok(())
    }

    /// What a shape has from a style sheet that the file has no attribute for (rounded corners and a shadow): the copy has no link to the sheet, so the cells are given as
    /// constants of what the shape resolves to, unless the shape holds them (its own cells are copied as they are) or its master does (the link gives them).
    fn style_look(&self, at: &SheetRef, shape: &Shape, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        if !sheet.cells.contains_key("Rounding")
            && !self.through_link(shape, "Rounding")
            && self.num(at, "Rounding", work)? != 0.0
            && let Some(cell) = self.number(at, "Rounding", work)?
        {
            sheet.cells.insert(CellName::new("Rounding"), cell);
        }
        if !sheet.cells.contains_key("ShdwPattern") && !self.through_link(shape, "ShdwPattern") && self.num(at, "ShdwPattern", work)? != 0.0 {
            for name in SHADOW_CELLS {
                if sheet.cells.contains_key(name) || self.through_link(shape, name) {
                    continue;
                }
                let cell = if name == "ShdwForegnd" { self.colour(at, name, work)? } else { self.number(at, name, work)? };
                if let Some(cell) = cell {
                    sheet.cells.insert(CellName::new(name), cell);
                }
            }
        }
        Ok(())
    }

    /// The kind of loss that is left: the text style its text reaches. The cells and sections of the shape's master are not among them: the copy keeps the link and the writer
    /// writes the master into DiaCraft's document file. A style sheet is not: the shape a file gives is linked to a style sheet of its own, which the master's
    /// links do not reach, so the text style of the master's shape is a loss ([`Flattener::text_style_loss`]), until the writer writes a style sheet's text rows (a later task: wrote a shape's own).
    fn audit(&self, shape: &Shape, work: &mut Work) -> Result<Lost, String> {
        // The text a shape reaches through the `TextStyle` link of itself and of its master's shape: a style sheet that holds the rows of a run or a paragraph
        // gives the shape's text a look the copy, which has no link, does not carry (the review, parked: only the shape's own sheet and its
        // master's were audited, so a stencil master whose text links to a text style lost its text properties without a word).
        let master_sheet = self.master_sheet(shape);
        Ok(if self.text_style_loss(shape, master_sheet, work)? { TEXT_FORMAT } else { 0 })
    }

    /// Whether a style sheet the text of the shape reaches (the `TextStyle` link of the shape, of the shape its master holds, and of each sheet along the
    /// chain, but the root style, whose rows are the defaults of every shape) holds the rows of a run, a paragraph or a tab list, or a cell that qualifies the
    /// text: formatting the copy does not carry. The chain is followed at most [`MOST_STYLE_STEPS`] sheets.
    fn text_style_loss(&self, shape: &Shape, master_sheet: Option<&Sheet>, work: &mut Work) -> Result<bool, String> {
        let starts = [shape.sheet.links.text, master_sheet.and_then(|m| m.links.text)];
        for start in starts.into_iter().flatten() {
            let mut id = start;
            for _ in 0..MOST_STYLE_STEPS {
                if id == 0 {
                    break;
                }
                work.tick(1)?;
                let Some(style) = self.styles.get(&id) else { break };
                let sheet = &style.sheet;
                if sheet.sections.iter().any(|s| matches!(s.name.as_str(), "Character" | "Paragraph" | "Tabs") && !s.rows.is_empty())
                    || TEXT_FORMAT_CELLS.iter().any(|c| sheet.cells.contains_key(*c))
                {
                    return Ok(true);
                }
                id = sheet.links.text.unwrap_or(0);
            }
        }
        Ok(false)
    }

    /// The sections called `name` that the shape has once its master's are merged in, each with the cells and rows it resolves to, put into `sheet`.
    fn sections(&self, at: &SheetRef, name: &str, sheet: &mut Sheet, work: &mut Work) -> Result<(), String> {
        let res = &self.look.res;
        let keys = match res.effective_sections_within(at, name, MAX_SECTIONS) {
            None => return Ok(()),
            Some(Ok(keys)) => keys,
            Some(Err(count)) => {
                return Err(format!("a new shape has {count} {name} sections, and an .odg save writes at most {MAX_SECTIONS}"));
            }
        };
        for key in keys {
            work.tick(1)?;
            let mut section = Section::new(name, key.ix);
            for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
                work.tick(1)?;
                let addr = CellAddr { section: Some(key.clone()), row: None, cell: CellName::new(flag) };
                if let Some(found) = res.resolve(at, &addr).filter(|r| r.value.cell.is_some() || r.formula.cell.is_some()) {
                    section.cells.insert(CellName::new(flag), rebuilt(&found));
                }
            }
            // The rows are counted before any is merged, held to what the budget has left: the merge of a section is paid for as it is asked, and a section
            // too long for the budget is the budget's refusal, and one too long for a shape is the shape's.
            let left = usize::try_from(work.cap.saturating_sub(work.steps)).unwrap_or(usize::MAX).min(MAX_ROWS);
            let rows = match res.effective_rows_within(at, &key, left) {
                None => Vec::new(),
                Some(Ok(rows)) => rows,
                Some(Err(count)) => {
                    work.tick(u64::try_from(count).unwrap_or(u64::MAX))?;
                    return Err(format!("a new shape has a {name} section of {count} rows, and an .odg save writes at most {MAX_ROWS}"));
                }
            };
            work.tick(u64::try_from(rows.len()).unwrap_or(u64::MAX))?;
            for effective in rows {
                work.tick(u64::try_from(effective.cells.len()).unwrap_or(u64::MAX))?;
                let mut row = Row { row_type: effective.row_type.clone(), ..Row::default() };
                match &effective.key {
                    RowKey::Ix(ix) => row.ix = Some(*ix),
                    RowKey::Name(n) => row.name = Some(Arc::clone(n)),
                }
                for (cell_name, found) in &effective.cells {
                    row.cells.insert(cell_name.clone(), rebuilt(found));
                }
                section.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(section));
        }
        Ok(())
    }
}

/// Whether a cell of the text block resolves to a number other than the default of the block (the number a file says by having no cell): more than a billionth of one plus the default's size apart. False for
/// equal numbers and for a number that is not one (a comparison with it is false).
fn block_differs(now: f64, default: f64) -> bool {
    (now - default).abs() > 1e-9 * (1.0 + default.abs())
}

/// The section `own` laid over the section `master` of the same name and `IX` (rows and cells merge): the master's rows in their order, each with the cells the shape's row of the same key
/// holds put over them, then the shape's rows that the master has none for. A row or a section the shape deletes (`Del`) is gone, whatever the master holds. The rows of the two are charged before
/// anything is merged, and each row merged by its cells.
fn section_over(master: &Section, own: &Section, work: &mut Work) -> Result<Section, String> {
    work.tick((master.rows.len() as u64).saturating_add(own.rows.len() as u64))?;
    if own.del {
        return Ok(own.clone());
    }
    let name = master.name.as_str();
    let mut out = Section::new(name, own.ix);
    work.tick((master.cells.len() as u64).saturating_add(own.cells.len() as u64))?;
    out.cells = master.cells.clone();
    out.cells.extend(own.cells.iter().map(|(k, v)| (k.clone(), v.clone())));
    out.raw = own.raw.clone();
    out.passthrough = own.passthrough.clone();
    out.ext = own.ext.clone();
    let mut by_key: HashMap<RowKey, usize> = HashMap::new();
    by_key.try_reserve(own.rows.len()).map_err(|_| "there is not enough memory to save the new shapes".to_owned())?;
    for (at, row) in own.rows.iter().enumerate() {
        if let Some(key) = row.key(name) {
            by_key.entry(key).or_insert(at);
        }
    }
    let mut used = vec![false; own.rows.len()];
    out.rows
        .try_reserve(master.rows.len().saturating_add(own.rows.len()))
        .map_err(|_| "there is not enough memory to save the new shapes".to_owned())?;
    for row in master.rows.iter() {
        let over = row.key(name).and_then(|key| by_key.get(&key).copied()).and_then(|at| own.rows.get(at).map(|r| (at, r)));
        let Some((at, over)) = over else {
            out.rows.push(Arc::clone(row));
            continue;
        };
        if let Some(flag) = used.get_mut(at) {
            *flag = true;
        }
        work.tick((row.cells.len() as u64).saturating_add(over.cells.len() as u64))?;
        if over.del {
            continue;
        }
        let mut merged = Row::clone(row);
        merged.cells.extend(over.cells.iter().map(|(k, v)| (k.clone(), v.clone())));
        if over.row_type.is_some() {
            merged.row_type.clone_from(&over.row_type);
        }
        out.rows.push(Arc::new(merged));
    }
    for (row, used) in own.rows.iter().zip(used) {
        if !used && !row.del {
            out.rows.push(Arc::clone(row));
        }
    }
    Ok(out)
}

/// The cell a resolution found, as a cell of its own: the formula it resolved to when that is the cell's own, and the value this sheet holds.
fn rebuilt(found: &Resolved<'_>) -> Cell {
    let formula = found.formula.cell.map(|c| c.formula.clone()).filter(|f| matches!(f, FormulaSlot::Local(_))).unwrap_or_default();
    Cell { formula, value: found.value.cell.and_then(|c| c.value.clone()), unit: unit_of(found), ..Cell::default() }
}

/// The unit a resolution found: that of the cell that holds the value, else of the cell that holds the formula.
fn unit_of(found: &Resolved<'_>) -> Option<Arc<str>> {
    found.value.cell.and_then(|c| c.unit.clone()).or_else(|| found.formula.cell.and_then(|c| c.unit.clone()))
}

/// The colour cells the writer has an attribute for.
const COLOUR_CELLS: [&str; 2] = ["FillForegnd", "LineColor"];

/// The cells of the place of a label that the engine writes on a connector when it routes it (the label stays upright on its middle leg): the writer
/// has no attribute for them, the reader makes none, and a connector that holds no text has no label for them to place, so the copy of such a connector has
/// none (a connector with a text keeps them, and a change of one is the writer's refusal in words).
const LABEL_CELLS: [&str; 8] = ["TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtAngle", "TxtWidth", "TxtHeight", "TextBkgnd"];

/// A colour cell that holds `RGB(r,g,b)` of the colour it shows, as the constant it stands for. A colour a person chooses is written so by the
/// format commands (`F="RGB(r,g,b)"`, `V="#rrggbb"`), which is the colour and no more: the formula names no cell, and a file holds the colour as the
/// attribute of a style. Any other formula in a style cell is a formula (a theme's, say), which the writer refuses in words.
pub(crate) fn constant_colour(cell: &Cell) -> Option<Cell> {
    let FormulaSlot::Local(formula) = &cell.formula else { return None };
    let source: &str = &formula.source;
    let inside = source.trim().strip_prefix("RGB(")?.strip_suffix(')')?;
    let mut parts = inside.split(',');
    let mut rgb = [0u8; 3];
    for slot in &mut rgb {
        *slot = parts.next()?.trim().parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    let shown = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    cell.value.as_ref()?.text().eq_ignore_ascii_case(&shown).then(|| Cell { formula: FormulaSlot::Absent, ..cell.clone() })
}

/// The shape with its `RGB(r,g,b)` colours made constants (see [`constant_colour`]), and its members the same way; `None` when nothing changed, so that
/// a shape the save does not touch is the very shape it was. `depth` counts the levels so far, held to the model's own bound.
fn constants(shape: &Arc<Shape>, depth: usize, work: &mut Work) -> Result<Option<Arc<Shape>>, String> {
    work.tick(1)?;
    let swapped: Vec<(CellName, Cell)> =
        COLOUR_CELLS.iter().filter_map(|name| shape.sheet.cell(name).and_then(constant_colour).map(|cell| (CellName::new(name), cell))).collect();
    // The colours of the runs: a `Character` row holds the colour a person chose as `RGB(r,g,b)` too, and the writer takes a constant.
    let mut run_colours: Vec<(usize, usize, Cell)> = Vec::new();
    for (at, section) in shape.sheet.sections.iter().enumerate().filter(|(_, s)| s.name.as_str() == "Character") {
        for (row_at, row) in section.rows.iter().enumerate() {
            work.tick(1)?;
            if let Some(cell) = row.cells.get("Color").and_then(constant_colour) {
                run_colours.try_reserve(1).map_err(|_| "there is not enough memory to save the shapes".to_owned())?;
                run_colours.push((at, row_at, cell));
            }
        }
    }
    // The label cells of a connector with no text.
    let unlabelled = shape.text.is_none() && shape.sheet.cell("BeginX").is_some() && LABEL_CELLS.iter().any(|name| shape.sheet.cell(name).is_some());
    let mut members: Option<Vec<Arc<Shape>>> = None;
    if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
        for (at, member) in shape.children.iter().enumerate() {
            let Some(new) = constants(member, depth + 1, work)? else { continue };
            if members.is_none() {
                let list = (shape.children.len() as u64).saturating_mul(8);
                work.charge(0, list.saturating_add(shape.children.len() as u64 / diacraft_doc::CHUNK as u64 * 8))?;
                let mut all: Vec<Arc<Shape>> = Vec::new();
                all.try_reserve_exact(shape.children.len()).map_err(|_| "there is not enough memory to save the shapes".to_owned())?;
                all.extend(shape.children.iter().cloned());
                members = Some(all);
            }
            if let Some(slot) = members.as_mut().and_then(|all| all.get_mut(at)) {
                *slot = new;
            }
        }
    }
    if swapped.is_empty() && run_colours.is_empty() && !unlabelled && members.is_none() {
        return Ok(None);
    }
    // The copy is charged before it is made: a shape whose sheet changes is a second sheet beside the model's, and a group that holds changed members is a
    // second list of them (the first version of this charged neither, and a drawing of 175,000 members whose colours a person had chosen aborted the save).
    let own = if !swapped.is_empty() || !run_colours.is_empty() || unlabelled { work.price(shape) } else { 0 };
    work.charge(own, own.saturating_add(std::mem::size_of::<Shape>() as u64))?;
    let mut copy = Shape::clone(shape);
    if !swapped.is_empty() || !run_colours.is_empty() || unlabelled {
        let sheet = Arc::make_mut(&mut copy.sheet);
        for (name, cell) in swapped {
            sheet.cells.insert(name, cell);
        }
        for (at, row_at, cell) in run_colours {
            let row = sheet.sections.get_mut(at).map(Arc::make_mut).and_then(|section| section.rows.get_mut(row_at)).map(Arc::make_mut);
            if let Some(row) = row {
                row.cells.insert(CellName::new("Color"), cell);
            }
        }
        if unlabelled {
            for name in LABEL_CELLS {
                sheet.cells.remove(name);
            }
        }
    }
    if let Some(all) = members {
        copy.children = ShapeList::from_vec(all);
    }
    Ok(Some(Arc::new(copy)))
}

/// A shape's extension slot without the ids of the file's own among the attributes of its `odf` data (an `.odg` shape keeps its `xml:id` and `draw:id` there since
///): an id names one element, and a copy is another. The rest of the slot is shared with the shape.
pub(crate) fn ext_without_ids(ext: &diacraft_doc::Ext<diacraft_doc::OdfShape>) -> diacraft_doc::Ext<diacraft_doc::OdfShape> {
    let mut out = ext.clone();
    if ext.get().is_some_and(|d| d.odf.kept.raw.get("xml:id").is_some() || d.odf.kept.raw.get("draw:id").is_some()) {
        let data = out.make_mut();
        data.odf.kept.raw = without_ids(&data.odf.kept.raw);
    }
    out
}

/// `raw` without the ids of the file's own: an id names one element, and a copy is another.
pub(crate) fn without_ids(raw: &RawAttrs) -> RawAttrs {
    if raw.get("xml:id").is_none() && raw.get("draw:id").is_none() {
        return raw.clone();
    }
    RawAttrs::from_pairs(raw.iter().filter(|(name, _)| !matches!(*name, "xml:id" | "draw:id")))
}
