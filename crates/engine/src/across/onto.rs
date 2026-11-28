//! The model a model writer is given for a drawing of the other format (harvested from the shelved the `across/onto.rs`, which re-based the drawing on a blank package and emptied its
//! `raw` and `passthrough`; the model writers now take the whole model and route what the target cannot say, so this only does what the target needs of the drawing that the model cannot hold).
//!
//! * **`drop_objects`**: the objects of the other format are left out, and what is left of their shapes is the picture they show ([`super::hold::without_objects`]).
//! * **The glue formulas of an end glued to the whole of a shape**. The VSDX application rebuilds glue from the formulas of the end cells when it loads a file and drops a `Connect`
//!   row whose end cells hold none, so a save to VSDX **gives the model** the `_WALKGLUE` formulas and the trigger of each such end that holds plain places ([`crate::draw::walkglue::dress_shape`]) **on a
//!   connector of DiaCraft's alone** (the route marker: "only DiaCraft's ends"). An `.odg` reader gives a whole-shape end plain places and the marker, and so
//!   does a drawing that an earlier build made. **A file's own connector, with no marker, keeps the ends the other program wrote** on this route as on every other: a person who glues an end again
//!   has the formulas written with the glue, which is their edit. The drawing the person has is not changed here either.
//! * **The cells that make a connector of DiaCraft's one the VSDX application routes** ([`crate::draw::routable`]): `ObjType` 2 and `GlueType` 2. The application routes
//!   again only a one-dimensional shape whose `ObjType` holds the routable flag, so a save to VSDX **gives them to each connector of
//!   DiaCraft's** (the route marker) **that lacks them**: one an `.odg` reader read, and one an earlier build made. A file's own shapes, and a connector whose `ObjType` says something already,
//!   are left as they are.
//! * **The field rows of a VSDX file** ([`super::fields`]): the `.odg` reader makes a field row of the `Value` cell alone, an older build made a field with an empty `Format`, and a program that
//!   reads the file takes a field with no picture for one that shows nothing and a text with no unit for a number it cannot read. A save to VSDX **gives the model** the `Type` of the field's kind, the `Format`
//!   of and `STR` on a text (and `DATE` on a date), where a row has none: the drawing the person has is not changed, and the `.odg` reader and the splice comparisons are not touched.
//! * **The text rows of a VSDX file** ([`super::text_rows`]): a reader of the file takes a cell that a `Character` or `Paragraph` row past the first lacks from the
//!   master's row of its `IX` alone, and reads 0 where there is none, so a `FontScale` of 0 piles a run's letters on one another. A save to VSDX **gives the model** each cell of such a row that it lacks
//!   and takes from nowhere else, with the value the drawing reads for it: the rows of an `.odg` (a paragraph that states no alignment is left-aligned, and the held styles stand beside the rows) and
//!   of an older build, and **no row of a VSDX file of another program** (a file's own rows are written as read). The drawing the person has is not changed.
//! * **The picture box of a picture an `.odg` held** (the concern 2: "the `Img` cells are written as for a new picture"). The `.odg` reader
//!   gives a picture frame no `ImgOffsetX`, `ImgOffsetY`, `ImgWidth` or `ImgHeight`, since the frame is the picture; a `Foreign` shape of a VSDX file draws its picture in the box those four
//!   cells say, and **LibreOffice draws a `Foreign` shape only with them** (measured: the first run of its files converted to empty pages). So a save to VSDX **gives the model those
//!   cells** for each shape that shows a picture and came from an `.odg`: the picture fills the shape (`ImgWidth` is `Width*1`, `ImgHeight` is `Height*1`, the offsets are 0). The drawing the person
//!   has is not changed; the copy the writer is given has them, and a save that adopts the model the file reads as (`Session::adopt_saved`) gives the drawing them.
//! * **The text of the `User` and `Field` rows of a VSDX file** (`diacraft_vsdx::string_units`): no `Type` cell says the `Value` of such a row is a string, so the unit alone does, and the program that opens the
//!   file takes a text with none for a value of the wrong type and says so for each cell. The `User` rows of a container or a swimlane in a `.diacraft` file of an earlier build hold their text with none, and so does
//!   an `.odg` reader's. A save to VSDX **gives the model** `STR` on the `Value` of each such row that holds a text (`diacraft_doc::Cell::lacks_untyped_string_unit`), whatever made it, and on no other cell (a `Property`
//!   row's `Value` is typed by its `Type`, and the cells of a text kind carry no unit in the files of the program that opens them): the drawing the person has is not changed, and a save that adopts the model the file
//!   reads as ([`super::adopts_by_working_file`]) gives the drawing the units. **Only for a drawing whose units are DiaCraft's to state** (`diacraft_doc::units_are_ours_to_state`: it came from no VSDX package, or from one whose extended
//!   properties name DiaCraft): the package of another program is written as it was read, and so is a `Field` row whose `Type` says its value is no string (a date field).
//! * **No style link of an instance that is its master's**. The `.odg` reader gives each shape the links its automatic style's parent names, so an instance whose
//!   master's shape links the `Theme` style (a theme links the masters' shapes, and an `.odg` carries the masters in DiaCraft's document file) comes back linked to it itself. The VSDX application reads an
//!   instance's own link before its master's cells of that family, and so reads the style's `THEMEVAL()` for the line ends, the pattern and the fill the master holds. A save to VSDX **takes off the copy each
//!   link of a shape read from an `.odg` that is the link of the master's shape it stands for** (no link counting as the root's), which says nothing in DiaCraft: a shape with no link of its own follows
//!   its master shape's (step 3). A link that is not the master's, and every shape of another origin, are left as they are.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, Document, Format, Links, MAX_SHAPE_DEPTH, OdfShape, Owner, Page, RawAttrs, Resolver, Shape, ShapeList, ShapeRef, ShapeType,
    SheetRef, Value,
};

use super::Options;
use super::hold::{Left, hold, rewrite_list, rewrite_shapes, without_objects};
use super::{MAX_STEPS, TOO_BIG};
use crate::draw::Reader;
use crate::draw::routable;

/// What the copy handed to a model writer is, and what was done to make it.
#[derive(Debug, Default)]
pub(crate) struct Given {
    /// The copy of the drawing, or `None` when it is given as it is.
    pub doc: Option<Document>,
    /// The objects of the other format that were left out (`drop_objects`).
    pub left: Left,
    /// The shapes that were given the cells of a picture's box.
    pub boxed: u64,
    /// The shapes that were given the glue formulas of an end glued to the whole of a shape.
    pub dressed: u64,
    /// The connectors of DiaCraft's that were given the cells that make them ones the VSDX application routes.
    pub routable: u64,
    /// The shapes whose field rows were given the `Type`, the `Format` or the unit they lacked ([`super::fields`]).
    pub fielded: u64,
    /// The shapes whose text rows past the first were given the cells they lacked ([`super::text_rows`]).
    pub texts: u64,
    /// The shapes read from an `.odg` that were given no style link that is their master's.
    pub unlinked: u64,
    /// The sheets (a shape's, a page's, a master's, the document's) whose `User` or `Field` text was given the unit `STR` it lacked (`diacraft_vsdx::string_units`).
    pub stringed: u64,
}

/// The number a shape's own cell holds.
fn number(shape: &Shape, name: &str) -> Option<f64> {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()).and_then(Value::number)
}

/// Whether a shape was read from an `.odg`: it holds the data of an element of one.
fn from_odg(shape: &Shape) -> bool {
    shape.ext.get().is_some_and(|data| data.odf != OdfShape::default())
}

/// A shape that shows a picture it held as an `.odg` frame and has no picture box: it gets the four cells that make the picture fill the shape. `None` for any other shape.
fn boxed(shape: &Shape) -> Option<Shape> {
    let odf = from_odg(shape);
    let picture = shape.foreign.as_ref().is_some_and(|f| f.picture.is_some());
    if shape.kind != ShapeType::Foreign || !odf || !picture || shape.sheet.cell("ImgWidth").is_some() || shape.sheet.cell("ImgHeight").is_some() {
        return None;
    }
    let (width, height) = (number(shape, "Width")?, number(shape, "Height")?);
    let mut own = Shape::clone(shape);
    // The copy's own nodes and its sheet's count the cells, so the four go in as the writers of the model put a cell in.
    let mut counted = own.counted_sheet();
    for (name, cell) in [
        ("ImgOffsetX", Value::from_number(0.0).map(|v| Cell::constant(v).with_unit("IN"))),
        ("ImgOffsetY", Value::from_number(0.0).map(|v| Cell::constant(v).with_unit("IN"))),
        ("ImgWidth", Value::from_number(width).map(|v| Cell::with_formula("Width*1", Some(v)).with_unit("IN"))),
        ("ImgHeight", Value::from_number(height).map(|v| Cell::with_formula("Height*1", Some(v)).with_unit("IN"))),
    ] {
        // A sheet at the most cells it may hold takes no more: the shape is then given no box, as it is without one in the drawing.
        counted.set_cell(&CellAddr::direct(name), cell?).ok()?;
    }
    Some(own)
}

/// The copy of `doc` that the model writer of `target` is given, or none when `doc` is given as it is (see the module documentation).
pub(crate) fn given(doc: &Document, target: Format, options: Options) -> Result<Given, &'static str> {
    let mut out = Given::default();
    let mut steps = 0u64;
    // (A drawing that holds no object has nothing to leave out, which one pass over its parts and shapes says before any copy is made.)
    if options.drop_objects {
        let held = hold(doc)?;
        if (held.object_parts > 0 || held.object_shapes > 0)
            && let Some((without, left)) = without_objects(doc, target)?
        {
            out.doc = Some(without);
            out.left = left;
        }
    }
    if target == Format::Vsdx {
        let mut count = 0u64;
        let mut change = |shape: &Shape| {
            let own = boxed(shape);
            count = count.saturating_add(u64::from(own.is_some()));
            own
        };
        let base = out.doc.as_ref().unwrap_or(doc);
        if let Some(with) = rewrite_shapes(base, true, &mut change, &mut steps)? {
            out.doc = Some(with);
        }
        out.boxed = count;
        // The ends glued to the whole of a shape, which the application drops from a file whose end cells hold no glue formula.
        let base = out.doc.as_ref().unwrap_or(doc);
        let (dressed, with) = dress_whole_glue(base, &mut steps)?;
        out.dressed = dressed;
        if with.is_some() {
            out.doc = with;
        }
        // The connectors of DiaCraft's that the application would not route again.
        let base = out.doc.as_ref().unwrap_or(doc);
        let (routable, with) = dress_routable(base, &mut steps)?;
        out.routable = routable;
        if with.is_some() {
            out.doc = with;
        }
        // The field rows that lack what the file needs: an `.odg`'s, an older build's and another program's.
        let base = out.doc.as_ref().unwrap_or(doc);
        let (fielded, with) = super::fields::fit(base, &mut steps)?;
        out.fielded = fielded;
        if with.is_some() {
            out.doc = with;
        }
        // The text rows past the first that hold some of their cells: an `.odg`'s and an older build's.
        let base = out.doc.as_ref().unwrap_or(doc);
        let (texts, with) = super::text_rows::fit(base, &mut steps)?;
        out.texts = texts;
        if with.is_some() {
            out.doc = with;
        }
        // The links of an `.odg`'s instances that are their masters'.
        let base = out.doc.as_ref().unwrap_or(doc);
        let (unlinked, with) = without_masters_links(base, &mut steps)?;
        out.unlinked = unlinked;
        if with.is_some() {
            out.doc = with;
        }
        // The text of a `User` or `Field` row that says no unit: a `.diacraft` file of an earlier build's, an `.odg`'s and another program's.
        let base = out.doc.as_ref().unwrap_or(doc);
        if let Some(fitted) = diacraft_vsdx::string_units(base) {
            out.stringed = fitted.sheets;
            out.doc = Some(fitted.doc);
        }
    }
    Ok(out)
}

/// The drawing with the cells that make a connector one the VSDX application routes given to each connector of DiaCraft's that lacks them ([`crate::draw::routable::lack_of`], read through
/// inheritance), and how many it gave them to; `None` when there are none (and then the count is 0). One walk of each page finds them, and the copy shares everything it does not change.
fn dress_routable(doc: &Document, steps: &mut u64) -> Result<(u64, Option<Document>), &'static str> {
    let reader = Reader::new(doc);
    let mut out: Option<Document> = None;
    let mut count = 0u64;
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        let lacking = lacking_on(&reader, page, steps)?;
        if lacking.is_empty() {
            continue;
        }
        let mut change = |shape: &Shape| {
            let own = routable::dressed(shape, *lacking.get(&shape.id)?);
            count = count.saturating_add(u64::from(own.is_some()));
            own
        };
        if let Some(shapes) = rewrite_list(&page.shapes, &mut change, steps, 0)? {
            let mut copy = Page::clone(page);
            copy.shapes = shapes;
            // (`at` is a place in the list.)
            let _ = out.get_or_insert_with(|| doc.clone()).pages.set(at, Arc::new(copy));
        }
    }
    Ok((count, out))
}

/// The links a shape read from an `.odg` keeps of its own, when it stands for a shape of a master (`theirs`, that shape's links) and holds a link that is that shape's: each such link taken off
/// (no link of the master's shape counting as the root's). `None` when it keeps all it has.
fn own_links(shape: &Shape, theirs: Links) -> Option<Links> {
    let keep = |own: Option<u32>, theirs: Option<u32>| own.filter(|id| *id != theirs.unwrap_or(0));
    let own = shape.sheet.links;
    let kept = Links { line: keep(own.line, theirs.line), fill: keep(own.fill, theirs.fill), text: keep(own.text, theirs.text) };
    (kept != own).then_some(kept)
}

/// The drawing with no style link of a shape read from an `.odg` that is the link of the master's shape it stands for (the module documentation), and how many shapes lost one;
/// `None` when there are none (and then the count is 0). One walk of each page finds them, each shape counted against `steps`, and the copy shares everything it does not change.
fn without_masters_links(doc: &Document, steps: &mut u64) -> Result<(u64, Option<Document>), &'static str> {
    let res = Resolver::new(doc);
    let mut out: Option<Document> = None;
    let mut count = 0u64;
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        // The links each such shape keeps, by `ID` (unique on a page, members of groups among them).
        let mut kept: HashMap<u32, Links> = HashMap::new();
        visit_page(page, steps, |shape, place| {
            if shape.del || shape.sheet.links == Links::default() || !from_odg(shape) {
                return;
            }
            let Ok(Some(step)) = res.master_step(place) else { return };
            let Some(theirs) = res.shape(&step.shape).map(|s| s.sheet.links) else { return };
            if let Some(links) = own_links(shape, theirs) {
                kept.insert(shape.id, links);
            }
        })?;
        if kept.is_empty() {
            continue;
        }
        let mut change = |shape: &Shape| {
            let links = *kept.get(&shape.id)?;
            let mut own = Shape::clone(shape);
            let sheet = Arc::make_mut(&mut own.sheet);
            sheet.links = links;
            // A link the sheet keeps as text would be written in place of the model's (raw wins on a write).
            if sheet.raw.iter().any(|(name, _)| ["LineStyle", "FillStyle", "TextStyle"].contains(&name)) {
                sheet.raw = RawAttrs::from_pairs(sheet.raw.iter().filter(|(name, _)| !["LineStyle", "FillStyle", "TextStyle"].contains(name)));
            }
            count = count.saturating_add(1);
            Some(own)
        };
        if let Some(shapes) = rewrite_list(&page.shapes, &mut change, steps, 0)? {
            let mut copy = Page::clone(page);
            copy.shapes = shapes;
            // (`at` is a place in the list.)
            let _ = out.get_or_insert_with(|| doc.clone()).pages.set(at, Arc::new(copy));
        }
    }
    Ok((count, out))
}

/// Visits each shape of `page`, members of groups among them, with its place (one walk of the page, iterative and bounded by [`MAX_SHAPE_DEPTH`]): each shape is a step of `steps`, counted before it
/// is visited.
fn visit_page(page: &Page, steps: &mut u64, mut visit: impl FnMut(&Shape, &ShapeRef)) -> Result<(), &'static str> {
    let mut stack: Vec<(&ShapeList, usize, Vec<u32>)> = vec![(&page.shapes, 0, Vec::new())];
    while let Some(top) = stack.last_mut() {
        let (list, next) = (top.0, top.1);
        let Some(shape) = list.get(next) else {
            stack.pop();
            continue;
        };
        top.1 = next.saturating_add(1);
        let mut path = top.2.clone();
        path.push(u32::try_from(next).map_err(|_| TOO_BIG)?);
        *steps = steps.saturating_add(1);
        if *steps > MAX_STEPS {
            return Err(TOO_BIG);
        }
        let at = ShapeRef { owner: Owner::Page(page.id), path };
        visit(shape, &at);
        if !shape.children.is_empty() {
            if stack.len() >= MAX_SHAPE_DEPTH {
                return Err(TOO_BIG);
            }
            stack.push((&shape.children, 0, at.path));
        }
    }
    Ok(())
}

/// What each connector of DiaCraft's on `page` lacks, by `ID` (unique on a page, members of groups among them): one walk of its shapes, each counted against `steps`.
fn lacking_on(reader: &Reader<'_>, page: &Page, steps: &mut u64) -> Result<HashMap<u32, routable::Lack>, &'static str> {
    let mut lacking = HashMap::new();
    visit_page(page, steps, |shape, at| {
        if !shape.del
            && let Some(lack) = routable::lack_of(reader, &SheetRef::Shape(at.clone()))
        {
            lacking.insert(shape.id, lack);
        }
    })?;
    Ok(lacking)
}

/// The shapes of `page` among `ends` (by `ID`) that are DiaCraft's: they hold the route marker `User.DiaCraftRoute`, itself or through their master, which a connector made here holds, and which the
/// `.odg` reader and every earlier build gave theirs. One walk of the page, each shape counted against `steps`.
fn ours_among(reader: &Reader<'_>, page: &Page, ends: &HashMap<u32, [Option<u32>; 2]>, steps: &mut u64) -> Result<HashSet<u32>, &'static str> {
    let mut ours = HashSet::new();
    visit_page(page, steps, |shape, at| {
        if !shape.del && ends.contains_key(&shape.id) && reader.is_routed(&SheetRef::Shape(at.clone())) {
            ours.insert(shape.id);
        }
    })?;
    Ok(ours)
}

/// The drawing with the glue formulas given to the ends glued to the whole of a shape that hold plain places ([`crate::draw::walkglue::dress_shape`]), **on the connectors of DiaCraft's alone**
/// (a file's own whole-shape ends are what the other program wrote, on every save route), and how many shapes it gave them to; `None` when there are none (and then the count is 0).
/// The copy shares everything it does not change.
fn dress_whole_glue(doc: &Document, steps: &mut u64) -> Result<(u64, Option<Document>), &'static str> {
    let reader = Reader::new(doc);
    let mut out: Option<Document> = None;
    let mut count = 0u64;
    for at in 0..doc.pages.len() {
        let Some(page) = doc.pages.get(at) else { continue };
        let ends = crate::draw::walkglue::whole_ends(page);
        if ends.is_empty() {
            continue;
        }
        let ours = ours_among(&reader, page, &ends, steps)?;
        if ours.is_empty() {
            continue;
        }
        let mut change = |shape: &Shape| {
            if !ours.contains(&shape.id) {
                return None;
            }
            let own = crate::draw::walkglue::dress_shape(shape, ends.get(&shape.id)?);
            count = count.saturating_add(u64::from(own.is_some()));
            own
        };
        if let Some(shapes) = rewrite_list(&page.shapes, &mut change, steps, 0)? {
            let mut copy = Page::clone(page);
            copy.shapes = shapes;
            // (`at` is a place in the list.)
            let _ = out.get_or_insert_with(|| doc.clone()).pages.set(at, Arc::new(copy));
        }
    }
    Ok((count, out))
}

/// The words that say what `drop_objects` left out, for the warnings of the save (none when it left nothing out): the drawing keeps the objects, and each shape that showed one shows its picture.
pub(crate) fn left_out_words(given: &Given) -> Option<String> {
    let Left { shapes, parts } = given.left;
    let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    if shapes > 0 {
        Some(format!(
            "{} left out of the file, as asked: each shape stays as the picture it showed, and the drawing still holds them (save it as .diacraft to keep them)",
            plural(shapes, "embedded object was", "embedded objects were")
        ))
    } else if parts > 0 {
        Some(format!(
            "{} of embedded objects left out of the file, as asked: the drawing still holds them (save it as .diacraft to keep them)",
            plural(u64::try_from(parts).unwrap_or(u64::MAX), "part was", "parts were")
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{CellName, ForeignData, Media, Passthrough, Placed, RawNode};

    use super::*;

    /// A picture frame an `.odg` held, with the four cells of the box missing, and a node among its cells after each of the counts `afters`.
    fn frame(afters: &[u32]) -> Shape {
        let mut shape = Shape::new(1);
        shape.kind = ShapeType::Foreign;
        shape.ext.make_mut().odf.style_name = Some(Arc::from("gr1"));
        shape.foreign = Some(Arc::new(ForeignData {
            picture: Some(Arc::new(Media { part: Arc::from("Pictures/a.png"), content_type: Arc::from("image/png"), bytes: Arc::from(&b"png"[..]) })),
            ..ForeignData::default()
        }));
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, text) in [("Height", "1"), ("PinX", "2"), ("PinY", "2"), ("Width", "3")] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(text)));
        }
        shape.passthrough = Passthrough::from_nodes(
            afters.iter().map(|after| Placed { after: *after, node: RawNode::Comment(Arc::from(format!("after {after}"))) }).collect(),
        );
        shape
    }

    /// The four cells of the picture's box go in as the writers of the model put a cell in, so a node among the cells of the frame stays after the cell it followed (a node
    /// that followed `Height` goes behind the four, which sort right after it, and one that followed the last cell trails them).
    #[test]
    fn the_cells_of_a_pictures_box_go_in_ahead_of_the_nodes_that_followed_the_cell_before_them() {
        let boxed = boxed(&frame(&[0, 1, 2, 4])).expect("a frame with a picture and no box is given one");
        let names: Vec<&str> = boxed.sheet.cells.keys().map(CellName::as_str).collect();
        assert_eq!(names, ["Height", "ImgHeight", "ImgOffsetX", "ImgOffsetY", "ImgWidth", "PinX", "PinY", "Width"]);
        let places: Vec<u32> = boxed.passthrough.iter().map(|placed| placed.after).collect();
        assert_eq!(places, [0, 5, 6, 8]);
        // A frame that holds the box already is left as it is.
        assert!(super::boxed(&boxed).is_none());
    }
}
