//! Dropping a master on a page of another drawing scale (the range of eight).
//!
//! The 23 vectors of the specification, then every way of dropping a master (a built-in two-dimensional master, a one-dimensional one, a stencil
//! file's master of one shape, of a group and of several top-level shapes, AutoConnect), the answer of the command, one undo step, a save and a reopen in
//! `.diacraft`, VSDX and `.odg`, and hostile scales and masters. Every package is built in code from the specifications' XML;
//! no file from another application is used.
//!
//! **What the vectors count.** Lengths are inches as the cells hold them (drawing units), x right and y up, compared within 1E-9 relative. "Written" is a cell the
//! instance holds itself as a value; "inherits" is a cell it does not hold (a cell that holds only `Inh` and a result a recalculation worked out is inherited).

use diacraft_doc::{Cell, Document, FormulaSlot, Owner, Resolver, SheetRef};
use diacraft_engine::Session;
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::{Value, json};

use crate::edit::{doc, id_of, num};
use crate::group_size::own;
use crate::stencil_open::{CT_STENCIL, Dir, LOOK, box_cells, ids, open};
use crate::{blank_session, check_session, run, snapshot};

// ---- the masters and the pages --------------------------------------------------------------------------------------------------

/// A master page sheet at `paper` inches to `drawing` inches.
fn at_scale(paper: f64, drawing: f64) -> String {
    format!(r#"<Cell N="PageScale" V="{paper}"/><Cell N="DrawingScale" V="{drawing}"/>"#)
}

/// The master `name` (ID `id`) whose page sheet is `sheet` and whose shapes are `shapes`.
fn master(id: u32, name: &str, sheet: &str, shapes: &str) -> MasterSpec {
    let mut m = MasterSpec::new(id, name).attrs(&ids(100 + id, 200 + id)).shapes(shapes);
    m.sheet = Some(sheet.to_owned());
    m
}

/// A box `w` by `h` of one shape (ID 5), as a file writes one, with its page sheet at 1:1.
fn flat(id: u32, name: &str, w: f64, h: f64) -> MasterSpec {
    master(id, name, &at_scale(1.0, 1.0), &shape(5, &format!("{LOOK}{}", box_cells(w, h))))
}

/// The stencil file holding `masters`.
fn stencil_bytes(masters: Vec<MasterSpec>) -> Vec<u8> {
    let mut b = VsdxBuilder::new().document_type(CT_STENCIL).page(PageSpec::new(1, "Page-1").without_part());
    for m in masters {
        b = b.master(m);
    }
    b.build()
}

/// A blank drawing whose page is at `paper` inches to `drawing` inches, with the stencil of `masters` open as `file-1`.
fn world(page: (f64, f64), masters: Vec<MasterSpec>) -> (Session, Dir) {
    let dir = Dir::new();
    let path = dir.write("shapes.vssx", &stencil_bytes(masters));
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"paper": page.0, "drawing": page.1}));
    open(&mut s, &path);
    (s, dir)
}

/// The root style of a file's drawing as the corpus's files hold it: the three flags, and the factor 1.
const ROOT_WITH_FACTOR: &str = r#"<StyleSheet ID="0" NameU="No Style" Name="No Style"><Cell N="EnableFillProps" V="1"/><Cell N="EnableLineProps" V="1"/><Cell N="EnableTextProps" V="1"/><Cell N="DropOnPageScale" V="1"/></StyleSheet>"#;

/// [`world`] for a drawing a file holds, whose root style holds the factor 1 as the root of every file of the corpus does, with the stencil of `masters` open as `file-1`.
fn world_from_file(page: (f64, f64), masters: Vec<MasterSpec>) -> (Session, Dir) {
    let dir = Dir::new();
    let sheet = format!(r#"<Cell N="PageWidth" V="11"/><Cell N="PageHeight" V="8.5"/>{}"#, at_scale(page.0, page.1));
    let drawing = VsdxBuilder::new().styles(ROOT_WITH_FACTOR).page(PageSpec::new(1, "Page-1").sheet(&sheet)).build();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": dir.write("drawing.vsdx", &drawing)}));
    open(&mut s, &dir.write("shapes.vssx", &stencil_bytes(masters)));
    (s, dir)
}

/// Drops `key` of the stencil file with its centre at the paper point (`x`, `y`).
fn drop_at(s: &mut Session, key: &str, (x, y): (f64, f64)) -> Value {
    run(s, "shape.drop", json!({"master": key, "stencil": "file-1", "page": 1, "x": x, "y": y}))
}

/// Drops `key` of the stencil file with its centre at the page's point (`x`, `y`) in drawing units, which is where the vectors say it goes.
fn drop_units(s: &mut Session, key: &str, (x, y): (f64, f64)) -> Value {
    let k = diacraft_engine::draw::units_per_inch(doc(s), 1);
    drop_at(s, key, (x / k, y / k))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0e-9 * a.abs().max(b.abs()).max(1.0e-300)
}

#[track_caller]
fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

/// What the instance holds of its own for `name`, as a plain value: `Some` when the cell is there with no formula and no `Inh`.
fn written(d: &Document, id: u32, name: &str) -> Option<f64> {
    let cell = own(d, id, name)?;
    if matches!(cell.formula, FormulaSlot::Absent) { cell.value.as_ref()?.number() } else { None }
}

/// Whether the instance reads `name` from its master: it holds no value or formula of its own (an `Inh` cell with a result is still inherited).
fn inherits(d: &Document, id: u32, name: &str) -> bool {
    own(d, id, name).is_none_or(|c: Cell| matches!(c.formula, FormulaSlot::Inherit))
}

/// The three cells a drop beyond the range writes, by what the instance holds: (`Width`, `Height`, `DropOnPageScale`).
fn written_three(d: &Document, id: u32) -> (Option<f64>, Option<f64>, Option<f64>) {
    (written(d, id, "Width"), written(d, id, "Height"), written(d, id, "DropOnPageScale"))
}

/// Nothing written of the three, and the first two read from the master.
#[track_caller]
fn assert_unscaled(d: &Document, id: u32) {
    assert_eq!(written_three(d, id), (None, None, None), "nothing is written");
    assert!(inherits(d, id, "Width") && inherits(d, id, "Height"), "the size is the master's");
    assert!(own(d, id, "DropOnPageScale").is_none(), "no DropOnPageScale of its own");
}

/// The answer's size on paper, in inches.
fn paper_size(r: &Value) -> (f64, f64) {
    (r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap())
}

/// The box of a shape on the page, from its four corners: (width along x, height along y, centre).
fn page_box(d: &Document, id: u32) -> (f64, f64, (f64, f64)) {
    let c = diacraft_engine::draw::shape_corners(d, 1, id).unwrap();
    let (xs, ys): (Vec<f64>, Vec<f64>) = (c.iter().map(|p| p.0).collect(), c.iter().map(|p| p.1).collect());
    let (x0, x1) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let (y0, y1) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    (x1 - x0, y1 - y0, ((x0 + x1) / 2.0, (y0 + y1) / 2.0))
}

/// The names of the sections the instance holds itself.
fn shape_sections(d: &Document, id: u32) -> Vec<String> {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    d.shape(&at).unwrap().sheet.sections.iter().map(|s| s.name.as_str().to_owned()).collect()
}

/// The `ID`s of the members of an instance, directly below it.
fn members_of(d: &Document, id: u32) -> Vec<u32> {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    d.shape(&at).unwrap().children.iter().map(|m| m.id).collect()
}

/// The scale of the page 1:100 as the vectors write it: 1 centimetre to a metre.
const ONE_TO_100: (f64, f64) = (0.393701, 39.3701);

// ----: which outcome a drop gets ----------------------------------------------------------------------------------------

/// A master at 1:24 on a page at 1:120 is within the range (r = 5): nothing is written and it keeps its drawing size.
#[test]
fn v1_a_master_at_1_to_24_on_a_page_at_1_to_120_keeps_its_drawing_size() {
    let table = master(1, "Table", &at_scale(0.5, 12.0), &shape(5, &format!("{LOOK}{}", box_cells(48.0, 30.0))));
    // The drawing is a file's, whose root style holds the factor 1: the shape reads it.
    let (mut s, _dir) = world_from_file((1.0, 120.0), vec![table]);
    let r = drop_units(&mut s, "Table", (240.0, 240.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert_near("Width", num(d, id, "Width"), 48.0);
    let reads = run(&mut s, "sheet.evaluate", json!({"id": id, "page": 1, "formula": "DropOnPageScale"}));
    assert_near("DropOnPageScale", reads["value"].as_f64().unwrap(), 1.0);
    let (w, h) = paper_size(&r);
    assert_near("paper width", w, 0.4);
    assert_near("paper height", h, 0.25);
    assert!(r.get("drop_on_page_scale").is_none(), "{r}");
    assert_eq!(check_session(&s), Ok(()));
}

/// In a drawing whose root holds the factor 1, as the root of every file does, a shape no drop scaled reads 1 in a formula, so a formula of a master that
/// names it is the master's own size; and a shape dropped beyond the range reads its own factor.
#[test]
fn a_shape_no_drop_scaled_reads_a_factor_of_1_in_a_formula() {
    let (mut s, _dir) = world_from_file((1.0, 4.0), vec![flat(1, "One", 1.0, 1.0)]);
    let id = id_of(&drop_at(&mut s, "One", (5.0, 4.0)));
    let mut value = |formula: &str| run(&mut s, "sheet.evaluate", json!({"id": id, "page": 1, "formula": formula}))["value"].as_f64().unwrap();
    assert_near("DropOnPageScale*3", value("DropOnPageScale*3"), 3.0);
    assert_near("0.11 in*DropOnPageScale", value("0.11 in*DropOnPageScale"), 0.11);
    let (mut s, _dir) = world_from_file((1.0, 100.0), vec![flat(1, "One", 1.0, 1.0)]);
    let id = id_of(&drop_at(&mut s, "One", (5.0, 4.0)));
    let value = run(&mut s, "sheet.evaluate", json!({"id": id, "page": 1, "formula": "DropOnPageScale*3"}))["value"].as_f64().unwrap();
    assert_near("a shape dropped at r = 100 reads its own factor", value, 300.0);
}

/// The same master on a page at 1:240 is beyond it (r = 10): `DropOnPageScale` 10 as a plain value, `Width` and `Height` times 10, the text size still the master's.
#[test]
fn v2_the_same_master_at_1_to_240_is_scaled_to_keep_its_paper_size_and_its_text_stays() {
    let inner = format!(r#"{LOOK}{}<Section N="Character"><Row IX="0"><Cell N="Size" V="0.1111111" U="PT"/></Row></Section>"#, box_cells(48.0, 30.0));
    let table = master(1, "Table", &at_scale(0.5, 12.0), &shape(5, &inner));
    let (mut s, _dir) = world((1.0, 240.0), vec![table]);
    let r = drop_units(&mut s, "Table", (480.0, 480.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, h, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("DropOnPageScale is written"), 10.0);
    assert_near("Width", w.expect("Width is written"), 480.0);
    assert_near("Height", h.expect("Height is written"), 300.0);
    let factor = own(d, id, "DropOnPageScale").unwrap();
    assert!(factor.formula.text().is_none() && factor.unit.is_none(), "a plain value with no formula and no unit: {factor:?}");
    let (pw, ph) = paper_size(&r);
    assert_near("paper width", pw, 2.0);
    assert_near("paper height", ph, 1.25);
    assert_near("the answer's factor", r["drop_on_page_scale"].as_f64().unwrap(), 10.0);
    // The text size is the master's: the instance holds no `Character` section.
    assert!(!shape_sections(d, id).contains(&"Character".to_owned()), "{:?}", shape_sections(d, id));
    assert_eq!(check_session(&s), Ok(()));
}

/// A master at 1:1 on a page at 1:100 is antiscaled; no line weight or text size is written.
#[test]
fn v3_a_one_to_one_master_on_a_page_at_1_to_100_is_a_hundred_times_the_master() {
    let inner = format!(
        r#"{LOOK}{}<Cell N="LineWeight" V="0.01"/><Section N="Character"><Row IX="0"><Cell N="Size" V="0.125"/></Row></Section>"#,
        box_cells(1.0, 0.5)
    );
    let rect = master(1, "Rect", &at_scale(1.0, 1.0), &shape(5, &inner));
    let (mut s, _dir) = world(ONE_TO_100, vec![rect]);
    let r = drop_units(&mut s, "Rect", (500.0, 500.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, h, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("DropOnPageScale is written"), 100.0);
    assert_near("Width", w.expect("Width is written"), 100.0);
    assert_near("Height", h.expect("Height is written"), 50.0);
    let (pw, ph) = paper_size(&r);
    assert_near("paper width", pw, 1.0);
    assert_near("paper height", ph, 0.5);
    assert!(own(d, id, "LineWeight").is_none() && !shape_sections(d, id).contains(&"Character".to_owned()), "no line weight or text size written");
    assert_eq!(check_session(&s), Ok(()));
}

/// A master at 1:50 on a page at 1:1 has r = 0.02, so the desk shrinks to a fiftieth.
#[test]
fn v4_a_master_at_1_to_50_on_an_unscaled_page_is_a_fiftieth() {
    let desk = master(1, "Desk", &at_scale(0.787402, 39.3701), &shape(5, &format!("{LOOK}{}", box_cells(62.99212, 31.49606))));
    let (mut s, _dir) = world((1.0, 1.0), vec![desk]);
    let r = drop_at(&mut s, "Desk", (5.0, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, h, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("DropOnPageScale is written"), 0.02);
    assert_near("Width", w.expect("Width is written"), 1.259_842_4);
    assert_near("Height", h.expect("Height is written"), 0.629_921_2);
}

/// A ratio of 8 is within the range, the tolerance catching the division that lands a hair above 8.
#[test]
fn v5_a_ratio_of_exactly_eight_is_within_the_range() {
    let m = master(1, "Eighth", &at_scale(1.0, 12.0), &shape(5, &format!("{LOOK}{}", box_cells(6.0, 3.0))));
    let (mut s, _dir) = world((0.125, 12.0), vec![m]);
    let r = drop_units(&mut s, "Eighth", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert_near("Width", num(d, id, "Width"), 6.0);
}

/// The same master on a page at 1:100 has r = 8.333333, beyond the range.
#[test]
fn v6_a_ratio_of_eight_and_a_third_is_beyond_the_range() {
    let m = master(1, "Eighth", &at_scale(1.0, 12.0), &shape(5, &format!("{LOOK}{}", box_cells(6.0, 3.0))));
    let (mut s, _dir) = world(ONE_TO_100, vec![m]);
    let r = drop_units(&mut s, "Eighth", (500.0, 500.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, _, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("DropOnPageScale is written"), 100.0 / 12.0);
    assert_near("Width", w.expect("Width is written"), 50.0);
}

/// The test runs both ways: a page drawn nine times larger than life is beyond the range, eight times within it.
#[test]
fn v7_the_range_is_two_sided_nine_to_one_is_beyond_and_eight_to_one_is_within() {
    let (mut s, _dir) = world((9.0, 1.0), vec![flat(1, "Two", 2.0, 1.0)]);
    let r = drop_at(&mut s, "Two", (5.0, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, _, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("DropOnPageScale is written"), 1.0 / 9.0);
    assert_near("Width", w.expect("Width is written"), 2.0 / 9.0);
    assert_near("two inches on paper", paper_size(&r).0, 2.0);

    let (mut s, _dir) = world((8.0, 1.0), vec![flat(1, "Two", 2.0, 1.0)]);
    let r = drop_at(&mut s, "Two", (5.0, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert_near("sixteen inches on paper", paper_size(&r).0, 16.0);
}

/// A master drawn at one inch to a thousand antiscales on a page at 1:1, and is kept on one within eight times of it.
#[test]
fn v8_a_master_at_one_to_a_thousand_is_scaled_on_usual_pages_and_kept_near_its_own() {
    let title = master(1, "Title", &at_scale(1.0, 1000.0), &shape(5, &format!("{LOOK}{}", box_cells(5000.0, 2000.0))));
    let (mut s, _dir) = world((1.0, 1.0), vec![title.clone()]);
    let r = drop_at(&mut s, "Title", (5.0, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, h, f) = written_three(d, id);
    assert_near("DropOnPageScale", f.expect("written"), 0.001);
    assert_near("Width", w.expect("written"), 5.0);
    assert_near("Height", h.expect("written"), 2.0);

    let (mut s, _dir) = world((1.0, 200.0), vec![title]);
    let r = drop_units(&mut s, "Title", (1000.0, 1000.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert_near("Width", num(d, id, "Width"), 5000.0);
    assert_near("25 inches on paper", paper_size(&r).0, 25.0);
}

// ----: members, ends, text, guards and masters that hold a factor --------------------------------------------------------

/// A group master (the group ID 23, holding `Width` 2 and `Height` 1) with member A (24, sized by the group and holding the group's factor) and member B (25, a plain 0.3).
fn group_23() -> MasterSpec {
    let a = shape(
        24,
        &format!(
            r#"{LOOK}<Cell N="PinX" V="0.5" F="Sheet.23!Width*0.25"/><Cell N="PinY" V="0.5" F="Sheet.23!Height*0.5"/><Cell N="Width" V="0.875" F="Sheet.23!Width*0.4375"/>
<Cell N="Height" V="0.5" F="Sheet.23!Height*0.5"/><Cell N="LocPinX" V="0.4375" F="Width*0.5"/><Cell N="LocPinY" V="0.25" F="Height*0.5"/>
<Cell N="DropOnPageScale" V="1" F="Sheet.23!DropOnPageScale"/>"#
        ),
    );
    let b = shape(
        25,
        &format!(
            r#"{LOOK}<Cell N="PinX" V="1.6"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="0.3"/><Cell N="Height" V="0.3"/><Cell N="LocPinX" V="0.15" F="Width*0.5"/><Cell N="LocPinY" V="0.15" F="Height*0.5"/>"#
        ),
    );
    master(1, "Pair", &at_scale(1.0, 1.0), &shape_with(23, r#" Type="Group""#, &format!("{}<Shapes>{a}{b}</Shapes>", box_cells(2.0, 1.0))))
}

/// A group master dropped at r = 10: the group holds the factor and the new size, A follows by the formulas it inherits, B keeps its plain size.
#[test]
fn v9_a_group_master_scales_the_group_and_its_members_follow_by_their_formulas() {
    let (mut s, _dir) = world((1.0, 10.0), vec![group_23()]);
    let r = drop_units(&mut s, "Pair", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    let (w, h, f) = written_three(d, id);
    assert_near("the group's DropOnPageScale", f.expect("written"), 10.0);
    assert_near("the group's Width", w.expect("written"), 20.0);
    assert_near("the group's Height", h.expect("written"), 10.0);
    let members = members_of(d, id);
    assert_eq!(members.len(), 2, "{members:?}");
    let (a, b) = (members[0], members[1]);
    assert_near("A's Width", num(d, a, "Width"), 8.75);
    assert_near("A's DropOnPageScale", num(d, a, "DropOnPageScale"), 10.0);
    assert_near("B's Width", num(d, b, "Width"), 0.3);
    for m in [a, b] {
        assert!(written(d, m, "DropOnPageScale").is_none() && inherits(d, m, "DropOnPageScale"), "member {m} holds no factor of its own");
    }
    assert_eq!(check_session(&s), Ok(()));
}

/// A one-dimensional master from (0, 0) to (2, 0), as a file writes one.
fn line(id: u32, name: &str, sheet: &str) -> MasterSpec {
    let inner = r#"<Cell N="BeginX" V="0"/><Cell N="BeginY" V="0"/><Cell N="EndX" V="2"/><Cell N="EndY" V="0"/>
<Cell N="PinX" V="1" F="(BeginX+EndX)/2"/><Cell N="PinY" V="0" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="2" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/>
<Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/><Cell N="LinePattern" V="1"/><Cell N="LineColor" V="0"/><Cell N="LineWeight" V="0.01"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row></Section>"#;
    master(id, name, sheet, &shape(5, inner))
}

/// The two ends of a one-dimensional shape, as its cells hold them.
fn ends(d: &Document, id: u32) -> [f64; 4] {
    ["BeginX", "BeginY", "EndX", "EndY"].map(|n| num(d, id, n))
}

/// A one-dimensional master of length 2 centred on the drop point: its ends are unscaled within the range and a length times r apart beyond it.
#[test]
fn v10_a_one_dimensional_master_has_its_length_scaled_about_the_drop_point() {
    let (mut s, _dir) = world((1.0, 4.0), vec![line(1, "Link", &at_scale(1.0, 1.0))]);
    let r = drop_units(&mut s, "Link", (30.0, 40.0));
    let d = doc(&s);
    let id = id_of(&r);
    let e = ends(d, id);
    for (got, want) in e.iter().zip([29.0, 40.0, 31.0, 40.0]) {
        assert_near("an end at r = 4", *got, want);
    }
    assert!(written(d, id, "DropOnPageScale").is_none(), "within the range nothing is written");

    let (mut s, _dir) = world((1.0, 10.0), vec![line(1, "Link", &at_scale(1.0, 1.0))]);
    let r = drop_units(&mut s, "Link", (30.0, 40.0));
    let d = doc(&s);
    let id = id_of(&r);
    for (got, want) in ends(d, id).iter().zip([20.0, 40.0, 40.0, 40.0]) {
        assert_near("an end at r = 10", *got, want);
    }
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 10.0);
    assert_near("Width follows the ends", num(d, id, "Width"), 20.0);
    assert_near("two inches on paper", paper_size(&r).0, 2.0);
    assert_eq!(check_session(&s), Ok(()));
}

/// The text size and the line weight are never written, within the range or beyond it.
#[test]
fn v11_the_text_size_and_the_line_weight_are_never_written() {
    let inner = format!(
        r#"{LOOK}{}<Cell N="LineWeight" V="0.0138889"/><Section N="Character"><Row IX="0"><Cell N="Size" V="0.1388889" U="PT"/></Row></Section>"#,
        box_cells(2.0, 1.0)
    );
    let m = master(1, "Boxed", &at_scale(1.0, 1.0), &shape(5, &inner));
    for (drawing, scaled, width_on_paper) in [(4.0, false, 0.5), (100.0, true, 2.0)] {
        let (mut s, _dir) = world((1.0, drawing), vec![m.clone()]);
        let r = drop_at(&mut s, "Boxed", (5.0, 4.0));
        let (d, id) = (doc(&s), id_of(&r));
        assert_eq!(written(d, id, "Width").is_some(), scaled, "at 1:{drawing}");
        assert!(own(d, id, "LineWeight").is_none() && !shape_sections(d, id).contains(&"Character".to_owned()), "at 1:{drawing}");
        assert_near("the width on paper", paper_size(&r).0, width_on_paper);
    }
}

/// A guarded `Width` stays inherited and is recalculated; a plain `Height` is written; a connection point that names the factor follows it.
#[test]
fn v12_a_guarded_size_formula_stays_inherited_and_is_recalculated() {
    let inner = format!(
        r#"{LOOK}<Cell N="Width" V="0.73" F="GUARD(0.73 in*DropOnPageScale)"/><Cell N="Height" V="0.4"/><Cell N="PinX" V="0.365" F="Width*0.5"/><Cell N="PinY" V="0.2" F="Height*0.5"/>
<Cell N="LocPinX" V="0.365" F="Width*0.5"/><Cell N="LocPinY" V="0.2" F="Height*0.5"/>
<Section N="Connection"><Row IX="0"><Cell N="X" V="0.11" F="0.11 in*DropOnPageScale"/><Cell N="Y" V="0.2"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row></Section>"#
    );
    let m = master(1, "Guarded", &at_scale(1.0, 1.0), &shape(5, &inner));
    let (mut s, _dir) = world((1.0, 20.0), vec![m]);
    let r = drop_units(&mut s, "Guarded", (100.0, 100.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 20.0);
    assert!(written(d, id, "Width").is_none() && inherits(d, id, "Width"), "the guard keeps the width inherited");
    assert_near("Width on recalculation", num(d, id, "Width"), 14.6);
    assert_near("Height", written(d, id, "Height").expect("written"), 8.0);
    let res = Resolver::new(d);
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let rows = res.effective_rows(&SheetRef::Shape(at), &diacraft_doc::SectionKey::new("Connection", None)).unwrap();
    let x = rows[0].cells.iter().find(|(n, _)| n.as_str() == "X").and_then(|(_, c)| c.value.cell?.value.as_ref()?.number()).unwrap();
    assert_near("the connection point's X", x, 2.2);
    assert_eq!(check_session(&s), Ok(()));
}

/// A master at 1:12 whose shape holds a `DropOnPageScale` of 12.
fn twelve() -> MasterSpec {
    let inner = format!(r#"{LOOK}{}<Cell N="DropOnPageScale" V="12"/>"#, box_cells(6.0, 3.0));
    master(1, "Twelve", &at_scale(1.0, 12.0), &shape(5, &inner))
}

/// At its own scale the instance inherits the master's factor.
#[test]
fn v13_a_master_that_holds_a_factor_is_inherited_at_its_own_scale() {
    let (mut s, _dir) = world((1.0, 12.0), vec![twelve()]);
    let r = drop_units(&mut s, "Twelve", (60.0, 60.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert_near("DropOnPageScale", num(d, id, "DropOnPageScale"), 12.0);
}

/// Beyond the range such a master gets this drop's ratio, not the ratio times its own factor.
#[test]
fn v14_a_master_that_holds_a_factor_gets_the_ratio_of_this_drop() {
    let (mut s, _dir) = world((1.0, 1.0), vec![twelve()]);
    let r = drop_at(&mut s, "Twelve", (5.0, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 1.0 / 12.0);
}

// ----: where the scales come from ---------------------------------------------------------------------------------------

/// A master page sheet with a `PageScale` of 0, or with neither cell, reads as one inch; no division by 0 and no refusal.
#[test]
fn v15_an_unusable_master_scale_reads_as_one_inch() {
    let shapes = shape(5, &format!("{LOOK}{}", box_cells(1.0, 1.0)));
    let zero = master(1, "Zero", r#"<Cell N="PageScale" V="0"/><Cell N="DrawingScale" V="1"/>"#, &shapes);
    let none = master(2, "None", "", &shapes);
    for key in ["Zero", "None"] {
        let (mut s, _dir) = world((1.0, 1.0), vec![zero.clone(), none.clone()]);
        let r = drop_at(&mut s, key, (5.0, 4.0));
        assert_unscaled(doc(&s), id_of(&r));
        let (mut s, _dir) = world(ONE_TO_100, vec![zero.clone(), none.clone()]);
        let r = drop_at(&mut s, key, (5.0, 4.0));
        let (d, id) = (doc(&s), id_of(&r));
        assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
        assert_near("Width", written(d, id, "Width").expect("written"), 100.0);
    }
}

/// A ratio that makes a width that is not a number drops the master unscaled, with words, and no crash.
#[test]
fn v16_a_ratio_that_would_make_an_infinite_width_drops_the_master_unscaled_with_words() {
    // The ratio of 1E300 from the master's side: a master page of 1E300 inches to an inch, and a width of 1E10.
    let big = master(1, "Big", &at_scale(1.0e300, 1.0), &shape(5, &format!("{LOOK}{}", box_cells(1.0e10, 1.0))));
    let (mut s, _dir) = world((1.0, 1.0), vec![big.clone()]);
    let before = snapshot(&s);
    let r = drop_at(&mut s, "Big", (0.0, 0.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_unscaled(d, id);
    assert!(r.get("drop_on_page_scale").is_none(), "{r}");
    let notes = r["notes"].as_array().expect("words say why").iter().map(|n| n.as_str().unwrap().to_owned()).collect::<Vec<_>>().join(" ");
    assert!(notes.contains("not scaled") && notes.contains("1e300"), "{notes}");
    assert_ne!(snapshot(&s).document, before.document, "the shape is dropped");
    assert_eq!(check_session(&s), Ok(()));

    // And from the page's side: a page of 1E300 inches to an inch.
    let (mut s, _dir) = world((1.0, 1.0), vec![big]);
    run(&mut s, "sheet.set_cell", json!({"page": 1, "cell": "DrawingScale", "formula": "1E300"}));
    let r = drop_at(&mut s, "Big", (0.0, 0.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert!(written(d, id, "DropOnPageScale").is_none() && written(d, id, "Width").is_none());
    assert_eq!(check_session(&s), Ok(()));
}

/// The target is the page the drop lands on, a background page with a scale of its own included.
#[test]
fn v17_a_background_page_is_the_target_with_its_own_scale() {
    let (mut s, _dir) = world((1.0, 1.0), vec![flat(1, "One", 1.0, 1.0)]);
    let back = run(&mut s, "page.add", json!({"background": true}))["id"].as_u64().unwrap();
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 50.0, "page": back}));
    let r = run(&mut s, "shape.drop", json!({"master": "One", "stencil": "file-1", "page": back, "x": 2.0, "y": 2.0}));
    let id = id_of(&r);
    let d = doc(&s);
    let at = d.find_shape(Owner::Page(u32::try_from(back).unwrap()), id).expect("the shape is on the background page");
    let cell = |name: &str| d.shape(&at).unwrap().sheet.cells.get(&diacraft_doc::CellName::new(name)).and_then(|c| c.value.as_ref()?.number());
    assert_near("DropOnPageScale", cell("DropOnPageScale").expect("written"), 50.0);
    assert_near("Width", cell("Width").expect("written"), 50.0);
}

// ----: what is not a drop -------------------------------------------------------------------------------------------------

/// A paste, a duplicate or a copy to a page of another scale keeps the cells the shape has.
#[test]
fn v18_a_paste_and_a_duplicate_keep_their_cells() {
    let (mut s, _dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    let page2 = run(&mut s, "page.add", json!({"name": "Plain"}))["id"].as_u64().unwrap();
    let r = drop_at(&mut s, "Rect", (5.0, 4.0));
    let id = id_of(&r);
    run(&mut s, "edit.copy", json!({"ids": [id], "page": 1}));
    let pasted = run(&mut s, "edit.paste", json!({"page": page2, "x": 3.0, "y": 3.0}));
    let new = pasted["ids"][0].as_u64().map(|n| u32::try_from(n).unwrap()).unwrap();
    let d = doc(&s);
    let at = d.find_shape(Owner::Page(u32::try_from(page2).unwrap()), new).unwrap();
    let cell = |name: &str| d.shape(&at).unwrap().sheet.cells.get(&diacraft_doc::CellName::new(name)).and_then(|c| c.value.as_ref()?.number());
    assert_near("pasted Width", cell("Width").expect("kept"), 100.0);
    assert_near("pasted DropOnPageScale", cell("DropOnPageScale").expect("kept"), 100.0);
    let dup = run(&mut s, "shape.duplicate", json!({"ids": [id], "page": 1}));
    let copy = dup["ids"][0].as_u64().map(|n| u32::try_from(n).unwrap()).unwrap();
    let d = doc(&s);
    assert_near("duplicate Width", written(d, copy, "Width").expect("kept"), 100.0);
    assert_near("duplicate DropOnPageScale", written(d, copy, "DropOnPageScale").expect("kept"), 100.0);
}

/// A later change of the page's scale rewrites no cell.
#[test]
fn v19_a_later_change_of_the_pages_scale_rewrites_nothing() {
    let (mut s, _dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    let r = drop_at(&mut s, "Rect", (5.0, 4.0));
    let id = id_of(&r);
    run(&mut s, "page.scale", json!({"paper": 0.393701, "drawing": 19.68505}));
    let d = doc(&s);
    assert_near("Width", written(d, id, "Width").expect("kept"), 100.0);
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("kept"), 100.0);
}

/// A drop into a group that sits on the page compares the master with the page's scale, and the group's size is no scale.
#[test]
fn v20_a_drop_over_a_group_uses_the_pages_scale_and_not_the_groups_size() {
    let (mut s, _dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 4.0})));
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let r = drop_at(&mut s, "Rect", (5.5, 4.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_ne!(id, g);
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
    assert_near("Width", written(d, id, "Width").expect("written"), 100.0);
}

// ----: several top-level shapes, an unguarded formula, constants ----------------------------------------------------------

/// A box of the master part with its pin at (`x`, `y`): 1 by 1, the geometry holding the size.
fn pinned_box(id: u32, x: f64, y: f64) -> String {
    shape(
        id,
        &format!(
            r#"{LOOK}<Cell N="PinX" V="{x}"/><Cell N="PinY" V="{y}"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="1" F="Width"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row><Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#
        ),
    )
}

/// A master of two top-level shapes dropped beyond the range: the instance and the members that stand for them hold the factor, sizes and offsets scale alike.
#[test]
fn v21_a_master_of_two_top_level_shapes_scales_its_members_sizes_and_offsets() {
    let m = master(1, "Couple", &at_scale(1.0, 1.0), &format!("{}{}", pinned_box(5, 1.0, 1.0), pinned_box(6, 3.0, 1.0)));
    // Within the range the pair is as it was: members 2 inches apart, centred at the drop point.
    let (mut s, _dir) = world((1.0, 4.0), vec![m.clone()]);
    let r = drop_units(&mut s, "Couple", (40.0, 40.0));
    let d = doc(&s);
    let id = id_of(&r);
    let kids = members_of(d, id);
    let (pa, pb) = (page_box(d, kids[0]), page_box(d, kids[1]));
    assert_near("A and B 2 apart", pb.2.0 - pa.2.0, 2.0);
    assert!(written(d, id, "DropOnPageScale").is_none() && kids.iter().all(|k| written(d, *k, "DropOnPageScale").is_none()));

    let (mut s, _dir) = world((1.0, 10.0), vec![m]);
    let r = drop_units(&mut s, "Couple", (100.0, 100.0));
    let d = doc(&s);
    let id = id_of(&r);
    assert_near("the instance's factor", written(d, id, "DropOnPageScale").expect("written"), 10.0);
    let kids = members_of(d, id);
    assert_eq!(kids.len(), 2);
    for k in &kids {
        assert_near("a member's factor", written(d, *k, "DropOnPageScale").expect("a member that stands for a top-level shape holds it"), 10.0);
        assert_near("a member's Width", num(d, *k, "Width"), 10.0);
    }
    let (pa, pb) = (page_box(d, kids[0]), page_box(d, kids[1]));
    assert_near("A and B 20 apart", pb.2.0 - pa.2.0, 20.0);
    assert_near("centred where the unscaled pair is", (pa.2.0 + pb.2.0) / 2.0, 100.0);
    assert_near("level", pa.2.1, pb.2.1);
    assert_eq!(check_session(&s), Ok(()));
}

/// An unguarded size formula is replaced by the scaled value, as a resize replaces it.
#[test]
fn v22_an_unguarded_size_formula_is_replaced_by_the_scaled_value() {
    let inner = format!(
        r#"{LOOK}<Cell N="Width" V="0.7" F="0.35 in*2"/><Cell N="Height" V="0.6"/><Cell N="PinX" V="0.35" F="Width*0.5"/><Cell N="PinY" V="0.3" F="Height*0.5"/>
<Cell N="LocPinX" V="0.35" F="Width*0.5"/><Cell N="LocPinY" V="0.3" F="Height*0.5"/>"#
    );
    let m = master(1, "Formula", &at_scale(1.0, 1.0), &shape(5, &inner));
    let (mut s, _dir) = world((1.0, 10.0), vec![m]);
    let r = drop_units(&mut s, "Formula", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("Height", written(d, id, "Height").expect("a plain value"), 6.0);
    assert_near("Width", written(d, id, "Width").expect("a plain value, not a formula"), 7.0);
}

/// A constant in a formula or a plain connection point is not scaled; the geometry that reads the width follows it.
#[test]
fn v23_constants_are_not_adjusted() {
    let inner = format!(
        r#"{LOOK}<Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="PinX" V="0.5" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/>
<Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="0.75" F="Width-0.25 in"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection"><Row IX="0"><Cell N="X" V="0.5"/><Cell N="Y" V="0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row></Section>"#
    );
    let m = master(1, "Constants", &at_scale(1.0, 1.0), &shape(5, &inner));
    let (mut s, _dir) = world((1.0, 10.0), vec![m]);
    let r = drop_units(&mut s, "Constants", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("Width", written(d, id, "Width").expect("written"), 10.0);
    assert!(!shape_sections(d, id).contains(&"Connection".to_owned()), "no connection cell written: {:?}", shape_sections(d, id));
    let rows = |section: &str| {
        let res = Resolver::new(d);
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        res.effective_rows(&SheetRef::Shape(at), &diacraft_doc::SectionKey::new(section, if section == "Geometry" { Some(0) } else { None }))
            .unwrap()
            .iter()
            .map(|row| row.cells.iter().find(|(n, _)| n.as_str() == "X").and_then(|(_, c)| c.value.cell?.value.as_ref()?.number()).unwrap())
            .collect::<Vec<f64>>()
    };
    assert_near("the geometry's X on recalculation", rows("Geometry")[1], 9.75);
    assert_near("the connection point's X", rows("Connection")[0], 0.5);
}

// ---- where the box lands, what a lock keeps, Change Shape -------------------------------------------------------------------------

/// A master `w` by `h` whose pin is its lower-left corner (a plain local pin of 0) and that is turned by `angle` radians.
fn corner_pinned(w: f64, h: f64, angle: f64) -> MasterSpec {
    let inner = format!(
        r#"{LOOK}<Cell N="PinX" V="0"/><Cell N="PinY" V="0"/><Cell N="Width" V="{w}"/><Cell N="Height" V="{h}"/><Cell N="LocPinX" V="0"/><Cell N="LocPinY" V="0"/><Cell N="Angle" V="{angle}"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="{w}" F="Width"/><Cell N="Y" V="{h}" F="Height"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="{h}" F="Height"/></Row></Section>"#
    );
    master(1, "Corner", &at_scale(1.0, 1.0), &shape(5, &inner))
}

/// The drop point is where the middle of the box goes, scaled or not: a local pin that is a plain number does not follow the size, so the pin is put where the new box has
/// its middle at the point, whatever the shape's turn.
#[test]
fn the_scaled_box_has_its_middle_at_the_drop_point_whatever_the_pin_and_the_turn() {
    for angle in [0.0, std::f64::consts::FRAC_PI_2, 0.7] {
        for (drawing, scale) in [(1.0, 1.0), (10.0, 10.0)] {
            let (mut s, _dir) = world((1.0, drawing), vec![corner_pinned(2.0, 1.0, angle)]);
            let r = drop_units(&mut s, "Corner", (50.0, 40.0));
            let (d, id) = (doc(&s), id_of(&r));
            let (w, h, centre) = page_box(d, id);
            assert_near(&format!("centre x at {angle} rad, r = {scale}"), centre.0, 50.0);
            assert_near(&format!("centre y at {angle} rad, r = {scale}"), centre.1, 40.0);
            let (own_w, own_h) = (2.0 * scale, scale);
            let (sin, cos) = angle.sin_cos();
            // The box on the page is the master's box times r, carried by the turn.
            assert_near("the box's width on the page", w, (own_w * cos).abs() + (own_h * sin).abs());
            assert_near("the box's height on the page", h, (own_w * sin).abs() + (own_h * cos).abs());
        }
    }
}

/// A size that the shape's lock keeps and that holds a formula stays inherited (the height of a group of compartments is the sum of theirs, as a resize keeps it); the same
/// formula with no lock is replaced by the scaled value.
#[test]
fn a_size_formula_that_the_shapes_lock_keeps_stays_inherited() {
    let make = |lock: &str| {
        let inner = format!(
            r#"{LOOK}<Cell N="Width" V="2"/><Cell N="Height" V="1" F="Width*0.5"/>{lock}<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>"#
        );
        master(1, "Locked", &at_scale(1.0, 1.0), &shape(5, &inner))
    };
    let (mut s, _dir) = world((1.0, 10.0), vec![make(r#"<Cell N="LockHeight" V="1"/>"#)]);
    let r = drop_units(&mut s, "Locked", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("Width", written(d, id, "Width").expect("written"), 20.0);
    assert!(written(d, id, "Height").is_none() && inherits(d, id, "Height"), "the lock keeps the height a formula");
    assert_near("Height follows the width", num(d, id, "Height"), 10.0);

    let (mut s, _dir) = world((1.0, 10.0), vec![make("")]);
    let r = drop_units(&mut s, "Locked", (50.0, 50.0));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("Height of an unlocked formula is a value", written(d, id, "Height").expect("replaced by a value"), 10.0);
}

/// Change Shape is no drop on the page: the shape keeps its own cells, and the trial instance of the new master is not scaled by the page's scale.
#[test]
fn change_shape_on_a_scaled_page_keeps_the_size_the_shape_has() {
    let (mut s, _dir) = world((1.0, 1.0), vec![flat(1, "Rect", 1.0, 0.5), flat(2, "Wide", 3.0, 1.0)]);
    let r = drop_at(&mut s, "Rect", (5.0, 4.0));
    let id = id_of(&r);
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 100.0}));
    let changed = run(&mut s, "shape.change", json!({"ids": [id], "master": "Wide", "stencil": "file-1", "page": 1}));
    assert_eq!(changed["changed"], json!([id]), "{changed}");
    let d = doc(&s);
    assert_near("Width stays the old master's", num(d, id, "Width"), 1.0);
    assert_near("Height stays", num(d, id, "Height"), 0.5);
    assert!(written(d, id, "DropOnPageScale").is_none(), "no factor is written by a change");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- every way of dropping ------------------------------------------------------------------------------------------------------

/// A built-in two-dimensional master: a stock rectangle is a master at 1:1.
#[test]
fn a_built_in_master_is_scaled_on_a_page_beyond_the_range_and_not_within_it() {
    let spec = diacraft_engine::stencil::find_master("rectangle").unwrap();
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 100.0}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0}));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
    assert_near("Width", written(d, id, "Width").expect("written"), spec.width * 100.0);
    assert_near("Height", written(d, id, "Height").expect("written"), spec.height * 100.0);
    assert_near("the answer's width on paper", paper_size(&r).0, spec.width);
    let (bw, bh, centre) = page_box(d, id);
    assert_near("the box on the page is as wide as the cell says", bw, spec.width * 100.0);
    assert_near("the box's height", bh, spec.height * 100.0);
    assert_near("centred at x", centre.0, 500.0);
    assert_near("centred at y", centre.1, 400.0);
    assert_eq!(check_session(&s), Ok(()));

    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 4.0}));
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0}));
    assert_unscaled(doc(&s), id_of(&r));
    assert!(r.get("drop_on_page_scale").is_none());
}

/// A built-in one-dimensional master (a connector) and a fixed one: the ends are the master's length times r apart.
#[test]
fn a_built_in_one_dimensional_master_is_scaled_about_the_drop_point() {
    for key in ["gane-sarson.data-flow", "network.comm-link"] {
        let spec = diacraft_engine::stencil::find_master(key).unwrap();
        let mut s = blank_session();
        run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 10.0}));
        let r = run(&mut s, "shape.drop", json!({"master": key, "page": 1, "x": 3.0, "y": 4.0}));
        let (d, id) = (doc(&s), id_of(&r));
        let e = ends(d, id);
        assert_near(&format!("{key}: length"), e[2] - e[0], spec.width * 10.0);
        assert_near(&format!("{key}: centre"), (e[0] + e[2]) / 2.0, 30.0);
        assert_near(&format!("{key}: level"), e[1], e[3]);
        assert_near(&format!("{key}: DropOnPageScale"), written(d, id, "DropOnPageScale").expect("written"), 10.0);
        assert_eq!(check_session(&s), Ok(()), "{key}");
    }
}

/// AutoConnect: the shape it makes is a drop of its master, and it is placed by the size it comes out.
#[test]
fn autoconnect_drops_a_master_by_the_same_rule() {
    let (mut s, _dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    let from = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 4.0})));
    let made = run(&mut s, "connector.autoconnect", json!({"from": from, "direction": "right", "master": "Rect", "stencil": "file-1"}));
    let (d, id) = (doc(&s), id_of(&made));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
    assert_near("Width", written(d, id, "Width").expect("written"), 100.0);
    assert_near("the answer says the factor", made["drop_on_page_scale"].as_f64().unwrap(), 100.0);
    // It is beside the source with a gap, not on top of it: the boxes do not overlap.
    let (src, new) = (page_box(d, from), page_box(d, id));
    assert!(new.2.0 - new.0 / 2.0 >= src.2.0 + src.0 / 2.0 - 1.0e-9, "overlap: source {src:?}, new {new:?}");
    let builtin = run(&mut s, "connector.autoconnect", json!({"from": from, "direction": "up", "master": "rectangle"}));
    let built = id_of(&builtin);
    assert!(written(doc(&s), built, "DropOnPageScale").is_some(), "a built-in master is dropped by the rule too");
    assert_eq!(check_session(&s), Ok(()));
}

/// One undo step takes the whole drop back, the scale included.
#[test]
fn a_drop_beyond_the_range_is_one_undo_step() {
    let (mut s, _dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    let before = snapshot(&s);
    drop_at(&mut s, "Rect", (5.0, 4.0));
    let after = snapshot(&s);
    assert_eq!(after.undo_steps, before.undo_steps + 1, "one step");
    run(&mut s, "edit.undo", json!({}));
    let undone = snapshot(&s);
    assert_eq!(undone.undo_steps, before.undo_steps);
    let shapes = |snap: &crate::Snapshot| snap.document.as_ref().and_then(|d| d.page(1)).map(|p| p.shapes.len());
    assert_eq!(shapes(&undone), Some(0), "the page is as it was");
    run(&mut s, "edit.redo", json!({}));
    let redone = doc(&s);
    let at = redone.find_shape(Owner::Page(1), 1).unwrap();
    assert!(redone.shape(&at).unwrap().sheet.cells.contains_key(&diacraft_doc::CellName::new("DropOnPageScale")));
}

// ---- save and reopen ------------------------------------------------------------------------------------------------------------

/// The `ID` of the first shape of page 1, however the file numbered it.
fn first_shape(d: &Document) -> u32 {
    d.page(1).unwrap().shapes.get(0).unwrap().id
}

/// A drop beyond the range and one within it, saved in a type and read back: the cells are as they were. `keeps_size_cells` says whether the type keeps a shape's size as the
/// master's (a drawing that is not `.odg`): an `.odg` states a size for every shape, so a shape within the range holds its size there and reads the same number.
fn trip(file: &str, keeps_size_cells: bool) {
    let (mut s, dir) = world(ONE_TO_100, vec![flat(1, "Rect", 1.0, 0.5)]);
    drop_at(&mut s, "Rect", (5.0, 4.0));
    let before = written_three(doc(&s), first_shape(doc(&s)));
    assert!(before.0.is_some() && before.1.is_some() && before.2.is_some(), "{file}: written before the save");
    let path = dir.path(file);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let after = written_three(doc(&t), first_shape(doc(&t)));
    assert_eq!(after.0.map(f64::to_bits), before.0.map(f64::to_bits), "{file}: Width");
    assert_eq!(after.1.map(f64::to_bits), before.1.map(f64::to_bits), "{file}: Height");
    assert_eq!(after.2.map(f64::to_bits), before.2.map(f64::to_bits), "{file}: DropOnPageScale");
    assert_eq!(check_session(&t), Ok(()), "{file}");
    // A save of the reopened drawing again is the same drawing: the scale is not applied twice.
    let again = dir.path(&format!("again-{file}"));
    run(&mut t, "file.save_as", json!({"path": again}));
    let mut u = Session::new();
    run(&mut u, "file.open", json!({"path": again}));
    assert_eq!(written_three(doc(&u), first_shape(doc(&u))), after, "{file}: a second trip");

    // Within the range nothing is written, and the shape reads the master's size after the trip.
    let (mut s, dir) = world((1.0, 4.0), vec![flat(1, "Rect", 1.0, 0.5)]);
    drop_at(&mut s, "Rect", (5.0, 4.0));
    let path = dir.path(file);
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "file.open", json!({"path": path}));
    let (d, id) = (doc(&t), first_shape(doc(&t)));
    assert!(written(d, id, "DropOnPageScale").is_none(), "{file}: within the range");
    assert_near(&format!("{file}: Width"), num(d, id, "Width"), 1.0);
    assert_near(&format!("{file}: Height"), num(d, id, "Height"), 0.5);
    if keeps_size_cells {
        assert_eq!(written_three(d, id), (None, None, None), "{file}: within the range");
    }
}

#[test]
fn the_scaled_cells_survive_a_save_and_a_reopen_in_diacraft() {
    trip("scaled.diacraft", true);
}

#[test]
fn the_scaled_cells_survive_a_save_and_a_reopen_in_vsdx() {
    trip("scaled.vsdx", true);
}

#[test]
fn the_scaled_cells_survive_a_save_and_a_reopen_in_odg() {
    trip("scaled.odg", false);
}

// ---- hostile inputs, timed ------------------------------------------------------------------------------------------------------

/// A group master of `levels` levels with `per_level` boxes at each, the last member of each level the group of the next, every size and factor a formula of the level above.
fn nested_group(levels: u32, per_level: u32) -> MasterSpec {
    fn level(depth: u32, levels: u32, per_level: u32, next_id: &mut u32, parent: u32) -> String {
        let mut members = String::new();
        for _ in 0..per_level {
            let id = *next_id;
            *next_id += 1;
            members.push_str(&shape(
                id,
                &format!(
                    r#"<Cell N="PinX" V="0.5" F="Sheet.{parent}!Width*0.5"/><Cell N="PinY" V="0.5" F="Sheet.{parent}!Height*0.5"/><Cell N="Width" V="0.5" F="Sheet.{parent}!Width*0.5"/><Cell N="Height" V="0.5" F="Sheet.{parent}!Height*0.5"/><Cell N="DropOnPageScale" V="1" F="Sheet.{parent}!DropOnPageScale"/>"#
                ),
            ));
        }
        if depth + 1 < levels {
            let id = *next_id;
            *next_id += 1;
            let inner = level(depth + 1, levels, per_level, next_id, id);
            members.push_str(&shape_with(
                id,
                r#" Type="Group""#,
                &format!(
                    r#"<Cell N="PinX" V="0.5" F="Sheet.{parent}!Width*0.5"/><Cell N="PinY" V="0.5" F="Sheet.{parent}!Height*0.5"/><Cell N="Width" V="1" F="Sheet.{parent}!Width*0.9"/><Cell N="Height" V="1" F="Sheet.{parent}!Height*0.9"/><Cell N="DropOnPageScale" V="1" F="Sheet.{parent}!DropOnPageScale"/><Shapes>{inner}</Shapes>"#
                ),
            ));
        }
        members
    }
    let mut next = 100;
    let members = level(0, levels, per_level, &mut next, 99);
    master(1, "Deep", &at_scale(1.0, 1.0), &shape_with(99, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(4.0, 3.0))))
}

/// How many shapes are below `shape`, nested ones counted.
fn below_count(shape: &diacraft_doc::Shape) -> usize {
    shape.children.iter().map(|c| 1 + below_count(c)).sum()
}

/// The `ID` of the deepest member below the instance `id`, following the last member at each level.
fn deepest(d: &Document, id: u32) -> u32 {
    let mut at = d.find_shape(Owner::Page(1), id).unwrap();
    let mut shape = d.shape(&at).unwrap();
    while let Some(last) = shape.children.len().checked_sub(1) {
        at = at.child(u32::try_from(last).unwrap());
        shape = d.shape(&at).unwrap();
    }
    shape.id
}

/// Runs `f` and says how long it took, with a hang guard of half a minute: the claim is that it ends, and that it does not walk what it need not.
fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let out = f();
    let took = started.elapsed();
    eprintln!("{what}: {took:?}");
    assert!(took < std::time::Duration::from_secs(30), "{what} took {took:?}");
    out
}

#[test]
fn a_group_master_of_24_levels_and_32_members_a_level_is_dropped_beyond_the_range_in_time() {
    let (mut s, _dir) = world((1.0, 100.0), vec![nested_group(24, 32)]);
    let r = timed("a group master of 24 levels of 32 members dropped at r = 100", || drop_at(&mut s, "Deep", (5.0, 4.0)));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
    assert_near("Width", written(d, id, "Width").expect("written"), 400.0);
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    assert_eq!(below_count(d.shape(&at).unwrap()), 23 * 33 + 32, "every member is listed: the drop is not cut short");
    // Only the top-level shape is written: no member holds a factor of its own, and the deepest follows the group's through 24 formulas.
    let deep = deepest(d, id);
    assert!(own(d, deep, "DropOnPageScale").is_none_or(|c| matches!(c.formula, FormulaSlot::Inherit)));
    assert_near("the deepest member's factor", num(d, deep, "DropOnPageScale"), 100.0);
    assert_eq!(check_session(&s), Ok(()));
}

/// A group master with more members than a drop lists (5,000) is dropped beyond the range with its members left to the master, and the scale is no walk of them.
#[test]
fn a_group_master_of_more_than_the_listed_members_is_scaled_without_walking_them() {
    let mut members = String::new();
    for k in 0..5_100_u32 {
        members.push_str(&shape(100 + k, r#"<Cell N="Width" V="0.5" F="Sheet.99!Width*0.5"/><Cell N="Height" V="0.5" F="Sheet.99!Height*0.5"/>"#));
    }
    let big =
        master(1, "Crowd", &at_scale(1.0, 1.0), &shape_with(99, r#" Type="Group""#, &format!("{}<Shapes>{members}</Shapes>", box_cells(4.0, 3.0))));
    let (mut s, _dir) = world((1.0, 100.0), vec![big]);
    let r = timed("a group master of 5,100 members dropped at r = 100", || drop_at(&mut s, "Crowd", (5.0, 4.0)));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 100.0);
    assert_near("Width", written(d, id, "Width").expect("written"), 400.0);
    assert!(members_of(d, id).is_empty(), "no member is listed past the cap");
    assert_eq!(check_session(&s), Ok(()));
}

/// Scales that are not numbers or not usable: nothing crashes and every one of them reads as one inch or drops unscaled, with words where the ratio cannot be used.
#[test]
fn zero_negative_and_extreme_page_scales_never_crash_a_drop() {
    for (paper, drawing) in [
        ("0", "1"),
        ("1", "0"),
        ("-3", "1"),
        ("1", "-7"),
        ("1E300", "1"),
        ("1", "1E300"),
        ("1E-300", "1"),
        ("1", "1E-300"),
        ("1E300", "1E-300"),
        ("1E-300", "1E300"),
        ("1E999", "1"),
        ("1", "1E999"),
        ("NaN", "1"),
        ("1", "NaN"),
        ("1.#INF", "1"),
        ("1", "-1.#INF"),
    ] {
        let (mut s, _dir) = world((1.0, 1.0), vec![flat(1, "One", 1.0, 1.0)]);
        for (cell, v) in [("PageScale", paper), ("DrawingScale", drawing)] {
            // A scale a command refuses is a scale the page does not get: the drop then meets the page as it was.
            let _ = s.execute("sheet.set_cell", &json!({"page": 1, "cell": cell, "formula": v}));
        }
        let r = timed(&format!("page scale {paper}:{drawing}"), || {
            s.execute("shape.drop", &json!({"master": "One", "stencil": "file-1", "page": 1, "x": 0.0, "y": 0.0}))
        });
        if let Ok(r) = r {
            let d = doc(&s);
            let id = id_of(&r);
            if let Some(f) = written(d, id, "DropOnPageScale") {
                assert!(f.is_finite() && f > 0.0, "{paper}:{drawing}: {f}");
            }
        }
        assert_eq!(check_session(&s), Ok(()), "{paper}:{drawing}");
    }
}

/// A master page whose scale cells are not numbers (an infinity, text, 0 and a negative length) reads as one inch, and the drop is the unscaled drop.
#[test]
fn master_scales_that_are_not_numbers_read_as_one_inch() {
    for sheet in [
        r#"<Cell N="PageScale" V="1.#INF"/><Cell N="DrawingScale" V="1"/>"#,
        r#"<Cell N="PageScale" V="abc"/><Cell N="DrawingScale" V="-1"/>"#,
        r#"<Cell N="PageScale" V="0"/><Cell N="DrawingScale" V="0"/>"#,
        r#"<Cell N="PageScale" V="NaN"/><Cell N="DrawingScale" V="1.#QNAN"/>"#,
        r#"<Cell N="PageScale" V="1E999"/><Cell N="DrawingScale" V="-1.#INF"/>"#,
    ] {
        let m = master(1, "Odd", sheet, &shape(5, &format!("{LOOK}{}", box_cells(1.0, 1.0))));
        let (mut s, _dir) = world((1.0, 1.0), vec![m]);
        let r = timed(sheet, || drop_at(&mut s, "Odd", (1.0, 1.0)));
        assert!(written(doc(&s), id_of(&r), "DropOnPageScale").is_none(), "{sheet}");
        assert_eq!(check_session(&s), Ok(()), "{sheet}");
    }
}

/// A ratio of 1E300 that makes a width that is a number is dropped as scaled, with the factor as it is: a hostile factor does not hang or crash the recalculation.
#[test]
fn a_ratio_of_1e300_with_a_width_that_fits_is_scaled_and_settles() {
    let tiny = master(1, "Tiny", &at_scale(1.0e300, 1.0), &shape(5, &format!("{LOOK}{}", box_cells(1.0e-300, 1.0e-300))));
    let (mut s, _dir) = world((1.0, 1.0), vec![tiny]);
    let r = timed("a ratio of 1E300", || drop_at(&mut s, "Tiny", (1.0, 1.0)));
    let (d, id) = (doc(&s), id_of(&r));
    assert_near("DropOnPageScale", written(d, id, "DropOnPageScale").expect("written"), 1.0e300);
    assert_near("Width", written(d, id, "Width").expect("written"), 1.0);
    assert_eq!(check_session(&s), Ok(()));
}
