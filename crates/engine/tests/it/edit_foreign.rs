//! A drawing from another application, edited: the connectors of a file follow a shape that is moved, whichever way it is glued.
//!
//! The package is built in code from the specifications' XML;
//! no file from another application is used. It holds four shapes and four connectors the way a file holds them:
//!
//! * connector 20 is glued to connection points (`PAR(PNT(Sheet.n!Connections.Xk, ...))`), with a route of two rows and a label;
//! * connector 21 is glued to the whole of shapes 12 and 13 (`ToPart` 3, `_WALKGLUE` in its end cells), with a label;
//! * connector 22 is glued to points and its `ConFixedCode` says its route is fixed, with a label;
//! * connector 23 is glued to a point of shape 11 at one end and to the whole of shape 13, which has no connection points, at the other.
//!
//! Every shape is moved the four ways in turn, the drawing is saved, and every cell that the move does not reach must be as the file had it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, Owner, Resolver, Sheet, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::draw::{Pose, connection_points};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape, shape_with};
use serde_json::json;

use crate::edit::{doc, drop_shape, geometry, id_of, num, route};
use crate::{check_session, run};

type P = (f64, f64);

/// The line of a shape, which a file holds on the shape or on its style: black, a hundredth of an inch.
const LINE: &str = r##"<Cell N="LinePattern" V="1"/><Cell N="LineColor" V="#000000"/><Cell N="LineWeight" V="0.01"/>"##;

/// A master of a 2 by 1 box with a connection point on its right (row 0), left (1), top (2) and bottom (3), as a file writes it.
fn box_master() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0.5" F="Height*0.5"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row>
<Row IX="2"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="1" F="Height"/><Cell N="DirX" V="0"/><Cell N="DirY" V="1"/></Row>
<Row IX="3"><Cell N="X" V="1" F="Width*0.5"/><Cell N="Y" V="0"/><Cell N="DirX" V="0"/><Cell N="DirY" V="-1"/></Row></Section>"#,
    )
}

/// The same box with no connection point at all.
fn blob_master() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#,
    )
}

/// How an end of a file's connector is glued.
#[derive(Clone, Copy)]
enum Glue {
    /// To connection point `IX` of the shape: `PAR(PNT(...))` in both end cells, `ToPart` 100 plus the `IX`.
    Point(u32, u32),
    /// To the whole of the shape: `_WALKGLUE(...)` in both end cells, `ToPart` 3.
    Whole(u32),
}

impl Glue {
    fn formula(self) -> String {
        match self {
            Glue::Point(target, ix) => {
                format!("PAR(PNT(Sheet.{target}!Connections.X{n},Sheet.{target}!Connections.Y{n}))", n = ix + 1)
            }
            Glue::Whole(_) => "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)".to_owned(),
        }
    }

    fn connect(self, from: u32, begin: bool) -> String {
        let (cell, part) = if begin { ("BeginX", 9) } else { ("EndX", 12) };
        match self {
            Glue::Point(target, ix) => format!(
                r#"<Connect FromSheet="{from}" FromCell="{cell}" FromPart="{part}" ToSheet="{target}" ToCell="Connections.X{}" ToPart="{}"/>"#,
                ix + 1,
                100 + ix
            ),
            Glue::Whole(target) => {
                format!(r#"<Connect FromSheet="{from}" FromCell="{cell}" FromPart="{part}" ToSheet="{target}" ToCell="PinX" ToPart="3"/>"#)
            }
        }
    }
}

/// A connector as a file holds it: its ends at `b` and `e` (where the saved values say they are), the creation formulas of the one-dimensional
/// frame, a route of two rows in that frame, and, with a `label`, text and the text-transform cells with the formulas a file writes. A `fixed`
/// connector has `ConFixedCode` 1, and its last row is `Width*1`.
fn connector(id: u32, b: P, e: P, glue_b: Glue, glue_e: Glue, label: Option<(&str, bool)>, fixed: bool) -> String {
    let (dx, dy) = (e.0 - b.0, e.1 - b.1);
    let width = dx.hypot(dy);
    let angle = dy.atan2(dx);
    let mid = ((b.0 + e.0) / 2.0, (b.1 + e.1) / 2.0);
    let (fb, fe) = (glue_b.formula(), glue_e.formula());
    let walk = matches!(glue_b, Glue::Whole(_)) || matches!(glue_e, Glue::Whole(_));
    let triggers = if walk {
        r#"<Cell N="BegTrigger" V="2" F="_XFTRIGGER(Sheet.12!EventXFMod)"/><Cell N="EndTrigger" V="2" F="_XFTRIGGER(Sheet.13!EventXFMod)"/>"#
    } else {
        ""
    };
    let text = label.map_or(String::new(), |(t, guarded)| {
        let pin_x = if guarded { "GUARD(Width*0.5)" } else { "Width*0.5" };
        format!(
            r#"<Cell N="TxtPinX" V="{h}" F="{pin_x}"/><Cell N="TxtPinY" V="0" F="Height*0.5"/><Cell N="TxtLocPinX" V="{h}" F="TxtWidth*0.5"/>
<Cell N="TxtLocPinY" V="0" F="TxtHeight*0.5"/><Cell N="TxtWidth" V="{width}" F="Width*1"/><Cell N="TxtHeight" V="0" F="Height*1"/><Cell N="TxtAngle" V="0"/>
<Text>{t}</Text>"#,
            h = width / 2.0
        )
    });
    let last_x = if fixed { format!(r#"<Cell N="X" V="{width}" F="Width*1"/>"#) } else { format!(r#"<Cell N="X" V="{width}"/>"#) };
    let code = if fixed { r#"<Cell N="ConFixedCode" V="1"/>"# } else { "" };
    shape(
        id,
        &format!(
            r#"<Cell N="BeginX" V="{bx}" F="{fb}"/><Cell N="BeginY" V="{by}" F="{fb}"/><Cell N="EndX" V="{ex}" F="{fe}"/><Cell N="EndY" V="{ey}" F="{fe}"/>
<Cell N="PinX" V="{px}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{py}" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{width}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{h}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Cell N="EndArrow" V="4"/>{LINE}{code}{triggers}{text}
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2">{last_x}<Cell N="Y" V="0"/></Row></Section>"#,
            bx = b.0,
            by = b.1,
            ex = e.0,
            ey = e.1,
            px = mid.0,
            py = mid.1,
            h = width / 2.0,
        ),
    )
}

/// The connection point of a box instance at (`x`, `y`): 0 right, 1 left, 2 top, 3 bottom.
pub(crate) fn point_of(x: f64, y: f64, ix: u32) -> P {
    match ix {
        0 => (x + 1.0, y),
        1 => (x - 1.0, y),
        2 => (x, y + 0.5),
        _ => (x, y - 0.5),
    }
}

/// Shapes 10 and 11 side by side (A and B), 12 and 13 above them (C, a box, and D, a blob), and the four connectors.
fn page_shapes(guarded_label: bool) -> String {
    let (a, b, c, d) = ((2.0, 2.0), (8.0, 2.0), (2.0, 7.0), (8.0, 7.0));
    let inst = |id: u32, master: u32, at: P| {
        shape_with(id, &format!(r#" Master="{master}""#), &format!(r#"<Cell N="PinX" V="{}"/><Cell N="PinY" V="{}"/>{LINE}"#, at.0, at.1))
    };
    let c20 =
        connector(20, point_of(a.0, a.1, 0), point_of(b.0, b.1, 1), Glue::Point(10, 0), Glue::Point(11, 1), Some(("to B", guarded_label)), false);
    // C to D: the saved ends are where the application put them, C's right and D's left side.
    let c21 = connector(21, point_of(c.0, c.1, 0), (d.0 - 1.0, d.1), Glue::Whole(12), Glue::Whole(13), Some(("whole", false)), false);
    let c22 = connector(22, point_of(a.0, a.1, 2), point_of(c.0, c.1, 3), Glue::Point(10, 2), Glue::Point(12, 3), Some(("fixed", false)), true);
    let c23 = connector(23, point_of(b.0, b.1, 0), (d.0 + 1.0, d.1 - 0.5), Glue::Point(11, 0), Glue::Whole(13), None, false);
    format!("{}{}{}{}{c20}{c21}{c22}{c23}", inst(10, 7, a), inst(11, 7, b), inst(12, 7, c), inst(13, 8, d))
}

fn connects() -> String {
    [
        Glue::Point(10, 0).connect(20, true),
        Glue::Point(11, 1).connect(20, false),
        Glue::Whole(12).connect(21, true),
        Glue::Whole(13).connect(21, false),
        Glue::Point(10, 2).connect(22, true),
        Glue::Point(12, 3).connect(22, false),
        Glue::Point(11, 0).connect(23, true),
        Glue::Whole(13).connect(23, false),
    ]
    .join("\n")
}

fn drawing(page_sheet: &str, guarded_label: bool) -> Vec<u8> {
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&box_master()))
        .master(MasterSpec::new(8, "Blob").shapes(&blob_master()))
        .page(PageSpec::new(1, "Page-1").sheet(page_sheet).shapes(&page_shapes(guarded_label)).connects(&connects()))
        .build()
}

/// A scratch folder under `TMPDIR`, removed on drop.
pub(crate) struct Dir(std::path::PathBuf);

impl Dir {
    pub(crate) fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-engine-foreign-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub(crate) fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A session that has opened the drawing, with `page_sheet` the cells of the page's sheet, and `TxtPinX` of connector 20 guarded or not.
fn opened_as(dir: &Dir, page_sheet: &str, guarded_label: bool) -> Session {
    let path = dir.path("in.vsdx");
    std::fs::write(&path, drawing(page_sheet, guarded_label)).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

fn opened_with(dir: &Dir, page_sheet: &str) -> Session {
    opened_as(dir, page_sheet, false)
}

pub(crate) fn opened(dir: &Dir) -> Session {
    opened_with(dir, "")
}

const SHAPES: [u32; 8] = [10, 11, 12, 13, 20, 21, 22, 23];

/// Every cell of every shape of the page as text: shape, section, row and cell, with `F`, `V`, `U` and `E`.
fn dump(d: &Document) -> BTreeMap<String, String> {
    fn cell_text(c: &diacraft_doc::Cell) -> String {
        format!("F={:?} V={:?} U={:?} E={:?}", c.formula.text(), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_deref(), c.error.as_deref())
    }
    fn sheet_cells(out: &mut BTreeMap<String, String>, at: &str, sheet: &Sheet) {
        for (name, c) in &sheet.cells {
            out.insert(format!("{at}.{}", name.as_str()), cell_text(c));
        }
        for section in &sheet.sections {
            for (name, c) in &section.cells {
                out.insert(format!("{at}.{}[{:?}].{}", section.name.as_str(), section.ix, name.as_str()), cell_text(c));
            }
            for (place, row) in section.rows.iter().enumerate() {
                for (name, c) in &row.cells {
                    out.insert(format!("{at}.{}[{:?}].row{place}.{}", section.name.as_str(), section.ix, name.as_str()), cell_text(c));
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for id in SHAPES {
        let Some(at) = d.find_shape(Owner::Page(1), id) else { continue };
        let shape = d.shape(&at).unwrap();
        sheet_cells(&mut out, &format!("{id}"), &shape.sheet);
        out.insert(format!("{id}.<text>"), format!("{:?}", shape.text.as_ref().map(|t| format!("{t:?}"))));
    }
    out
}

/// The cells that differ between two dumps, as `shape.cell` keys.
fn differing(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect();
    out.extend(before.keys().filter(|k| !after.contains_key(*k)).cloned());
    out.sort();
    out
}

/// The shape a key belongs to.
fn owner_of(key: &str) -> u32 {
    key.split('.').next().unwrap().parse().unwrap()
}

/// The cell of a key, without the shape: `Geometry[Some(0)].row1.X` or `PinX`.
fn cell_of(key: &str) -> &str {
    key.split_once('.').map_or(key, |(_, rest)| rest)
}

/// The connectors a shape is glued to by the file's `Connect` rows.
fn glued_to(shape: u32) -> Vec<u32> {
    match shape {
        10 => vec![20, 22],
        11 => vec![20, 23],
        12 => vec![21, 22],
        13 => vec![21, 23],
        _ => vec![],
    }
}

/// The cells of a connector that a reroute may write: the frame, the route, the label and its background.
fn may_change_when_rerouted(cell: &str) -> bool {
    const NAMES: [&str; 17] = [
        "BeginX",
        "BeginY",
        "EndX",
        "EndY",
        "PinX",
        "PinY",
        "Width",
        "Angle",
        "LocPinX",
        "LocPinY",
        "TxtPinX",
        "TxtPinY",
        "TxtLocPinX",
        "TxtLocPinY",
        "TxtAngle",
        "TxtWidth",
        "TextBkgnd",
    ];
    NAMES.contains(&cell) || cell.starts_with("Geometry[Some(0)]")
}

/// The cells of a connector whose route is fixed that a move may change: its ends, its frame, and the cells with formulas that read them.
fn may_change_when_fixed(cell: &str) -> bool {
    const NAMES: [&str; 12] = ["BeginX", "BeginY", "EndX", "EndY", "PinX", "PinY", "Width", "Angle", "LocPinX", "TxtPinX", "TxtLocPinX", "TxtWidth"];
    NAMES.contains(&cell) || (cell.starts_with("Geometry[Some(0)].row") && cell.ends_with(".X"))
}

/// After moving `moved`: only that shape's pin, and the cells of the connectors glued to it that the move can reach, differ.
fn assert_scope(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>, moved: u32, fixed: &[u32]) {
    let glued = glued_to(moved);
    for key in differing(before, after) {
        let (owner, cell) = (owner_of(&key), cell_of(&key));
        if owner == moved {
            assert!(cell == "PinX" || cell == "PinY", "the move of {moved} changed its cell {key}");
        } else if fixed.contains(&owner) && glued.contains(&owner) {
            assert!(may_change_when_fixed(cell), "the move of {moved} changed {key}, a cell of a connector whose route is fixed");
        } else {
            assert!(glued.contains(&owner) && may_change_when_rerouted(cell), "the move of {moved} changed {key}");
        }
    }
    // Nothing is added to or taken from a connector whose route is fixed.
    for key in before.keys().chain(after.keys()) {
        if fixed.contains(&owner_of(key)) {
            assert_eq!(before.contains_key(key), after.contains_key(key), "{key} came or went on a fixed route");
        }
    }
}

const FOUR_WAYS: [(f64, f64); 4] = [(2.0, 0.0), (-2.0, 0.0), (0.0, 2.0), (0.0, -2.0)];

fn point(d: &Document, shape: u32, ix: u32) -> P {
    let p = connection_points(d, 1, shape).into_iter().find(|p| p.index == ix).unwrap();
    (p.x, p.y)
}

pub(crate) fn end_of(d: &Document, connector: u32) -> (P, P) {
    ((num(d, connector, "BeginX"), num(d, connector, "BeginY")), (num(d, connector, "EndX"), num(d, connector, "EndY")))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_point(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// Whether the route's legs are all level or upright, none is of no length, and none goes back along the one before it.
pub(crate) fn is_clean(points: &[P]) -> bool {
    points.windows(2).all(|w| (near(w[0].0, w[1].0) != near(w[0].1, w[1].1)) || (near_point(w[0], w[1]) && points.len() == 2))
        && points.windows(3).all(|w| {
            let (a, b) = ((w[1].0 - w[0].0, w[1].1 - w[0].1), (w[2].0 - w[1].0, w[2].1 - w[1].1));
            a.0 * b.0 + a.1 * b.1 > -1.0e-9 || (a.0 * b.1 - a.1 * b.0).abs() > 1.0e-9
        })
}

// ---- the file as it is -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_file_opens_as_built() {
    let dir = Dir::new();
    let s = opened(&dir);
    let d = doc(&s);
    assert_eq!(end_of(d, 20), ((3.0, 2.0), (7.0, 2.0)));
    assert_eq!(geometry(d, 20).len(), 2);
    assert_eq!(d.page(1).unwrap().connects.len(), 8);
    assert_eq!(connection_points(d, 1, 13).len(), 0, "shape 13 has no connection points");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- point glue -----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_file_connector_glued_to_points_is_rerouted_when_either_of_its_shapes_moves_each_way() {
    for moved in [10, 11] {
        for (dx, dy) in FOUR_WAYS {
            let dir = Dir::new();
            let mut s = opened(&dir);
            let before = dump(doc(&s));
            run(&mut s, "shape.move", json!({"ids": [moved], "dx": dx, "dy": dy}));
            let d = doc(&s);
            let what = format!("shape {moved} moved by ({dx}, {dy})");
            // The ends are on the points they are glued to, by the file's own formulas.
            let (begin, end) = end_of(d, 20);
            assert!(near_point(begin, point(d, 10, 0)) && near_point(end, point(d, 11, 1)), "{what}: {begin:?} {end:?}");
            // And the route is made again from them: it starts and ends there, in level and upright legs, and is not the file's two rows.
            let points = route(d, 20);
            assert!(near_point(points[0], begin) && near_point(points[points.len() - 1], end), "{what}: {points:?}");
            assert!(is_clean(&points), "{what}: {points:?}");
            if !near(begin.1, end.1) {
                assert!(points.len() >= 4, "{what}: a Z, not the file's straight rows: {points:?}");
            }
            assert_scope(&before, &dump(d), moved, &[22]);
            assert_eq!(check_session(&s), Ok(()), "{what}");
        }
    }
}

// ---- glue to the whole of a shape ------------------------------------------------------------------------------------------------------

/// The nearest point to `p` on the boundary of the box `w` by `h` centred at `c`, found without the engine's code.
pub(crate) fn box_nearest(c: P, w: f64, h: f64, p: P) -> P {
    let (x0, x1, y0, y1) = (c.0 - w / 2.0, c.0 + w / 2.0, c.1 - h / 2.0, c.1 + h / 2.0);
    let outside = (p.0.clamp(x0, x1), p.1.clamp(y0, y1));
    if !near_point(outside, p) {
        return outside;
    }
    // Inside: the nearest side.
    let sides = [((x0, p.1), p.0 - x0), ((x1, p.1), x1 - p.0), ((p.0, y0), p.1 - y0), ((p.0, y1), y1 - p.1)];
    sides.iter().fold(sides[0], |best, side| if side.1 < best.1 { *side } else { best }).0
}

pub(crate) fn pin(d: &Document, id: u32) -> P {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

/// The cell text up to its value: the formula, which a move must leave as the file had it.
fn formula_of(dump: &BTreeMap<String, String>, key: &str) -> String {
    dump[key].split(" V=").next().unwrap().to_owned()
}

#[test]
fn an_end_glued_to_the_whole_of_a_shape_goes_to_the_nearest_point_when_the_shape_moves_and_keeps_its_formula() {
    for moved in [11, 12, 13] {
        for (dx, dy) in FOUR_WAYS {
            let dir = Dir::new();
            let mut s = opened(&dir);
            let before = dump(doc(&s));
            run(&mut s, "shape.move", json!({"ids": [moved], "dx": dx, "dy": dy}));
            let d = doc(&s);
            let what = format!("shape {moved} moved by ({dx}, {dy})");
            // Connector 21 joins the whole of C (12, with four points) to the whole of D (13, with none): D's end is the point of its box nearest
            // the middle of C, and C's end is the point of C nearest that.
            let (c, dd) = (pin(d, 12), pin(d, 13));
            let d_end = box_nearest(dd, 2.0, 1.0, c);
            let c_end = (0..4).map(|ix| point_of(c.0, c.1, ix)).fold(None, |best: Option<P>, p| match best {
                Some(b) if (b.0 - d_end.0).hypot(b.1 - d_end.1) <= (p.0 - d_end.0).hypot(p.1 - d_end.1) => Some(b),
                _ => Some(p),
            });
            let (begin, end) = end_of(d, 21);
            assert!(
                near_point(begin, c_end.unwrap()) && near_point(end, d_end),
                "{what}: connector 21 is at {begin:?} {end:?}, not {c_end:?} {d_end:?}"
            );
            // Connector 23 joins point 0 of B (11) to the whole of D: D's end is the point of its box nearest B's point.
            let (begin, end) = end_of(d, 23);
            let b_point = point(d, 11, 0);
            assert!(
                near_point(begin, b_point) && near_point(end, box_nearest(dd, 2.0, 1.0, b_point)),
                "{what}: connector 23 is at {begin:?} {end:?}"
            );
            // The routes are made from the ends, and the file's `_WALKGLUE` formulas are as they were.
            for id in [21, 23] {
                let (b, e) = end_of(d, id);
                let points = route(d, id);
                assert!(
                    near_point(points[0], b) && near_point(points[points.len() - 1], e) && is_clean(&points),
                    "{what}: connector {id} {points:?}"
                );
            }
            let after = dump(d);
            for key in ["21.BeginX", "21.BeginY", "21.EndX", "21.EndY", "23.EndX", "23.EndY"] {
                assert_eq!(formula_of(&before, key), formula_of(&after, key), "{what}: {key}");
            }
            assert!(formula_of(&after, "21.BeginX").contains("_WALKGLUE"), "{what}");
            assert_scope(&before, &after, moved, &[22]);
            assert_eq!(check_session(&s), Ok(()), "{what}");
        }
    }
}

/// The end cells of connector 21 are `_WALKGLUE`, which calc cannot work out: the engine puts the ends (`draw::glue`), and what reads them is worked out again
/// after (`settle::recalculate_within`). A cell that reads `BeginX` follows the end the engine placed.
#[test]
fn a_cell_that_reads_an_end_the_engine_placed_is_worked_out_again() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    s.edit("a cell that reads the begin", |d| {
        let page = d.pages.get(0).map(|p| p.id).unwrap();
        let at = d.find_shape(Owner::Page(page), 21).unwrap();
        let formula = diacraft_doc::Cell::with_formula("BeginX*2", Some(diacraft_doc::Value::from_text("0"))).with_unit("IN");
        let write = diacraft_doc::CellWrite {
            addr: CellAddr::direct("TxtHeight"),
            edit: diacraft_doc::CellEdit::Cell(formula),
            origin: diacraft_doc::WriteOrigin::Drawing,
        };
        d.write_batch(vec![(SheetRef::Shape(at), write)]).unwrap();
        Ok(())
    })
    .unwrap();
    for (dx, dy) in FOUR_WAYS {
        run(&mut s, "shape.move", json!({"ids": [12], "dx": dx, "dy": dy}));
        let d = doc(&s);
        let begin = num(d, 21, "BeginX");
        assert!(
            (num(d, 21, "TxtHeight") - 2.0 * begin).abs() < 1.0e-6,
            "moved by ({dx}, {dy}): the cell reads {} and the begin is {begin}",
            num(d, 21, "TxtHeight")
        );
    }
}

// ---- a fixed route ----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_connector_whose_route_is_fixed_only_has_its_ends_moved() {
    for moved in [10, 12] {
        for (dx, dy) in FOUR_WAYS {
            let dir = Dir::new();
            let mut s = opened(&dir);
            let before = dump(doc(&s));
            run(&mut s, "shape.move", json!({"ids": [moved], "dx": dx, "dy": dy}));
            let d = doc(&s);
            let what = format!("shape {moved} moved by ({dx}, {dy})");
            let (begin, end) = end_of(d, 22);
            assert!(
                near_point(begin, point(d, 10, 2)) && near_point(end, point(d, 12, 3)),
                "{what}: the ends follow their shapes: {begin:?} {end:?}"
            );
            // The rows are the file's: two, the last still `Width*1`, and no label, background or other cell was written.
            let after = dump(d);
            assert_eq!(geometry(d, 22).len(), 2, "{what}");
            assert_eq!(before["22.Geometry[Some(0)].row1.X"].split(" V=").next(), after["22.Geometry[Some(0)].row1.X"].split(" V=").next(), "{what}");
            for kept in ["22.TxtAngle", "22.TxtPinY", "22.TxtLocPinY", "22.<text>"] {
                assert_eq!(before[kept], after[kept], "{what}: {kept}");
            }
            assert!(!after.contains_key("22.TextBkgnd"), "{what}");
            assert_scope(&before, &after, moved, &[22]);
            assert_eq!(check_session(&s), Ok(()), "{what}");
        }
    }
}

// ---- a page with its dynamics off ---------------------------------------------------------------------------------------------------------

#[test]
fn a_page_with_dynamics_off_reroutes_nothing_and_its_ends_still_follow() {
    for (dx, dy) in FOUR_WAYS {
        let dir = Dir::new();
        let mut s = opened_with(&dir, r#"<Cell N="DynamicsOff" V="1"/>"#);
        let before = dump(doc(&s));
        run(&mut s, "shape.move", json!({"ids": [10], "dx": dx, "dy": dy}));
        let d = doc(&s);
        let (begin, _) = end_of(d, 20);
        assert!(near_point(begin, point(d, 10, 0)), "({dx}, {dy}): the end follows by its formula");
        assert_eq!(geometry(d, 20).len(), 2, "({dx}, {dy}), the connector's rows are unchanged");
        assert_scope(&before, &dump(d), 10, &[20, 22]);
    }
}

// ---- the file saved ---------------------------------------------------------------------------------------------------------------------

/// The text of each shape's element in a saved page, by `ID`.
fn shapes_of_page(bytes: &[u8]) -> BTreeMap<u32, String> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let name = pkg.parts().find(|p| p.ends_with("/pages/page1.xml")).expect("the page part").to_owned();
    let xml = String::from_utf8(pkg.part(&name).unwrap().to_vec()).unwrap();
    let mut out = BTreeMap::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<Shape ID=\"") {
        let tail = &rest[start..];
        let end = tail.find("</Shape>").map_or(tail.len(), |e| e + "</Shape>".len());
        let id: u32 = tail["<Shape ID=\"".len()..].split('"').next().unwrap().parse().unwrap();
        out.insert(id, tail[..end].to_owned());
        rest = &tail[end..];
    }
    out
}

#[test]
fn a_save_after_a_move_leaves_the_shapes_it_did_not_reach_byte_for_byte_as_they_were() {
    let dir = Dir::new();
    let mut untouched = opened(&dir);
    let kept = dir.path("kept.vsdx");
    run(&mut untouched, "file.save_as", json!({"path": kept}));
    let a = shapes_of_page(&std::fs::read(&kept).unwrap());
    for moved in [10, 11, 12, 13] {
        for (n, (dx, dy)) in FOUR_WAYS.into_iter().enumerate() {
            let mut s = opened(&dir);
            run(&mut s, "shape.move", json!({"ids": [moved], "dx": dx, "dy": dy}));
            let edited = dir.path(&format!("edited-{moved}-{n}.vsdx"));
            run(&mut s, "file.save_as", json!({"path": edited}));
            let b = shapes_of_page(&std::fs::read(&edited).unwrap());
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "the same shapes");
            let reached: Vec<u32> = std::iter::once(moved).chain(glued_to(moved)).collect();
            for id in SHAPES {
                if !reached.contains(&id) {
                    assert_eq!(a[&id], b[&id], "moving {moved} by ({dx}, {dy}): shape {id} was not reached and is as it was");
                }
            }
            // And what is saved reads back as the drawing in the window.
            let mut again = Session::new();
            run(&mut again, "file.open", json!({"path": edited}));
            assert_eq!(dump(doc(&again)), dump(doc(&s)), "moving {moved} by ({dx}, {dy})");
        }
    }
}

// ---- the labels -----------------------------------------------------------------------------------------------------------------------

/// The middle leg's centre of a route, found without the engine's code: the middle leg of an odd number, the longer of the two in the middle of an
/// even number (the first if they are as long).
fn middle_leg_centre(points: &[P]) -> P {
    let legs: Vec<(P, P)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    let len = |l: &(P, P)| (l.1.0 - l.0.0).hypot(l.1.1 - l.0.1);
    let leg = if legs.len() % 2 == 1 {
        legs[legs.len() / 2]
    } else {
        let (a, b) = (legs[legs.len() / 2 - 1], legs[legs.len() / 2]);
        if len(&b) > len(&a) { b } else { a }
    };
    ((leg.0.0 + leg.1.0) / 2.0, (leg.0.1 + leg.1.1) / 2.0)
}

/// Where the middle of the text block of connector `id` is on the page, and the turn of the block on the page: with for the
/// connector's pose, carried out here and not by the engine.
fn label_on_page(d: &Document, id: u32) -> (P, f64) {
    let n = |name: &str| num(d, id, name);
    let block = (n("TxtWidth") / 2.0, n("TxtHeight") / 2.0);
    let (s, c) = n("TxtAngle").sin_cos();
    let (x, y) = (block.0 - n("TxtLocPinX"), block.1 - n("TxtLocPinY"));
    let in_shape = (x * c - y * s + n("TxtPinX"), x * s + y * c + n("TxtPinY"));
    let pose = Pose { pin: (n("PinX"), n("PinY")), loc: (n("LocPinX"), n("LocPinY")), angle: n("Angle"), flip_x: false, flip_y: false };
    (pose.up(in_shape), n("Angle") + n("TxtAngle"))
}

fn is_upright(turn: f64) -> bool {
    near(turn.sin(), 0.0) && near(turn.cos(), 1.0)
}

/// The text a cell holds, by inheritance.
fn text_of(d: &Document, id: u32, cell: &str) -> Option<String> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id)?);
    let found = Resolver::new(d).resolve(&sheet, &CellAddr::direct(cell))?;
    found.value.cell?.value.as_ref().map(|v| v.text().to_owned())
}

#[test]
fn the_label_of_a_file_connector_that_is_rerouted_is_upright_on_the_middle_leg_with_a_background() {
    // Connector 20 is glued to points and 21 to the whole of two shapes; each has text.
    for (moved, connector, label) in [(10, 20, "to B"), (11, 20, "to B"), (12, 21, "whole"), (13, 21, "whole")] {
        for (dx, dy) in FOUR_WAYS {
            let dir = Dir::new();
            let mut s = opened(&dir);
            run(&mut s, "shape.move", json!({"ids": [moved], "dx": dx, "dy": dy}));
            let d = doc(&s);
            let what = format!("shape {moved} moved by ({dx}, {dy}), connector {connector}");
            let (centre, turn) = label_on_page(d, connector);
            let want = middle_leg_centre(&route(d, connector));
            assert!(near_point(centre, want), "{what}: the label is centred at {centre:?}, the middle leg's middle is {want:?}");
            assert!(is_upright(turn), "{what}: the label nets a turn of {turn}");
            assert_eq!(text_of(d, connector, "TextBkgnd").as_deref(), Some("#FFFFFF"), "{what}");
            // The text itself is the file's.
            assert!(dump(d)[&format!("{connector}.<text>")].contains(label), "{what}");
        }
    }
}

#[test]
fn a_file_connector_without_text_gets_no_label_cells_and_a_fixed_one_keeps_its_label() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let before = dump(doc(&s));
    run(&mut s, "shape.move", json!({"ids": [13], "dx": 0.0, "dy": 2.0}));
    let after = dump(doc(&s));
    for cell in ["TxtPinX", "TxtPinY", "TxtAngle", "TxtLocPinX", "TextBkgnd"] {
        assert!(!after.contains_key(&format!("23.{cell}")), "connector 23 has no text and got {cell}");
    }
    assert_eq!(before["22.TxtPinX"], after["22.TxtPinX"]);
    // Connector 23 was routed again all the same.
    assert_ne!(before["23.Geometry[Some(0)].row1.X"], after["23.Geometry[Some(0)].row1.X"]);
}

#[test]
fn a_label_a_guard_pins_stays_where_it_is_and_the_move_goes_on() {
    let dir = Dir::new();
    let mut s = opened_as(&dir, "", true);
    let before = dump(doc(&s));
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 0.0, "dy": 2.0}));
    let after = dump(doc(&s));
    assert_eq!(formula_of(&before, "20.TxtPinX"), "F=Some(\"GUARD(Width*0.5)\")");
    assert_eq!(formula_of(&after, "20.TxtPinX"), formula_of(&before, "20.TxtPinX"), "the guarded formula is as it was");
    // The rest of the label was placed, and the connector routed.
    assert_eq!(text_of(doc(&s), 20, "TextBkgnd").as_deref(), Some("#FFFFFF"));
    assert!(is_upright(num(doc(&s), 20, "Angle") + num(doc(&s), 20, "TxtAngle")));
    assert!(route(doc(&s), 20).len() >= 4);
    assert_eq!(check_session(&s), Ok(()));
}

// ---- a second connector between the same shapes ---------------------------------------------------------------------------------------

/// A master of a 2 by 1 box with two connection points on its right side (rows 0 and 1) and two on its left (rows 2 and 3).
fn comb_master() -> String {
    shape(
        4,
        r#"<Cell N="PinX" V="1" F="Width*0.5"/><Cell N="PinY" V="0.5" F="Height*0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="0"/>
<Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="1" F="Height"/></Row>
<Row T="LineTo" IX="5"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
<Section N="Connection">
<Row IX="0"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.75" F="Height*0.75"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="1"><Cell N="X" V="2" F="Width"/><Cell N="Y" V="0.25" F="Height*0.25"/><Cell N="DirX" V="1"/><Cell N="DirY" V="0"/></Row>
<Row IX="2"><Cell N="X" V="0"/><Cell N="Y" V="0.75" F="Height*0.75"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row>
<Row IX="3"><Cell N="X" V="0"/><Cell N="Y" V="0.25" F="Height*0.25"/><Cell N="DirX" V="-1"/><Cell N="DirY" V="0"/></Row></Section>"#,
    )
}

#[test]
fn a_later_connector_between_the_same_shapes_takes_a_free_point_on_the_same_side_when_there_is_one() {
    let bytes = VsdxBuilder::new()
        .master(MasterSpec::new(9, "Comb").shapes(&comb_master()))
        .page(PageSpec::new(1, "Page-1").shapes(&format!(
            "{}{}",
            shape_with(30, r#" Master="9""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#),
            shape_with(31, r#" Master="9""#, r#"<Cell N="PinX" V="8"/><Cell N="PinY" V="2"/>"#)
        )))
        .build();
    let dir = Dir::new();
    let path = dir.path("comb.vsdx");
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    // The first connector takes the nearest pair: the upper point on each side, the first of the pairs as near as each other.
    let first = run(&mut s, "connector.add", json!({"from": {"id": 30}, "to": {"id": 31}}));
    assert_eq!((first["begin"].clone(), first["end"].clone()), (json!({"id": 30, "point": 0}), json!({"id": 31, "point": 2})));
    // The second would take them again and run on top of the first: it takes the free point on the same side of each shape.
    let second = run(&mut s, "connector.add", json!({"from": {"id": 30}, "to": {"id": 31}}));
    assert_eq!((second["begin"].clone(), second["end"].clone()), (json!({"id": 30, "point": 1}), json!({"id": 31, "point": 3})));
    let (a, b) = (route(doc(&s), id_of(&first)), route(doc(&s), id_of(&second)));
    assert!(!near(a[0].1, b[0].1), "the two connectors leave at different heights: {a:?} {b:?}");
    // A third finds every point on that side taken, and shares the nearest, as before.
    let third = run(&mut s, "connector.add", json!({"from": {"id": 30}, "to": {"id": 31}}));
    assert_eq!((third["begin"].clone(), third["end"].clone()), (json!({"id": 30, "point": 0}), json!({"id": 31, "point": 2})));
    // Reconnecting an end to its shape without naming a point does not count the connector's own place as taken: the second connector's other end
    // is put at (7, 2.4), nearest to the upper point of the first shape, which the first connector holds; its own lower point is free for it.
    let id = id_of(&second);
    run(&mut s, "connector.reconnect", json!({"id": id, "end": "end", "target": {"x": 7.0, "y": 2.4}}));
    let again = run(&mut s, "connector.reconnect", json!({"id": id, "end": "begin", "target": {"id": 30}}));
    assert_eq!(again["begin"], json!({"id": 30, "point": 1}), "its own point is free for it");
    // A point that is named is the point, taken or not.
    let named = run(&mut s, "connector.add", json!({"from": {"id": 30, "point": 0}, "to": {"id": 31, "point": 3}}));
    assert_eq!((named["begin"].clone(), named["end"].clone()), (json!({"id": 30, "point": 0}), json!({"id": 31, "point": 3})));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- connectors DiaCraft makes --------------------------------------------------------------------------------------------------------

#[test]
fn the_label_of_a_connector_we_make_is_upright_on_the_middle_leg_with_a_background_whenever_it_is_routed() {
    let mut s = crate::blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 8.0, 6.0), 2);
    // From the right of the first (2.75, 2) to the bottom of the second (8, 5.5): an L of legs 5.25 and 3.5, whose middle is the longer.
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 3}, "to": {"id": 2, "point": 2}})));
    let points = route(doc(&s), c);
    assert_eq!(points.len(), 3, "{points:?}");
    assert!(near_point(label_on_page(doc(&s), c).0, (5.375, 2.0)), "{:?}", label_on_page(doc(&s), c).0);
    run(&mut s, "shape.set_text", json!({"id": c, "text": "label"}));
    for (dx, dy) in [(0.0, 0.0), (0.0, -3.0), (3.0, 0.0), (-6.0, 4.0), (0.0, 6.0)] {
        run(&mut s, "shape.move", json!({"ids": [2], "dx": dx, "dy": dy}));
        let d = doc(&s);
        let (centre, turn) = label_on_page(d, c);
        let points = route(d, c);
        assert!(near_point(centre, middle_leg_centre(&points)), "moved by ({dx}, {dy}): {centre:?} for {points:?}");
        assert!(is_upright(turn), "moved by ({dx}, {dy}): a turn of {turn}");
        assert_eq!(text_of(d, c, "TextBkgnd").as_deref(), Some("#FFFFFF"));
    }
}

// ---- undo, the route styles, a broken connector and a group ---------------------------------------------------------------------------

#[test]
fn a_move_that_reroutes_the_files_connectors_is_one_undo_step() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    let read = doc(&s).clone();
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 1.0, "dy": 2.0}));
    assert_ne!(doc(&s), &read);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &read, "one undo takes the move, the ends placed, the routes and the labels");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_file_connector_of_the_centre_to_centre_style_keeps_its_rows_and_only_its_end_moves() {
    let dir = Dir::new();
    let mut s = opened_with(&dir, r#"<Cell N="RouteStyle" V="16"/>"#);
    let before = dump(doc(&s));
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 0.0, "dy": 2.0}));
    let d = doc(&s);
    assert!(near_point(end_of(d, 20).1, point(d, 11, 1)), "the end follows its shape");
    assert_eq!(geometry(d, 20).len(), 2, "The rows of a centre to centre connector are the file's");
    assert!(!dump(d).contains_key("20.TextBkgnd"));
    assert_scope(&before, &dump(d), 11, &[20, 22, 23]);
}

#[test]
fn a_file_connector_of_a_straight_style_is_routed_straight_when_its_shape_moves() {
    let dir = Dir::new();
    let mut s = opened_with(&dir, r#"<Cell N="RouteStyle" V="2"/>"#);
    let before = dump(doc(&s));
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 0.0, "dy": 2.0}));
    let d = doc(&s);
    for connector in [20, 23] {
        let rows = geometry(d, connector);
        assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo"], "connector {connector}: one leg");
        let (begin, end) = end_of(d, connector);
        let points = route(d, connector);
        assert!(near_point(points[0], begin) && near_point(points[points.len() - 1], end), "connector {connector}: {points:?}");
    }
    assert!(near_point(end_of(d, 20).1, point(d, 11, 1)), "the end follows its shape");
    assert_eq!(text_of(d, 20, "TextBkgnd").as_deref(), Some("#FFFFFF"), "A route made again places the label");
    assert_scope(&before, &dump(d), 11, &[]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_file_connector_of_a_curved_appearance_is_routed_curved_when_its_shape_moves() {
    let dir = Dir::new();
    let mut s = opened_with(&dir, r#"<Cell N="LineRouteExt" V="2"/>"#);
    let before = dump(doc(&s));
    run(&mut s, "shape.move", json!({"ids": [11], "dx": 0.0, "dy": 2.0}));
    let d = doc(&s);
    let rows = geometry(d, 20);
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "NURBSTo"], "A curve is NURBSTo rows");
    let (begin, end) = end_of(d, 20);
    let points = route(d, 20);
    assert!(near_point(points[0], begin) && near_point(points[points.len() - 1], end), "{points:?}");
    assert_scope(&before, &dump(d), 11, &[]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A page with a box (10), and a connector (11) whose begin formula names a shape that is not there, so that its cells hold an error.
fn broken_drawing() -> Vec<u8> {
    let a = shape_with(10, r#" Master="7""#, r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>"#);
    let c = shape(
        11,
        r#"<Cell N="BeginX" V="3" F="PAR(PNT(Sheet.99!Connections.X1,Sheet.99!Connections.Y1))"/><Cell N="BeginY" V="2" F="PAR(PNT(Sheet.99!Connections.X1,Sheet.99!Connections.Y1))"/>
<Cell N="EndX" V="6" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/><Cell N="EndY" V="2" F="PAR(PNT(Sheet.10!Connections.X1,Sheet.10!Connections.Y1))"/>
<Cell N="PinX" V="4.5" F="(BeginX+EndX)/2"/><Cell N="PinY" V="2" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="3" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="1.5" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="0" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="3"/><Cell N="Y" V="0"/></Row></Section>"#,
    );
    let connects = r#"<Connect FromSheet="11" FromCell="BeginX" FromPart="9" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>
<Connect FromSheet="11" FromCell="EndX" FromPart="12" ToSheet="10" ToCell="Connections.X1" ToPart="100"/>"#;
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&box_master()))
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{a}{c}")).connects(connects))
        .build()
}

#[test]
fn a_connector_of_a_file_whose_glue_does_not_work_out_does_not_stop_its_shape_moving() {
    let dir = Dir::new();
    let path = dir.path("broken.vsdx");
    std::fs::write(&path, broken_drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 1.0, "dy": 1.0}));
    assert_eq!((num(doc(&s), 10, "PinX"), num(doc(&s), 10, "PinY")), (3.0, 3.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A group (40) of one box (41), and a box (43), with a connector (42) from the member to the box, glued the way a file glues them.
fn grouped_drawing() -> Vec<u8> {
    let member = shape_with(41, r#" Master="7""#, r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/>"#);
    let group = shape_with(
        40,
        r#" Type="Group""#,
        &format!(
            r#"<Cell N="PinX" V="4"/><Cell N="PinY" V="4"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="1"/><Cell N="LocPinY" V="0.5"/><Shapes>{member}</Shapes>"#
        ),
    );
    let target = shape_with(43, r#" Master="7""#, r#"<Cell N="PinX" V="10"/><Cell N="PinY" V="4"/>"#);
    let c = connector(42, (5.0, 4.0), (9.0, 4.0), Glue::Point(41, 0), Glue::Point(43, 1), None, false);
    let connects = format!("{}\n{}", Glue::Point(41, 0).connect(42, true), Glue::Point(43, 1).connect(42, false));
    VsdxBuilder::new()
        .master(MasterSpec::new(7, "Box").shapes(&box_master()))
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{group}{target}{c}")).connects(&connects))
        .build()
}

#[test]
fn moving_a_group_routes_again_the_connector_glued_to_one_of_its_members() {
    let dir = Dir::new();
    let path = dir.path("grouped.vsdx");
    std::fs::write(&path, grouped_drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    assert_eq!(geometry(doc(&s), 42).len(), 2);
    run(&mut s, "shape.move", json!({"ids": [40], "dx": 0.0, "dy": 3.0}));
    let d = doc(&s);
    let (begin, end) = end_of(d, 42);
    assert!(begin.1 > 6.9 && near(end.1, 4.0), "the begin follows the group up: {begin:?} {end:?}");
    let points = route(d, 42);
    assert!(points.len() >= 3, "routed again, not the file's two rows: {points:?}");
    assert!(near_point(points[0], begin) && near_point(points[points.len() - 1], end), "{points:?}");
    assert_eq!(check_session(&s), Ok(()));
}

/// A box with no connection point (10) at (2, 2), and a file's connector (24) glued at its begin to the whole of it (`_WALKGLUE`, `ToPart` 3) and free at
/// its end (6, 5). The begin is saved at (3, 2.3), on the box's right side but not where the engine's rule (the point of the outline nearest the other
/// end, (3, 2.5)) would put it: an end the engine places again is seen to move.
fn half_glued_drawing() -> Vec<u8> {
    let target = shape_with(10, r#" Master="8""#, &format!(r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/>{LINE}"#));
    let (b, e) = ((3.0_f64, 2.3_f64), (6.0_f64, 5.0_f64));
    let (width, angle) = ((e.0 - b.0).hypot(e.1 - b.1), (e.1 - b.1).atan2(e.0 - b.0));
    let walk = "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)";
    let c = shape(
        24,
        &format!(
            r#"<Cell N="BeginX" V="{bx}" F="{walk}"/><Cell N="BeginY" V="{by}" F="{walk}"/><Cell N="EndX" V="{ex}"/><Cell N="EndY" V="{ey}"/>
<Cell N="PinX" V="{px}" F="(BeginX+EndX)/2"/><Cell N="PinY" V="{py}" F="(BeginY+EndY)/2"/>
<Cell N="Width" V="{width}" F="SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)"/><Cell N="Height" V="0"/>
<Cell N="LocPinX" V="{h}" F="Width*0.5"/><Cell N="LocPinY" V="0" F="Height*0.5"/><Cell N="Angle" V="{angle}" F="ATAN2(EndY-BeginY,EndX-BeginX)"/>
<Cell N="EndArrow" V="4"/>{LINE}<Cell N="BegTrigger" V="2" F="_XFTRIGGER(Sheet.10!EventXFMod)"/>
<Section N="Geometry" IX="0"><Cell N="NoFill" V="1"/><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
<Row T="LineTo" IX="2"><Cell N="X" V="{width}"/><Cell N="Y" V="0"/></Row></Section>"#,
            bx = b.0,
            by = b.1,
            ex = e.0,
            ey = e.1,
            px = (b.0 + e.0) / 2.0,
            py = (b.1 + e.1) / 2.0,
            h = width / 2.0,
        ),
    );
    let connects = Glue::Whole(10).connect(24, true);
    VsdxBuilder::new()
        .master(MasterSpec::new(8, "Blob").shapes(&blob_master()))
        .page(PageSpec::new(1, "Page-1").shapes(&format!("{target}{c}")).connects(&connects))
        .build()
}

#[test]
fn a_connector_with_one_end_glued_to_the_whole_of_a_shape_keeps_that_end_when_the_command_is_about_the_connector() {
    // The glued end is where the file saved it, and no command that changes the connector itself puts it by the rule: only a command that
    // changes the shape it is glued to does, or one that changes a group the connector is in. ROTATE-1D-1's exact angle holds.
    let commands = [
        ("shape.rotate", json!({"ids": [24], "angle": 90})),
        ("shape.rotate", json!({"ids": [24], "angle": 33})),
        ("shape.rotate", json!({"ids": [24], "by": 45})),
        ("shape.flip", json!({"ids": [24], "axis": "horizontal"})),
        ("shape.flip", json!({"ids": [24], "axis": "vertical"})),
        ("shape.move", json!({"ids": [24], "dx": 1.0, "dy": -0.5})),
        ("shape.align", json!({"ids": [10, 24], "how": "bottom"})),
        ("connector.reconnect", json!({"id": 24, "end": "end", "target": {"x": 8.0, "y": 6.0}})),
    ];
    for (command, params) in commands {
        let dir = Dir::new();
        let path = dir.path("half.vsdx");
        std::fs::write(&path, half_glued_drawing()).unwrap();
        let mut s = Session::new();
        run(&mut s, "file.open", json!({"path": path}));
        let (begin, _) = end_of(doc(&s), 24);
        assert!(near_point(begin, (3.0, 2.3)), "{begin:?}");
        run(&mut s, command, params.clone());
        let d = doc(&s);
        let (now, end) = end_of(d, 24);
        assert!(near_point(now, begin), "{command} {params}: the glued end went from {begin:?} to {now:?}");
        assert_eq!(d.page(1).unwrap().connects.len(), 1, "{command} {params}: and stays glued");
        if command == "shape.rotate" && params.get("angle").is_some() {
            let want = params["angle"].as_f64().unwrap().to_radians();
            assert!(near(num(d, 24, "Angle"), want), "{command} {params}: the angle is {} and not {want}", num(d, 24, "Angle"));
            // The free end turned about the glued one: the line from the glued end to it points the way asked.
            assert!(near((end.1 - now.1).atan2(end.0 - now.0), want), "{command} {params}: {end:?}");
        }
        assert_eq!(check_session(&s), Ok(()), "{command} {params}");
    }
    // The shape it is glued to moving is another matter: the end follows it, to the point nearest the other end.
    let dir = Dir::new();
    let path = dir.path("half.vsdx");
    std::fs::write(&path, half_glued_drawing()).unwrap();
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    run(&mut s, "shape.move", json!({"ids": [10], "dx": 0.0, "dy": 0.0001}));
    let (begin, _) = end_of(doc(&s), 24);
    assert!(near(begin.0, 3.0) && near(begin.1, 2.5001), "{begin:?}");
}
