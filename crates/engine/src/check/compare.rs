//! The comparison: the drawing a model writer was given against the drawing the file it wrote reads back as, class by class.
//!
//! **One direction.** Every item the drawing holds is looked for in the file read back, and what is not there, or is there with another value, is counted in its class. What the file holds
//! beyond the drawing is no loss and is not counted, but for the structure the drawing is made of (pages, shapes, masters) and a cell whose value the drawing gave another way.
//!
//! **The same content is not a difference**: a number in another unit, a colour in another spelling, `Width*1` for `Width`, a value a style sheet gives where the drawing
//! held it on the shape, a style link written as the values it gives. These set one flag (`as_values`), which the card says once, and are never counted. **A formula that is not a formula
//! in the file is counted**, whatever its value.
//!
//! **The cost is a walk of both drawings**, charged in steps (one for a shape, a sheet, a section, a row, a cell, a text, a part), before any table is built, against [`MAX_STEPS`]: a
//! drawing that needs more is not checked, and the save says so.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, DefFamily, Definition, Document, ForeignData, Format, MAX_SHAPE_DEPTH, Master, Owner, Page, PartRole, Resolver, Row, RowKey,
    Section, Shape, ShapeList, ShapeRef, ShapeType, Sheet, SheetRef, Style, Text, TextPart,
};

use super::cells::{self, Same, class_of, is_default, is_drawn_default, kind_of, own_formula, same_formula, same_value};
use super::class::{ALL, Class};
use super::kept;
use super::meter::{Meter, Stop};

/// The fewest steps a comparison may take: a shape, a sheet, a section, a row, a cell, a text or a part is a step. The same figure as every other walk of a whole drawing in the engine
/// (`across::MAX_STEPS`). **A drawing larger than it is given more** ([`budget`], item 10), in proportion to what the two drawings hold.
pub(crate) const MAX_STEPS: u64 = crate::across::MAX_STEPS;

/// The steps a comparison may take for each item the two drawings hold (a shape, a sheet, a section, a row, a cell, 64 bytes of text, a node or an attribute kept, a definition, 4,096 bytes of a
/// part): what the dearest of the linear walks takes for one, a cell of the drawing that the file gives by its style sheets (a step for it, [`RESOLVE_STEPS`] for the lookup) and a cell the
/// file adds (as many again), twice over. A scan that compares every item of a list with every item of another costs the product, which no figure in proportion to the lists is, and stops.
const STEPS_PER_ITEM: u64 = 2 * 2 * (1 + RESOLVE_STEPS as u64);

/// **The steps a comparison of `before` and `after` may take** (item 10): [`MAX_STEPS`], or [`STEPS_PER_ITEM`] for each item the two drawings hold, whichever is more. A fixed
/// figure refused the largest drawings of the sample files as "too big to compare in one step" when their walk was a walk of every cell, no dearer for each than a small drawing's; the walk of
/// a drawing is now held to what a walk of a drawing of its size costs, and a dearer one (a scan of every item against every other, which a hostile file makes) still stops, in words.
/// The items are counted in one pass over each drawing, a step each and no table: what the save that wrote the file walked already.
pub(crate) fn budget(before: &Document, after: &Document) -> u64 {
    MAX_STEPS.max(items_of(before).saturating_add(items_of(after)).saturating_mul(STEPS_PER_ITEM))
}

/// The items a drawing holds, as [`budget`] counts them: every page's, master's and style sheet's sheet and shapes (down every group, with a stack), the parts, the definitions and the
/// document's own sheet.
fn items_of(doc: &Document) -> u64 {
    let mut items: u64 = 0;
    let mut add = |n: usize| items = items.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
    let sheet_items = |sheet: &Sheet| -> usize {
        let mut n = 1usize.saturating_add(sheet.cells.len()).saturating_add(sheet.raw.len()).saturating_add(sheet.passthrough.len());
        for section in &sheet.sections {
            n = n.saturating_add(1).saturating_add(section.cells.len()).saturating_add(section.rows.len());
            for row in section.rows.iter() {
                n = n.saturating_add(row.cells.len());
            }
        }
        n
    };
    let shape_items = |shape: &Shape| -> usize {
        let text = shape.text.as_ref().map_or(0, |t| {
            t.parts.len().saturating_add(text_bytes(t) >> 6).saturating_add(held_in_text(t).map(node_cost).fold(0, usize::saturating_add))
        });
        let kept = shape.ext.get().map_or(0, |e| e.odf.held.len().saturating_add(e.odf.kept.kept.len()).saturating_add(e.odf.kept.raw.len()));
        1usize.saturating_add(sheet_items(&shape.sheet)).saturating_add(text).saturating_add(kept).saturating_add(shape.passthrough.len())
    };
    let mut lists: Vec<&ShapeList> = Vec::new();
    for page in doc.pages.iter() {
        add(sheet_items(&page.sheet).saturating_add(page.connects.len()));
        lists.push(&page.shapes);
    }
    for master in doc.masters.iter() {
        add(sheet_items(&master.sheet));
        lists.push(&master.shapes);
    }
    for style in doc.styles.iter() {
        add(sheet_items(&style.sheet));
    }
    while let Some(list) = lists.pop() {
        for shape in list.iter() {
            add(shape_items(shape));
            if !shape.children.is_empty() {
                lists.push(&shape.children);
            }
        }
    }
    add(sheet_items(&doc.sheet));
    add(doc.parts.iter().map(|p| p.bytes.len() >> 12).fold(doc.parts.len(), usize::saturating_add));
    add(doc.fonts.len().saturating_add(doc.themes.len()).saturating_add(doc.props.custom.len()).saturating_add(doc.comments.iter().count()));
    if let Some(ext) = doc.ext.get() {
        add(ext.odf.definitions.len().saturating_add(ext.odf.styles.len()).saturating_add(ext.odf.layers.len()).saturating_mul(ELEMENT_STEPS));
    }
    items
}

/// What a comparison found: for each class the items that differ, and whether anything was the same content in another form.
#[derive(Clone, Debug)]
pub(crate) struct Tally {
    counts: [u64; ALL.len()],
    /// Some content is written in a form the file format holds (a unit, a formula form, a value from a style sheet, a style link as its values).
    pub as_values: bool,
}

impl Default for Tally {
    fn default() -> Tally {
        Tally { counts: [0; ALL.len()], as_values: false }
    }
}

impl Tally {
    /// Counts `n` more items of `class`.
    pub(crate) fn add(&mut self, class: Class, n: u64) {
        if let Some(slot) = self.counts.get_mut(class as usize) {
            *slot = slot.saturating_add(n);
        }
    }

    /// The items counted for `class`.
    pub(crate) fn count(&self, class: Class) -> u64 {
        self.counts.get(class as usize).copied().unwrap_or(0)
    }

    /// One more item of each class in `flags`.
    pub(crate) fn flagged(&mut self, flags: u32) {
        for class in ALL {
            if flags & class.bit() != 0 {
                self.add(class, 1);
            }
        }
    }
}

/// The plain `text:span`s of a paragraph that hold values of their style no cell says, by where they begin.
fn plain_runs_that_hold_values(runs: &[diacraft_doc::OdfRun]) -> Vec<&diacraft_doc::OdfRun> {
    let mut held: Vec<&diacraft_doc::OdfRun> = runs.iter().filter(|r| r.element.is_none() && r.from < r.to && !r.style.held.is_empty()).collect();
    held.sort_by_key(|r| r.from);
    held
}

/// What the comparison of a paragraph's runs found (`Compare::runs_differ`): why the formatting of its text differs, if it does, and how many hyperlinks of the text the file lacks.
struct RunsSaid {
    why: Option<&'static str>,
    links: u64,
}

/// The classes of rows, which count each row that differs and not the sheet that holds it.
fn counts_rows(class: Class) -> bool {
    matches!(class, Class::Fields | Class::Hyperlinks | Class::UserCells | Class::Connections)
}

/// The steps a comparison of two elements of kept markup (a definition, a layer, a style) is charged: each is a tree of its own in its own allocation, so that a scan of a long list is bound by the memory
/// it walks and not by the compare (a hundred thousand definitions scanned for each of a hundred thousand took nine seconds for the sixty million steps they were charged at one each, measured).
const ELEMENT_STEPS: usize = 16;

/// The steps a lookup of what a cell resolves to is charged: it walks a master and a chain of at most [`diacraft_doc::MAX_STYLE_STEPS`] style sheets, and the tables it reads are built once.
const RESOLVE_STEPS: usize = 16;

/// Where a cell is, for the lookup of what it resolves to.
struct Place<'p> {
    sheets: &'p (SheetRef, SheetRef),
    section: Option<(&'p str, Option<u32>)>,
    row: Option<&'p RowKey>,
    name: &'p str,
}

impl Place<'_> {
    fn addr(&self) -> CellAddr {
        match (self.section, self.row) {
            (None, _) => CellAddr::direct(self.name),
            (Some((section, ix)), None) => CellAddr::in_section(section, ix, self.name),
            (Some((section, ix)), Some(row)) => CellAddr::in_row(section, ix, row.clone(), self.name),
        }
    }
}

/// What a cell resolves to where it is not local: the value and the formula the sheets above it give.
#[derive(Default)]
struct Effective<'a> {
    value: Option<&'a str>,
    formula: Option<&'a str>,
}

fn effective<'a>(resolver: &Resolver<'a>, sheet: &SheetRef, addr: &CellAddr) -> Effective<'a> {
    let given = |sheet: &SheetRef| {
        resolver.resolve(sheet, addr).map(|resolved| Effective {
            value: resolved.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text()),
            formula: resolved.formula.cell.and_then(own_formula),
        })
    };
    let own = given(sheet).unwrap_or_default();
    // **A cell nothing gives a shape is what the root style gives it**: a shape that follows no style sheet is drawn with the root's line, fill and text (the renderer's own fallback
    // root for a cell no family reaches), and a file that writes those values on the shape draws it as it was drawn.
    if own.value.is_none() && !matches!(sheet, SheetRef::Style(_)) {
        let root = given(&SheetRef::Style(0)).unwrap_or_default();
        return Effective { value: root.value, formula: own.formula.or(root.formula) };
    }
    own
}

/// A list of shapes of the drawing and of the file read back, to be paired, with where they are.
struct Job<'d> {
    before: &'d ShapeList,
    after: &'d ShapeList,
    owners: (Owner, Owner),
    paths: (Vec<u32>, Vec<u32>),
}

/// Two shapes that stand for one, and where each is in its list.
struct Pair<'a> {
    before: &'a Arc<Shape>,
    after: &'a Arc<Shape>,
    at: (usize, usize),
}

/// The shapes of two lists, paired.
struct Paired<'a> {
    pairs: Vec<Pair<'a>>,
    /// Shapes of the drawing the file has none for.
    lost: Vec<&'a Arc<Shape>>,
    /// Shapes of the file the drawing has none for.
    added: Vec<&'a Arc<Shape>>,
}

/// What makes two connections one: the two shapes and the cell of each.
type ConnectKey = (u32, u32, Option<Arc<str>>, Option<Arc<str>>);

/// The comparison of two drawings.
pub(crate) struct Compare<'d> {
    before: &'d Document,
    after: &'d Document,
    resolvers: (OnceCell<Resolver<'d>>, OnceCell<Resolver<'d>>),
    pub(crate) tally: Tally,
    /// The caller chose to leave the macro project out of the file: it is no loss.
    macros_left_out: bool,
    meter: Meter,
    /// The first places that differ, in words with the names of cells and no more, for a test or a person who asks [`super::explain`]; at most [`MOST_TRACE`].
    pub(crate) trace: Vec<String>,
    /// Whether the trace is built at all: only [`super::explain`] says so, and a comparison for a save never does (a file of cells of megabytes of text would make a trace many times its size).
    pub(crate) trace_on: bool,
    /// The bytes the trace holds.
    trace_bytes: usize,
    /// Where the walk is (a page, a master, a style sheet, a shape), in words for the trace: kept only while the trace is on.
    at: String,
    /// For each page in hand, the `ID`s of the drawing's shapes and the shapes of the file paired with them: a connection is the same when it joins the same two shapes, whatever
    /// number the file gave them (that is [`Class::ShapeNumbers`], counted once for the shape).
    paired_ids: HashMap<u32, u32>,
    /// The ends of the drawing's shapes that a connection of the page or master in hand glues: the shape's `ID`, and whether it is the begin.
    glued: std::collections::HashSet<(u32, bool)>,
    /// Whether the begin and the end of the shape in hand are glued.
    glued_here: (bool, bool),
    /// Whether the shape in hand is a line or a connector that holds no text in the drawing and none in the file: its label's cells are the engine's ([`LABEL_CELLS`]).
    unlabelled_here: bool,
}

/// The cells of a connector's label that the engine writes when it routes one (`draw::label`: the place and size of the text block, and a white background where none is
/// given), and the white it gives the background.
const LABEL_CELLS: [&str; 8] = ["TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtAngle", "TxtWidth", "TxtHeight", "TextBkgnd"];
const LABEL_BACKGROUND: &str = "#FFFFFF";

/// The most places the trace holds.
const MOST_TRACE: usize = 64;

/// The most bytes the whole trace holds: it is checked before an entry is made, and an entry is cut to what is left.
pub(crate) const TRACE_BYTES: usize = 16 * 1024;

/// The most characters of a value, a name or a formula an entry of the trace holds.
const TRACE_VALUE: usize = 80;

/// A value, a name or a formula as the trace holds it: cut, and with the characters that could hide something shown as escapes.
fn cut(text: &str) -> String {
    crate::shown(text, TRACE_VALUE)
}

/// The plain text of a shape's text, paragraph by paragraph: the characters and the shown text of its fields, split at every line break (a carriage return is a paragraph break), the
/// empty paragraphs at the end left off.
fn paragraphs(text: &Text) -> Vec<String> {
    let mut list: Vec<String> = vec![String::new()];
    // A carriage return and a line feed after it are one break.
    let mut after_return = false;
    for part in &text.parts {
        let chars: &str = match part {
            TextPart::Chars(chars) => chars,
            TextPart::Fld { text, .. } => text,
            TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_) | TextPart::Raw(_) | TextPart::OdfRaw(_) | TextPart::NativeRaw(_) => continue,
        };
        // The text is copied once, in slices between the breaks (the breaks are ASCII, so a slice ends where a character does).
        let mut start = 0;
        for (at, byte) in chars.bytes().enumerate() {
            if byte != b'\r' && byte != b'\n' {
                continue;
            }
            if let (Some(last), Some(slice)) = (list.last_mut(), chars.get(start..at)) {
                last.push_str(slice);
            }
            // (A slice that is empty after a return that a line feed follows is no break of its own.)
            let continues_return = byte == b'\n' && after_return && start == at;
            if !continues_return {
                list.push(String::new());
            }
            after_return = byte == b'\r';
            start = at.saturating_add(1);
        }
        if let (Some(last), Some(slice)) = (list.last_mut(), chars.get(start..)) {
            if !slice.is_empty() {
                after_return = false;
            }
            last.push_str(slice);
        }
    }
    while list.last().is_some_and(String::is_empty) {
        list.pop();
    }
    list
}

/// The bytes of characters a text holds.
fn text_bytes(text: &Text) -> usize {
    text.parts
        .iter()
        .map(|part| match part {
            TextPart::Chars(chars) => chars.len(),
            TextPart::Fld { text, .. } => text.len(),
            _ => 0,
        })
        .fold(0, usize::saturating_add)
}

/// How many fields a text holds.
fn fields_in(text: &Text) -> usize {
    text.parts.iter().filter(|p| matches!(p, TextPart::Fld { .. })).count()
}

/// Whether two style links are one: no link is the root style, link 0.
fn same_link(before: Option<u32>, after: Option<u32>) -> bool {
    before.unwrap_or(0) == after.unwrap_or(0)
}

/// The items of two lists paired by key, **the n-th item of `before` that has a key with the n-th item of `after` that has it**, as the shapes of one `ID` are paired
/// ([`Compare::pair`]): a list that holds a key twice (two `Paragraph` rows of `IX` 0, two `Paragraph` sections, a deleted `Geometry` row and the row with no `IX` that the rows before
/// it number the same) is paired as it was written, where the first item of a key was the pair of every item of it. An item `keyed` gives no key for (a deleted one) takes no part.
///
/// Gives, for each item of `before`, the place of its pair in `after`, and for each item of `after` whether it was paired. One table of the keys of `after` and, for each of its places, the
/// next place of the same key: a step for each item of the two lists, which the caller charges.
pub(crate) fn pair_keyed<'a, T, K: Eq + std::hash::Hash>(
    before: &'a [T],
    after: &'a [T],
    keyed: impl Fn(&'a T) -> Option<K>,
) -> (Vec<Option<usize>>, Vec<bool>) {
    // The first place of each key not yet paired, and from each place the next of its key: made from the end, so that each key's first place is its first.
    let mut next: Vec<Option<usize>> = vec![None; after.len()];
    let mut first: HashMap<K, usize> = HashMap::new();
    for (at, item) in after.iter().enumerate().rev() {
        if let Some(key) = keyed(item) {
            if let Some(slot) = next.get_mut(at) {
                *slot = first.get(&key).copied();
            }
            first.insert(key, at);
        }
    }
    let mut taken = vec![false; after.len()];
    let pairs = before
        .iter()
        .map(|item| {
            let key = keyed(item)?;
            let at = first.get(&key).copied()?;
            match next.get(at).copied().flatten() {
                Some(following) => first.insert(key, following),
                None => first.remove(&key),
            };
            if let Some(slot) = taken.get_mut(at) {
                *slot = true;
            }
            Some(at)
        })
        .collect();
    (pairs, taken)
}

impl<'d> Compare<'d> {
    /// A comparison of `before`, the drawing the writer was given, with `after`, the drawing its file reads back as.
    pub(crate) fn new(before: &'d Document, after: &'d Document, macros_left_out: bool, max: u64) -> Compare<'d> {
        Compare {
            before,
            after,
            resolvers: (OnceCell::new(), OnceCell::new()),
            tally: Tally::default(),
            macros_left_out,
            meter: Meter::new(max),
            trace: Vec::new(),
            trace_on: false,
            trace_bytes: 0,
            at: String::new(),
            paired_ids: HashMap::new(),
            glued: std::collections::HashSet::new(),
            glued_here: (false, false),
            unlabelled_here: false,
        }
    }

    /// Says where something differs, for the trace: **the words are made only when the trace is asked for and while there is room for them** (the closure is not called otherwise), the values in them are
    /// cut by the caller ([`cut`]), and an entry is cut again to the bytes the budget has left.
    fn said(&mut self, what: impl FnOnce() -> String) {
        if !self.trace_on || self.trace.len() >= MOST_TRACE || self.trace_bytes >= TRACE_BYTES {
            return;
        }
        let words = what();
        let room = TRACE_BYTES.saturating_sub(self.trace_bytes);
        let mut words = if words.len() > room { crate::shown(&words, room) } else { words };
        // The cut is by characters, and a character (the mark of the cut among them) may be more than a byte: the bytes are held to the room too.
        while words.len() > room {
            words.pop();
        }
        self.trace_bytes = self.trace_bytes.saturating_add(words.len());
        self.trace.push(words);
    }

    /// Counts `n` items of `class` that differ, and says what they are in the trace ([`super::explain`] traces **every** class, so that a card a person doubts can be followed to
    /// the place it counted). Nothing is counted or said for none.
    fn count(&mut self, class: Class, n: u64, what: impl FnOnce() -> String) {
        if n == 0 {
            return;
        }
        self.tally.add(class, n);
        self.say(class, what);
    }

    /// The bit of `class`, for the sheet or the shape in hand to count once, said in the trace with what differs.
    fn flag(&mut self, class: Class, what: impl FnOnce() -> String) -> u32 {
        self.say(class, what);
        class.bit()
    }

    /// The classes of `bits`, said in the trace as `what` (the held values that did not come back, which the caller counts): the bits are given back.
    fn flags(&mut self, bits: u32, what: &str) -> u32 {
        if bits != 0 && self.trace_on {
            for class in ALL.into_iter().filter(|class| bits & class.bit() != 0) {
                self.say(class, || what.to_owned());
            }
        }
        bits
    }

    /// Says in the trace where the walk is and that `class` differs there, with `what`.
    fn say(&mut self, class: Class, what: impl FnOnce() -> String) {
        if !self.trace_on {
            return;
        }
        let at = std::mem::take(&mut self.at);
        self.said(|| format!("{at}{}: {}", class.id(), what()));
        self.at = at;
    }

    /// Where the walk is, for the trace: made only while the trace is on (a save never makes it).
    fn here(&mut self, words: impl FnOnce() -> String) {
        if self.trace_on {
            self.at = words();
        }
    }

    /// Takes `n` steps, or stops the comparison.
    fn step(&mut self, n: usize) -> Result<(), Stop> {
        self.meter.charge(n);
        self.meter.check()
    }

    // What the format keeps beside the cells is compared with the meter: each scan is charged before it is made.
    fn attrs_kept(&self, before: &diacraft_doc::RawAttrs, after: &diacraft_doc::RawAttrs) -> bool {
        kept::attrs_kept(before, after, &self.meter)
    }

    fn nodes_kept(&self, before: &diacraft_doc::Passthrough, after: &diacraft_doc::Passthrough) -> bool {
        kept::nodes_kept(before, after, &self.meter)
    }

    fn kept_kept(&self, before: &diacraft_doc::Kept, after: &diacraft_doc::Kept) -> bool {
        kept::kept_kept(before, after, &self.meter)
    }

    fn rest_kept(&self, before: &diacraft_doc::VsdxRest, after: &diacraft_doc::VsdxRest) -> bool {
        kept::rest_kept(before, after, &self.meter)
    }

    fn held_lost(&self, before: &[diacraft_doc::Held], after: &[diacraft_doc::Held]) -> u32 {
        kept::held_lost(before, after, &self.meter)
    }

    /// Whether the held formatting of a text's paragraph or run is in the file's: every held value of the drawing's has one of its group with the same attributes.
    fn held_lost_text(&self, before: &[diacraft_doc::HeldStyleValue], after: &[diacraft_doc::HeldStyleValue]) -> bool {
        if before.is_empty() || !self.meter.charge(before.len().saturating_mul(after.len().max(1))) {
            return false;
        }
        before.iter().any(|wanted| !after.iter().any(|found| found.group == wanted.group && found.attrs == wanted.attrs && found.kept == wanted.kept))
    }

    /// Whether the kept data of a master's slot (a VSDX element the master had no place for) is all in the file's.
    fn kept_kept_ext(&self, before: &diacraft_doc::Ext<diacraft_doc::Kept>, after: &diacraft_doc::Ext<diacraft_doc::Kept>) -> bool {
        match before.get() {
            None => true,
            Some(wanted) => {
                let found = after.get();
                let empty = diacraft_doc::Kept::default();
                self.kept_kept(&wanted.odf, found.map_or(&empty, |f| &f.odf))
                    && self.rest_kept(&wanted.vsdx, &found.map(|f| f.vsdx.clone()).unwrap_or_default())
            }
        }
    }

    /// The steps the comparison has taken.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> u64 {
        self.meter.steps()
    }

    fn resolver_before(&self) -> &Resolver<'d> {
        self.resolvers.0.get_or_init(|| Resolver::new(self.before))
    }

    fn resolver_after(&self) -> &Resolver<'d> {
        self.resolvers.1.get_or_init(|| Resolver::new(self.after))
    }

    // ---------------------------------------------------------------------------------------------------------------------------------- cells

    /// One cell of the drawing against the cell of the file at its place (`None`: the file holds none there). A formula that is not in the file is counted here; the class the **value**
    /// differs in is given back, for the sheet or the row that holds the cell to count.
    fn cell(&mut self, place: &Place<'_>, before: &Cell, after: Option<&Cell>) -> Result<Option<Class>, Stop> {
        self.step(1)?;
        // **The label of a line or a connector with no text is the engine's**: the engine writes its text block's place and a white background whenever it routes a
        // connector it made, and again for a connector of a file routed with text, and a text block with no text draws nothing; the `.odg` writer leaves them out of the copy of such a
        // shape (`odgfile::flatten`'s one drop), so a file that holds none of them is no difference. A background of another colour is a person's, and is judged.
        if after.is_none()
            && self.unlabelled_here
            && place.section.is_none()
            && LABEL_CELLS.contains(&place.name)
            && (place.name != "TextBkgnd" || before.value.as_ref().is_some_and(|v| v.text().eq_ignore_ascii_case(LABEL_BACKGROUND)))
        {
            return Ok(None);
        }
        let kind = kind_of(place.section.map(|(name, _)| name), place.name, before);
        let mut differs = false;
        if let Some(wanted) = before.value.as_ref() {
            let local = after.and_then(|c| c.value.as_ref());
            let verdict = match local {
                Some(found) => same_value(self.before, self.after, kind, wanted.text(), found.text()),
                None => {
                    // The file holds no value of its own here: what the sheets above it give is the value.
                    self.step(RESOLVE_STEPS)?;
                    let given = effective(self.resolver_after(), &place.sheets.1, &place.addr());
                    match given.value {
                        Some(found) => same_value(self.before, self.after, kind, wanted.text(), found),
                        None if is_default(kind, wanted.text()) => Same::InAnotherForm,
                        // What the shape is drawn with when nothing gives the cell (a line end's size), or, for a cell the file does not hold at all, what its default formula gives
                        // when the drawing's cell holds that formula (the text block's cells): the file gives the shape what the drawing did.
                        None if is_drawn_default(place.section.map(|(name, _)| name), place.name, wanted.text()) => Same::InAnotherForm,
                        None if after.is_none() && self.reads_default_formula(place, before) => Same::InAnotherForm,
                        None => Same::No,
                    }
                }
            };
            let verdict = if verdict == Same::No && self.same_route(place)? == Some(true) { Same::InAnotherForm } else { verdict };
            match verdict {
                Same::Exactly => self.tally.as_values |= local.is_none(),
                Same::InAnotherForm => self.tally.as_values = true,
                Same::No => {
                    differs = true;
                    let class = class_of(place.section.map(|(name, _)| name), place.name);
                    self.say(class, || {
                        format!(
                            "the value of {:?}.{}: {:?} is {:?} in the file",
                            place.section.map(|(name, _)| cut(name)),
                            cut(place.name),
                            cut(wanted.text()),
                            local.map(|v| cut(v.text()))
                        )
                    });
                }
            }
        }
        // **A glued end's formula is the glue's** (item 7): the reader of each format makes `BeginX`, `BeginY`, `EndX` and `EndY` of a glued end from the glue (`PAR(PNT(..))`), so
        // they are no formulas of the person's, and a file that keeps the glue gives them back in its reader's form. The connection is judged as one ([`Compare::connections`], in its
        // class), and its cells' formulas are not counted again.
        let glue_formula = place.section.is_none()
            && match place.name {
                "BeginX" | "BeginY" => self.glued_here.0,
                "EndX" | "EndY" => self.glued_here.1,
                _ => false,
            };
        if let Some(wanted) = own_formula(before).filter(|_| !glue_formula) {
            let local = after.and_then(own_formula);
            let verdict = match local {
                Some(found) => same_formula(wanted, found),
                None => {
                    self.step(RESOLVE_STEPS)?;
                    let given = effective(self.resolver_after(), &place.sheets.1, &place.addr());
                    match given.formula {
                        Some(found) => same_formula(wanted, found),
                        // A cell the file does not hold reads its default formula: the drawing's that is that formula comes back as it.
                        None if after.is_none() && self.reads_default_formula(place, before) => Same::InAnotherForm,
                        None => Same::No,
                    }
                }
            };
            match verdict {
                Same::Exactly => {}
                Same::InAnotherForm => self.tally.as_values = true,
                Same::No => {
                    self.count(Class::Formulas, 1, || {
                        format!("the formula of {:?}.{} is lost: {}", place.section.map(|(name, _)| cut(name)), cut(place.name), cut(wanted))
                    });
                }
            }
        }
        // A unit the file states the length in another way is the same value in another form.
        if let (Some(unit), Some(found)) = (before.unit.as_deref(), after.and_then(|c| c.unit.as_deref()))
            && !unit.eq_ignore_ascii_case(found)
        {
            self.tally.as_values = true;
        }
        // What the cell holds that no field reads (an attribute of its own, a node inside it) is kept data.
        if let Some(found) = after
            && (!self.attrs_kept(&before.raw, &found.raw))
        {
            self.count(Class::KeptData, 1, || {
                format!("the attributes of the cell {:?}.{}", place.section.map(|(name, _)| cut(name)), cut(place.name))
            });
        }
        Ok(differs.then(|| class_of(place.section.map(|(name, _)| name), place.name)))
    }

    /// Whether the drawing's cell `before` holds the formula a cell of its place reads when no sheet holds it ([`cells::default_formula`]: the text block's cells), so that a file that holds no
    /// such cell gives the shape the same formula and the value it works out to.
    fn reads_default_formula(&self, place: &Place<'_>, before: &Cell) -> bool {
        let Some(default) = cells::default_formula(place.section.map(|(name, _)| name), place.name) else { return false };
        own_formula(before).is_some_and(|wanted| same_formula(wanted, default) != Same::No)
    }

    /// **A connector's route**: for `ShapeRouteStyle` and `ConLineRouteExt` directly under a shape of a page, whether the route the drawing draws the shape in is the one the file
    /// draws it in, each cell being the shape's own or, where that is 0, the page's `RouteStyle` and `LineRouteExt` (as `draw::path_style` reads them); `None` for any other cell. The
    /// `.odg` writer writes the route a connector is drawn in on the connector (`draw:type`, which reads back as the two cells), so a connector that took its route from the page, or from nothing,
    /// comes back with it said on the shape: the same route, in the form the file holds. Charged for the lookups before they are made.
    fn same_route(&mut self, place: &Place<'_>) -> Result<Option<bool>, Stop> {
        if place.section.is_some() || !matches!(place.name, "ShapeRouteStyle" | "ConLineRouteExt") {
            return Ok(None);
        }
        let (SheetRef::Shape(drawn), SheetRef::Shape(written)) = place.sheets else { return Ok(None) };
        let (Owner::Page(drawn_page), Owner::Page(written_page)) = (drawn.owner, written.owner) else { return Ok(None) };
        // Four cells on each side, each the shape's and the page's.
        self.step(RESOLVE_STEPS.saturating_mul(8))?;
        let route = |resolver: &Resolver<'_>, sheet: &SheetRef, page: u32| {
            let number = |sheet: &SheetRef, name: &str| {
                resolver
                    .resolve(sheet, &CellAddr::direct(name))
                    .and_then(|r| r.value.cell)
                    .and_then(|c| c.value.as_ref())
                    .and_then(|v| v.number())
                    .unwrap_or(0.0)
            };
            let either = |own: &str, of_page: &str| {
                let held = number(sheet, own);
                if held != 0.0 { held } else { number(&SheetRef::Page(page), of_page) }
            };
            crate::draw::style_of(either("ShapeRouteStyle", "RouteStyle"), either("ConLineRouteExt", "LineRouteExt"))
        };
        let drawn = route(self.resolver_before(), &place.sheets.0, drawn_page);
        let written = route(self.resolver_after(), &place.sheets.1, written_page);
        Ok(Some(drawn == written))
    }

    /// A cell the file holds and the drawing holds none of: the value the drawing gives it where it is (the sheets above it) against the file's. Only a value that is not the same counts.
    fn cell_added(&mut self, place: &Place<'_>, after: &Cell) -> Result<Option<Class>, Stop> {
        self.step(1)?;
        let Some(found) = after.value.as_ref() else { return Ok(None) };
        let kind = kind_of(place.section.map(|(name, _)| name), place.name, after);
        self.step(RESOLVE_STEPS)?;
        let given = effective(self.resolver_before(), &place.sheets.0, &place.addr());
        let verdict = match given.value {
            Some(wanted) => same_value(self.before, self.after, kind, wanted, found.text()),
            // **The drawing held nothing for the cell**, not even from a style sheet or its master: it was drawn with what nothing gives a cell of its kind (the number 0, black, nothing), and a file that
            // writes that is the same; **one that writes anything else draws the shape otherwise, and is a difference** (the comparison goes both ways; a reader that added a
            // gradient's angle reading the file back was told as a value of the file's own, and nothing was named).
            None if is_default(kind, found.text()) => Same::Exactly,
            // What the shape is drawn with when nothing gives the cell (a line end's size, 2), written on it.
            None if is_drawn_default(place.section.map(|(name, _)| name), place.name, found.text()) => Same::InAnotherForm,
            None => Same::No,
        };
        let verdict = if verdict == Same::No && self.same_route(place)? == Some(true) { Same::InAnotherForm } else { verdict };
        Ok(match verdict {
            // A value the sheets above gave the shape in the drawing, written on the shape in the file, is a value the file holds.
            Same::Exactly => {
                self.tally.as_values |= given.value.is_some();
                None
            }
            Same::InAnotherForm => {
                self.tally.as_values = true;
                None
            }
            Same::No => {
                let class = class_of(place.section.map(|(name, _)| name), place.name);
                self.say(class, || {
                    format!(
                        "the file adds {:?}.{} = {:?}, which the drawing gives as {:?}",
                        place.section.map(|(name, _)| cut(name)),
                        cut(place.name),
                        cut(found.text()),
                        given.value.map(cut)
                    )
                });
                Some(class)
            }
        })
    }

    /// The cells of one container (a sheet, a section or a row): each of the drawing's against the file's, and the file's that the drawing has none of. → the classes the values differ in,
    /// as bits, and the classes of rows that differ are counted by the caller.
    fn cells(
        &mut self,
        sheets: &(SheetRef, SheetRef),
        section: Option<(&str, Option<u32>)>,
        row: Option<&RowKey>,
        before: &diacraft_doc::Cells,
        after: &diacraft_doc::Cells,
    ) -> Result<u32, Stop> {
        let mut flags = 0;
        for (name, wanted) in before {
            let place = Place { sheets, section, row, name: name.as_str() };
            if let Some(class) = self.cell(&place, wanted, after.get(name))? {
                flags |= class.bit();
            }
        }
        for (name, found) in after {
            if before.contains_key(name) || name.as_str().starts_with("ODF") {
                continue;
            }
            let place = Place { sheets, section, row, name: name.as_str() };
            if let Some(class) = self.cell_added(&place, found)? {
                flags |= class.bit();
            }
        }
        Ok(flags)
    }

    // ------------------------------------------------------------------------------------------------------------------------------- sheets

    /// The rows of a section of the drawing against those of the file: matched by key, the rows of one key in order and a deleted row with none ([`pair_keyed`]). → the classes (bits) of
    /// the rows that differ and are counted by the sheet, and the rows counted at once.
    fn rows(&mut self, sheets: &(SheetRef, SheetRef), name: &str, ix: Option<u32>, before: &Section, after: &Section) -> Result<u32, Stop> {
        let mut flags = 0;
        self.step(before.rows.len().saturating_add(after.rows.len()))?;
        // The rows that are not deleted, by key: the n-th of a key of the drawing's is the n-th of the file's (the first of a key was the pair of every row of it, so the second
        // of two rows of `IX` 0 was compared with the first, and the row after two deleted ones that the rows before it number 3 with the deleted row 3).
        let (pairs, taken) = pair_keyed(&before.rows, &after.rows, |row: &Arc<Row>| if row.del { None } else { row.key(name) });
        for (position, wanted) in before.rows.iter().enumerate() {
            if wanted.del {
                continue;
            }
            let key = wanted.key(name);
            let found: Option<&Arc<Row>> = match &key {
                Some(_) => pairs.get(position).copied().flatten().and_then(|at| after.rows.get(at)),
                None => after.rows.get(position).filter(|row| !row.del),
            };
            let class = class_of(Some(name), "");
            let mut differs = 0u32;
            match found {
                None => {
                    // The row is not in the file: every formula in it is lost with it.
                    differs |= self.flag(class, || format!("the row {position} of {} is not in the file", cut(name)));
                    self.step(wanted.cells.len())?;
                    let lost = wanted.cells.values().filter(|c| own_formula(c).is_some()).count();
                    self.count(Class::Formulas, u64::try_from(lost).unwrap_or(u64::MAX), || {
                        format!("the formulas of the row {position} of {}", cut(name))
                    });
                }
                Some(row) => {
                    if wanted.row_type.as_deref().is_some_and(|t| !row.row_type.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(t))) {
                        differs |= self.flag(class, || format!("the row {position} of {} is of another type in the file", cut(name)));
                    }
                    let place = key.as_ref();
                    differs |= self.cells(sheets, Some((name, ix)), place, &wanted.cells, &row.cells)?;
                    // A glue point of the row that did not come back, or a paragraph's or a run's held style.
                    if let Some(data) = wanted.ext.get()
                        && let Some(held) = data.odf.held.as_ref()
                    {
                        let have: Vec<diacraft_doc::Held> = row.ext.get().and_then(|f| f.odf.held.clone()).into_iter().collect();
                        let lost = self.held_lost(std::slice::from_ref(held), &have);
                        differs |= self.flags(lost, "a held value of a row did not come back");
                    }
                    if !self.attrs_kept(&wanted.raw, &row.raw) || !self.nodes_kept(&wanted.passthrough, &row.passthrough) {
                        self.count(Class::KeptData, 1, || format!("what the row {position} of {} kept beside its cells", cut(name)));
                    }
                }
            }
            for c in ALL {
                if differs & c.bit() != 0 {
                    if counts_rows(c) {
                        self.count(c, 1, || format!("the row {position} of {}", cut(name)));
                    } else {
                        flags |= c.bit();
                    }
                }
            }
        }
        // **The rows the file adds** (both ways): a row of the file's section that no row of the drawing's was paired with (by key, or past the drawing's rows when it has no
        // key) is judged as the cells the file adds, against what the drawing's sheets give there.
        for (position, found) in after.rows.iter().enumerate() {
            if found.del {
                continue;
            }
            let key = found.key(name);
            let added = match &key {
                Some(RowKey::Name(n)) if n.starts_with("ODF") => false,
                Some(_) => !taken.get(position).copied().unwrap_or(false),
                None => position >= before.rows.len(),
            };
            if added {
                flags |= self.row_added(sheets, (name, ix), key.as_ref(), found, position)?;
            }
        }
        Ok(flags)
    }

    /// A row the file holds that the drawing does not: each of its cells as a cell the file adds. A class that counts rows is counted for it here; the others are given back as bits, for the sheet.
    fn row_added(
        &mut self,
        sheets: &(SheetRef, SheetRef),
        (name, ix): (&str, Option<u32>),
        key: Option<&RowKey>,
        found: &Row,
        position: usize,
    ) -> Result<u32, Stop> {
        let mut differs = 0u32;
        for (cell, value) in &found.cells {
            if cell.as_str().starts_with("ODF") {
                continue;
            }
            let place = Place { sheets, section: Some((name, ix)), row: key, name: cell.as_str() };
            if let Some(class) = self.cell_added(&place, value)? {
                differs |= class.bit();
            }
        }
        let mut flags = 0u32;
        for c in ALL {
            if differs & c.bit() != 0 {
                if counts_rows(c) {
                    self.count(c, 1, || format!("the file adds the row {position} of {}", cut(name)));
                } else {
                    flags |= self.flag(c, || format!("the file adds the row {position} of {}", cut(name)));
                }
            }
        }
        Ok(flags)
    }

    /// Two sheets: the links, the cells, the sections. → the classes that differ, as bits (counted once for the sheet by the caller).
    fn sheet(&mut self, sheets: &(SheetRef, SheetRef), before: &Sheet, after: &Sheet) -> Result<u32, Stop> {
        self.step(1)?;
        let mut flags = 0;
        if !same_link(before.links.line, after.links.line)
            || !same_link(before.links.fill, after.links.fill)
            || !same_link(before.links.text, after.links.text)
        {
            // The links are the file's form of what the sheets above give: the values are compared cell by cell.
            self.tally.as_values = true;
        }
        flags |= self.cells(sheets, None, None, &before.cells, &after.cells)?;
        self.step(before.sections.len().saturating_add(after.sections.len()))?;
        // The sections that are not deleted, by name and `IX`: the n-th of a key of the drawing's is the n-th of the file's (two `Paragraph` sections were each compared with
        // the first of the file's).
        let (pairs, taken) =
            pair_keyed(&before.sections, &after.sections, |s: &Arc<Section>| if s.del { None } else { Some((s.name.as_str(), s.ix)) });
        for (place, wanted) in before.sections.iter().enumerate() {
            if wanted.del {
                continue;
            }
            let name = wanted.name.as_str();
            let class = class_of(Some(name), "");
            match pairs.get(place).copied().flatten().and_then(|at| after.sections.get(at)) {
                None => {
                    let rows = u64::try_from(wanted.rows.len().max(1)).unwrap_or(u64::MAX);
                    if counts_rows(class) {
                        self.count(class, rows, || format!("the section {} is not in the file", cut(name)));
                    } else {
                        flags |= self.flag(class, || format!("the section {} is not in the file", cut(name)));
                    }
                    // The cells of a section that is gone are walked to count their formulas, and charged first (the rows, then the cells in them).
                    self.step(wanted.rows.len())?;
                    let cells = wanted.rows.iter().map(|row| row.cells.len()).fold(wanted.cells.len(), usize::saturating_add);
                    self.step(cells)?;
                    let lost = wanted
                        .rows
                        .iter()
                        .flat_map(|row| row.cells.values())
                        .chain(wanted.cells.values())
                        .filter(|c| own_formula(c).is_some())
                        .count();
                    self.count(Class::Formulas, u64::try_from(lost).unwrap_or(u64::MAX), || format!("the formulas of the section {}", cut(name)));
                }
                Some(found) => {
                    flags |= self.cells(sheets, Some((name, wanted.ix)), None, &wanted.cells, &found.cells)?;
                    flags |= self.rows(sheets, name, wanted.ix, wanted, found)?;
                    if !self.attrs_kept(&wanted.raw, &found.raw) || !self.nodes_kept(&wanted.passthrough, &found.passthrough) {
                        self.count(Class::KeptData, 1, || format!("what the section {} kept beside its rows", cut(name)));
                    }
                }
            }
        }
        // **The sections the file adds** (both ways): each of its cells and rows as the file's additions, against what the drawing's sheets give there (a master's section, a style
        // sheet's): a section no section of the drawing's was paired with.
        for (_, found) in after.sections.iter().enumerate().filter(|(at, s)| !s.del && !taken.get(*at).copied().unwrap_or(false)) {
            let name = found.name.as_str();
            let mut differs = 0u32;
            for (cell, value) in &found.cells {
                if cell.as_str().starts_with("ODF") {
                    continue;
                }
                let place = Place { sheets, section: Some((name, found.ix)), row: None, name: cell.as_str() };
                if let Some(class) = self.cell_added(&place, value)? {
                    differs |= class.bit();
                }
            }
            if differs != 0 {
                for c in ALL.into_iter().filter(|c| differs & c.bit() != 0) {
                    flags |= self.flag(c, || format!("the file adds the section {}", cut(name)));
                }
            }
            self.step(found.rows.len())?;
            for (position, row) in found.rows.iter().enumerate() {
                if row.del || row.key(name).is_some_and(|key| matches!(&key, RowKey::Name(n) if n.starts_with("ODF"))) {
                    continue;
                }
                let key = row.key(name);
                flags |= self.row_added(sheets, (name, found.ix), key.as_ref(), row, position)?;
            }
        }
        if !self.attrs_kept(&before.raw, &after.raw) || !self.nodes_kept(&before.passthrough, &after.passthrough) {
            self.count(Class::KeptData, 1, || "what the sheet kept beside its cells".to_owned());
        }
        Ok(flags)
    }

    // ------------------------------------------------------------------------------------------------------------------------------- shapes

    /// The text of a shape: the characters paragraph by paragraph, the fields, and the formatting the paragraphs and runs held. → the classes (bits) that differ.
    fn text(&mut self, before: &Text, after: Option<&Text>) -> Result<u32, Stop> {
        self.step(before.parts.len())?;
        // The characters are copied once for each side to be told paragraph by paragraph: a step for each sixty-four bytes, charged before they are.
        self.step(text_bytes(before).saturating_add(after.map_or(0, text_bytes)) >> 6)?;
        let mut flags = 0;
        let wanted = paragraphs(before);
        let found = after.map(paragraphs).unwrap_or_default();
        let mut differing = 0usize;
        let mut first: Option<usize> = None;
        for (at, paragraph) in wanted.iter().enumerate() {
            if found.get(at) != Some(paragraph) {
                differing += 1;
                first.get_or_insert(at);
            }
        }
        self.count(Class::Text, u64::try_from(differing).unwrap_or(u64::MAX), || {
            let at = first.unwrap_or(0);
            format!("the paragraph {at} {:?} is {:?} in the file", wanted.get(at).map(|p| cut(p)), found.get(at).map(|p| cut(p)))
        });
        // **What a text holds whole where it stands** (a mark, a drawing object anchored as a character): each of the drawing's is in the file's text, in order. One that is not is
        // told as a picture when it shows one (a frame of a `draw:image`), and as kept data otherwise. Charged before the walk: a step for each part of both texts.
        self.step(before.parts.len().saturating_add(after.map_or(0, |t| t.parts.len())))?;
        // Each node with what a comparison of it walks (its nodes, attributes and bytes, [`node_cost`]), found once: a walk of each tree, charged as the walk of the text's parts is.
        let found = after.map(sized_in_text).unwrap_or_default();
        let wanted_list = sized_in_text(before);
        let walked = found.iter().chain(wanted_list.iter()).map(|(_, cost)| *cost).fold(0usize, usize::saturating_add);
        self.step(walked)?;
        // The file's are looked through from the one after the last found, and **each comparison is charged by the smaller of the two trees before it is made** (a comparison of two trees walks
        // them to where they differ, at most the smaller): a text of many large ones that are not there stops the walk, in words, rather than scan without end.
        let mut next = 0usize;
        for (wanted, cost) in wanted_list {
            let mut at = next;
            let mut hit = None;
            while let Some((have, have_cost)) = found.get(at) {
                self.step(cost.min(*have_cost).max(1))?;
                if **have == *wanted {
                    hit = Some(at);
                    break;
                }
                at += 1;
            }
            match hit {
                Some(at) => next = at.saturating_add(1),
                None => {
                    let class = if shows_picture(wanted) { Class::Pictures } else { Class::KeptData };
                    self.count(class, 1, || {
                        "an element the text held where it stands (a mark, or an object anchored as a character) is not in the file's text".to_owned()
                    });
                }
            }
        }
        let fields = fields_in(before);
        if fields > after.map_or(0, fields_in) {
            let lost = fields.saturating_sub(after.map_or(0, fields_in));
            self.count(Class::Fields, u64::try_from(lost).unwrap_or(u64::MAX), || format!("{fields} fields, {lost} of them not in the file"));
        }
        if let Some(held) = before.ext.get() {
            let found = after.and_then(|t| t.ext.get());
            let have = found.map_or(&[][..], |f| f.odf.paragraphs.as_slice());
            let held_paragraphs = &held.odf.paragraphs;
            if held_paragraphs.len() > have.len() {
                let (wanted, found) = (held_paragraphs.len(), have.len());
                flags |=
                    self.flag(Class::TextFormatting, || format!("the text held the formatting of {wanted} paragraphs, and the file's of {found}"));
            }
            for (at, (wanted, found)) in held_paragraphs.iter().zip(have.iter()).enumerate() {
                let mut why: Option<&'static str> = None;
                if self.held_lost_text(&wanted.style.held, &found.style.held) {
                    why = Some("a held value of its style");
                }
                // The runs, by the characters they cover and not by where one ends and the next begins: the writer joins neighbouring runs of one style.
                let runs = self.runs_differ(&wanted.runs, &found.runs)?;
                if let Some(runs) = runs.why {
                    why = Some(runs);
                }
                let links = runs.links;
                self.count(Class::Hyperlinks, links, || format!("{links} links of the paragraph {at} are not in the file's"));
                if wanted.levels.len() != found.levels.len() {
                    why = Some("its list levels");
                }
                if wanted.element != found.element && wanted.element.is_some() {
                    why = Some("its element");
                }
                if let Some(why) = why {
                    flags |= self.flag(Class::TextFormatting, || format!("the paragraph {at} differs in {why}"));
                }
            }
            if !self.kept_kept(&held.native, &found.map(|f| f.native.clone()).unwrap_or_default()) {
                self.count(Class::KeptData, 1, || "what the text kept beside its characters".to_owned());
            }
        }
        Ok(flags)
    }

    /// **The runs of a paragraph of the drawing against the file's, by the characters they cover**.
    ///
    /// A run is a `text:span` or another element of the paragraph's text, held by where it lies in the paragraph's characters. The writer joins neighbouring `text:span`s that the drawing's
    /// rows give one formatting into one, so the file's paragraph has fewer runs over the same characters with the same formatting, and that is no change: counting the runs said that three
    /// runs of one style were another formatting than one run of the same words. **So the runs are told in two kinds:**
    ///
    /// * **A run that is an element** (a hyperlink, a field the model has no cell for, a span that holds an attribute besides its style name, any other element round characters) is kept as
    ///   it stands: each of the drawing's is looked for in the file's at the same characters, in order, with the held values of its style. A hyperlink the file lacks is counted as
    ///   a hyperlink (`hyperlinks`), any other element as the formatting of the text; an element the file holds that the drawing has not is the formatting of the text too.
    /// * **A plain `text:span`** is the formatting it holds, and what it holds that no cell says is its held style values (`HeldStyleValue`): each character of the drawing's has the held values of
    ///   the plain runs over it, and the file's character has them too, whichever way the runs that give them begin and end. A plain run that holds none says nothing of its characters here
    ///   (the cells of the `Character` rows are compared with the sheet's), so it is no difference for the file to give them one run or three.
    ///
    /// **The cost** is a step for each run of both lists, and for the plain runs that hold values a step for each stretch between two places where any begins or ends, for each run of both, charged
    /// before the stretches are walked.
    fn runs_differ(&mut self, wanted: &[diacraft_doc::OdfRun], found: &[diacraft_doc::OdfRun]) -> Result<RunsSaid, Stop> {
        let mut said = RunsSaid { why: None, links: 0 };
        self.step(wanted.len().saturating_add(found.len()))?;
        // The elements: the drawing's and the file's, each in the order the runs begin, walked together.
        let mut wanted_elements = wanted.iter().filter(|r| r.element.is_some()).peekable();
        let mut found_elements = found.iter().filter(|r| r.element.is_some()).peekable();
        let place = |r: &diacraft_doc::OdfRun| (r.from, std::cmp::Reverse(r.to));
        let is_link = |r: &diacraft_doc::OdfRun| r.element.as_ref().is_some_and(|e| e.name.rsplit(':').next() == Some("a"));
        let lost = |said: &mut RunsSaid, r: &diacraft_doc::OdfRun| {
            if is_link(r) {
                said.links = said.links.saturating_add(1);
            } else {
                said.why = Some("its runs");
            }
        };
        loop {
            match (wanted_elements.peek().copied(), found_elements.peek().copied()) {
                (None, None) => break,
                (Some(run), None) => {
                    lost(&mut said, run);
                    wanted_elements.next();
                }
                (None, Some(_)) => {
                    said.why = Some("its runs");
                    found_elements.next();
                }
                (Some(run), Some(other)) => match place(run).cmp(&place(other)) {
                    std::cmp::Ordering::Equal => {
                        if self.held_lost_text(&run.style.held, &other.style.held) {
                            said.why = Some("a held value of a run's style");
                        }
                        wanted_elements.next();
                        found_elements.next();
                    }
                    std::cmp::Ordering::Less => {
                        lost(&mut said, run);
                        wanted_elements.next();
                    }
                    std::cmp::Ordering::Greater => {
                        said.why = Some("its runs");
                        found_elements.next();
                    }
                },
            }
        }
        // The plain runs that hold values: what each character holds, told stretch by stretch, with the runs that cover the stretch kept as the walk goes (a sweep: each run is taken in once
        // and let go once, and a stretch costs the runs that cover it).
        let (wanted_held, found_held) = (plain_runs_that_hold_values(wanted), plain_runs_that_hold_values(found));
        if wanted_held.is_empty() {
            return Ok(said);
        }
        let mut places: Vec<u32> = wanted_held.iter().chain(found_held.iter()).flat_map(|r| [r.from, r.to]).collect();
        places.sort_unstable();
        places.dedup();
        let (mut next_wanted, mut next_found) = (wanted_held.iter().peekable(), found_held.iter().peekable());
        let (mut wanted_over, mut found_over): (Vec<&diacraft_doc::OdfRun>, Vec<&diacraft_doc::OdfRun>) = (Vec::new(), Vec::new());
        for pair in places.windows(2) {
            let &[from, to] = pair else { continue };
            for (next, over) in [(&mut next_wanted, &mut wanted_over), (&mut next_found, &mut found_over)] {
                while let Some(run) = next.next_if(|r| r.from <= from) {
                    over.push(run);
                }
                over.retain(|r| r.to >= to);
            }
            // **Charged before the lists are made and before they are compared**: the runs that cover the stretch, the values they hold, and each value of the drawing's against each of the file's
            // (the comparison of a value is a step, as `held_lost_text`'s is), so that a paragraph of many stretches that each hold many values stops the walk at the budget, in words.
            let held_in = |over: &[&diacraft_doc::OdfRun]| over.iter().fold(0usize, |sum, r| sum.saturating_add(r.style.held.len()));
            let (many_before, many_after) = (held_in(&wanted_over), held_in(&found_over));
            self.step(
                many_before
                    .saturating_mul(many_after.max(1))
                    .saturating_add(many_before)
                    .saturating_add(many_after)
                    .saturating_add(wanted_over.len())
                    .saturating_add(found_over.len())
                    .saturating_add(1),
            )?;
            let (before, after): (Vec<_>, Vec<_>) = (
                wanted_over.iter().flat_map(|r| r.style.held.iter()).collect::<Vec<_>>(),
                found_over.iter().flat_map(|r| r.style.held.iter()).collect::<Vec<_>>(),
            );
            if before.iter().any(|w| !after.iter().any(|f| f.group == w.group && f.attrs == w.attrs && f.kept == w.kept)) {
                said.why = Some("a held value of a run's style");
                break;
            }
        }
        Ok(said)
    }

    /// A picture or an object a shape shows, and the paragraphs a picture held.
    fn foreign(&mut self, before: &ForeignData, after: Option<&ForeignData>) {
        let object = before.attr("ForeignType") == Some("Object");
        let class = if object { Class::Objects } else { Class::Pictures };
        // **The paragraphs a picture's `draw:image` held** (the words LibreOffice draws on a picture, which the `.odg` reader keeps whole on the picture): each in the file's, or the
        // paragraphs are counted as text that differs.
        if let Some(wanted) = before.ext.get().map(|data| &data.odf.kept).filter(|kept| !kept.is_empty()) {
            let none = diacraft_doc::Passthrough::new();
            let have = after.and_then(|f| f.ext.get()).map_or(&none, |data| &data.odf.kept);
            if !self.nodes_kept(wanted, have) {
                let n = u64::try_from(wanted.len()).unwrap_or(u64::MAX);
                self.count(Class::Text, n, || format!("the {n} paragraphs the picture held are not in the file's"));
            }
        }
        let Some(found) = after else {
            self.count(class, 1, || "the shape shows none in the file".to_owned());
            return;
        };
        let same_picture = match (&before.picture, &found.picture) {
            (Some(wanted), Some(have)) => Arc::ptr_eq(wanted, have) || wanted.bytes == have.bytes,
            (Some(_), None) => false,
            (None, _) => true,
        };
        let same_kind = !object || found.attr("ForeignType") == Some("Object");
        if !same_picture || !same_kind {
            self.count(class, 1, || if same_kind { "the picture's bytes differ" } else { "it is no object in the file" }.to_owned());
        }
    }

    /// Two shapes that stand for one: everything the drawing's holds. → the classes that differ, as bits.
    fn shape(&mut self, before: &Shape, after: &Shape, at: &(ShapeRef, ShapeRef)) -> Result<u32, Stop> {
        self.glued_here = (self.glued.contains(&(before.id, true)), self.glued.contains(&(before.id, false)));
        self.unlabelled_here = before.text.is_none() && after.text.is_none() && before.sheet.cell("BeginX").is_some();
        let mut flags = 0;
        if before.kind != ShapeType::Unspecified && before.kind != after.kind {
            let (wanted, found) = (&before.kind, &after.kind);
            flags |= self.flag(Class::Shapes, || format!("a shape of the kind {wanted:?} is of the kind {found:?} in the file"));
        }
        if before.id != after.id {
            let found = after.id;
            self.count(Class::ShapeNumbers, 1, || format!("the shape is the file's shape {found}"));
        }
        if before.name_u.is_some() && before.name_u != after.name_u || before.name.is_some() && before.name != after.name {
            flags |= self.flag(Class::ShapeAttributes, || "the shape's name".to_owned());
        }
        if before.master.is_some() && before.master != after.master || before.master_shape.is_some() && before.master_shape != after.master_shape {
            flags |= self.flag(Class::Masters, || "the master the shape is an instance of".to_owned());
        }
        for (k, (wanted, found)) in
            [(&before.data1, &after.data1), (&before.data2, &after.data2), (&before.data3, &after.data3)].into_iter().enumerate()
        {
            if wanted.is_some() && wanted != found {
                flags |= self.flag(Class::UserCells, || format!("the shape's Data{}", k + 1));
            }
        }
        let sheets = (SheetRef::Shape(at.0.clone()), SheetRef::Shape(at.1.clone()));
        flags |= self.sheet(&sheets, &before.sheet, &after.sheet)?;
        if let Some(wanted) = before.text.as_ref() {
            flags |= self.text(wanted, after.text.as_deref())?;
        }
        if let Some(wanted) = before.foreign.as_ref() {
            self.foreign(wanted, after.foreign.as_deref());
        }
        // What the shape kept beside its cells: attributes, elements, and each format's data.
        if !self.attrs_kept(&before.raw, &after.raw) || !self.nodes_kept(&before.passthrough, &after.passthrough) {
            self.count(Class::KeptData, 1, || "what the shape kept beside its cells (attributes and elements)".to_owned());
        }
        if let Some(wanted) = before.ext.get() {
            let found = after.ext.get();
            let have: &[diacraft_doc::Held] = found.map_or(&[], |f| f.odf.held.as_slice());
            let lost = self.held_lost(&wanted.odf.held, have);
            flags |= self.flags(lost, "a held value of the shape did not come back");
            let empty = diacraft_doc::Kept::default();
            let kept = found.map_or(&empty, |f| &f.odf.kept);
            // The element of an embedded object (`draw:object`, `draw:object-ole`) is the object's: a VSDX file holds it as an embedding, which the shape reads back as the object, and an object that is
            // in neither form in the file is an object that did not come back (an `.odg`'s frame shows the replacement picture, and holds the object's element).
            let object_in_file = after.foreign.as_ref().is_some_and(|f| f.attr("ForeignType") == Some("Object"));
            let wanted_kept = without_object_elements(&wanted.odf.kept);
            let held_objects = object_elements(&wanted.odf.kept);
            if held_objects > 0 && !object_in_file && object_elements(kept) < held_objects {
                self.count(Class::Objects, 1, || "the element of the shape's object is not in the file".to_owned());
            }
            if !self.kept_kept(&wanted_kept, kept) {
                let class = kept_class(&wanted_kept, kept);
                let trace = self.trace_on;
                flags |=
                    self.flag(class, || format!("the shape's kept markup: {}", if trace { missing_kept(&wanted_kept, kept) } else { String::new() }));
            }
            if !self.rest_kept(&wanted.vsdx, &found.map(|f| f.vsdx.clone()).unwrap_or_default()) {
                self.count(Class::KeptData, 1, || "the VSDX elements the shape had no place for".to_owned());
            }
        }
        Ok(flags)
    }

    /// The shapes of two lists (a page's, a master's, a group's) paired: by `ID`, and what the file gave another number by its place among those left. The shapes of the drawing the file has
    /// none for, and the shapes the file has that the drawing has none for, are left over.
    fn pair<'a>(&mut self, before: &'a ShapeList, after: &'a ShapeList) -> Result<Paired<'a>, Stop> {
        self.step(before.len().saturating_add(after.len()))?;
        let mut paired = Paired { pairs: Vec::new(), lost: Vec::new(), added: Vec::new() };
        if before.len() == after.len() && before.iter().zip(after.iter()).all(|(b, a)| b.id == a.id) {
            for (at, (b, a)) in before.iter().zip(after.iter()).enumerate() {
                paired.pairs.push(Pair { before: b, after: a, at: (at, at) });
            }
            return Ok(paired);
        }
        // By `ID`: a file may hold an `ID` twice, and the first not yet taken is the one.
        let mut ids: Vec<(u32, usize)> = after.iter().enumerate().map(|(at, shape)| (shape.id, at)).collect();
        ids.sort_unstable();
        // For each `ID`, the next place in `ids` not yet given to a shape of the drawing, and where its places end: the shapes of one `ID` are paired in order, each in one step, however many share it.
        let mut cursors: HashMap<u32, (usize, usize)> = HashMap::new();
        for (place, (id, _)) in ids.iter().enumerate() {
            cursors.entry(*id).and_modify(|(_, end)| *end = place.saturating_add(1)).or_insert((place, place.saturating_add(1)));
        }
        let mut taken = vec![false; after.len()];
        let mut lost: Vec<(usize, &'a Arc<Shape>)> = Vec::new();
        for (at, shape) in before.iter().enumerate() {
            let found = cursors.get_mut(&shape.id).and_then(|(next, end)| {
                let place = (*next < *end).then(|| ids.get(*next).map(|(_, place)| *place)).flatten();
                *next = next.saturating_add(1);
                place
            });
            match found.and_then(|place| after.get(place).map(|other| (place, other))) {
                Some((place, other)) => {
                    if let Some(slot) = taken.get_mut(place) {
                        *slot = true;
                    }
                    paired.pairs.push(Pair { before: shape, after: other, at: (at, place) });
                }
                None => lost.push((at, shape)),
            }
        }
        let mut added: Vec<(usize, &'a Arc<Shape>)> = after.iter().enumerate().filter(|(at, _)| taken.get(*at) == Some(&false)).collect();
        // What the file gave another number: the first of those left on each side, in order.
        let renumbered = lost.len().min(added.len());
        for ((at, shape), (place, other)) in lost.drain(..renumbered).zip(added.drain(..renumbered)) {
            paired.pairs.push(Pair { before: shape, after: other, at: (at, place) });
        }
        paired.lost = lost.into_iter().map(|(_, shape)| shape).collect();
        paired.added = added.into_iter().map(|(_, shape)| shape).collect();
        Ok(paired)
    }

    /// A shape and every shape in it counted as lost (the file has none like it) or as added (the drawing has none like it), with the formulas of the lost.
    fn whole(&mut self, root: &Shape, lost: bool) -> Result<(), Stop> {
        let mut stack: Vec<(&ShapeList, usize)> = Vec::new();
        let visit = |this: &mut Compare<'d>, shape: &Shape| -> Result<(), Stop> {
            this.step(1)?;
            if !shape.del {
                let (id, name) = (shape.id, shape.name_u.clone());
                this.count(Class::Shapes, 1, || {
                    format!(
                        "the shape {id} {:?} is {}",
                        name.as_deref().map(cut),
                        if lost { "not in the file" } else { "in the file and not in the drawing" }
                    )
                });
                if lost {
                    let (formulas, cells) = shape_formulas(shape);
                    this.step(cells)?;
                    this.count(Class::Formulas, formulas, || format!("the formulas of the shape {id}, which is not in the file"));
                }
            }
            Ok(())
        };
        visit(self, root)?;
        stack.push((&root.children, 0));
        while let Some(top) = stack.last_mut() {
            let current: &ShapeList = top.0;
            let Some(shape) = current.get(top.1) else {
                stack.pop();
                continue;
            };
            top.1 = top.1.saturating_add(1);
            visit(self, shape)?;
            if !shape.children.is_empty() {
                if stack.len() >= MAX_SHAPE_DEPTH {
                    return Err(Stop);
                }
                stack.push((&shape.children, 0));
            }
        }
        Ok(())
    }

    /// Every shape of a list counted as lost or as added.
    fn whole_list(&mut self, list: &ShapeList, lost: bool) -> Result<(), Stop> {
        for shape in list.iter() {
            self.whole(shape, lost)?;
        }
        Ok(())
    }

    /// The shapes of a page or a master part of the drawing against the file's, down every group.
    fn shapes(&mut self, before: &'d ShapeList, after: &'d ShapeList, owners: (Owner, Owner)) -> Result<(), Stop> {
        let mut jobs = vec![Job { before, after, owners, paths: (Vec::new(), Vec::new()) }];
        while let Some(job) = jobs.pop() {
            let paired = self.pair(job.before, job.after)?;
            for shape in &paired.lost {
                self.whole(shape, true)?;
            }
            for shape in &paired.added {
                self.whole(shape, false)?;
            }
            for pair in paired.pairs {
                self.step(1)?;
                if pair.before.del {
                    continue;
                }
                if job.paths.0.len() >= MAX_SHAPE_DEPTH {
                    return Err(Stop);
                }
                self.step(job.paths.0.len())?;
                let (mut path_b, mut path_a) = job.paths.clone();
                path_b.push(u32::try_from(pair.at.0).unwrap_or(u32::MAX));
                path_a.push(u32::try_from(pair.at.1).unwrap_or(u32::MAX));
                let at = (ShapeRef { owner: job.owners.0, path: path_b.clone() }, ShapeRef { owner: job.owners.1, path: path_a.clone() });
                self.paired_ids.insert(pair.before.id, pair.after.id);
                let owner = job.owners.0;
                let (id, name) = (pair.before.id, pair.before.name_u.as_deref());
                self.here(|| format!("{owner:?}, shape {id} {:?}: ", name.map(cut)));
                let flags = self.shape(pair.before, pair.after, &at)?;
                self.tally.flagged(flags);
                if !pair.before.children.is_empty() {
                    jobs.push(Job { before: &pair.before.children, after: &pair.after.children, owners: job.owners, paths: (path_b, path_a) });
                }
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------------------------------------------------------------------- the document

    /// A page of the drawing against the page of the file at its place.
    fn page(&mut self, before: &'d Arc<Page>, after: &'d Arc<Page>, ids: &(HashMap<u32, usize>, HashMap<u32, usize>)) -> Result<(), Stop> {
        let page = before.id;
        self.here(|| format!("page {page}: "));
        let mut flags = 0;
        if before.name != after.name || before.background != after.background {
            flags |= self.flag(Class::Pages, || "the page's name or whether it is a background page".to_owned());
        }
        // The background page is the one at that place in each.
        let place = |ids: &HashMap<u32, usize>, id: Option<u32>| id.and_then(|id| ids.get(&id).copied());
        if before.back_page.is_some() && place(&ids.0, before.back_page) != place(&ids.1, after.back_page) {
            flags |= self.flag(Class::Pages, || "the page's background page".to_owned());
        }
        let sheets = (SheetRef::Page(before.id), SheetRef::Page(after.id));
        flags |= self.sheet(&sheets, &before.sheet, &after.sheet)?;
        if let Some(held) = before.ext.get() {
            let found = after.ext.get();
            let master_page = found.and_then(|f| f.odf.master_page.clone());
            if held.odf.master_page.is_some() && held.odf.master_page != master_page {
                flags |= self.flag(Class::Masters, || "the page's master page".to_owned());
            }
            let layout = found.and_then(|f| f.odf.layout.clone());
            if held.odf.layout.is_some() && held.odf.layout != layout {
                flags |= self.flag(Class::Masters, || "the page's layout".to_owned());
            }
            let empty = diacraft_doc::Kept::default();
            if !self.kept_kept(&held.odf.kept, found.map_or(&empty, |f| &f.odf.kept)) {
                self.count(Class::KeptData, 1, || "the page's kept markup".to_owned());
            }
        }
        if !self.attrs_kept(&before.raw, &after.raw) || !self.nodes_kept(&before.passthrough, &after.passthrough) {
            self.count(Class::KeptData, 1, || "what the page kept beside its cells".to_owned());
        }
        self.tally.flagged(flags);
        // The shapes, which pair the drawing's with the file's.
        self.paired_ids.clear();
        self.glued_ends(&before.connects)?;
        self.shapes(&before.shapes, &after.shapes, (Owner::Page(before.id), Owner::Page(after.id)))?;
        self.here(|| format!("page {page}: "));
        self.connections(before, after)
    }

    /// The glued ends of the shapes of a page or a master, from its connections: a step for each.
    fn glued_ends(&mut self, connects: &diacraft_doc::Connects) -> Result<(), Stop> {
        self.step(connects.len())?;
        self.glued.clear();
        for c in connects.iter() {
            match c.from_cell.as_deref() {
                Some(cell) if cell.starts_with("Begin") => {
                    self.glued.insert((c.from_sheet, true));
                }
                Some(cell) if cell.starts_with("End") => {
                    self.glued.insert((c.from_sheet, false));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The connections of a page: each of the drawing's is in the file's, **between the shapes paired with its own** (a file that numbers the shapes anew holds the same
    /// connections under other numbers, which [`Class::ShapeNumbers`] tells once for each shape; a connection is lost when it joins other shapes or other cells, or is not there).
    fn connections(&mut self, before: &Page, after: &Page) -> Result<(), Stop> {
        self.step(before.connects.len().saturating_add(after.connects.len()))?;
        let key = |c: &diacraft_doc::Connect| (c.from_sheet, c.to_sheet, c.from_cell.clone(), c.to_cell.clone());
        let mut have: HashMap<ConnectKey, usize> = HashMap::new();
        for connect in after.connects.iter() {
            *have.entry(key(connect)).or_insert(0) += 1;
        }
        for wanted in before.connects.iter() {
            let paired = |id: u32| self.paired_ids.get(&id).copied().unwrap_or(id);
            let mapped = (paired(wanted.from_sheet), paired(wanted.to_sheet), wanted.from_cell.clone(), wanted.to_cell.clone());
            match have.get_mut(&mapped) {
                Some(left) if *left > 0 => *left -= 1,
                _ => {
                    let (from, to, cell) = (wanted.from_sheet, wanted.to_sheet, wanted.from_cell.clone());
                    self.count(Class::Connections, 1, || {
                        format!("the end {:?} of the shape {from} glued to the shape {to}", cell.as_deref().map(cut))
                    });
                }
            }
        }
        Ok(())
    }

    /// The pages: each of the drawing's against the file's at its place, and the pages that are missing or added.
    fn pages(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        self.step(before.pages.len().saturating_add(after.pages.len()))?;
        let positions = |doc: &Document| -> HashMap<u32, usize> {
            let mut map = HashMap::new();
            for (at, page) in doc.pages.iter().enumerate() {
                map.entry(page.id).or_insert(at);
            }
            map
        };
        let ids = (positions(before), positions(after));
        for (at, wanted) in before.pages.iter().enumerate() {
            match after.pages.get(at) {
                Some(found) => self.page(wanted, found, &ids)?,
                None => {
                    self.count(Class::Pages, 1, || format!("the page {at} is not in the file"));
                    self.whole_list(&wanted.shapes, true)?;
                }
            }
        }
        let added = after.pages.len().saturating_sub(before.pages.len());
        self.count(Class::Pages, u64::try_from(added).unwrap_or(u64::MAX), || format!("the file has {added} pages more"));
        Ok(())
    }

    /// The masters, by `ID`, and by name where the file renumbered them.
    fn masters(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        self.step(before.masters.len().saturating_add(after.masters.len()))?;
        let mut by_id: HashMap<u32, &Arc<Master>> = HashMap::new();
        let mut by_name: HashMap<&str, &Arc<Master>> = HashMap::new();
        for master in after.masters.iter() {
            by_id.entry(master.id).or_insert(master);
            if let Some(name) = master.name_u.as_deref() {
                by_name.entry(name).or_insert(master);
            }
        }
        for wanted in before.masters.iter() {
            let found: Option<&Arc<Master>> = by_id
                .get(&wanted.id)
                .filter(|m| wanted.name_u.is_none() || m.name_u == wanted.name_u)
                .or_else(|| wanted.name_u.as_deref().and_then(|name| by_name.get(name)))
                .copied();
            let master = wanted.id;
            self.here(|| format!("master {master}: "));
            let Some(found) = found else {
                self.count(Class::Masters, 1, || "the master is not in the file".to_owned());
                self.whole_list(&wanted.shapes, true)?;
                continue;
            };
            let mut flags = 0;
            if wanted.name != found.name {
                flags |= self.flag(Class::Masters, || "the master's name".to_owned());
            }
            let sheets = (SheetRef::Master(wanted.id), SheetRef::Master(found.id));
            flags |= self.sheet(&sheets, &wanted.sheet, &found.sheet)?;
            if !self.attrs_kept(&wanted.raw, &found.raw)
                || !self.nodes_kept(&wanted.passthrough, &found.passthrough)
                || !self.kept_kept_ext(&wanted.ext, &found.ext)
            {
                self.count(Class::KeptData, 1, || "what the master kept beside its cells".to_owned());
            }
            self.tally.flagged(flags);
            self.glued_ends(&wanted.connects)?;
            self.shapes(&wanted.shapes, &found.shapes, (Owner::Master(wanted.id), Owner::Master(found.id)))?;
        }
        Ok(())
    }

    /// The style sheets, by `ID`, and by name where the file renumbered them.
    fn styles(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        self.step(before.styles.len().saturating_add(after.styles.len()))?;
        let mut by_id: HashMap<u32, &Arc<Style>> = HashMap::new();
        let mut by_name: HashMap<&str, &Arc<Style>> = HashMap::new();
        for style in after.styles.iter() {
            by_id.entry(style.id).or_insert(style);
            if let Some(name) = style.name_u.as_deref() {
                by_name.entry(name).or_insert(style);
            }
        }
        for wanted in before.styles.iter() {
            let found: Option<&Arc<Style>> = by_id
                .get(&wanted.id)
                .filter(|s| wanted.name_u.is_none() || s.name_u == wanted.name_u)
                .or_else(|| wanted.name_u.as_deref().and_then(|name| by_name.get(name)))
                .copied();
            let (style, name) = (wanted.id, wanted.name_u.clone());
            self.here(|| format!("style sheet {style} {:?}: ", name.as_deref().map(cut)));
            let Some(found) = found else {
                self.count(Class::StyleSheets, 1, || "the style sheet is not in the file".to_owned());
                continue;
            };
            let mut flags = 0;
            if wanted.name != found.name || wanted.name_u != found.name_u {
                flags |= self.flag(Class::StyleSheets, || "the style sheet's name".to_owned());
            }
            let sheets = (SheetRef::Style(wanted.id), SheetRef::Style(found.id));
            flags |= self.sheet(&sheets, &wanted.sheet, &found.sheet)?;
            if let Some(held) = wanted.ext.get() {
                let other = found.ext.get();
                let have: &[diacraft_doc::Held] = other.map_or(&[], |f| f.odf.held.as_slice());
                let lost = self.held_lost(&held.odf.held, have);
                flags |= self.flags(lost, "a held value of the style did not come back");
                if held.odf.parent.is_some() && other.and_then(|f| f.odf.parent.as_ref()) != held.odf.parent.as_ref() {
                    flags |= self.flag(Class::StyleSheets, || "the style's parent".to_owned());
                }
                if held.odf.name.is_some() && other.and_then(|f| f.odf.name.as_ref()) != held.odf.name.as_ref() {
                    flags |= self.flag(Class::StyleSheets, || "the style's name in the file".to_owned());
                }
                let unmapped = other.map(|f| f.odf.unmapped.clone()).unwrap_or_default();
                let unmapped_lost = !held.odf.unmapped.is_empty() && !self.attrs_kept(&held.odf.unmapped, &unmapped);
                if unmapped_lost {
                    let trace = self.trace_on;
                    let lost: Vec<String> = if trace {
                        held.odf.unmapped.iter().filter(|(n, v)| unmapped.get(n) != Some(*v)).map(|(n, _)| cut(n)).take(8).collect()
                    } else {
                        Vec::new()
                    };
                    flags |= self.flag(Class::StyleContent, || format!("the style's properties no cell reads: {lost:?}"));
                }
            }
            if !self.attrs_kept(&wanted.raw, &found.raw) || !self.nodes_kept(&wanted.passthrough, &found.passthrough) {
                self.count(Class::KeptData, 1, || "what the style sheet kept beside its cells".to_owned());
            }
            self.tally.flagged(flags);
        }
        Ok(())
    }

    /// The named definitions of the document (markers, gradients, hatches, dashes, opacities, fill images) and the other style families.
    fn definitions(&mut self) -> Result<(), Stop> {
        let (Some(before), after) = (self.before.ext.get(), self.after.ext.get()) else { return Ok(()) };
        self.here(String::new);
        let empty: &[Definition] = &[];
        let have = after.map_or(empty, |a| a.odf.definitions.as_slice());
        self.step(before.odf.definitions.len().saturating_add(have.len()))?;
        let mut by_name: HashMap<(DefFamily, &str), Vec<&Definition>> = HashMap::new();
        for definition in have {
            by_name.entry((definition.family, &*definition.name)).or_default().push(definition);
        }
        // How many of a name the drawing's list has been through: the file's definition of that place is the one a faithful save wrote, and is compared first.
        let mut seen: HashMap<(DefFamily, &str), usize> = HashMap::new();
        for wanted in &before.odf.definitions {
            let same = |found: &&Definition| found.element == wanted.element && found.bytes == wanted.bytes;
            let key = (wanted.family, &*wanted.name);
            let nth = {
                let count = seen.entry(key).or_insert(0);
                let nth = *count;
                *count = count.saturating_add(1);
                nth
            };
            if let Some(list) = by_name.get(&key) {
                if list.get(nth).is_some_and(same) {
                    continue;
                }
                // Not in its place: a scan of the definitions of that name, charged before it is made.
                self.step(list.len().saturating_mul(ELEMENT_STEPS))?;
                if list.iter().any(same) {
                    continue;
                }
            }
            // A definition the file named another way is the same definition: a scan of the file's, charged before it is made.
            self.step(have.len().saturating_mul(ELEMENT_STEPS))?;
            let renamed = have.iter().any(|found| {
                found.family == wanted.family
                    && found.bytes == wanted.bytes
                    && attrs_without_name(&found.element.attrs).eq(attrs_without_name(&wanted.element.attrs))
                    && found.element.children == wanted.element.children
            });
            if renamed {
                continue;
            }
            let class = match wanted.family {
                DefFamily::Marker => Class::LineEnds,
                DefFamily::Gradient => Class::Gradients,
                DefFamily::FillImage => Class::Pictures,
                DefFamily::Opacity | DefFamily::Hatch | DefFamily::StrokeDash => Class::StyleContent,
            };
            let (family, name) = (wanted.family, &wanted.name);
            self.count(class, 1, || format!("the definition {family:?} {:?} is not in the file as it was", cut(name)));
        }
        // The other style families (paragraph, text, drawing page, list styles): the writer names an automatic one anew, and a common one keeps its name.
        let kept_have = after.map_or(&[][..], |a| a.odf.styles.as_slice());
        self.step(before.odf.styles.len().saturating_mul(kept_have.len()).saturating_mul(ELEMENT_STEPS))?;
        for wanted in before.odf.styles.iter().filter(|s| !s.family.starts_with("auto:")) {
            if !kept_have.iter().any(|found| found.family == wanted.family && found.name == wanted.name && found.element == wanted.element) {
                let (family, name) = (&wanted.family, &wanted.name);
                self.count(Class::StyleContent, 1, || {
                    format!("the style {:?} of the family {:?} is not in the file as it was", cut(name), cut(family))
                });
            }
        }
        // The layer set: each layer, with its attributes.
        let layers = after.map_or(&[][..], |a| a.odf.layers.as_slice());
        self.step(before.odf.layers.len().saturating_mul(layers.len()).saturating_mul(ELEMENT_STEPS))?;
        for wanted in &before.odf.layers {
            if !layers.iter().any(|found| found == wanted) {
                self.count(Class::Layers, 1, || "a layer of the layer set is not in the file as it was".to_owned());
            }
        }
        let empty = diacraft_doc::Kept::default();
        if !self.kept_kept(&before.odf.kept, after.map_or(&empty, |a| &a.odf.kept)) {
            self.count(Class::KeptData, 1, || "the document's kept markup".to_owned());
        } else if !self.rest_kept(&before.vsdx, &after.map(|a| a.vsdx.clone()).unwrap_or_default()) {
            self.count(Class::KeptData, 1, || "the VSDX elements the document had no place for".to_owned());
        }
        Ok(())
    }

    /// The parts the package holds beside the model: the macro project, the objects, the fonts, the others. A part is found in the file by its bytes, and by what it is for where that matters
    /// (a macro project that is no macro project in the file, an object that is no object entry, is not found).
    fn parts(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        self.here(String::new);
        self.step(before.parts.len().saturating_add(after.parts.len()))?;
        let class_of_part = |part: &diacraft_doc::Part| {
            if crate::across::hold::is_macro_part(part) {
                Class::Macros
            } else if crate::across::hold::is_object_part(part) {
                Class::Objects
            } else if part.role == PartRole::Font {
                Class::Fonts
            } else {
                Class::KeptData
            }
        };
        // The bytes are hashed once each, and charged as such: a step for each 4,096 bytes.
        let hashed: usize = before.parts.iter().chain(after.parts.iter()).map(|p| p.bytes.len()).fold(0, usize::saturating_add);
        self.step(hashed >> 12)?;
        // For each set of bytes, how many parts of the file are of each kind that has not been given to a part of the drawing: no scan of the parts that share their bytes.
        let slot = |class: Class| match class {
            Class::Macros => 0,
            Class::Objects => 1,
            Class::Fonts => 2,
            _ => 3,
        };
        let mut have: HashMap<&[u8], [usize; 4]> = HashMap::new();
        for part in after.parts.iter() {
            if let Some(count) = have.entry(part.bytes.as_ref()).or_default().get_mut(slot(class_of_part(part))) {
                *count = count.saturating_add(1);
            }
        }
        for part in before.parts.iter() {
            // The thumbnail is made again by every save, and a signature does not match the file a model writer makes (the note says so): neither is content.
            if part.role == PartRole::Thumbnail || part.role == PartRole::Signature || part.format == Format::Native {
                continue;
            }
            let class = class_of_part(part);
            if class == Class::Macros && self.macros_left_out {
                continue;
            }
            // A part of no particular kind is found among the file's parts of any kind (its own first); one that is a macro project, an object or a font only among those of its kind.
            let wanted_kinds: &[usize] = if class == Class::KeptData { &[3, 0, 1, 2] } else { &[slot(class)] };
            let counts = have.get_mut(part.bytes.as_ref());
            let taken = counts.is_some_and(|counts| {
                wanted_kinds.iter().any(|kind| match counts.get_mut(*kind) {
                    Some(count) if *count > 0 => {
                        *count -= 1;
                        true
                    }
                    _ => false,
                })
            });
            if !taken {
                let name = &part.name;
                self.count(class, 1, || format!("the part {:?} is not in the file as it was", cut(name)));
            }
        }
        Ok(())
    }

    /// The font faces, the themes and the properties.
    fn document_data(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        self.step(before.fonts.len().saturating_add(after.fonts.len()).saturating_add(before.themes.len()))?;
        let faces: std::collections::HashSet<String> = after.fonts.iter().map(|f| f.name_u.to_ascii_lowercase()).collect();
        for face in before.fonts.iter() {
            if !faces.contains(&face.name_u.to_ascii_lowercase()) {
                let name = &face.name_u;
                self.count(Class::Fonts, 1, || format!("the face {:?}", cut(name)));
            }
        }
        // The themes by their bytes, each hashed once and charged as such.
        let theme_bytes: usize = before.themes.iter().chain(after.themes.iter()).map(|t| t.bytes.len()).fold(0, usize::saturating_add);
        self.step(theme_bytes >> 12)?;
        let have_themes: std::collections::HashSet<&[u8]> = after.themes.iter().map(|t| t.bytes.as_ref()).collect();
        for theme in before.themes.iter() {
            if !have_themes.contains(theme.bytes.as_ref()) {
                self.count(Class::StyleContent, 1, || "a theme is not in the file as it was".to_owned());
            }
        }
        // The properties: what the drawing says of itself. The time of the last save, who made it and the version of the program are the writer's own.
        let (wanted, found) = (&*before.props, &*after.props);
        let texts = [
            ("subject", &wanted.subject, &found.subject),
            ("creator", &wanted.creator, &found.creator),
            ("keywords", &wanted.keywords, &found.keywords),
            ("description", &wanted.description, &found.description),
            ("category", &wanted.category, &found.category),
            ("company", &wanted.company, &found.company),
            ("manager", &wanted.manager, &found.manager),
            ("hyperlink base", &wanted.hyperlink_base, &found.hyperlink_base),
            ("created", &wanted.created, &found.created),
            ("language", &wanted.language, &found.language),
        ];
        self.here(String::new);
        for (field, b, a) in texts {
            if b.is_some() && b != a {
                self.count(Class::Properties, 1, || format!("the {field} {:?} is {:?} in the file", b.as_deref().map(cut), a.as_deref().map(cut)));
            }
        }
        if !before.title.is_empty() && before.title != after.title {
            self.count(Class::Properties, 1, || format!("the title {:?} is {:?} in the file", cut(&before.title), cut(&after.title)));
        }
        // The custom properties by name and value, in a set: no scan of the file's for each of the drawing's.
        self.step(wanted.custom.len().saturating_add(found.custom.len()))?;
        let have_custom: std::collections::HashSet<(&str, &str)> = found.custom.iter().map(|p| (&*p.name, &*p.value)).collect();
        for prop in &wanted.custom {
            if !have_custom.contains(&(&*prop.name, &*prop.value)) {
                let name = &prop.name;
                self.count(Class::Properties, 1, || format!("the custom property {:?}", cut(name)));
            }
        }
        // The document's own sheet and the elements of the document that no field reads.
        let sheets = (SheetRef::Document, SheetRef::Document);
        self.here(|| "the document's sheet: ".to_owned());
        let flags = self.sheet(&sheets, &before.sheet, &after.sheet)?;
        self.tally.flagged(flags);
        if !self.nodes_kept(&before.passthrough, &after.passthrough) {
            self.count(Class::KeptData, 1, || "the elements of the document no field reads".to_owned());
        }
        Ok(())
    }

    /// The whole comparison.
    pub(crate) fn run(&mut self) -> Result<(), Stop> {
        self.pages()?;
        self.masters()?;
        self.styles()?;
        self.definitions()?;
        self.parts()?;
        self.document_data()?;
        self.comments()
    }

    /// The comments of the drawing, each looked for in the file's (`comments.rs`).
    fn comments(&mut self) -> Result<(), Stop> {
        let (before, after) = (self.before, self.after);
        let differs = super::comments::differing(before, after, &mut |n| self.step(n))?;
        self.here(String::new);
        self.count(Class::Comments, differs, || format!("{differs} comments of the drawing are not in the file as they were"));
        Ok(())
    }
}

/// What a comparison of a node of kept markup walks at the most: a step for each node and each attribute, and one for each 64 bytes it holds (an element's name and its attributes' names and values,
/// a text, a comment, an instruction's target and data), counted down its tree with a stack of its own (the tree is the reader's, within its depth and its items).
pub(crate) fn node_cost(node: &diacraft_doc::RawNode) -> usize {
    let mut cost = 0usize;
    let mut stack: Vec<&diacraft_doc::RawNode> = vec![node];
    while let Some(next) = stack.pop() {
        cost = cost.saturating_add(1);
        match next {
            diacraft_doc::RawNode::Element(e) => {
                let bytes = e.attrs.iter().map(|(n, v)| n.len().saturating_add(v.len())).fold(e.name.len(), usize::saturating_add);
                cost = cost.saturating_add(e.attrs.len()).saturating_add(bytes >> 6);
                stack.extend(e.children.iter());
            }
            // Every node that holds bytes is charged by them: a text, a comment, an instruction's target and data (the security scan's second finding).
            diacraft_doc::RawNode::Text(t) | diacraft_doc::RawNode::Comment(t) => cost = cost.saturating_add(t.len() >> 6),
            diacraft_doc::RawNode::Pi { target, data } => cost = cost.saturating_add(target.len().saturating_add(data.len()) >> 6),
        }
    }
    cost
}

/// The nodes a text holds whole where they stand, each with what a comparison of it walks ([`node_cost`]).
fn sized_in_text(text: &Text) -> Vec<(&diacraft_doc::RawNode, usize)> {
    held_in_text(text).map(|node| (node, node_cost(node))).collect()
}

/// The nodes a text holds whole where they stand (`TextPart::OdfRaw`), in order.
fn held_in_text(text: &Text) -> impl Iterator<Item = &diacraft_doc::RawNode> {
    text.parts.iter().filter_map(|p| if let TextPart::OdfRaw(node) = p { Some(&**node) } else { None })
}

/// Whether a node a text holds shows a picture: it is, or holds, a `draw:image` (by its local name), looked for down its tree with a stack of its own.
fn shows_picture(node: &diacraft_doc::RawNode) -> bool {
    let mut stack: Vec<&diacraft_doc::RawNode> = vec![node];
    while let Some(next) = stack.pop() {
        if let diacraft_doc::RawNode::Element(e) = next {
            if e.name.rsplit(':').next() == Some("image") {
                return true;
            }
            stack.extend(e.children.iter());
        }
    }
    false
}

/// How many elements of an embedded object a shape's kept markup holds.
fn object_elements(kept: &diacraft_doc::Kept) -> usize {
    kept.kept
        .iter()
        .filter(|placed| matches!(&placed.node, diacraft_doc::RawNode::Element(e) if &*e.name == "draw:object" || &*e.name == "draw:object-ole"))
        .count()
}

/// The kept markup of a shape without the elements of an embedded object.
fn without_object_elements(kept: &diacraft_doc::Kept) -> diacraft_doc::Kept {
    let nodes: Vec<diacraft_doc::Placed> = kept
        .kept
        .iter()
        .filter(|placed| !matches!(&placed.node, diacraft_doc::RawNode::Element(e) if &*e.name == "draw:object" || &*e.name == "draw:object-ole"))
        .cloned()
        .collect();
    diacraft_doc::Kept { raw: kept.raw.clone(), kept: diacraft_doc::Passthrough::from_nodes(nodes) }
}

/// The attributes of a definition but its name, for the match of a definition the file named another way.
fn attrs_without_name(attrs: &diacraft_doc::RawAttrs) -> impl Iterator<Item = (&str, &str)> {
    attrs.iter().filter(|(name, _)| *name != "draw:name" && *name != "style:name")
}

/// The class a shape's kept markup that did not come back is told in: the outline and the glue points are geometry and connections, the rest is kept data.
fn kept_class(before: &diacraft_doc::Kept, after: &diacraft_doc::Kept) -> Class {
    let names = |kept: &diacraft_doc::Kept| -> Vec<String> {
        kept.kept
            .iter()
            .filter_map(|placed| match &placed.node {
                diacraft_doc::RawNode::Element(e) => Some(e.name.to_string()),
                _ => None,
            })
            .collect()
    };
    let (wanted, found) = (names(before), names(after));
    let missing = |what: &str| wanted.iter().filter(|n| n.contains(what)).count() > found.iter().filter(|n| n.contains(what)).count();
    if missing("enhanced-geometry") {
        Class::Geometry
    } else if missing("glue-point") {
        Class::Connections
    } else {
        Class::KeptData
    }
}

/// What of a shape's kept markup is not in the file's, for the trace: the attributes whose value is not there, and the names of the elements of which the file holds fewer, at most eight of each.
fn missing_kept(before: &diacraft_doc::Kept, after: &diacraft_doc::Kept) -> String {
    let attrs: Vec<String> = before.raw.iter().filter(|(n, v)| after.raw.get(n) != Some(*v)).map(|(n, _)| cut(n)).take(8).collect();
    let names = |kept: &diacraft_doc::Kept| -> HashMap<String, usize> {
        let mut out: HashMap<String, usize> = HashMap::new();
        for placed in kept.kept.iter() {
            let name = match &placed.node {
                diacraft_doc::RawNode::Element(e) => e.name.to_string(),
                diacraft_doc::RawNode::Text(_) => "text".to_owned(),
                _ => "other node".to_owned(),
            };
            *out.entry(name).or_insert(0) += 1;
        }
        out
    };
    let (wanted, found) = (names(before), names(after));
    let mut elements: Vec<String> = wanted.iter().filter(|(n, k)| found.get(*n).copied().unwrap_or(0) < **k).map(|(n, _)| cut(n)).collect();
    elements.sort();
    elements.truncate(8);
    format!("attributes {attrs:?}, elements {elements:?} (or an element of the same name that differs)")
}

/// The formulas of a shape and of every shape in it.
fn shape_formulas(shape: &Shape) -> (u64, usize) {
    let sheet = &shape.sheet;
    let mut formulas = sheet.cells.values().filter(|c| own_formula(c).is_some()).count();
    let mut cells = sheet.cells.len();
    for section in &sheet.sections {
        formulas = formulas.saturating_add(section.cells.values().filter(|c| own_formula(c).is_some()).count());
        cells = cells.saturating_add(section.cells.len());
        for row in section.rows.iter() {
            formulas = formulas.saturating_add(row.cells.values().filter(|c| own_formula(c).is_some()).count());
            cells = cells.saturating_add(row.cells.len()).saturating_add(1);
        }
    }
    (u64::try_from(formulas).unwrap_or(u64::MAX), cells)
}
