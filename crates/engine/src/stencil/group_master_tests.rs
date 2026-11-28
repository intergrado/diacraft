//! Group masters and container masters (tests first). The masters are made here: no stencil ships one yet, so each test drops a master of its own through
//! `draw::drop_shape_above`, the call `shape.drop` makes for a built-in master, and edits what it dropped by the commands a person's edits are.
//!
//! * `t.card` is a group of three compartments (`kit::compartments`): each as tall as its text, stacked from the top, the group as tall as the three together, holding `LockHeight`.
//! * `t.badge` is a group of two members placed by formulas of the group's own frame, with a field in one of them.
//! * `t.pen` is a container master: a tab over a body, typed `Container`, with a heading and a margin and no style row.

use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, EvalCtx, Value as Calc};
use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, ShapeType, SheetRef};
use diacraft_render::{Renderer, View};
use serde_json::{Value, json};

use super::container::Side;
use super::flowchart_tests::Dir;
use super::group_master::{Factor, MemberFrame, MemberSpec, drop_factor};
use super::kit;
use super::*;
use crate::Session;
use crate::draw;

// ---- the masters made for these tests -------------------------------------------------------------------------------------------------

const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

static HEAD_DRESS: Dress = Dress { text: TextSpec::Plain("Head"), chars: &[CharRow { size_pt: 11.0, style: 1, color: None }], ..Dress::NONE };
static BODY_DRESS: Dress = Dress { text: TextSpec::Plain("Body"), ..Dress::NONE };
static FOOT_DRESS: Dress = Dress { text: TextSpec::Plain("Foot"), ..Dress::NONE };

/// Three compartments, each as tall as its text and at least a twentieth of an inch.
const CARD_FRAMES: [MemberFrame; 3] = kit::compartments([0.05, 0.05, 0.05]);
static CARD_MEMBERS: [MemberSpec; 3] = [
    MemberSpec { name: "Head", frame: CARD_FRAMES[0], outline: Outline::Polygon(RECT), points: &[], dress: Some(&HEAD_DRESS) },
    MemberSpec { name: "Body", frame: CARD_FRAMES[1], outline: Outline::Polygon(RECT), points: &[], dress: Some(&BODY_DRESS) },
    MemberSpec { name: "Foot", frame: CARD_FRAMES[2], outline: Outline::Polygon(RECT), points: &[], dress: Some(&FOOT_DRESS) },
];
static CARD_DRESS: Dress = Dress {
    data: &[FieldDef { row: "Kind", label: "Kind", kind: FieldKind::String, value: "Card", prompt: "" }],
    select_mode: Some(1),
    text_edit_target: Some(false),
    ..Dress::NONE
};
static CARD_MORE: More = More { dress: Some(&CARD_DRESS), hidden: false, brings: &[] };
static CARD: MasterSpec = MasterSpec {
    id: "t.card",
    name: "Card",
    width: 1.6,
    height: 0.92,
    identity: (200, 70),
    outline: Outline::Group { members: &CARD_MEMBERS, text: TextBox::FULL },
    points: &kit::GRID3,
    more: &CARD_MORE,
};

/// The same three compartments with the name last in the table (the member typing goes to) and first from the top: the table's order is the members', the rank is the stack's.
const KLASS_FRAMES: [MemberFrame; 3] = kit::compartments([0.05, 0.05, 0.05]);
static KLASS_MEMBERS: [MemberSpec; 3] = [
    MemberSpec { name: "Operations", frame: KLASS_FRAMES[2], outline: Outline::Polygon(RECT), points: &[], dress: Some(&FOOT_DRESS) },
    MemberSpec { name: "Attributes", frame: KLASS_FRAMES[1], outline: Outline::Polygon(RECT), points: &[], dress: Some(&BODY_DRESS) },
    MemberSpec { name: "Name", frame: KLASS_FRAMES[0], outline: Outline::Polygon(RECT), points: &[], dress: Some(&HEAD_DRESS) },
];
static KLASS: MasterSpec = MasterSpec {
    id: "t.klass",
    name: "Klass",
    width: 1.6,
    height: 0.92,
    identity: (200, 74),
    outline: Outline::Group { members: &KLASS_MEMBERS, text: TextBox::FULL },
    points: &kit::GRID3,
    more: &CARD_MORE,
};

/// A disc with a mark in its middle, placed by shares of the group.
static BADGE_LABEL: [FieldText; 1] = [FieldText { formula: "Sheet.5!Prop.Label", shown: "Label", char_row: 0, line_after: false }];
static MARK_DRESS: Dress = Dress { text: TextSpec::Fields(&BADGE_LABEL), chars: &[CharRow { size_pt: 9.0, style: 0, color: None }], ..Dress::NONE };
static BADGE_MEMBERS: [MemberSpec; 2] = [
    MemberSpec {
        name: "Disc",
        frame: MemberFrame::Formulas { pin_x: "Sheet.5!Width*0.5", pin_y: "Sheet.5!Height*0.5", width: "Sheet.5!Width", height: "Sheet.5!Height" },
        outline: Outline::Ellipse,
        points: &[],
        dress: None,
    },
    MemberSpec {
        name: "Mark",
        frame: MemberFrame::Formulas {
            pin_x: "Sheet.5!Width*0.5",
            pin_y: "Sheet.5!Height*0.5",
            width: "Sheet.5!Width*0.4",
            height: "Sheet.5!Height*0.4",
        },
        outline: Outline::Polygon(RECT),
        points: &[],
        dress: Some(&MARK_DRESS),
    },
];
static BADGE_DRESS: Dress =
    Dress { data: &[FieldDef { row: "Label", label: "Label", kind: FieldKind::String, value: "Label", prompt: "" }], ..Dress::NONE };
static BADGE_MORE: More = More { dress: Some(&BADGE_DRESS), hidden: false, brings: &[] };
static BADGE: MasterSpec = MasterSpec {
    id: "t.badge",
    name: "Badge",
    width: 1.0,
    height: 1.0,
    identity: (200, 71),
    outline: Outline::Group { members: &BADGE_MEMBERS, text: TextBox::FULL },
    points: &kit::SIDES,
    more: &BADGE_MORE,
};

/// A tab over a body: the body is the whole box under a tab 0.4 inch wide and 0.2 inch tall at its top left.
fn tabbed() -> Vec<OwnedPath> {
    let owned = |fill, segs| OwnedPath { fill, segs, no_show: Flag::Off, no_line: Flag::Off };
    let (z, w, top) = (fw(0.0), fw(1.0), mix(0.0, 1.0).plus(fk(-0.2)));
    vec![
        owned(true, vec![Seg::Move(z, z), Seg::Line(w, z), Seg::Line(w, top), Seg::Line(z, top), Seg::Line(z, z)]),
        owned(true, vec![Seg::Move(z, top), Seg::Line(fk(0.4), top), Seg::Line(fk(0.4), fh(1.0)), Seg::Line(z, fh(1.0)), Seg::Line(z, top)]),
    ]
}

static PEN_DRESS: Dress =
    Dress { text: TextSpec::Plain("Pen"), role: Role::Container { heading: Side::Top, margin_in: 0.25 }, obj_type: Some(1), ..Dress::NONE };
static PEN_MORE: More = More { dress: Some(&PEN_DRESS), hidden: false, brings: &[] };
static PEN: MasterSpec = MasterSpec {
    id: "t.pen",
    name: "Pen",
    width: 3.0,
    height: 2.0,
    identity: (200, 72),
    outline: Outline::Code { build: tabbed, text: TextBox::FULL },
    points: &kit::GRID3,
    more: &PEN_MORE,
};

// ---- helpers --------------------------------------------------------------------------------------------------------------------------

fn blank() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn page_of(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// Drops `spec` centred at (`x`, `y`) as `shape.drop` does (it cannot find a master that is in no stencil): one undo step. → the new shape's `ID`.
fn drop_at(s: &mut Session, spec: &MasterSpec, x: f64, y: f64) -> u32 {
    let page = page_of(s);
    let above = s.id_floor(page);
    let info = s.edit("Drop shape", |doc| draw::drop_shape_above(doc, page, spec, x, y, above).map_err(|p| p.into_engine("shape.drop"))).unwrap();
    s.note_id(page, info.id);
    info.id
}

fn rectangle(s: &mut Session, x: f64, y: f64) -> u32 {
    drop_at(s, find_master("rectangle").unwrap(), x, y)
}

/// What a cell of a shape works out to now: the formula is evaluated live, whatever value the file or the last recalculation cached.
fn live(doc: &Document, page: u32, id: u32, cell: &str) -> f64 {
    let at = doc.find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id}"));
    let mut ctx = EvalCtx::new(crate::measure::eval_options(), Budget::default());
    match diacraft_calc::evaluate(doc, &CellAddress::new(SheetRef::Shape(at), CellAddr::direct(cell)), &mut ctx) {
        Calc::Number(q) => q.stored,
        other => panic!("{cell} of shape {id}: {other:?}"),
    }
}

/// What a cell of a shape reads through its master, as the last recalculation left it: the cached value.
fn cached(doc: &Document, page: u32, id: u32, cell: &str) -> f64 {
    let at = doc.find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(doc).resolve(&SheetRef::Shape(at), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("no cell {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("no number in {cell} of shape {id}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn shape_of(doc: &Document, page: u32, id: u32) -> Arc<diacraft_doc::Shape> {
    let at = doc.find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id}"));
    Arc::clone(doc.shape(&at).unwrap())
}

/// The names of the `Geometry` sections a shape holds itself.
fn own_geometry(shape: &diacraft_doc::Shape) -> usize {
    shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count()
}

fn text_of(doc: &Document, page: u32, id: u32) -> String {
    draw::text_view(doc, page, id).unwrap().chars
}

// ---- 1 to 4: the formulas of the members read the instance ----------------------------------------------------------------------------

#[test]
fn a_member_formula_naming_master_shape_5_reads_the_instance() {
    // The members' `Sheet.5!Width` is the instance's, and no shape of the page that happens to have the `ID` 5.
    let mut s = blank();
    let page = page_of(&s);
    for k in 0..5 {
        rectangle(&mut s, 1.0 + f64::from(k), 1.0);
    }
    run(&mut s, "sheet.set_cell", json!({ "id": 5, "cell": "Width", "formula": "9 in" }));
    let card = drop_at(&mut s, &CARD, 5.0, 6.0);
    run(&mut s, "sheet.set_cell", json!({ "id": card, "cell": "Width", "formula": "4 in" }));
    let doc = s.document().unwrap();
    assert!(near(live(doc, page, 5, "Width"), 9.0), "the page's shape 5 is nine inches wide");
    for member in [card + 1, card + 2, card + 3] {
        assert!(
            near(live(doc, page, member, "Width"), 4.0),
            "member {member}'s width is the instance's (4), not shape 5's (9) nor the master's (1.6)"
        );
        assert!(near(cached(doc, page, member, "Width"), 4.0), "and so is the value the recalculation left");
        assert!(near(live(doc, page, member, "PinX"), 2.0), "the middle of the group's frame, from the instance's width");
        assert!(near(cached(doc, page, member, "PinX"), 2.0));
    }
}

#[test]
fn a_top_level_formula_summing_members_reads_the_instances_members() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let members = [card + 1, card + 2, card + 3];
    let before = live(s.document().unwrap(), page, card, "Height");
    let was = members.map(|m| live(s.document().unwrap(), page, m, "Height"));
    run(&mut s, "shape.set_text", json!({ "id": members[1], "text": "a\nb\nc\nd\ne" }));
    let doc = s.document().unwrap();
    let heights = members.map(|m| live(doc, page, m, "Height"));
    let after = live(doc, page, card, "Height");
    assert!(after > before + 0.3, "five lines make the group taller: {before} to {after}");
    assert!(near(after, heights.iter().sum::<f64>()), "the group is as tall as its three compartments: {after} against {heights:?}");
    assert!(near(cached(doc, page, card, "Height"), after), "the recalculation wrote the instance's result");
    assert!(
        heights[1] > was[1] + 0.3 && near(heights[0], was[0]) && near(heights[2], was[2]),
        "only the compartment that was typed in grew: {was:?} to {heights:?}"
    );
}

#[test]
fn a_member_naming_another_member_reads_the_instances_copy() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let (head, body, foot) = (card + 1, card + 2, card + 3);
    run(&mut s, "shape.set_text", json!({ "id": body, "text": "a\nb\nc\nd" }));
    let doc = s.document().unwrap();
    let (height, h) = (live(doc, page, card, "Height"), |m: u32| live(doc, page, m, "Height"));
    // Each compartment stands under the one above it: the pin is the group's top less the heights above and half its own.
    let top = |m: u32| live(doc, page, m, "PinY") + h(m) / 2.0;
    assert!(near(top(head), height), "the first is at the top of the group");
    assert!(near(top(body), top(head) - h(head)), "the second is under the first");
    assert!(near(top(foot), top(body) - h(body)), "the third is under the second: it follows the copy of the second that the instance has");
    assert!(near(top(foot) - h(foot), 0.0), "and the last ends at the group's bottom");
    for m in [head, body, foot] {
        assert!(near(cached(doc, page, m, "PinY"), live(doc, page, m, "PinY")), "member {m}: the recalculation wrote what the formula gives");
    }
}

#[test]
fn two_instances_on_one_page_do_not_read_each_other() {
    let mut s = blank();
    let page = page_of(&s);
    let (a, b) = (drop_at(&mut s, &CARD, 3.0, 5.0), drop_at(&mut s, &CARD, 7.0, 5.0));
    let was = live(s.document().unwrap(), page, b, "Height");
    run(&mut s, "shape.set_text", json!({ "id": a + 2, "text": "a\nb\nc\nd\ne\nf" }));
    run(&mut s, "sheet.set_cell", json!({ "id": b, "cell": "Width", "formula": "3 in" }));
    let doc = s.document().unwrap();
    assert!(live(doc, page, a, "Height") > was + 0.4, "the first grew");
    assert!(near(live(doc, page, b, "Height"), was), "the second did not: {} against {was}", live(doc, page, b, "Height"));
    assert!(near(live(doc, page, a + 1, "Width"), 1.6) && near(live(doc, page, b + 1, "Width"), 3.0), "each member has its own group's width");
    // Undo gives both back as they were.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let doc = s.document().unwrap();
    assert!(near(live(doc, page, a, "Height"), was) && near(live(doc, page, b + 1, "Width"), 1.6));
}

// ---- 5: typing in a compartment -------------------------------------------------------------------------------------------------------

#[test]
fn typing_a_line_in_a_compartment_makes_the_group_taller_in_the_same_undo_step() {
    let mut s = blank();
    let page = page_of(&s);
    let (one, two) = (drop_at(&mut s, &CARD, 3.0, 5.0), drop_at(&mut s, &CARD, 7.0, 5.0));
    let before = s.document().unwrap().clone();
    run(&mut s, "shape.set_text", json!({ "id": one + 2, "text": "one line" }));
    let after_one = s.document().unwrap().clone();
    run(&mut s, "shape.set_text", json!({ "id": two + 2, "text": "one line\nand a second" }));
    let doc = s.document().unwrap();
    // Never an absolute height (the faces on the machine decide a line's): the same text of two lines makes it taller than of one, by about a line.
    let (h1, h2) = (live(doc, page, one, "Height"), live(doc, page, two, "Height"));
    assert!(h2 > h1 + 0.1, "two lines make the group taller than one: {h1} and {h2}");
    assert!(near(live(doc, page, two + 2, "Height") - live(doc, page, one + 2, "Height"), h2 - h1), "all of it is the compartment's");
    // It is the one step of the edit: undo gives the group as it was with the text of one line, and again as it was before any.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap().page(page).unwrap().shapes.len(), 2);
    assert!(
        near(live(s.document().unwrap(), page, two, "Height"), live(&before, page, two, "Height")),
        "one undo takes the two lines and the height away"
    );
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before, "and the first edit the same");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document().unwrap(), &after_one, "redo brings back the text and the height together");
    // The shape holds `LockHeight`, so a resize leaves its height to its text.
    assert!(near(cached(s.document().unwrap(), page, one, "LockHeight"), 1.0), "a group of compartments keeps its height");
}

// ---- 6: the drop ----------------------------------------------------------------------------------------------------------------------

#[test]
fn a_built_in_group_master_drops_with_every_member_listed_and_a_local_copy_of_its_geometry() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let doc = s.document().unwrap();
    let group = shape_of(doc, page, card);
    assert_eq!(group.kind, ShapeType::Group);
    let own: Vec<&str> = group.sheet.cells.keys().map(|n| n.as_str()).collect();
    let mut own_sorted = own.clone();
    own_sorted.sort_unstable();
    assert_eq!(own_sorted, ["PinX", "PinY"], "the instance holds only its pin (the rest is the master's)");
    assert_eq!(own_geometry(&group), 0, "and no geometry of its own: a group draws through its members");
    assert_eq!(group.children.len(), 3, "a member for each member of the master");
    let master = doc.master(group.master.unwrap()).unwrap();
    let top = master.shapes.get(0).unwrap();
    assert_eq!((master.shapes.len(), top.id, top.kind.clone(), top.children.len()), (1, MASTER_SHAPE_ID, ShapeType::Group, 3));
    assert_eq!(top.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count(), 0, "the master's group has no geometry of its own");
    for (k, member) in group.children.iter().enumerate() {
        let k = u32::try_from(k).unwrap();
        assert_eq!(
            (member.id, member.master_shape),
            (card + 1 + k, Some(MASTER_SHAPE_ID + 1 + k)),
            "member {k} stands for the master's shape {}",
            6 + k
        );
        assert_eq!(member.kind, ShapeType::Shape);
        assert!(member.master.is_none(), "a member has no master of its own: its group's is its");
        assert!(member.sheet.cells.is_empty(), "member {k} holds no cell: its frame is the master's formulas, worked out on the instance");
        assert!(
            own_geometry(member) >= 1,
            "member {k} holds a local copy of its master shape's geometry, so that a reader that does not know the master draws it"
        );
        let geometry = member.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry").unwrap();
        assert!(
            geometry.rows.iter().flat_map(|r| r.cells.values()).all(|c| !matches!(c.formula, FormulaSlot::Local(_))),
            "the copy inherits the formulas of the master's rows (a plain value is held as a constant): none is copied down"
        );
        assert!(own_geometry(top.children.get(usize::try_from(k).unwrap()).unwrap()) >= 1, "the master's member holds the outline");
    }
    // The members' text is the master's own, and each is named for its place in the master.
    assert_eq!(text_of(doc, page, card + 1), "", "an instance's member holds no text of its own until it is typed in");
    let names: Vec<String> = top.children.iter().map(|m| m.name.as_deref().unwrap_or("").to_owned()).collect();
    assert_eq!(names, ["Head", "Body", "Foot"]);
    // The master's cached values are what its formulas give at its default size: a recalculation finds nothing to change.
    let mut again = doc.clone();
    let report = diacraft_calc::commit(&mut again, &Budget::default(), diacraft_calc::EvalOptions::default());
    assert_eq!((report.written, report.refused), (0, 0), "every cached value of the master and of the page is what its formula gives");
    // The group's connection points are on the group.
    assert_eq!(draw::connection_points(doc, page, card).len(), 12);
}

// ---- 7: GUARD -------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_member_alone_is_not_moved_and_the_refusal_says_why() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let before = s.document().unwrap().clone();
    let said = s.execute("shape.move", &json!({ "ids": [card + 2], "dx": 0.5, "dy": 0.0 })).unwrap_err().to_string();
    assert!(said.contains("GUARD") && said.contains("PinX"), "the refusal names the cell and the protection: {said}");
    assert_eq!(s.document().unwrap(), &before, "nothing was changed");
    // The group moves with its members: their pins are formulas of its frame.
    let corners = |doc: &Document, id: u32| draw::shape_corners(doc, page, id).unwrap();
    let was = corners(s.document().unwrap(), card + 2);
    run(&mut s, "shape.move", json!({ "ids": [card], "dx": 1.0, "dy": -0.5 }));
    let now = corners(s.document().unwrap(), card + 2);
    assert!(
        was.iter().zip(now.iter()).all(|(a, b)| near(a.0 + 1.0, b.0) && near(a.1 - 0.5, b.1)),
        "the member went with the group: {was:?} to {now:?}"
    );
}

// ---- 8: files -------------------------------------------------------------------------------------------------------------------------

fn pixels(doc: &Document) -> (u32, u32, Vec<u8>) {
    let page = doc.pages.get(0).unwrap().id;
    let view = View::fit_page(doc, page, 50.0).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    let frame = renderer.render(doc, page, &view);
    assert!(frame.skipped.is_empty(), "everything was drawn: {:?}", frame.skipped);
    (frame.width, frame.height, frame.rgba_premul)
}

#[test]
fn a_group_master_round_trips_through_vsdx_odg_and_diacraft() {
    for ext in ["vsdx", "diacraft", "odg"] {
        let dir = Dir::new();
        let mut s = blank();
        let page = page_of(&s);
        let card = drop_at(&mut s, &CARD, 4.0, 5.0);
        run(&mut s, "shape.set_text", json!({ "id": card + 2, "text": "typed\nin two lines" }));
        run(&mut s, "shape.set_text", json!({ "id": card + 1, "text": "Name" }));
        let path = dir.path(&format!("x.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut again = Session::new();
        run(&mut again, "file.open", json!({ "path": path }));
        let (made, read) = (s.document().unwrap(), again.document().unwrap());
        let read_page = read.pages.get(0).unwrap().id;
        let what = format!("as {ext}");
        let (a, b) = (pixels(made), pixels(read));
        assert_eq!((a.0, a.1, a.2.len()), (b.0, b.1, b.2.len()), "{what}: the picture's size");
        let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
        assert!(worst <= 2, "{what}: the file draws the group differently from the session (worst channel {worst})");
        assert!(a.2.iter().any(|v| *v != 255), "{what}: the picture is not blank");
        assert_eq!(text_of(read, read_page, card + 1), "Name", "{what}: the typed text");
        assert_eq!(text_of(read, read_page, card + 2), "typed\nin two lines", "{what}: the typed text");
        // The group is the same group: the master's GUID is kept, and the points are where they were.
        let (m, r) = (shape_of(made, page, card), shape_of(read, read_page, card));
        assert_eq!(
            made.master(m.master.unwrap()).unwrap().unique_id,
            read.master(r.master.unwrap()).unwrap().unique_id,
            "{what}: the master keeps its GUID"
        );
        assert_eq!(r.children.len(), 3, "{what}: the members");
        let (pa, pb) = (draw::connection_points(made, page, card), draw::connection_points(read, read_page, card));
        assert_eq!(pa.len(), pb.len(), "{what}: the connection points");
        for (x, y) in pa.iter().zip(pb.iter()) {
            assert!(near(x.x, y.x) && near(x.y, y.y), "{what}: a point moved: {x:?} to {y:?}");
        }
        // The text is edited again after the reopen. In a `.vsdx` and a `.diacraft` the group still follows it: the master's formulas came through the file. An `.odg` writes a group as its
        // members placed on the page (a `draw:g` has no frame of its own), so the reopened group and its members hold their places as numbers and keep them.
        let was = live(read, read_page, card, "Height");
        run(&mut again, "shape.set_text", json!({ "id": card + 2, "text": "typed\nin\nfour\nlines" }));
        let now = live(again.document().unwrap(), read_page, card, "Height");
        if ext == "odg" {
            assert!(near(now, was), "{what}: an .odg group keeps the size it was saved at");
            assert!(pixels(again.document().unwrap()).2.iter().any(|v| *v != 255), "{what}: and still draws");
        } else {
            assert!(now > was + 0.1, "{what}: the group grows with its text after a reopen: {was} to {now}");
        }
    }
}

// ---- 9 and 10: container masters ------------------------------------------------------------------------------------------------------

#[test]
fn a_container_master_takes_a_shape_dropped_inside_and_moves_it() {
    let mut s = blank();
    let page = page_of(&s);
    let pen = drop_at(&mut s, &PEN, 5.0, 5.0);
    let info = run(&mut s, "container.info", json!({ "ids": [pen] }));
    assert_eq!(info["containers"][0]["id"], pen, "an instance of a container master is a container: {info}");
    assert_eq!(info["containers"][0]["style"], Value::Null, "it has no style of the gallery: its look is its master's");
    // A shape dropped inside joins it.
    let dropped = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 5.0, "y": 4.8 }));
    let inner = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
    let info = run(&mut s, "container.info", json!({ "ids": [pen] }));
    assert_eq!(info["containers"][0]["members"], json!([inner]), "{info}");
    // A move of the container takes its member.
    let was = draw::shape_corners(s.document().unwrap(), page, inner).unwrap();
    run(&mut s, "shape.move", json!({ "ids": [pen], "dx": 1.5, "dy": 1.0 }));
    let now = draw::shape_corners(s.document().unwrap(), page, inner).unwrap();
    assert!(was.iter().zip(now.iter()).all(|(a, b)| near(a.0 + 1.5, b.0) && near(a.1 + 1.0, b.1)), "the member went with the container");
    // A shape dropped outside does not.
    let outside = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 9.0, "y": 9.0 }));
    let outside = u32::try_from(outside["id"].as_u64().unwrap()).unwrap();
    let info = run(&mut s, "container.info", json!({ "ids": [pen] }));
    assert_eq!(info["containers"][0]["members"], json!([inner]), "{outside} is not a member: {info}");
}

/// The geometry a shape draws: every cell of every row of every section it has once its master is put together with it, as (where, formula text, value).
fn drawn_geometry(doc: &Document, page: u32, id: u32) -> Vec<(String, String, String)> {
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    let res = Resolver::new(doc);
    let sheet = SheetRef::Shape(at);
    let mut out = Vec::new();
    for key in res.effective_sections(&sheet, "Geometry").unwrap_or_default() {
        for row in res.effective_rows(&sheet, &key).unwrap_or_default() {
            for (name, found) in &row.cells {
                let formula = found.formula.cell.and_then(|c| c.formula.text()).unwrap_or("").to_owned();
                let value = found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default();
                out.push((format!("{:?} {:?} {name}", key.ix, row.key), formula, value));
            }
        }
    }
    out
}

#[test]
fn every_container_command_leaves_a_container_masters_outline_as_its_master_draws_it() {
    let mut s = blank();
    let page = page_of(&s);
    let pen = drop_at(&mut s, &PEN, 5.0, 5.0);
    let inner =
        u32::try_from(run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": page, "x": 5.0, "y": 4.7 }))["id"].as_u64().unwrap()).unwrap();
    // A member that sticks out of the container but is still inside it (its middle is), so that there is something to fit.
    run(&mut s, "shape.move", json!({ "ids": [inner], "dx": 1.2, "dy": 0.0 }));
    let outline = |s: &Session| drawn_geometry(s.document().unwrap(), page, pen);
    let formulas = |s: &Session| outline(s).into_iter().map(|(at, formula, _)| (at, formula)).collect::<Vec<_>>();
    let master_draws = |s: &Session| {
        let doc = s.document().unwrap();
        let shape = shape_of(doc, page, pen);
        // A recalculation leaves the instance the results of the master's formulas at its size (`F` `Inh`), and never a formula or a row of its own.
        for section in shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry") {
            let own = section.rows.iter().flat_map(|r| r.cells.values()).filter(|c| matches!(c.formula, FormulaSlot::Local(_))).count();
            assert_eq!(own, 0, "the container holds no formula of its own in its outline: it draws its master's");
        }
        let user = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User");
        assert!(
            user.is_none_or(|u| u.rows.iter().all(|r| r.name.as_deref() != Some("DiaCraftContainerStyle"))),
            "and it holds no style row, which is what keeps it from being made again"
        );
    };
    master_draws(&s);
    let start = formulas(&s);
    assert!(
        start.iter().any(|(_, f)| !f.is_empty()) && start.iter().any(|(_, f)| f.is_empty()),
        "the outline is formulas and the constant 0: {start:?}"
    );
    // Fit to Contents, Margins, Automatic Resize (the container is brought to its setting now): each changes the box and none the outline's formulas.
    run(&mut s, "container.fit", json!({ "ids": [pen] }));
    master_draws(&s);
    assert_eq!(formulas(&s), start, "the same formulas after a fit");
    let fitted = draw::shape_frame(s.document().unwrap(), page, pen).unwrap();
    assert!(
        near(fitted.width, 2.0) && near(fitted.height, 1.5),
        "the container is the box of its member and the margin, 1.5 by 1 and a quarter inch round: {fitted:?}"
    );
    run(&mut s, "container.margin", json!({ "ids": [pen], "margin": 0.5 }));
    master_draws(&s);
    run(&mut s, "container.auto_resize", json!({ "ids": [pen], "mode": "fit" }));
    master_draws(&s);
    run(&mut s, "shape.move", json!({ "ids": [inner], "dx": -1.0, "dy": 0.0 }));
    master_draws(&s);
    run(&mut s, "container.lock", json!({ "ids": [pen], "locked": false }));
    master_draws(&s);
    // The outline is the master's formulas at the container's size: the tab is the master's 0.4 by 0.2 inch whatever the size. A fixed length is its length times the factor of the page's drop
    // scale, so the tab's width is a formula that holds the length and is 0.4 here, where the drop wrote no factor.
    let doc = s.document().unwrap();
    assert_eq!(formulas(&s), start, "after every command the outline is the master's formulas");
    assert!(outline(&s).iter().any(|(_, f, v)| f == "0.4*IF(DropOnPageScale>0,DropOnPageScale,1)" && v == "0.4"), "the tab is still 0.4 inch wide");
    assert!(near(live(doc, page, pen, "Height"), draw::shape_frame(doc, page, pen).unwrap().height));
    // A style is a look of the gallery, which would replace the master's: refused in words, and nothing changes. So is a heading, which is part of a style.
    let before = s.document().unwrap().clone();
    let said = s.execute("container.style", &json!({ "ids": [pen], "style": "strip" })).unwrap_err().to_string();
    assert!(said.contains("master") && said.contains("look"), "the words say the look is the master's: {said}");
    let said = s.execute("container.heading", &json!({ "ids": [pen], "side": "left" })).unwrap_err().to_string();
    assert!(said.contains("master"), "the words say why: {said}");
    assert_eq!(s.document().unwrap(), &before, "nothing was changed");
    // A container of the gallery, which has no master, is as it was: its look is the gallery's, and a style changes it.
    let gallery = u32::try_from(
        run(&mut s, "container.add", json!({ "style": "frame", "x": 8.0, "y": 8.0, "width": 2.0, "height": 1.5 }))["id"].as_u64().unwrap(),
    )
    .unwrap();
    run(&mut s, "container.style", json!({ "ids": [gallery], "style": "strip" }));
    run(&mut s, "container.heading", json!({ "ids": [gallery], "side": "left" }));
}

// ---- the factor of the page's drop scale ------------------------------------------------------------------------------------------------

/// The factor by which a fixed length follows the page's drop scale is one text wherever it is written: a shape of one piece reads its own `DropOnPageScale`, a member of a
/// group the group's (`Sheet.5`, the master's top-level shape), each falling back to 1 where the cell is found nowhere and reads 0; the macro a table of frames is written with makes the same
/// text as the function that makes the rest.
#[test]
fn the_factor_of_the_drop_scale_is_one_text_wherever_it_is_written() {
    assert_eq!(drop_factor(Factor::Shape), "IF(DropOnPageScale>0,DropOnPageScale,1)");
    let group = format!("IF(Sheet.{MASTER_SHAPE_ID}!DropOnPageScale>0,Sheet.{MASTER_SHAPE_ID}!DropOnPageScale,1)");
    assert_eq!(drop_factor(Factor::Group), group);
    assert_eq!(scaled!("0.08"), format!("0.08*{group}"));
    assert_eq!(scaled!("1"), format!("1*{group}"));
}

// ---- the kit's compartments -----------------------------------------------------------------------------------------------------------

#[test]
fn compartments_stack_from_the_top_and_the_group_is_as_tall_as_they_are() {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, &CARD).unwrap();
    let master = doc.master(id).unwrap();
    let top = master.shapes.get(0).unwrap();
    let text = |shape: &diacraft_doc::Shape, cell: &str| match &shape.sheet.cell(cell).unwrap().formula {
        FormulaSlot::Local(f) => f.source.to_string(),
        other => panic!("{cell}: {other:?}"),
    };
    assert_eq!(text(top, "Height"), "GUARD(Sheet.6!Height+Sheet.7!Height+Sheet.8!Height)", "the group is as tall as its compartments together");
    assert_eq!(
        text(top, "LocPinY"),
        "Height*0.5",
        "its pin stays at the middle of its box: it grows about its middle, as an `.odg` save of a group needs"
    );
    let (head, body, foot) = (top.children.get(0).unwrap(), top.children.get(1).unwrap(), top.children.get(2).unwrap());
    assert_eq!(text(head, "Width"), "GUARD(Sheet.5!Width)");
    assert_eq!(text(head, "PinX"), "GUARD(Sheet.5!Width*0.5)");
    assert_eq!(
        text(head, "Height"),
        "GUARD(MAX(0.05*IF(Sheet.5!DropOnPageScale>0,Sheet.5!DropOnPageScale,1),TEXTHEIGHT(TheText,Width)))",
        "the least height is the master's on paper and follows the group's factor, 1 where the group holds none"
    );
    assert_eq!(text(head, "PinY"), "GUARD(Sheet.5!Height-Height*0.5)");
    assert_eq!(text(body, "PinY"), "GUARD(Sheet.5!Height-Sheet.6!Height-Height*0.5)");
    assert_eq!(text(foot, "PinY"), "GUARD(Sheet.5!Height-Sheet.6!Height-Sheet.7!Height-Height*0.5)");
    // The group holds what a group of compartments needs: `LockHeight`, and typing goes to the members.
    let value = |cell: &str| top.sheet.cell(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number());
    assert_eq!((value("LockHeight"), value("IsTextEditTarget"), value("SelectMode")), (Some(1.0), Some(0.0), Some(1.0)));
    // `kit::compartment` is one of `kit::compartments`.
    assert!(matches!(kit::compartment(4, 0.3), MemberFrame::Compartment { rank: 4, min_in } if near(min_in, 0.3)));
    assert!(matches!(CARD_FRAMES[2], MemberFrame::Compartment { rank: 2, min_in } if near(min_in, 0.05)));
}

#[test]
fn a_compartment_stands_by_its_rank_and_not_by_its_place_in_the_table() {
    let mut s = blank();
    let page = page_of(&s);
    let klass = drop_at(&mut s, &KLASS, 5.0, 5.0);
    // The members are 6 operations, 7 attributes and 8 the name, and the name is the last: the one typing goes to. From the top they stand 8, 7, 6.
    let (operations, attributes, name) = (klass + 1, klass + 2, klass + 3);
    let doc = s.document().unwrap();
    let group = shape_of(doc, page, klass);
    let master = doc.master(group.master.unwrap()).unwrap();
    let top = master.shapes.get(0).unwrap();
    let text = |shape: &diacraft_doc::Shape, cell: &str| shape.sheet.cell(cell).unwrap().formula.text().unwrap().to_owned();
    assert_eq!(text(top, "Height"), "GUARD(Sheet.8!Height+Sheet.7!Height+Sheet.6!Height)", "the sum goes down the stack");
    assert_eq!(text(top.children.get(2).unwrap(), "PinY"), "GUARD(Sheet.5!Height-Height*0.5)", "the name, the last member, is at the top");
    assert_eq!(text(top.children.get(1).unwrap(), "PinY"), "GUARD(Sheet.5!Height-Sheet.8!Height-Height*0.5)");
    assert_eq!(text(top.children.get(0).unwrap(), "PinY"), "GUARD(Sheet.5!Height-Sheet.8!Height-Sheet.7!Height-Height*0.5)");
    // Typed in, the name makes the group taller and the others go down with its bottom edge.
    run(&mut s, "shape.set_text", json!({ "id": name, "text": "a\nb\nc" }));
    let doc = s.document().unwrap();
    let (height, h) = (live(doc, page, klass, "Height"), |m: u32| live(doc, page, m, "Height"));
    let top_of = |m: u32| live(doc, page, m, "PinY") + h(m) / 2.0;
    assert!(near(top_of(name), height) && near(top_of(attributes), height - h(name)) && near(top_of(operations), height - h(name) - h(attributes)));
    assert!(near(height, h(name) + h(attributes) + h(operations)));
    assert!(near(live(doc, page, operations, "PinY") - h(operations) / 2.0, 0.0), "and the last in the stack ends at the bottom");
    // The page draws it, with its members.
    assert!(pixels(doc).2.iter().any(|v| *v != 255));
}

#[test]
fn a_group_without_compartments_keeps_its_own_height_and_its_members_follow_by_formulas() {
    let mut s = blank();
    let page = page_of(&s);
    let badge = drop_at(&mut s, &BADGE, 5.0, 5.0);
    let doc = s.document().unwrap();
    let group = shape_of(doc, page, badge);
    let master = doc.master(group.master.unwrap()).unwrap();
    let top = master.shapes.get(0).unwrap();
    assert!(matches!(top.sheet.cell("Height").unwrap().formula, FormulaSlot::Absent), "its height is a number, as any shape's");
    assert!(top.sheet.cell("LockHeight").is_none(), "and it is no height lock: it is sized as any shape is");
    // A resize sizes both axes, and the members follow by the formulas of the group's frame.
    run(&mut s, "shape.resize", json!({ "id": badge, "x": 4.0, "y": 4.0, "width": 2.0, "height": 3.0 }));
    let doc = s.document().unwrap();
    let (disc, mark) = (badge + 1, badge + 2);
    assert!(near(live(doc, page, disc, "Width"), 2.0) && near(live(doc, page, disc, "Height"), 3.0));
    assert!(near(live(doc, page, mark, "Width"), 0.8) && near(live(doc, page, mark, "Height"), 1.2), "the mark is two fifths of the group");
    assert!(near(cached(doc, page, mark, "PinX"), 1.0) && near(cached(doc, page, mark, "PinY"), 1.5));
    // A member of a group of formulas is held as firmly as a compartment: it cannot be moved alone.
    let said = s.execute("shape.move", &json!({ "ids": [mark], "dx": 0.2, "dy": 0.0 })).unwrap_err().to_string();
    assert!(said.contains("GUARD"), "{said}");
}

#[test]
fn a_member_with_a_field_shows_the_groups_shape_data_and_follows_it() {
    let mut s = blank();
    let page = page_of(&s);
    let badge = drop_at(&mut s, &BADGE, 5.0, 5.0);
    let mark = badge + 2;
    assert_eq!(text_of(s.document().unwrap(), page, mark), "", "a field is not typed text");
    let shown = |s: &Session| {
        let shape = shape_of(s.document().unwrap(), page, mark);
        let text = shape.text.as_ref().expect("the member holds the text that holds its field (the shown form of a field lives there)");
        text.parts
            .iter()
            .map(|p| match p {
                diacraft_doc::TextPart::Fld { text, .. } => text.to_string(),
                _ => String::new(),
            })
            .collect::<String>()
    };
    assert_eq!(shown(&s), "Label");
    run(&mut s, "shapedata.set", json!({ "id": badge, "row": "Label", "value": "Hello" }));
    // The field's row reads the group's data at once. What it shows is worked out again by `text.update_fields` (follows a shape's own fields).
    run(&mut s, "text.update_fields", json!({}));
    assert_eq!(shown(&s), "Hello", "the field of a member reads the group's Shape Data");
}

// ---- the caps and hostile input ---------------------------------------------------------------------------------------------------------

const fn plain_member() -> MemberSpec {
    MemberSpec { name: "Cell", frame: kit::compartment(0, 0.05), outline: Outline::Polygon(RECT), points: &[], dress: None }
}

static MANY_MEMBERS: [MemberSpec; 33] = [const { plain_member() }; 33];
static FULL_MEMBERS: [MemberSpec; 32] = [const { plain_member() }; 32];

const fn group_of(members: &'static [MemberSpec]) -> MasterSpec {
    MasterSpec {
        id: "t.many",
        name: "Many",
        width: 2.0,
        height: 2.0,
        identity: (200, 73),
        outline: Outline::Group { members, text: TextBox::FULL },
        points: &kit::GRID3,
        more: &More::NONE,
    }
}

#[test]
fn a_group_master_past_32_members_fails_the_catalogue_check_and_is_built_with_32() {
    assert_eq!(caps_problem(&group_of(&FULL_MEMBERS)), None, "32 members are within the cap");
    let said = caps_problem(&group_of(&MANY_MEMBERS)).expect("33 members are past the cap");
    assert!(said.contains("33 members, past 32"), "{said}");
    let spec = group_of(&MANY_MEMBERS);
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, &spec).unwrap();
    assert_eq!(doc.master(id).unwrap().shapes.get(0).unwrap().children.len(), 32, "a build never goes past the cap");
}

#[test]
fn a_group_of_32_compartments_drops_edits_and_undoes_within_counted_work() {
    let spec = group_of(&FULL_MEMBERS);
    let mut s = blank();
    let page = page_of(&s);
    let group = drop_at(&mut s, &spec, 5.0, 5.0);
    let doc = s.document().unwrap();
    assert_eq!(shape_of(doc, page, group).children.len(), 32);
    let before = s.document().unwrap().clone();
    let started = std::time::Instant::now();
    run(&mut s, "shape.set_text", json!({ "id": group + 17, "text": "a\nb\nc" }));
    let h0 = live(&before, page, group, "Height");
    assert!(live(s.document().unwrap(), page, group, "Height") > h0, "the group grew");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before);
    // A hang guard, not a measure: the counted work is the claim of the engine's own tests.
    assert!(started.elapsed() < std::time::Duration::from_secs(120), "{:?}", started.elapsed());
}

#[test]
fn a_group_master_instance_nested_deep_in_groups_still_follows_its_text() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let was = live(s.document().unwrap(), page, card, "Height");
    // Group the instance again and again, each time with a rectangle: each group holds the one before.
    let mut outer = card;
    for k in 0..24 {
        let beside = rectangle(&mut s, 1.0 + f64::from(k % 8), 9.0);
        outer = u32::try_from(run(&mut s, "shape.group", json!({ "ids": [outer, beside] }))["id"].as_u64().unwrap()).unwrap();
    }
    let started = std::time::Instant::now();
    run(&mut s, "shape.set_text", json!({ "id": card + 2, "text": "a\nb\nc\nd" }));
    let doc = s.document().unwrap();
    assert!(live(doc, page, card, "Height") > was + 0.3, "the instance grew, 24 groups down");
    assert!(near(cached(doc, page, card, "Height"), live(doc, page, card, "Height")));
    assert!(started.elapsed() < std::time::Duration::from_secs(120), "{:?}", started.elapsed());
}

// ---- the edits of the per-stencil check, on a group master ------------------------------------------------------------------------------

#[test]
fn a_resize_of_a_group_master_with_lock_height_sizes_its_width_and_keeps_the_formula_of_its_height() {
    // The window's handles and `shape.resize` with `id` are the single-shape path, which ignores the locks: a group whose height is a formula (guarded) must keep it, as `shape.resize` with
    // `ids` does. Its width is sized and its middle goes where the box puts it; the height cell is not written.
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    run(&mut s, "shape.set_text", json!({ "id": card + 2, "text": "a\nb" }));
    let frame = draw::shape_frame(s.document().unwrap(), page, card).unwrap();
    let (w, h) = (frame.width * 2.0, frame.height * 0.5);
    run(&mut s, "shape.resize", json!({ "id": card, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": h }));
    let doc = s.document().unwrap();
    let after = draw::shape_frame(doc, page, card).unwrap();
    assert!(near(after.width, w), "the width is sized: {after:?}");
    assert!(near(after.height, frame.height), "the height is its text's: {} to {}", frame.height, after.height);
    let height = Resolver::new(doc).resolve(&SheetRef::Shape(doc.find_shape(Owner::Page(page), card).unwrap()), &CellAddr::direct("Height")).unwrap();
    assert!(
        height.formula.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_) | FormulaSlot::Inherit)),
        "the height is still a formula, the master's"
    );
    assert!(near(live(doc, page, card + 3, "Width"), w), "the members follow the width");
}

#[test]
fn a_group_master_goes_through_the_edits_of_the_per_stencil_check_and_undoes_them() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    run(&mut s, "shape.set_text", json!({ "id": card + 1, "text": "Card" }));
    let frame = draw::shape_frame(s.document().unwrap(), page, card).unwrap();
    // The check sizes a shape that holds `LockHeight` to twice its width and the same height, about its middle.
    let (w, h) = (frame.width * 2.0, frame.height);
    run(&mut s, "shape.resize", json!({ "id": card, "x": frame.pin_x - w / 2.0, "y": frame.pin_y - h / 2.0, "width": w, "height": h }));
    let after = draw::shape_frame(s.document().unwrap(), page, card).unwrap();
    assert!(near(after.width, w) && near(after.height, frame.height), "{after:?}");
    run(&mut s, "shape.rotate", json!({ "ids": [card], "angle": 30 }));
    run(&mut s, "shape.flip", json!({ "ids": [card], "axis": "horizontal" }));
    let before = s.document().unwrap().clone();
    let copy = run(&mut s, "shape.duplicate", json!({ "ids": [card] }));
    let copied = u32::try_from(copy["ids"][0].as_u64().unwrap()).unwrap();
    assert_eq!(text_of(s.document().unwrap(), page, copied + 1), "Card", "the copy has the typed text");
    assert_eq!(shape_of(s.document().unwrap(), page, copied).children.len(), 3, "and its members");
    let after = s.document().unwrap().clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before, "undo takes the copy away");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document().unwrap(), &after, "redo brings it back");
    // The copy follows its own text and not the original's.
    run(&mut s, "shape.set_text", json!({ "id": copied + 2, "text": "a\nb\nc\nd" }));
    let doc = s.document().unwrap();
    assert!(live(doc, page, copied, "Height") > live(doc, page, card, "Height") + 0.3, "the copy grew and the original did not");
    let page_pixels = pixels(doc);
    assert!(page_pixels.2.iter().any(|v| *v != 255), "the page draws");
}

// ---- what the catalogue asks of every shipped master, tried on the masters made here --------------------------------------------------

/// A group master's nominal height (`MasterSpec::height`: what the listing and the drop's preview say) is what its members make at the default size, within a fiftieth of an inch: the faces on the
/// machine decide a line's height, so it is not exact for a group of compartments.
#[track_caller]
fn assert_nominal_height(spec: &MasterSpec) {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, spec).unwrap();
    let top = doc.master(id).unwrap().shapes.get(0).unwrap().clone();
    let settled = top.sheet.cell("Height").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap();
    assert!((settled - spec.height).abs() < 0.02, "{}: the table says {} inch and the master is {settled} tall", spec.id, spec.height);
}

#[test]
fn the_catalogues_invariants_hold_for_the_group_masters_made_here() {
    for spec in [&CARD, &BADGE] {
        super::catalogue_tests::assert_formulas_hold(spec);
        super::catalogue_tests::assert_paths_close(spec);
        super::catalogue_tests::assert_points_on_the_outline(spec);
        assert_eq!(caps_problem(spec), None, "{}", spec.id);
        assert_nominal_height(spec);
    }
}

#[test]
fn every_shipped_group_master_is_within_the_cap_and_as_tall_as_its_table_says() {
    for spec in all_masters().filter(|m| m.is_group()) {
        assert_eq!(caps_problem(spec), None, "{}", spec.id);
        assert_nominal_height(spec);
    }
}

#[test]
fn typing_in_one_group_costs_the_same_on_a_page_of_sixty_groups_as_on_a_page_of_two() {
    let cost = |n: u32| -> (u64, u64, u64) {
        let mut s = blank();
        let mut first = 0;
        for k in 0..n {
            let id = drop_at(&mut s, &CARD, 1.0 + f64::from(k % 8), 1.0 + f64::from(k / 8) * 1.2);
            if k == 0 {
                first = id;
            }
        }
        // A first edit, so that what a session keeps between edits is made.
        run(&mut s, "shape.set_text", json!({ "id": first + 2, "text": "warm" }));
        diacraft_calc::work::reset();
        run(&mut s, "shape.set_text", json!({ "id": first + 2, "text": "a\nb\nc" }));
        (diacraft_calc::work::steps(), diacraft_calc::work::formulas_asked(), diacraft_calc::work::cells_walked())
    };
    let (small, large) = (cost(2), cost(60));
    // The edit works out the cells of its own group and no others: the same steps and the same cells walked on a page of sixty as on a page of two.
    assert_eq!(small, large, "(steps, formulas asked, cells walked) of one edit");
    assert!(small.0 > 0 && small.0 < 5_000, "the steps of the edit: {}", small.0);
}

#[test]
fn the_longest_text_a_shape_may_hold_in_a_compartment_makes_a_tall_finite_group_in_bounded_work() {
    let mut s = blank();
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 5.0, 5.0);
    let before = s.document().unwrap().clone();
    let long = "word ".repeat(100_000 / 5);
    assert_eq!(long.chars().count(), 100_000);
    run(&mut s, "shape.set_text", json!({ "id": card + 2, "text": long }));
    let doc = s.document().unwrap();
    let (member, group) = (live(doc, page, card + 2, "Height"), live(doc, page, card, "Height"));
    assert!(member.is_finite() && group.is_finite() && member > 100.0 && group > member, "a tall group: {member} and {group}");
    assert!(near(cached(doc, page, card, "Height"), group), "and the value the recalculation left is what the formulas give");
    // One undo gives it back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before);
}

#[test]
fn a_group_master_dropped_into_an_odg_is_saved_in_place_and_read_back() {
    // As the per-stencil check does for each master: dropped into a drawing the file holds, typed in, saved in place, read again, and saved again as no edit.
    let dir = Dir::new();
    let path = dir.path("a.odg");
    let mut base = blank();
    run(&mut base, "file.save_as", json!({ "path": path }));
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    let page = page_of(&s);
    let card = drop_at(&mut s, &CARD, 4.0, 5.0);
    run(&mut s, "shape.set_text", json!({ "id": card + 1, "text": "Name" }));
    run(&mut s, "shape.set_text", json!({ "id": card + 2, "text": "attribute one\nattribute two" }));
    let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("the save of a group master was refused: {e}"));
    for note in saved["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        assert!(note.contains("outline that is written as a rectangle"), "{note}");
    }
    let bytes = std::fs::read(&path).unwrap();
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": path }));
    let (made, read) = (s.document().unwrap(), again.document().unwrap());
    let (a, b) = (pixels(made), pixels(read));
    assert_eq!((a.0, a.1), (b.0, b.1));
    let worst = a.2.iter().zip(b.2.iter()).map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs()).max().unwrap_or(0);
    assert!(worst <= 2, "the file draws the group as the session does (worst channel {worst})");
    let read_page = read.pages.get(0).unwrap().id;
    assert_eq!(text_of(read, read_page, card + 2), "attribute one\nattribute two");
    let (m, r) = (shape_of(made, page, card), shape_of(read, read_page, card));
    assert_eq!(made.master(m.master.unwrap()).unwrap().unique_id, read.master(r.master.unwrap()).unwrap().unique_id, "the master is carried");
    // Saved again as no edit: the file is the same bytes.
    run(&mut s, "file.save", json!({}));
    assert!(std::fs::read(&path).unwrap() == bytes, "a second save with no edit changed the file");
}

#[test]
fn a_shape_changed_to_a_group_master_becomes_the_group_and_follows_its_text() {
    // Change Shape swaps a shape for a master and keeps its `ID`: the group's members are given `ID`s above the session's.
    let mut s = blank();
    let page = page_of(&s);
    let rect = rectangle(&mut s, 5.0, 5.0);
    let above = s.id_floor(page);
    let changed = s
        .edit("Change shape", |doc| {
            draw::change_shapes(doc, page, &[rect], &draw::Replacement::Builtin(&CARD), above).map_err(|p| p.into_engine("shape.change"))
        })
        .unwrap();
    assert_eq!((changed.changed.clone(), changed.skipped.len()), (vec![rect], 0));
    let doc = s.document().unwrap();
    let group = shape_of(doc, page, rect);
    assert_eq!((group.kind.clone(), group.children.len()), (ShapeType::Group, 3));
    assert!(group.children.iter().all(|m| m.id > above), "the members' IDs are above the session's");
    let first = group.children.get(0).unwrap().id;
    run(&mut s, "shape.set_text", json!({ "id": first + 1, "text": "a\nb\nc\nd" }));
    let doc = s.document().unwrap();
    let sum: f64 = [first, first + 1, first + 2].iter().map(|m| live(doc, page, *m, "Height")).sum();
    assert!(near(live(doc, page, rect, "Height"), sum), "the changed shape is as tall as its compartments");
}

// ---- a page of another drawing scale ----------------------------------------------------------------------------------

/// A built-in group master is dropped by the code a stencil file's is (`place`), so the range of eight reaches it: on a page at 1:100 the group keeps its size on paper (its
/// `Width` and `DropOnPageScale` are written, its members follow by the formulas they inherit), its height, the sum of its compartments' and kept by `LockHeight`, stays a formula,
/// and the box is centred at the drop point. Within the range, and for the trial instance Change Shape makes (`Follow::Never`), nothing is written.
#[test]
fn a_built_in_group_master_follows_the_pages_drawing_scale_beyond_the_range_of_eight() {
    let mut s = blank();
    let page = page_of(&s);
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 100.0}));
    let card = drop_at(&mut s, &CARD, 500.0, 600.0);
    let doc = s.document().unwrap();
    let held = |name: &str| shape_of(doc, page, card).sheet.cells.get(&diacraft_doc::CellName::new(name)).cloned();
    assert!(near(held("DropOnPageScale").and_then(|c| c.value?.number()).unwrap(), 100.0), "the factor is written");
    assert!(near(held("Width").and_then(|c| c.value?.number()).unwrap(), 160.0), "the width is the master's times the ratio");
    assert!(
        !held("Height").is_some_and(|c| matches!(c.formula, FormulaSlot::Absent)),
        "the height is the sum of the compartments', a formula `LockHeight` keeps: it is not replaced by a value"
    );
    for member in [card + 1, card + 2, card + 3] {
        assert!(near(cached(doc, page, member, "Width"), 160.0), "member {member} follows the group's width");
        assert!(
            !shape_of(doc, page, member).sheet.cells.contains_key(&diacraft_doc::CellName::new("DropOnPageScale")),
            "no member holds a factor of its own"
        );
    }
    let corners = draw::shape_corners(doc, page, card).unwrap();
    let (xs, ys): (Vec<f64>, Vec<f64>) = (corners.iter().map(|p| p.0).collect(), corners.iter().map(|p| p.1).collect());
    let middle = |v: &[f64]| (v.iter().copied().fold(f64::INFINITY, f64::min) + v.iter().copied().fold(f64::NEG_INFINITY, f64::max)) / 2.0;
    assert!(near(middle(&xs), 500.0) && near(middle(&ys), 600.0), "the box is centred at the drop point: {corners:?}");

    // Within the range the drop is as it was: nothing is written on the group.
    let mut s = blank();
    let page = page_of(&s);
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 4.0}));
    let card = drop_at(&mut s, &CARD, 5.0, 6.0);
    let doc = s.document().unwrap();
    for name in ["DropOnPageScale", "Width"] {
        assert!(!shape_of(doc, page, card).sheet.cells.contains_key(&diacraft_doc::CellName::new(name)), "{name} is not written within the range");
    }
    // Change Shape's trial instance follows no scale: the same drop on the page at 1:100 writes nothing.
    let mut s = blank();
    let page = page_of(&s);
    run(&mut s, "page.scale", json!({"paper": 1.0, "drawing": 100.0}));
    let above = s.id_floor(page);
    let info = s
        .edit("Drop shape", |doc| {
            draw::drop_shape_following(doc, page, &CARD, (5.0, 6.0), above, draw::Follow::Never).map_err(|p| p.into_engine("shape.drop"))
        })
        .unwrap();
    assert_eq!(info.drop_scale, None);
    assert!(!shape_of(s.document().unwrap(), page, info.id).sheet.cells.contains_key(&diacraft_doc::CellName::new("DropOnPageScale")));
}
