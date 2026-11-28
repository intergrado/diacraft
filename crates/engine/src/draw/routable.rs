//! The cells that make a connector one the VSDX application routes.
//!
//! **Why DiaCraft writes them.** The application's check of the files DiaCraft writes (on the project's test PC): a person dragged a shape that a DiaCraft connector was glued to; the glue held
//! and the end followed, but the legs tilted and the arrowhead stopped short of the shape, for the application did not route the connector again. The specification says just that of a
//! one-dimensional shape that is no connector: "a shape that isn't one never reroutes; its glued ends follow their targets by their formulas". **A
//! connector is a one-dimensional shape whose effective `ObjType` holds the routable flag 2 and not 4** (`visLOFlagsRoutable`), and a connector DiaCraft made, with no master and
//! no `ObjType` of its own, read the root style's 0, "the application chooses". **Glue to the whole of a shape is dynamic glue,** which `GlueType` 0 lets only the application's own dynamic connector
//! make and 2 allows, and the routable one-dimensional shapes of the corpus hold `GlueType` 2 on 17,268 of 17,359.
//!
//! **What a connector of DiaCraft's holds.** `ObjType` 2 and `GlueType` 2, plain values of its own: a connector is made with them (`connectors.rs`), so `connector.add`, the Connector tool,
//! AutoConnect, a dropped connector master and the connectors a layout makes have them, and every writer writes what the model holds (an `.odg` carries them in DiaCraft's data, as it
//! carries every cell it cannot say). **A connector of DiaCraft's that lacks them** (one an earlier build made, one an `.odg` reader read: both carry the route marker `User.DiaCraftRoute`)
//! **is given them when DiaCraft routes it again** (`connectors.rs` `write_planned`, `hand.rs` `edit_leg`, in the edit's undo step) **unless an `.odg` file holds it** (the cells say nothing
//! to LibreOffice, so the `.odg`'s connector is left as the file has it), **and in a save across to a VSDX type** (`across/onto.rs`), in the copy the writer is given, the `.odg`'s connectors
//! among them. **Left as they are:** a shape with no route marker (a file's own connector, routable already by its master, and a file's plain line, which the application does not route), a
//! fixed one-dimensional figure (no marker: `shapes.rs`), and a connector whose `ObjType` says something already (flag 1, 2 or 4: a person's choice of placement is kept) or holds a formula
//! of its own. `GlueType` is written only where it reads 0 and holds no formula of its own.
//!
//! **What DiaCraft does with them:** nothing. Its own routing takes a connector by its route marker and its glue, and reads neither cell, so a file read back routes as the
//! drawing did. The cells a connector holds are no reason to route it.

use diacraft_doc::{Cell, CellAddr, CellEdit, CellWrite, FormulaSlot, OdfShape, Shape, SheetRef, Value, WriteOrigin};

use super::obstacle::{ROUTABLE, flags_of, obj_kind};
use super::read::Reader;

/// The cell that says what layout and routing take a shape for.
pub(crate) const OBJ_TYPE: &str = "ObjType";

/// The cell that says what glue a one-dimensional shape may make.
pub(crate) const GLUE_TYPE: &str = "GlueType";

/// `GlueType` 2: dynamic glue, shape to shape, is allowed.
const DYNAMIC_GLUE: u8 = 2;

/// The two cells of a connector DiaCraft makes: `ObjType` 2, routable, and `GlueType` 2, dynamic glue allowed.
pub(crate) fn new_cells() -> [(&'static str, Cell); 2] {
    [(OBJ_TYPE, plain(ROUTABLE)), (GLUE_TYPE, plain(DYNAMIC_GLUE))]
}

/// A plain whole number, as the cells of a byte hold it.
fn plain(n: u8) -> Cell {
    Cell::constant(Value::from_text(&n.to_string()))
}

/// What a connector of DiaCraft's lacks of the two cells: the `ObjType` it is to hold (the flags it reads and 2), and whether it is to hold `GlueType` 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lack {
    pub obj_type: u8,
    pub glue_type: bool,
}

/// The rule on the numbers alone: `obj_type` and `glue_type` are what the shape reads for each cell, and `*_formula` whether its own cell holds a formula. **`None`** when `ObjType` says
/// something already (flag 1, 2 or 4, read by mask) or holds a formula of its own; **else** the flags it holds with 2 (a group's 8 is kept, as keeps it), and
/// `GlueType` 2 where it reads 0 and holds no formula of its own.
pub(crate) fn lack(obj_type: f64, obj_formula: bool, glue_type: f64, glue_formula: bool) -> Option<Lack> {
    let flags = flags_of(obj_type);
    if obj_formula || !obj_kind(flags).undecided {
        return None;
    }
    Some(Lack { obj_type: flags | ROUTABLE, glue_type: !glue_formula && flags_of(glue_type) == 0 })
}

/// What the shape at `sheet` lacks: `None` for a shape that is not a connector of DiaCraft's (one-dimensional with the route marker), and for one that lacks nothing ([`lack`]).
pub(crate) fn lack_of(reader: &Reader<'_>, sheet: &SheetRef) -> Option<Lack> {
    if !reader.is_one_d(sheet) || !reader.is_routed(sheet) {
        return None;
    }
    let shape = match sheet {
        SheetRef::Shape(at) => reader.doc().shape(at)?,
        _ => return None,
    };
    let has_formula = |name: &str| shape.sheet.cell(name).is_some_and(|c| !matches!(c.formula, FormulaSlot::Absent));
    lack(reader.number(sheet, OBJ_TYPE), has_formula(OBJ_TYPE), reader.number(sheet, GLUE_TYPE), has_formula(GLUE_TYPE))
}

/// What a connector at `sheet` that DiaCraft routes again lacks ([`lack_of`]), **save for one an `.odg` file holds** (it holds the `.odg`'s data of its element): the cells say nothing to
/// LibreOffice and nothing in an `.odg` says them, so a connector of an `.odg` file is left as the file has it, and a save across to a VSDX type gives them (`across/onto.rs`).
pub(crate) fn lack_when_routed(reader: &Reader<'_>, sheet: &SheetRef) -> Option<Lack> {
    let held_by_odg = match sheet {
        SheetRef::Shape(at) => reader.doc().shape(at).is_some_and(|shape| shape.ext.get().is_some_and(|data| data.odf != OdfShape::default())),
        _ => false,
    };
    if held_by_odg { None } else { lack_of(reader, sheet) }
}

/// The writes that give the shape at `sheet` what it lacks, as a drawing's own writes (a cell a `GUARD` protects is kept).
pub(crate) fn writes(sheet: &SheetRef, lack: Lack) -> Vec<(SheetRef, CellWrite)> {
    let write = |name: &str, n: u8| {
        (
            sheet.clone(),
            CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Value(Value::from_text(&n.to_string())), origin: WriteOrigin::Drawing },
        )
    };
    let mut out = vec![write(OBJ_TYPE, lack.obj_type)];
    if lack.glue_type {
        out.push(write(GLUE_TYPE, DYNAMIC_GLUE));
    }
    out
}

/// `shape` given what it lacks, as a writer is given it; `None` when its sheet takes no more cells (it is then written as it is).
pub(crate) fn dressed(shape: &Shape, lack: Lack) -> Option<Shape> {
    let mut own = Shape::clone(shape);
    let mut counted = own.counted_sheet();
    counted.set_cell(&CellAddr::direct(OBJ_TYPE), plain(lack.obj_type)).ok()?;
    if lack.glue_type {
        counted.set_cell(&CellAddr::direct(GLUE_TYPE), plain(DYNAMIC_GLUE)).ok()?;
    }
    Some(own)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_connector_that_reads_no_placement_is_given_the_routable_flag_and_dynamic_glue() {
        assert_eq!(lack(0.0, false, 0.0, false), Some(Lack { obj_type: 2, glue_type: true }));
        // A group's flag is kept (the flags add up).
        assert_eq!(lack(8.0, false, 0.0, false), Some(Lack { obj_type: 10, glue_type: true }));
        // What is not a byte reads 0 (`flags_of`), so it is given 2.
        for odd in [f64::NAN, -1.0, 300.0, f64::INFINITY] {
            assert_eq!(lack(odd, false, 0.0, false), Some(Lack { obj_type: 2, glue_type: true }), "{odd}");
        }
    }

    #[test]
    fn a_placement_said_already_or_a_formula_of_its_own_is_kept() {
        // 1 placeable, 2 routable, 4 neither, and their sums: a choice made already.
        for held in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 9.0, 10.0, 12.0, 14.0] {
            assert_eq!(lack(held, false, 0.0, false), None, "ObjType {held}");
        }
        assert_eq!(lack(0.0, true, 0.0, false), None, "a formula of its own in ObjType");
    }

    #[test]
    fn glue_type_is_written_only_where_it_reads_zero_and_holds_no_formula() {
        assert_eq!(lack(0.0, false, 4.0, false), Some(Lack { obj_type: 2, glue_type: false }), "4 forbids dynamic glue: a choice kept");
        assert_eq!(lack(0.0, false, 2.0, false), Some(Lack { obj_type: 2, glue_type: false }), "2 already");
        assert_eq!(lack(0.0, false, 0.0, true), Some(Lack { obj_type: 2, glue_type: false }), "a formula of its own");
    }

    #[test]
    fn a_new_connector_holds_two_plain_cells_of_two() {
        let cells = new_cells();
        assert_eq!(cells.iter().map(|(name, _)| *name).collect::<Vec<_>>(), ["ObjType", "GlueType"]);
        for (name, cell) in cells {
            assert!(matches!(cell.formula, FormulaSlot::Absent), "{name}");
            assert_eq!((cell.value.as_ref().and_then(Value::number), cell.unit.as_deref()), (Some(2.0), None), "{name}");
        }
    }

    #[test]
    fn the_writes_and_the_dress_give_what_is_lacked_and_nothing_else() {
        let sheet = SheetRef::Page(1);
        let names = |lack: Lack| writes(&sheet, lack).into_iter().map(|(_, w)| w.addr).collect::<Vec<_>>();
        assert_eq!(names(Lack { obj_type: 10, glue_type: true }), [CellAddr::direct("ObjType"), CellAddr::direct("GlueType")]);
        assert_eq!(names(Lack { obj_type: 2, glue_type: false }), [CellAddr::direct("ObjType")]);
        let shape = Shape::new(4);
        let dressed = dressed(&shape, Lack { obj_type: 10, glue_type: false }).unwrap();
        assert_eq!(dressed.sheet.cell("ObjType").and_then(|c| c.value.as_ref()).and_then(Value::number), Some(10.0));
        assert!(dressed.sheet.cell("GlueType").is_none());
    }
}
