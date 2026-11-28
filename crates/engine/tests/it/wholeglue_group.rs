//! A connector inside a group, glued to the whole of a sibling shape with its other end free, goes the whole way round the four formats (`.odg`, `.diacraft`, VSDX, `.diacraft`, `.odg`).
//!
//! The VSDX the third convert writes holds, for such an end, the glue formulas and the trigger of whole-shape glue and the cells of a connector the application routes. The `.odg` that is
//! written from what that file reads back must be written, and must give back what the file has: the last convert refused it, "a shape that carries tier 2 is not read back as the model
//! has it: a cell of the shape" (a connector outside a group was written).
//!
//! **The cause:** a cell DiaCraft's data carries on a connector that the markup cannot say (the glue formula of an end) is judged by the reader against the markup's own ends, which are
//! the page's, while the model holds the ends of a member of a group in the group's frame. The carried end was taken for one another program had moved, and the file's own end stood: no
//! formula, so the file did not read back as the model had it. A connector outside a group has the page for its frame, and was written. The writer now carries the ends of a member in the
//! page's frame, which the reader takes the group's corner off, as it does the markup's.
//!
//! The drawing is built in code from the vector: a page that holds a group with a rectangle (text "Alder") and a connector of type line glued to the rectangle as a whole
//! (`draw:start-shape` and no glue point), whose other end is free.

use diacraft_doc::{Document, Shape, ShapeList, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::odg_build::{Dir, Drawing};
use crate::run;

/// The rectangle of the vector: a member of the group, with the text "Alder".
const RECTANGLE: &str = r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="r1" xml:id="r1" draw:id="r1" svg:width="4cm" svg:height="2cm" svg:x="3cm" svg:y="10cm"><text:p>Alder</text:p></draw:rect>"#;

/// The connector of the vector: glued to `r1` as a whole (no `draw:start-glue-point`), its end free.
const CONNECTOR: &str = r#"<draw:connector draw:style-name="gr2" draw:text-style-name="P1" draw:layer="layout" draw:name="c1" draw:type="line" draw:start-shape="r1" svg:x1="7cm" svg:y1="11cm" svg:x2="12cm" svg:y2="14cm"><text:p/></draw:connector>"#;

/// Where the two shapes are.
#[derive(Clone, Copy)]
enum Within {
    /// At the top of the page.
    Nothing,
    /// In one group.
    Group,
    /// In a group that is a member of another.
    GroupInGroup,
}

/// The drawing, with the two shapes inside `within`.
fn drawing(within: Within) -> Drawing {
    let body = format!("{RECTANGLE}{CONNECTOR}");
    Drawing::new().page(&match within {
        Within::Nothing => body,
        Within::Group => format!(r#"<draw:g draw:name="G">{body}</draw:g>"#),
        // A shape beside the inner group makes the outer group's corner another than the inner's.
        Within::GroupInGroup => {
            let beside = r#"<draw:rect draw:style-name="gr1" draw:text-style-name="P1" draw:layer="layout" draw:name="r2" xml:id="r2" draw:id="r2" svg:width="2cm" svg:height="2cm" svg:x="1cm" svg:y="6cm"><text:p/></draw:rect>"#;
            format!(r#"<draw:g draw:name="Outer">{beside}<draw:g draw:name="G">{body}</draw:g></draw:g>"#)
        }
    })
}

/// One convert, as the crossing runs it: a session of its own opens `from`, and saves as `to`. → the error's message when the save is refused.
fn convert(dir: &Dir, from: &str, to: &str) -> Result<(), String> {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path(from) }));
    match s.execute("file.save_as", &json!({ "path": dir.path(to) })) {
        Ok(_) => Ok(()),
        Err(EngineError::Failed { msg, .. }) => Err(msg),
        Err(other) => Err(format!("{other:?}")),
    }
}

/// The four converts of the crossing, from `a.odg`; the message of the one that was refused.
fn crossing(dir: &Dir) -> Result<(), String> {
    convert(dir, "a.odg", "a.diacraft").map_err(|e| format!("a.odg to a.diacraft: {e}"))?;
    convert(dir, "a.diacraft", "b.vsdx").map_err(|e| format!("a.diacraft to b.vsdx: {e}"))?;
    convert(dir, "b.vsdx", "c.diacraft").map_err(|e| format!("b.vsdx to c.diacraft: {e}"))?;
    convert(dir, "c.diacraft", "d.odg").map_err(|e| format!("c.diacraft to d.odg: {e}"))
}

/// The model of the file `name`: a session of its own reads it.
fn model(dir: &Dir, name: &str) -> Document {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s.document().unwrap().clone()
}

/// The shape called `name` on the first page, at any depth.
fn named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    fn find<'a>(shapes: &'a ShapeList, name: &str) -> Option<&'a Shape> {
        shapes.iter().find_map(|shape| if shape.name_u.as_deref() == Some(name) { Some(&**shape) } else { find(&shape.children, name) })
    }
    find(&doc.pages.get(0).unwrap().shapes, name).unwrap_or_else(|| panic!("no shape {name}"))
}

/// The formula and the number a cell of `shape` holds.
fn cell_of(shape: &Shape, name: &str) -> (Option<String>, Option<f64>) {
    let cell = shape.sheet.cell(name).unwrap_or_else(|| panic!("no cell {name}"));
    (cell.formula.text().map(str::to_owned), cell.value.as_ref().and_then(Value::number))
}

#[test]
fn a_connector_glued_to_the_whole_of_a_sibling_in_a_group_goes_the_whole_way_round() {
    let dir = Dir::new();
    dir.write("a.odg", &drawing(Within::Group).bytes());
    assert_eq!(crossing(&dir), Ok(()));
}

#[test]
fn the_same_connector_outside_a_group_goes_the_whole_way_round() {
    let dir = Dir::new();
    dir.write("a.odg", &drawing(Within::Nothing).bytes());
    assert_eq!(crossing(&dir), Ok(()));
}

#[test]
fn the_same_connector_in_a_group_in_a_group_goes_the_whole_way_round() {
    let dir = Dir::new();
    dir.write("a.odg", &drawing(Within::GroupInGroup).bytes());
    assert_eq!(crossing(&dir), Ok(()));
}

/// A file that was written and does not give the glue back would be as bad as a refused one: the `.odg` is read again and its connector holds what the `.diacraft` file's holds, the glue
/// formula of the begin with the place it is at in the group's frame, and the trigger that names the rectangle.
#[test]
fn the_odg_that_is_written_gives_the_glue_back_with_its_formulas_and_its_places() {
    for within in [Within::Nothing, Within::Group, Within::GroupInGroup] {
        let dir = Dir::new();
        dir.write("a.odg", &drawing(within).bytes());
        assert_eq!(crossing(&dir), Ok(()));
        let (before, after) = (model(&dir, "c.diacraft"), model(&dir, "d.odg"));
        let (was, is) = (named(&before, "c1"), named(&after, "c1"));
        let glued = |d: &Document| -> Vec<(String, u32)> {
            d.pages.get(0).unwrap().connects.iter().map(|c| (c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_part.unwrap_or(0))).collect()
        };
        assert_eq!(glued(&before), [("BeginX".to_owned(), 3)], "the .diacraft file holds the glue to the whole of the rectangle");
        assert_eq!(glued(&before), glued(&after), "the .odg gives the same glue back");
        for name in ["BeginX", "BeginY", "BegTrigger"] {
            let (formula, value) = cell_of(is, name);
            assert!(formula.is_some(), "{name}: the formula of the glue is there, in the group's frame too");
            assert_eq!(formula, cell_of(was, name).0, "{name}: the formula is the file's");
            // The place is the place the group's frame gives it (to the file's grid of a hundredth of a millimetre).
            let want = cell_of(was, name).1.unwrap();
            assert!((value.unwrap() - want).abs() < 1.0e-3, "{name}: the .odg gives {value:?} and the .diacraft file {want}");
        }
    }
}
