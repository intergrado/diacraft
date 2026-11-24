//! The pin of a dropped line is where `LocPinX` of the width puts it and not always the middle of its ends, and what reads the
//! pin is worked out again. Packages are built in code.

use diacraft_doc::FormulaSlot;
use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::stencil_open::{CT_STENCIL, Dir, ids, num, open};
use crate::{blank_session, run};

/// A master of one line 2 long, from (3, 2) to (5, 2), whose `LocPinX` is `loc`. `pin_x` is the cell the master holds for `PinX` (a number or a
/// formula) and `begin_x` the one for `BeginX`; `TxtPinX` reads `PinX`, so a stale pin shows in it.
fn rail(loc: f64, pin_x: &str, begin_x: &str) -> Vec<u8> {
    let line = shape(
        5,
        &format!(
            r#"{pin_x}<Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="{loc}"/><Cell N="LocPinY" V="0"/>
{begin_x}<Cell N="BeginY" V="2"/><Cell N="EndX" V="5" F="BeginX+Width"/><Cell N="EndY" V="2" F="BeginY+Height"/>
<Cell N="TxtPinX" V="3" F="PinX"/>
<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.05"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#
        ),
    );
    VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Rail").attrs(&ids(71, 72)).shapes(&line)).build()
}

/// Whether the page shows ink at (`x`, `y`) inches, the page being 8.5 high and drawn at 100 pixels to the inch.
fn ink_at(d: &diacraft_doc::Document, x: f64, y: f64) -> bool {
    let view = diacraft_render::View::fit_page(d, 1, 100.0).unwrap();
    let r = diacraft_render::Renderer::new().render(d, 1, &view);
    let (w, h) = (r.width as usize, r.height as usize);
    let at = ((((8.5 - y) * 100.0) as usize).min(h - 1) * w + ((x * 100.0) as usize).min(w - 1)) * 4;
    u32::from(r.rgba_premul[at]) + u32::from(r.rgba_premul[at + 1]) + u32::from(r.rgba_premul[at + 2]) < 200
}

fn drop_rail(bytes: &[u8]) -> diacraft_engine::Session {
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("rail.vssx", bytes));
    run(&mut s, "shape.drop", json!({"master": "Rail", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    s
}

#[test]
fn a_line_whose_locpinx_is_nothing_has_its_pin_at_its_begin_and_what_reads_the_pin_follows() {
    // The master holds its pin as numbers (3, 2) and its ends as formulas of it: `BeginX` is `PinX-LocPinX`. The place (6, 4) moves the ends to
    // (5, 4)-(7, 4); a pin at the middle (6, 4) would be wrong, since LocPinX 0 is the begin.
    let s = drop_rail(&rail(0.0, r#"<Cell N="PinX" V="3"/>"#, r#"<Cell N="BeginX" V="3" F="PinX-LocPinX"/>"#));
    let d = s.document().unwrap();
    let id = d.page(1).unwrap().shapes.get(0).unwrap().id;
    assert_eq!((num(d, 1, id, "BeginX"), num(d, 1, id, "EndX")), (5.0, 7.0));
    assert_eq!((num(d, 1, id, "BeginY"), num(d, 1, id, "EndY")), (4.0, 4.0));
    assert_eq!((num(d, 1, id, "PinX"), num(d, 1, id, "PinY")), (5.0, 4.0), "the pin is the begin, not the middle");
    assert_eq!(num(d, 1, id, "TxtPinX"), 5.0, "what reads PinX is worked out after the pin is set");
    // The line is drawn from the pin: from x 5 to 7 at y 4.
    assert!(ink_at(d, 5.5, 4.0) && ink_at(d, 6.5, 4.0), "the line is on the page from 5 to 7");
    assert!(!ink_at(d, 7.5, 4.0) && !ink_at(d, 4.5, 4.0), "and not from the middle (6 to 8)");
}

#[test]
fn a_pin_a_quarter_along_is_a_quarter_along_and_a_pin_at_half_the_width_is_the_middle() {
    // LocPinX 0.5 of a width of 2: a quarter of the way from (5, 4) to (7, 4) is (5.5, 4).
    let s = drop_rail(&rail(0.5, r#"<Cell N="PinX" V="3"/>"#, r#"<Cell N="BeginX" V="3" F="PinX-LocPinX"/>"#));
    let d = s.document().unwrap();
    let id = d.page(1).unwrap().shapes.get(0).unwrap().id;
    assert_eq!((num(d, 1, id, "BeginX"), num(d, 1, id, "EndX")), (5.0, 7.0));
    assert_eq!((num(d, 1, id, "PinX"), num(d, 1, id, "PinY")), (5.5, 4.0));
    assert_eq!(num(d, 1, id, "TxtPinX"), 5.5);
    assert!(ink_at(d, 5.2, 4.0) && ink_at(d, 6.8, 4.0), "the ends stay where they were put");
    // LocPinX 1 of 2: the middle, as before.
    let s = drop_rail(&rail(1.0, r#"<Cell N="PinX" V="3"/>"#, r#"<Cell N="BeginX" V="3" F="PinX-LocPinX"/>"#));
    let d = s.document().unwrap();
    let id = d.page(1).unwrap().shapes.get(0).unwrap().id;
    assert_eq!((num(d, 1, id, "PinX"), num(d, 1, id, "PinY")), (6.0, 4.0));
    assert_eq!(num(d, 1, id, "TxtPinX"), 6.0);
}

#[test]
fn a_pin_the_master_holds_as_a_formula_is_not_overwritten() {
    // PinX is `BeginX+LocPinX`, a formula of the ends: the recalculation that follows the ends has it right, and it is the master's, so it stays
    // inherited. (PinY is a number and is set where the rule puts it: the begin's y, 4.)
    let s = drop_rail(&rail(0.0, r#"<Cell N="PinX" V="3" F="BeginX+LocPinX"/>"#, r#"<Cell N="BeginX" V="3"/>"#));
    let d = s.document().unwrap();
    let inst = d.page(1).unwrap().shapes.get(0).unwrap();
    assert!(inst.sheet.cells.get("PinX").is_none_or(|c| matches!(c.formula, FormulaSlot::Inherit)), "the pin stays the master's formula");
    assert_eq!((num(d, 1, inst.id, "PinX"), num(d, 1, inst.id, "PinY")), (5.0, 4.0));
    assert_eq!(num(d, 1, inst.id, "TxtPinX"), 5.0);
}

#[test]
fn a_line_among_the_members_of_a_dropped_group_with_locpinx_nothing_has_its_pin_at_its_begin() {
    // A box 2 by 1 at (10..12, 5..6) and a line from (12, 6) to (14, 6) whose pin is the number 12 and whose LocPinX is 0. The group's frame has its
    // origin at (10, 5), so the line runs from (2, 1) to (4, 1) and its pin is its begin (2, 1), not the middle (3, 1).
    let a = shape(
        5,
        r#"<Cell N="PinX" V="11"/><Cell N="PinY" V="5.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="2"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let line = shape(
        6,
        r#"<Cell N="BeginX" V="12"/><Cell N="BeginY" V="6"/><Cell N="EndX" V="14"/><Cell N="EndY" V="6"/>
<Cell N="PinX" V="12"/><Cell N="PinY" V="6"/><Cell N="Width" V="2"/><Cell N="Height" V="0"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/>
<Cell N="TxtPinX" V="12" F="PinX"/>
<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.05"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let bytes =
        VsdxBuilder::new().document_type(CT_STENCIL).master(MasterSpec::new(1, "Rail").attrs(&ids(73, 74)).shapes(&format!("{a}{line}"))).build();
    let dir = Dir::new();
    let mut s = blank_session();
    open(&mut s, &dir.write("rail.vssx", &bytes));
    run(&mut s, "shape.drop", json!({"master": "Rail", "stencil": "file-1", "page": 1, "x": 6.0, "y": 4.0}));
    let d = s.document().unwrap();
    let group = d.page(1).unwrap().shapes.get(0).unwrap();
    let line_id = group.children.get(1).unwrap().id;
    assert_eq!((num(d, 1, line_id, "BeginX"), num(d, 1, line_id, "EndX")), (2.0, 4.0));
    assert_eq!((num(d, 1, line_id, "PinX"), num(d, 1, line_id, "PinY")), (2.0, 1.0), "the begin in the group's frame");
    assert_eq!(num(d, 1, line_id, "TxtPinX"), 2.0, "what reads the member's pin is worked out after it is set");
    // The group's box is centred on (6, 4): its origin is (4, 3.5), so the line is at y 4.5 from x 6 to 8.
    assert!(ink_at(d, 6.5, 4.5) && ink_at(d, 7.5, 4.5), "the line is on the page from 6 to 8");
    assert!(!ink_at(d, 8.5, 4.5) && !ink_at(d, 5.5, 4.5), "and not from the middle (7 to 9)");
}
