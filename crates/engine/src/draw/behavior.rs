//! The Behavior dialog's Placement tab as cells: how a shape is laid out and routed round (`shape.behavior`, `shape.set_behavior`).
//!
//! **What the choices write**:
//!
//! * **Placement behavior** is the shape's `ObjType`: *Let the application decide* is 0 (the shape is placeable once a connector is glued to it), *Lay out and route around* 1 (always
//!   placeable) and *Do not lay out and route around* 4 (neither: it outranks 1 and 2). The write keeps every flag outside 1, 2 and 4 the shape holds (a group's 8 among them: values add up).
//! * **Do not move during placement** is flag 1 of `ShapeFixedCode`, the other flags kept (2, 4, 32, 64 and 128 are not this box's). The Placement tab's other box that
//!   `ShapeFixedCode` holds, "Do not allow other shapes to move this shape away on drop", has no word here yet; the cell is kept as it is.
//! * **Allow other shapes to be placed on top** is `ShapePermeablePlace`; **Move other shapes away on drop** is `ShapePlowCode`: 0 as the page, 1 plow no shapes, 2 plow every shape.
//! * **Route through horizontally and vertically** are `ShapePermeableX` and `ShapePermeableY`.
//!
//! A cell that already reads what is asked is not written, a cell is written locally (so that a master's value is overridden), and a call that writes nothing is no undo step.
//! **`shape.set_behavior` routes nothing** (ours): it is a setting, as `connector.reroute` is; a route that goes round a shape that stops being an obstacle, or
//! through one that starts to be, is made again when something asks (a change of the shape or of a connector). Only a two-dimensional shape has these choices:
//! a connector, a line or a guide named is refused in words.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, ShapeRef, ShapeType, SheetRef, Value};

use super::obstacle::{self, HiddenLayers, NEITHER, PLACEABLE, ROUTABLE, flags_of, obj_kind, read_candidate};
use super::read::Reader;
use super::{Problem, scan, write_cells};

/// The Placement behavior list: what `ObjType` says of a two-dimensional shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// *Let the application decide*: `ObjType` 0, placeable once a connector is glued to the shape.
    Auto,
    /// *Lay out and route around*: `ObjType` 1, always placeable.
    RouteAround,
    /// *Do not lay out and route around*: `ObjType` 4, neither.
    None,
}

impl Placement {
    /// The three, in the order of the list.
    pub const ALL: [Placement; 3] = [Placement::Auto, Placement::RouteAround, Placement::None];

    /// The `ObjType` flags it writes: 0, 1 or 4.
    pub const fn flags(self) -> u8 {
        match self {
            Placement::Auto => 0,
            Placement::RouteAround => PLACEABLE,
            Placement::None => NEITHER,
        }
    }

    /// The word a command takes for it.
    pub const fn word(self) -> &'static str {
        match self {
            Placement::Auto => "auto",
            Placement::RouteAround => "route_around",
            Placement::None => "none",
        }
    }

    /// The choice a word names (`auto`, `route_around` or `none`, in any case, with `-` or a space for `_`).
    pub fn from_word(word: &str) -> Option<Placement> {
        let word = word.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Placement::ALL.into_iter().find(|p| p.word() == word)
    }

    /// The choice the flags of an `ObjType` show: 4 is none, and else 1 is route around. Everything else (0, 2, a group's 8 alone) is auto.
    pub const fn of_flags(flags: u8) -> Placement {
        let kind = obj_kind(flags);
        if kind.neither {
            Placement::None
        } else if kind.placeable {
            Placement::RouteAround
        } else {
            Placement::Auto
        }
    }
}

/// *Move other shapes away on drop*: what `ShapePlowCode` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plow {
    /// 0: as the page says.
    AsPage,
    /// 1: plow no shapes.
    Never,
    /// 2: plow every shape.
    Always,
}

impl Plow {
    pub const ALL: [Plow; 3] = [Plow::AsPage, Plow::Never, Plow::Always];

    /// The `ShapePlowCode` that says it.
    pub const fn code(self) -> u8 {
        match self {
            Plow::AsPage => 0,
            Plow::Never => 1,
            Plow::Always => 2,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Plow::AsPage => "as_page",
            Plow::Never => "never",
            Plow::Always => "always",
        }
    }

    /// The choice a word names (`as_page`, `never` or `always`, in any case, with `-` or a space for `_`).
    pub fn from_word(word: &str) -> Option<Plow> {
        let word = word.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Plow::ALL.into_iter().find(|p| p.word() == word)
    }

    /// The choice a `ShapePlowCode` says: 1 and 2 are theirs, and every other number reads as the page's.
    pub fn from_code(code: f64) -> Plow {
        match flags_of(code) {
            1 => Plow::Never,
            2 => Plow::Always,
            _ => Plow::AsPage,
        }
    }
}

/// What a shape holds of the Placement tab's boxes, and whether a route goes round it now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behavior {
    pub id: u32,
    pub placement: Placement,
    /// The flags of the effective `ObjType`.
    pub obj_type: u8,
    pub route_through_x: bool,
    pub route_through_y: bool,
    /// `ShapeFixedCode` flag 1.
    pub do_not_move: bool,
    /// `ShapePermeablePlace`.
    pub allow_on_top: bool,
    pub plow: Plow,
    /// Why a route does not go round the shape, or `None` when it is an obstacle now (a connector is glued to a shape that decides, and the layers, are counted).
    pub why_not: Option<&'static str>,
}

/// What a call changes: each field that is set (`None` leaves the cell).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BehaviorChange {
    pub placement: Option<Placement>,
    pub route_through_x: Option<bool>,
    pub route_through_y: Option<bool>,
    pub do_not_move: Option<bool>,
    pub allow_on_top: Option<bool>,
    pub plow: Option<Plow>,
}

impl BehaviorChange {
    /// Whether nothing is asked.
    pub fn is_empty(&self) -> bool {
        *self == BehaviorChange::default()
    }
}

/// `ShapeFixedCode` flag 1: layout leaves the shape where it is.
const DO_NOT_MOVE: u8 = 1;

/// The Placement tab's values for the shapes `ids` of page `page`, in the order asked (each once), with whether each is an obstacle now. A shape that is not on the page is an error.
pub fn shape_behavior(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<Behavior>, Problem> {
    let wanted = scan::distinct(ids);
    let (_, found) = scan::named(doc, page, &wanted)?;
    let at: std::collections::HashMap<u32, ShapeRef> = found.into_iter().collect();
    let reader = Reader::new(doc);
    let glued = obstacle::glued_to(doc, page);
    let layers = HiddenLayers::of(doc, Owner::Page(page));
    Ok(wanted.iter().filter_map(|id| at.get(id).map(|at| describe(&reader, at, *id, &glued, &layers))).collect())
}

fn describe(reader: &Reader<'_>, at: &ShapeRef, id: u32, glued: &std::collections::HashSet<u32>, layers: &HiddenLayers) -> Behavior {
    let sheet = SheetRef::Shape(at.clone());
    let flags = flags_of(reader.number(&sheet, "ObjType"));
    let flag = |name: &str| reader.number(&sheet, name) != 0.0;
    let why_not = match read_candidate(reader, at, layers) {
        Err(why) => Some(why.words()),
        Ok(facts) => facts.check(glued.contains(&id)).err().map(obstacle::NotObstacle::words),
    };
    Behavior {
        id,
        placement: Placement::of_flags(flags),
        obj_type: flags,
        route_through_x: flag("ShapePermeableX"),
        route_through_y: flag("ShapePermeableY"),
        do_not_move: flags_of(reader.number(&sheet, "ShapeFixedCode")) & DO_NOT_MOVE != 0,
        allow_on_top: flag("ShapePermeablePlace"),
        plow: Plow::from_code(reader.number(&sheet, "ShapePlowCode")),
        why_not,
    }
}

/// A cell and the number a write gives it.
type Write = (SheetRef, CellAddr, CellEdit);

fn number_write(sheet: &SheetRef, name: &str, value: u8) -> Write {
    (sheet.clone(), CellAddr::direct(name), CellEdit::Value(Value::from_text(&value.to_string())))
}

/// The `ObjType` flags `held` with the placement `placement` says: the flags 1, 2 and 4 are what the choice writes (0, 1 or 4), and **every other flag is kept**, a group's 8 among them
/// (values add up).
const fn with_placement(held: u8, placement: Placement) -> u8 {
    (held & !(PLACEABLE | ROUTABLE | NEITHER)) | placement.flags()
}

/// The flags `held` with `flag` set or cleared, the others kept (`ShapeFixedCode` holds flags that are not one box's).
const fn with_flag(held: u8, flag: u8, on: bool) -> u8 {
    if on { held | flag } else { held & !flag }
}

/// The writes that bring the cells of one shape to what `change` asks, leaving out each that reads it already.
fn plan(reader: &Reader<'_>, sheet: &SheetRef, change: &BehaviorChange, writes: &mut Vec<Write>) {
    if let Some(placement) = change.placement {
        let held = flags_of(reader.number(sheet, "ObjType"));
        let next = with_placement(held, placement);
        if next != held {
            writes.push(number_write(sheet, "ObjType", next));
        }
    }
    for (name, wanted) in
        [("ShapePermeableX", change.route_through_x), ("ShapePermeableY", change.route_through_y), ("ShapePermeablePlace", change.allow_on_top)]
    {
        if let Some(on) = wanted
            && (reader.number(sheet, name) != 0.0) != on
        {
            writes.push(number_write(sheet, name, u8::from(on)));
        }
    }
    if let Some(on) = change.do_not_move {
        let held = flags_of(reader.number(sheet, "ShapeFixedCode"));
        let next = with_flag(held, DO_NOT_MOVE, on);
        if next != held {
            writes.push(number_write(sheet, "ShapeFixedCode", next));
        }
    }
    if let Some(plow) = change.plow
        && Plow::from_code(reader.number(sheet, "ShapePlowCode")) != plow
    {
        writes.push(number_write(sheet, "ShapePlowCode", plow.code()));
    }
}

/// Why shape `id` has no placement behavior, if it has none: a connector or a line, or a guide.
fn refusal(doc: &Document, reader: &Reader<'_>, id: u32, at: &ShapeRef) -> Option<Problem> {
    if doc.shape(at).is_some_and(|shape| shape.kind == ShapeType::Guide) {
        return Some(Problem::bad(format!("shape {id} is a guide: it is an aid and no shape a route goes round")));
    }
    reader.is_one_d(&SheetRef::Shape(at.clone())).then(|| {
        Problem::bad(format!(
            "shape {id} is a connector or a line: it has no placement behavior (connector.reroute and connector.style set how it is routed)"
        ))
    })
}

/// Sets the Placement tab's boxes of the shapes `ids` of page `page`, written locally, each only where it differs from what the shape reads, and routes nothing
/// → how many shapes were written. A shape that is not on the page, a connector, a line or a guide is an error and nothing changes (all or nothing).
pub fn set_behavior(doc: &mut Document, page: u32, ids: &[u32], change: &BehaviorChange) -> Result<usize, Problem> {
    let (_, found) = scan::named(doc, page, ids)?;
    let mut writes: Vec<Write> = Vec::new();
    let mut written = 0usize;
    {
        let reader = Reader::new(doc);
        for (id, at) in &found {
            if let Some(problem) = refusal(doc, &reader, *id, at) {
                return Err(problem);
            }
            let before = writes.len();
            plan(&reader, &SheetRef::Shape(at.clone()), change, &mut writes);
            written += usize::from(writes.len() > before);
        }
    }
    if !writes.is_empty() {
        write_cells(doc, writes)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Shape, ShapeList};

    use super::super::{NODES_WALKED, page_mut};
    use super::*;

    /// A page of `n` bare shapes with the `ID`s 1 to `n`.
    fn page_of(n: u32) -> Document {
        let mut doc = Document::new_blank();
        page_mut(&mut doc, 1).unwrap().shapes = ShapeList::from_vec((1..=n).map(|id| Arc::new(Shape::new(id))).collect());
        doc
    }

    /// The work of a call is the walks of the page it makes: **one**, for however many shapes it names, and no search for each (a search for each of 4,096 shapes of a page of 30,000 is
    /// 60 million visits), whether it reads the shapes or writes them.
    #[test]
    fn a_call_is_one_walk_of_the_page_whatever_the_number_of_shapes_it_names() {
        let mut doc = page_of(30_000);
        let ids: Vec<u32> = (0..4_096).map(|k| 1 + k * 7).collect();
        NODES_WALKED.with(|c| c.set(0));
        diacraft_doc::work::reset();
        let change = BehaviorChange { route_through_x: Some(true), plow: Some(Plow::Always), ..BehaviorChange::default() };
        assert_eq!(set_behavior(&mut doc, 1, &ids, &change).unwrap(), 4_096);
        assert!(NODES_WALKED.with(std::cell::Cell::get) <= 30_000, "one walk of the page to write the shapes");
        assert_eq!(diacraft_doc::work::finds_visited(), 0, "no search for a shape by its ID");
        NODES_WALKED.with(|c| c.set(0));
        let read = shape_behavior(&doc, 1, &ids).unwrap();
        assert_eq!(read.len(), 4_096);
        assert!(read.iter().all(|b| b.route_through_x && b.plow == Plow::Always && b.why_not.is_some()));
        assert!(NODES_WALKED.with(std::cell::Cell::get) <= 30_000, "one walk of the page to read them");
        assert_eq!(diacraft_doc::work::finds_visited(), 0);
        // The same call again writes nothing.
        assert_eq!(set_behavior(&mut doc, 1, &ids, &change).unwrap(), 0);
    }

    /// The answer is in the order the shapes were named, each once, and not the page's.
    #[test]
    fn the_query_answers_in_the_order_asked_and_names_each_shape_once() {
        let doc = page_of(10);
        let read = shape_behavior(&doc, 1, &[7, 3, 7, 9, 3]).unwrap();
        assert_eq!(read.iter().map(|b| b.id).collect::<Vec<_>>(), [7, 3, 9]);
        assert_eq!(shape_behavior(&doc, 1, &[3, 99]).unwrap_err().message, "page 1 has no shape 99");
        assert_eq!(shape_behavior(&doc, 4, &[3]).unwrap_err().message, "there is no page 4");
    }

    #[test]
    fn the_words_of_each_list_are_read_in_any_case_and_a_choice_is_read_back_from_its_flags() {
        for placement in Placement::ALL {
            assert_eq!(Placement::from_word(placement.word()), Some(placement));
            assert_eq!(Placement::of_flags(placement.flags()), placement);
        }
        assert_eq!(Placement::from_word(" Route-Around "), Some(Placement::RouteAround));
        assert_eq!(Placement::from_word("front"), None);
        for plow in Plow::ALL {
            assert_eq!(Plow::from_word(plow.word()), Some(plow));
            assert_eq!(Plow::from_code(f64::from(plow.code())), plow);
        }
        assert_eq!(Plow::from_word("AS PAGE"), Some(Plow::AsPage));
        assert_eq!(Plow::from_code(7.0), Plow::AsPage, "a number no choice has reads as the page's");
        assert_eq!(Plow::from_code(f64::NAN), Plow::AsPage);
    }

    #[test]
    fn a_placement_replaces_the_flags_1_2_and_4_and_keeps_every_other() {
        for held in 0..=255u8 {
            for placement in Placement::ALL {
                let next = with_placement(held, placement);
                assert_eq!(next & 7, placement.flags(), "{held} {placement:?}: exactly the flags of the choice");
                assert_eq!(next & !7, held & !7, "{held} {placement:?}: the rest is kept");
                assert_eq!(Placement::of_flags(next), placement, "{held} {placement:?}: read back as the choice");
            }
            for flag in [1u8, 2, 4, 32, 128] {
                assert_eq!(with_flag(held, flag, true) & flag, flag);
                assert_eq!(with_flag(held, flag, false) & flag, 0);
                assert_eq!(with_flag(held, flag, true) & !flag, held & !flag, "the others are kept");
                assert_eq!(with_flag(held, flag, false) & !flag, held & !flag);
            }
        }
    }

    #[test]
    fn a_choice_is_what_the_flags_say_by_mask() {
        // 4 outranks 1 and 2; 9 and 8 keep the group flag out of it; 2 alone and 16 alone are the application's.
        for (flags, want) in [
            (0, Placement::Auto),
            (1, Placement::RouteAround),
            (2, Placement::Auto),
            (3, Placement::RouteAround),
            (4, Placement::None),
            (5, Placement::None),
            (6, Placement::None),
            (8, Placement::Auto),
            (9, Placement::RouteAround),
            (12, Placement::None),
            (16, Placement::Auto),
            (17, Placement::RouteAround),
        ] {
            assert_eq!(Placement::of_flags(flags), want, "{flags}");
        }
    }
}
