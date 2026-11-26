//! The catalogue of built-in stencils: what ships, what opens by default, the GUID form, the masters' built cells, the formats a master can say (connector looks, one-dimensional
//! figures, geometry in code, labels below a figure, a dress), and the invariants every shipped master keeps. The second batch's masters are covered by the invariants as each family ships.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_calc::{Budget, EvalOptions, commit};
use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, SectionKey, SheetRef, Text, TextPart};
use diacraft_render::{Rendered, Renderer, View};
use diacraft_sheetxml::model::{MemBlobs, encode_master, stamp_of_bytes};
use serde_json::json;

use super::container::Side;
use super::kit;
use super::*;
use crate::Session;
use crate::draw::{self, End, Which};

// ---- what the first fifty-eight masters built before ---------------------------------------------------------------------------------

/// The stamp of the master part `spec` builds, as the sheet codec writes it: the one fingerprint of what a master holds (every cell, formula, value, unit and section).
fn stamp_of(master: &Master) -> String {
    let tree = encode_master(master, &mut MemBlobs::new()).unwrap();
    let mut bytes = Vec::new();
    diacraft_container::xml::write_element(&mut bytes, &tree).unwrap();
    stamp_of_bytes(&bytes).text()
}

/// The `ObjType` the master's shape holds, taken out of it (`None` for a master that holds none).
fn take_object_type(master: &mut Master) -> Option<f64> {
    let shape = Arc::make_mut(master.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.remove(&CellName::new("ObjType")).and_then(|cell| cell.value.as_ref().and_then(|v| v.number()))
}

/// The 58 masters of 629feee1 and the stamp of the part each one built **there**, taken before the catalogue grew and pinned here. A change that makes any of them build another cell, formula,
/// value, unit or GUID changes its stamp: the drawings that hold these masters (and match them by their GUIDs) would find them changed. **One cell was added on purpose** (owner
/// 2026-11-25): `ObjType` 1 on the masters people join; the test takes it out before it stamps, and asks for it on exactly those masters.
const PINNED: [(&str, &str); 58] = [
    ("rectangle", "61b003904aab6621"),
    ("square", "8d0c6ed881317e8b"),
    ("ellipse", "c5f303297fac9e7e"),
    ("circle", "48ef4698ac5644ef"),
    ("triangle", "b256d15956dbc21a"),
    ("right_triangle", "8e4f6b3dff271049"),
    ("diamond", "e589a6c374122c69"),
    ("pentagon", "057d883021ff6c0a"),
    ("hexagon", "d4f1007945f0b840"),
    ("octagon", "d346f3eba8304967"),
    ("parallelogram", "88beb2323aa68548"),
    ("trapezoid", "5bb1bfa87efb2a08"),
    ("process", "39b5695c422ec4f9"),
    ("decision", "a3c9462744608558"),
    ("terminator", "09ca0b07d1cd904e"),
    ("data", "6a1358b83f7c14ff"),
    ("document", "a94689a5d1e2b841"),
    ("multiple_documents", "e782465cfd876c60"),
    ("predefined_process", "b29c6f079b506257"),
    ("manual_input", "f07aa7d28c2a9047"),
    ("manual_operation", "c6801eb64f91391b"),
    ("preparation", "efb51a57232183ff"),
    ("on_page_reference", "d22a002dfe56eed5"),
    ("off_page_reference", "c682cea8d28840a0"),
    ("database", "073c6033cb515f5a"),
    ("internal_storage", "43c1f3228340c8ab"),
    ("delay", "f8c5343eceb3aff5"),
    ("display", "9a0a9998c91540e0"),
    ("summing_junction", "25d6681767250a73"),
    ("or", "bc8ac86379ecad68"),
    ("decorative.star-5", "2d03e0be9dd15b05"),
    ("decorative.star-6", "9df45c4507b0c40e"),
    ("decorative.star-8", "cdf932409cfde449"),
    ("decorative.burst", "9a573b7d6d71d2bd"),
    ("decorative.heart", "4a5c7bdf4eba4daf"),
    ("decorative.lightning-bolt", "78aacd7f4b80ec0d"),
    ("decorative.cross", "3e2940429125d87f"),
    ("decorative.ribbon", "d0af838d49db9cf9"),
    ("arrows.right", "49e43095f94764a6"),
    ("arrows.left", "5befe6561999da74"),
    ("arrows.up", "a0e08361b9c2a594"),
    ("arrows.down", "6c0280a890676bb4"),
    ("arrows.left-right", "b717401985572db4"),
    ("arrows.up-down", "f83ece4b58766cf2"),
    ("arrows.four-way", "42c6c7aec3c37f99"),
    ("arrows.bent", "8d3710fa7a6d84c0"),
    ("gane-sarson.interface", "8346d35a59dbbb96"),
    ("gane-sarson.process", "ca7b5ed78699951f"),
    ("gane-sarson.data-store", "62310027646adabb"),
    ("gane-sarson.data-flow", "ace24d0fcad75f3d"),
    ("xfunc.swimlane", "27873e0b7b6c14dc"),
    ("xfunc.separator", "d839527f2d3aa92b"),
    ("orgchart.executive", "4b7075d5cda5a784"),
    ("orgchart.manager", "4fc81cf037dc7faf"),
    ("orgchart.position", "b0161993bdcfae56"),
    ("orgchart.assistant", "20d6016cf193f854"),
    ("orgchart.consultant", "8e3f81c04fe94e93"),
    ("orgchart.vacancy", "fa5e3610e58b6680"),
];

/// The masters that hold `ObjType` 1 (owner 2026-11-25), said by the stencils: every two-dimensional master of Basic Shapes (series 1), the Basic Flowchart (2), the Gane-Sarson
/// shapes (6) and the Organization Chart Shapes (8). The decorative shapes (4), the arrows (5) and the Cross-Functional Flowchart's parts (7) hold none, and nor does a connector.
fn people_join(m: &MasterSpec) -> bool {
    !m.is_connector() && matches!(m.identity.0, 1 | 2 | 6 | 8)
}

#[test]
fn the_58_masters_of_629feee1_build_to_the_cells_they_built_before_and_hold_objtype_1_where_people_join_them() {
    let first: Vec<&MasterSpec> = all_masters().filter(|m| m.identity.0 <= 8).collect();
    assert_eq!(first.len(), 58);
    let mut joined = 0;
    for (m, (want_id, want)) in first.iter().zip(PINNED.iter()) {
        assert_eq!(m.id, *want_id, "the order of the catalogue");
        let mut master = build_master(m, 1);
        let object_type = take_object_type(&mut master);
        assert_eq!(object_type, people_join(m).then_some(1.0), "{}: the master's ObjType", m.id);
        joined += usize::from(object_type.is_some());
        assert_eq!(stamp_of(&master), *want, "{} builds other cells than it built at 629feee1", m.id);
    }
    assert_eq!(joined, 39, "twelve of Basic Shapes, eighteen of the flowchart, three of Gane-Sarson, six positions");
}

// ---- the GUID form -------------------------------------------------------------------------------------------------------------------

fn spec_with_identity(series: u8, at: u16) -> MasterSpec {
    MasterSpec {
        id: "test.guid",
        name: "Guid",
        width: 1.0,
        height: 1.0,
        identity: (series, at),
        outline: Outline::Ellipse,
        points: &[],
        more: &More::NONE,
    }
}

#[test]
fn the_guid_text_of_series_1_to_9_is_unchanged_and_series_10_to_22_are_guids() {
    for series in 1..=9_u8 {
        for at in [0_u16, 1, 2, 99, 4_095, 65_535] {
            let was = format!("{{D1AC000{series}-0000-4000-8000-{at:012X}}}");
            assert_eq!(spec_with_identity(series, at).guid(), was, "series {series} keeps the text it had");
        }
    }
    // A series past 9 is a GUID too: 38 characters, braces, hyphens in the places of a GUID, and upper-case hexadecimal digits everywhere else.
    for series in 0..=255_u8 {
        for at in [0_u16, 1, 4_095, 65_535] {
            let text = spec_with_identity(series, at).guid();
            assert_eq!(text.len(), 38, "{text}");
            assert!(text.starts_with('{') && text.ends_with('}'), "{text}");
            for (i, c) in text.char_indices() {
                match i {
                    0 => assert_eq!(c, '{'),
                    37 => assert_eq!(c, '}'),
                    9 | 14 | 19 | 24 => assert_eq!(c, '-', "{text}"),
                    _ => assert!(c.is_ascii_digit() || ('A'..='F').contains(&c), "{text}: `{c}` at {i}"),
                }
            }
            assert!(text.starts_with("{D1AC"), "{text}");
            assert_eq!(&text[5..9], format!("{series:04X}"), "the series is four hex digits: {text}");
            assert_eq!(&text[25..37], format!("{at:012X}"), "the ordinal is twelve: {text}");
        }
    }
    assert_eq!(spec_with_identity(10, 1).guid(), "{D1AC000A-0000-4000-8000-000000000001}");
    assert_eq!(spec_with_identity(22, 3).guid(), "{D1AC0016-0000-4000-8000-000000000003}");
    // Two series never give one text.
    let mut all: Vec<String> = (0..=255_u8).map(|series| spec_with_identity(series, 1).guid()).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 256);
}

// ---- shipped and open by default -----------------------------------------------------------------------------------------------------

static ONE_MASTER: [MasterSpec; 1] = [MasterSpec {
    id: "t.one",
    name: "One",
    width: 1.0,
    height: 1.0,
    identity: (200, 1),
    outline: Outline::Ellipse,
    points: &SIDES,
    more: &More::NONE,
}];

fn made(id: &'static str, masters: &'static [MasterSpec], default_open: bool) -> StencilSpec {
    StencilSpec { id, name: id, category: "General", masters, default_open }
}

#[test]
fn a_stencil_is_shipped_when_it_has_masters_and_open_by_default_when_it_is_both_shipped_and_says_so() {
    let list = [
        made("open-shipped", &ONE_MASTER, true),
        made("closed-shipped", &ONE_MASTER, false),
        made("open-empty", &[], true),
        made("closed-empty", &[], false),
        made("open-shipped-2", &ONE_MASTER, true),
    ];
    assert_eq!(list.iter().map(StencilSpec::shipped).collect::<Vec<_>>(), [true, true, false, false, true]);
    assert_eq!(
        shipped_among(&list).iter().map(|s| s.id).collect::<Vec<_>>(),
        ["open-shipped", "closed-shipped", "open-shipped-2"],
        "in catalogue order"
    );
    assert_eq!(default_open_among(&list), ["open-shipped", "open-shipped-2"], "a stencil with no masters is open nowhere, whatever it says");
    assert!(shipped_among(&[]).is_empty() && default_open_among(&[]).is_empty());
}

#[test]
fn the_default_open_stencils_are_the_seven_of_629feee1() {
    let seven = ["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"];
    assert_eq!(default_open_ids(), seven);
    assert_eq!(
        registered().iter().filter(|s| s.default_open).map(|s| s.id).collect::<Vec<_>>(),
        seven,
        "the thirteen new stencils are closed by default"
    );
    assert_eq!(OpenedStencils::new().open_builtins(), seven);
    let mut opened = OpenedStencils::new();
    opened.set_builtins(&["basic"]);
    opened.reset_builtins();
    assert_eq!(opened.open_builtins(), seven, "a new drawing has the seven open");
    // A new session lists the seven and nothing else as open.
    let mut s = Session::new();
    let listed = s.execute("stencil.list", &json!({})).unwrap();
    let open: Vec<&str> = listed["stencils"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert_eq!(open, seven);
}

#[test]
fn the_catalogue_registers_twenty_stencils_in_order_and_ships_the_first_seven() {
    let all: Vec<(&str, &str, &str)> = registered().iter().map(|s| (s.id, s.name, s.category)).collect();
    assert_eq!(
        all,
        [
            ("basic", "Basic Shapes", "General"),
            ("flowchart", "Basic Flowchart", "Flowchart"),
            ("decorative", "Decorative Shapes", "General"),
            ("arrows", "Arrow Shapes", "General"),
            ("gane-sarson", "Gane-Sarson", "Software and Database"),
            ("cross-functional", "Cross-Functional Flowchart Shapes", "Flowchart"),
            ("orgchart", "Organization Chart Shapes", "Business"),
            ("uml-class", "UML Class", "Software and Database"),
            ("uml-usecase", "UML Use Case", "Software and Database"),
            ("uml-sequence", "UML Sequence", "Software and Database"),
            ("uml-activity", "UML Activity", "Software and Database"),
            ("uml-state", "UML State Machine", "Software and Database"),
            ("uml-component", "UML Component and Deployment", "Software and Database"),
            ("bpmn", "BPMN Shapes", "Flowchart"),
            ("c4", "C4 Model", "Software and Database"),
            ("network", "Network Devices", "Network"),
            ("computers", "Computers and Devices", "Network"),
            ("cloud", "Cloud Services", "Network"),
            ("crows-foot", "Crow's Foot Database Notation", "Software and Database"),
            ("chen", "Chen's Database Notation", "Software and Database"),
        ]
    );
    let shipped: Vec<&str> = stencils().iter().map(|s| s.id).collect();
    let by_registration: Vec<&str> = registered().iter().filter(|s| s.shipped()).map(|s| s.id).collect();
    assert_eq!(shipped, by_registration, "`stencils()` is the shipped ones, in the order of the registration");
    assert!(shipped.starts_with(&["basic", "flowchart", "decorative", "arrows", "gane-sarson", "cross-functional", "orgchart"]));
}

/// The series (the first number of a master's identity, and the first group of its GUID) each stencil's masters hold, as the design's section 4.1 gives them. A family task fills its stub with
/// masters of its own series, and the hidden line ends are series 22.
const SERIES: [(&str, u8); 20] = [
    ("basic", 1),
    ("flowchart", 2),
    ("decorative", 4),
    ("arrows", 5),
    ("gane-sarson", 6),
    ("cross-functional", 7),
    ("orgchart", 8),
    ("uml-class", 9),
    ("uml-usecase", 10),
    ("uml-sequence", 11),
    ("uml-activity", 12),
    ("uml-state", 13),
    ("uml-component", 14),
    ("bpmn", 15),
    ("c4", 16),
    ("network", 17),
    ("computers", 18),
    ("cloud", 19),
    ("crows-foot", 20),
    ("chen", 21),
];

/// The series the hidden line-end masters have.
const LINE_END_SERIES: u8 = 22;

#[test]
fn every_stencil_has_one_series_of_its_own_and_every_master_of_it_holds_that_series() {
    let mut used = std::collections::BTreeSet::new();
    for (id, series) in SERIES {
        assert!(used.insert(series), "series {series} is no other stencil's");
        let stencil = registered().iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{id} is registered"));
        for m in stencil.masters() {
            assert_eq!(m.identity.0, series, "{}: its series", m.id);
        }
    }
    assert!(!used.contains(&LINE_END_SERIES) && !used.contains(&3), "22 is the hidden line ends' and 3 the GUID a copied master is given");
    for m in line_ends::MASTERS.iter() {
        assert_eq!(m.identity.0, LINE_END_SERIES, "{}: a hidden line end", m.id);
        assert!(m.is_hidden(), "{}", m.id);
    }
    assert_eq!(SERIES.len(), registered().len());
}

// ---- nothing unshipped is reachable --------------------------------------------------------------------------------------------------

#[test]
fn an_unshipped_stencil_is_in_no_listing_search_open_or_more_shapes_answer() {
    let mut s = Session::new();
    let unshipped: Vec<&StencilSpec> = registered().iter().filter(|s| !s.shipped()).collect();
    let listed = s.execute("stencil.list", &json!({})).unwrap();
    let ids =
        |value: &serde_json::Value| -> Vec<String> { value.as_array().unwrap().iter().filter_map(|e| e["id"].as_str().map(str::to_owned)).collect() };
    let (open, available) = (ids(&listed["stencils"]), ids(&listed["available"]));
    let unknown = s.execute("stencil.open", &json!({ "id": "no-such-stencil" })).expect_err("an unknown id is refused").to_string();
    for stencil in &unshipped {
        assert!(!open.iter().any(|id| id == stencil.id), "{}: not in stencil.list", stencil.id);
        assert!(!available.iter().any(|id| id == stencil.id), "{}: not in the More Shapes menu (`available`)", stencil.id);
        // `stencil.open` refuses it in words, as it refuses an unknown id.
        let refused = s.execute("stencil.open", &json!({ "id": stencil.id })).expect_err("an unshipped stencil is not opened").to_string();
        assert_eq!(refused.replace(stencil.id, "X"), unknown.replace("no-such-stencil", "X"), "{}: as an unknown id", stencil.id);
        assert!(refused.contains("no built-in stencil") && refused.contains("available"), "{refused}");
        assert!(!s.stencils().open_builtins().contains(&stencil.id), "{}: still closed", stencil.id);
        // The search finds no group of it, whatever it looks for.
        let found = s.execute("stencil.search", &json!({ "text": stencil.name })).unwrap();
        assert!(!ids(&found["results"]).iter().any(|id| id == stencil.id), "{}: not searched", stencil.id);
        // Its Quick Shapes are not there either.
        let quick = s.execute("stencil.quick", &json!({ "stencil": stencil.id })).expect_err("an unshipped stencil has no Quick Shapes").to_string();
        let unknown_quick = s.execute("stencil.quick", &json!({ "stencil": "no-such-stencil" })).expect_err("unknown").to_string();
        assert_eq!(quick.replace(stencil.id, "X"), unknown_quick.replace("no-such-stencil", "X"), "{}", stencil.id);
        assert!(builtin_stencil(stencil.id).is_none() && find_stencil(stencil.id).is_none(), "{}", stencil.id);
        assert!(!all_masters().any(|m| m.id.starts_with(&format!("{}.", stencil.id))), "{}", stencil.id);
    }
    let quick = s.execute("stencil.quick", &json!({})).unwrap();
    let text = quick.to_string();
    for stencil in &unshipped {
        assert!(!text.contains(&format!("\"{}\"", stencil.id)), "{}: not in the Quick Shapes stencil", stencil.id);
    }
    // The templates that open it are not listed either (the Gane-Sarson one is: its stencil ships).
    let templates = s.execute("template.list", &json!({})).unwrap();
    for stencil in &unshipped {
        assert!(!templates.to_string().contains(&format!("\"{}\"", stencil.id)), "{}: no listed template opens it", stencil.id);
    }
    assert!(!unshipped.is_empty() || stencils().len() == registered().len());
}

#[test]
fn a_hidden_master_is_in_no_listing_and_a_brought_master_is_copied_in_first() {
    // The hidden list is in no stencil: `all_masters` and the listing do not have it, `find_any` does.
    assert!(find_any("line-end.nothing").is_none());
    for m in line_ends::MASTERS.iter() {
        assert!(all_masters().all(|a| a.id != m.id) && find_any(m.id).is_some(), "{}", m.id);
    }
    // A master that brings another: the other is in the document first, and each is found again by its GUID.
    static BRINGS: More = More { dress: None, hidden: false, brings: &["rectangle"] };
    static BRINGER: MasterSpec = MasterSpec {
        id: "t.bringer",
        name: "Bringer",
        width: 1.0,
        height: 1.0,
        identity: (200, 2),
        outline: Outline::Ellipse,
        points: &SIDES,
        more: &BRINGS,
    };
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, &BRINGER).unwrap();
    let names: Vec<&str> = doc.masters.iter().filter_map(|m| m.name.as_deref()).collect();
    assert_eq!(names, ["Rectangle", "Bringer"], "the brought master first");
    assert_eq!(doc.masters.iter().map(|m| m.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(id, 2);
    assert_eq!(ensure_master(&mut doc, &BRINGER).unwrap(), 2, "found again");
    assert_eq!(doc.masters.len(), 2, "nothing is added twice");
    // A master that brings itself, or a chain that loops, ends.
    static LOOP_MORE: More = More { dress: None, hidden: false, brings: &["t.loop"] };
    static LOOP: MasterSpec = MasterSpec {
        id: "t.loop",
        name: "Loop",
        width: 1.0,
        height: 1.0,
        identity: (200, 3),
        outline: Outline::Ellipse,
        points: &SIDES,
        more: &LOOP_MORE,
    };
    assert!(find_any("t.loop").is_none(), "a test master is in no catalogue, so it brings nothing");
    let mut doc = Document::new_blank();
    ensure_master(&mut doc, &LOOP).unwrap();
    assert_eq!(doc.masters.len(), 1);
}

// ---- the master formats --------------------------------------------------------------------------------------------------------------

/// A document with a page and the instance of `spec` dropped at (`x`, `y`): the document, the page and the shape's `ID`.
fn dropped(spec: &MasterSpec, x: f64, y: f64) -> (Document, u32, u32) {
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let info = draw::drop_shape(&mut doc, page, spec, x, y).unwrap();
    (doc, page, info.id)
}

fn sheet_of(doc: &Document, page: u32, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(page), id).unwrap())
}

/// What a cell of a shape reads, through its master and the styles.
fn effective(doc: &Document, sheet: &SheetRef, cell: &str) -> Option<f64> {
    let found = Resolver::new(doc).resolve(sheet, &CellAddr::direct(cell))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// The text a shape shows: its characters and the shown form of its fields, in order.
fn shown_text(text: &Text) -> String {
    text.parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(s) => s.to_string(),
            TextPart::Fld { text, .. } => text.to_string(),
            _ => String::new(),
        })
        .collect()
}

fn has_own_cell(doc: &Document, page: u32, id: u32, cell: &str) -> bool {
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    doc.shape(&at).unwrap().sheet.cell(cell).is_some()
}

const PPI: f64 = 200.0;

fn render(doc: &Document, page: u32) -> Rendered {
    let view = View::fit_page(doc, page, PPI).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(None);
    renderer.render(doc, page, &view)
}

fn page_height(doc: &Document, page: u32) -> f64 {
    doc.page(page).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

/// Whether the pixel at a place on the page (inches from the left and bottom edges) is ink: clearly there and clearly dark.
fn ink_at(r: &Rendered, height: f64, x: f64, y: f64) -> bool {
    let (col, row) = ((x * PPI) as usize, ((height - y) * PPI) as usize);
    let at = (row.min(r.height as usize - 1) * r.width as usize + col.min(r.width as usize - 1)) * 4;
    let p = &r.rgba_premul[at..at + 4];
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink_in(r: &Rendered, height: f64, (left, bottom, right, top): (f64, f64, f64, f64)) -> usize {
    let step = 1.0 / PPI;
    let mut n = 0;
    let mut y = bottom;
    while y < top {
        let mut x = left;
        while x < right {
            n += usize::from(ink_at(r, height, x, y));
            x += step;
        }
        y += step;
    }
    n
}

/// How many separate runs of ink a horizontal line of the page crosses between `from` and `to`.
fn runs_along(r: &Rendered, height: f64, y: f64, from: f64, to: f64) -> usize {
    let (mut runs, mut was) = (0, false);
    let mut x = from;
    while x < to {
        let now = (-1..=1).any(|d| ink_at(r, height, x, y + f64::from(d) / PPI));
        runs += usize::from(now && !was);
        was = now;
        x += 1.0 / PPI;
    }
    runs
}

const fn look(begin: LineEnd, end: LineEnd, pattern: u8) -> ConnectorLook {
    ConnectorLook { begin, end, pattern, guard_pattern: false, route: Route::Straight, weight_pt: Some(3.0), compound: None }
}

static DASHED_DIAMOND: MasterSpec = MasterSpec {
    id: "t.dashed-diamond",
    name: "Dashed Diamond",
    width: 3.0,
    height: 0.0,
    identity: (200, 10),
    outline: Outline::Connector(look(LineEnd::Number(22), LineEnd::None, 2)),
    points: &[],
    more: &More::NONE,
};
static SOLID_PLAIN: MasterSpec = MasterSpec {
    id: "t.solid-plain",
    name: "Solid Plain",
    width: 3.0,
    height: 0.0,
    identity: (200, 11),
    outline: Outline::Connector(look(LineEnd::None, LineEnd::None, 1)),
    points: &[],
    more: &More::NONE,
};
static DASHED_PLAIN: MasterSpec = MasterSpec {
    id: "t.dashed-plain",
    name: "Dashed Plain",
    width: 3.0,
    height: 0.0,
    identity: (200, 12),
    outline: Outline::Connector(look(LineEnd::None, LineEnd::None, 2)),
    points: &[],
    more: &More::NONE,
};

#[test]
fn a_connector_master_draws_with_its_masters_ends_and_pattern_and_its_instance_holds_no_end_cell() {
    let (doc, page, id) = dropped(&DASHED_DIAMOND, 4.0, 4.0);
    let sheet = sheet_of(&doc, page, id);
    // The master's cells are what the instance reads; it holds none of its own.
    assert_eq!(effective(&doc, &sheet, "BeginArrow"), Some(22.0));
    assert_eq!(
        effective(&doc, &sheet, "EndArrow"),
        Some(0.0),
        "the master's end is none: the connector tool's solid arrowhead is not written on the instance"
    );
    assert_eq!(effective(&doc, &sheet, "LinePattern"), Some(2.0));
    for cell in ["BeginArrow", "EndArrow", "LinePattern"] {
        assert!(!has_own_cell(&doc, page, id, cell), "the instance holds no `{cell}`");
    }
    // It is placed level, its ends the master's width apart and centred on the drop.
    let at = |name: &str| effective(&doc, &sheet, name).unwrap();
    assert!(
        (at("BeginX") - 2.5).abs() < 1e-9 && (at("EndX") - 5.5).abs() < 1e-9 && (at("BeginY") - 4.0).abs() < 1e-9 && (at("EndY") - 4.0).abs() < 1e-9
    );
    // And it draws so: dashed (several runs of ink along the line), and with a hollow diamond at its begin (ink beside the line there, which the same dashed line with no begin end lacks).
    let (control, control_page, _) = dropped(&DASHED_PLAIN, 4.0, 4.0);
    let (solid, solid_page, solid_id) = dropped(&SOLID_PLAIN, 4.0, 4.0);
    assert!(effective(&solid, &sheet_of(&solid, solid_page, solid_id), "LinePattern").is_none_or(|p| p == 1.0));
    let (a, b, c) = (render(&doc, page), render(&control, control_page), render(&solid, solid_page));
    let height = page_height(&doc, page);
    let along = |r: &Rendered| runs_along(r, height, 4.0, 3.0, 5.4);
    assert!(along(&a) >= 3 && along(&b) >= 3, "the line is dashed: {} and {} runs", along(&a), along(&b));
    assert_eq!(along(&c), 1, "a solid control is one run");
    let beside_begin = |r: &Rendered| ink_in(r, height, (2.55, 4.06, 2.95, 4.25)) + ink_in(r, height, (2.55, 3.75, 2.95, 3.94));
    assert!(beside_begin(&a) > 20, "the diamond at the begin is drawn: {}", beside_begin(&a));
    assert_eq!(beside_begin(&b), 0, "and the control, with no begin end, has none");
    assert!(a.skipped.is_empty(), "{:?}", a.skipped);
    // The master carries them as cells of its own shape.
    let master = doc.master(doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().master.unwrap()).unwrap();
    let cell = |name: &str| master.shapes.get(0).unwrap().sheet.cell(name).and_then(|c| c.value.as_ref().and_then(|v| v.number()));
    assert_eq!((cell("BeginArrow"), cell("EndArrow"), cell("LinePattern")), (Some(22.0), Some(0.0), Some(2.0)));
    assert_eq!(cell("ShapeRouteStyle"), Some(2.0), "straight");
    assert!((cell("LineWeight").unwrap() - 3.0 / 72.0).abs() < 1e-9);
}

#[test]
fn a_connector_looks_cells_are_what_it_says_and_a_solid_look_writes_no_pattern() {
    let cells = |look: ConnectorLook| -> Vec<(String, String)> {
        look.cells().into_iter().map(|(name, cell)| (name.to_owned(), cell.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())).collect()
    };
    let flow = cells(ConnectorLook::DATA_FLOW);
    assert_eq!(
        flow,
        [("BeginArrow".into(), "0".into()), ("EndArrow".into(), "4".into()), ("FillPattern".into(), "0".into())],
        "what the one connector master had"
    );
    let guarded = ConnectorLook {
        guard_pattern: true,
        pattern: 2,
        route: Route::RightAngle,
        compound: Some(1),
        ..ConnectorLook::plain(LineEnd::Number(20), LineEnd::Number(16))
    };
    let names: Vec<String> = cells(guarded).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["BeginArrow", "EndArrow", "FillPattern", "LinePattern", "CompoundType", "ShapeRouteStyle", "ConLineRouteExt"]);
    let pattern = guarded.cells().into_iter().find(|(n, _)| *n == "LinePattern").unwrap().1;
    assert!(matches!(&pattern.formula, FormulaSlot::Local(f) if &*f.source == "GUARD(2)"), "a guarded pattern is a GUARD formula");
    assert_eq!(pattern.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("2"));
    // A solid line that is guarded still says so.
    let solid_guard = ConnectorLook { guard_pattern: true, ..ConnectorLook::plain(LineEnd::None, LineEnd::Number(4)) };
    assert!(solid_guard.cells().iter().any(|(n, c)| *n == "LinePattern" && matches!(&c.formula, FormulaSlot::Local(f) if &*f.source == "GUARD(1)")));
    // The ends: a formula of the shape's data, and a master of ours.
    let formula = LineEnd::Formula { formula: "IF(Prop.Kind=\"A\",28,30)", value: 28 };
    assert_eq!((formula.number(), LineEnd::None.number(), LineEnd::Number(16).number()), (28, 0, 16));
    let custom = LineEnd::Master { id: "line-end.filled-diamond", name: "Filled Diamond" };
    assert_eq!(custom.number(), 254);
    let cell = ConnectorLook::plain(custom, LineEnd::None).cells().into_iter().next().unwrap().1;
    assert!(matches!(&cell.formula, FormulaSlot::Local(f) if &*f.source == "USE(\"Filled Diamond\")"), "{cell:?}");
    assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("254"));
    assert!(custom.is_known() && formula.is_known() && LineEnd::Number(45).is_known() && !LineEnd::Number(46).is_known());
    assert_eq!(ConnectorLook::DATA_FLOW.numbers(), (0, 4, 1));
}

#[test]
fn the_data_flow_master_is_what_it_was_and_keeps_end_4() {
    let flow = find_master("gane-sarson.data-flow").unwrap();
    let look = flow.look().expect("a connector master has a look");
    assert_eq!(look.numbers(), (0, 4, 1));
    assert!(flow.is_connector() && !flow.is_fixed_1d());
    let (doc, page, id) = dropped(flow, 4.0, 4.0);
    let sheet = sheet_of(&doc, page, id);
    assert_eq!(effective(&doc, &sheet, "EndArrow"), Some(4.0));
    assert_eq!(effective(&doc, &sheet, "BeginArrow"), Some(0.0));
    assert!(!has_own_cell(&doc, page, id, "EndArrow"), "the instance inherits its end: it holds no `EndArrow`");
    assert!(has_own_cell(&doc, page, id, "BeginX") && has_own_cell(&doc, page, id, "EndX"), "its ends are its own");
    // It is a routed connector: DiaCraft's own route marker is on the instance.
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    let user = doc.shape(&at).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "User").expect("the route marker");
    assert!(user.rows.iter().any(|r| r.name.as_deref() == Some("DiaCraftRoute")));
}

static ZIGZAG: [PathSpec; 1] = [PathSpec {
    fill: false,
    segs: &[
        Seg::Move(fw(0.0), fh(0.0)),
        Seg::Line(fw(0.2), fk(0.1)),
        Seg::Line(fw(0.4), fk(-0.1)),
        Seg::Line(fw(0.6), fk(0.1)),
        Seg::Line(fw(0.8), fk(-0.1)),
        Seg::Line(fw(1.0), fh(0.0)),
    ],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

static COMM_LINK: MasterSpec = MasterSpec {
    id: "t.comm-link",
    name: "Comm Link",
    width: 2.0,
    height: 0.0,
    identity: (200, 20),
    outline: Outline::Fixed1D { look: ConnectorLook::plain(LineEnd::None, LineEnd::None), paths: &ZIGZAG },
    points: &[],
    more: &More::NONE,
};

fn geometry_rows(doc: &Document, sheet: &SheetRef) -> Vec<(String, f64, f64)> {
    let res = Resolver::new(doc);
    let rows = res.effective_rows(sheet, &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    rows.iter()
        .map(|row| {
            let at =
                |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap();
            (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
        })
        .collect()
}

#[test]
fn a_fixed_1d_master_drops_horizontal_both_ends_glue_and_a_move_of_a_target_does_not_route_it() {
    assert!(COMM_LINK.is_connector() && COMM_LINK.is_fixed_1d());
    let mut doc = Document::new_blank();
    let page = doc.pages.get(0).unwrap().id;
    let rectangle = find_master("rectangle").unwrap();
    let left = draw::drop_shape(&mut doc, page, rectangle, 2.0, 3.0).unwrap().id;
    let right = draw::drop_shape(&mut doc, page, rectangle, 7.0, 3.0).unwrap().id;
    let link = draw::drop_shape(&mut doc, page, &COMM_LINK, 4.5, 6.0).unwrap().id;
    let sheet = sheet_of(&doc, page, link);
    // Dropped level, its ends the master's width apart and centred on the drop point, with no route marker and no route of its own.
    let at = |doc: &Document, name: &str| effective(doc, &sheet_of(doc, page, link), name).unwrap();
    assert!((at(&doc, "BeginX") - 3.5).abs() < 1e-9 && (at(&doc, "EndX") - 5.5).abs() < 1e-9);
    assert_eq!(at(&doc, "BeginY"), at(&doc, "EndY"));
    let shape = doc.shape(&doc.find_shape(Owner::Page(page), link).unwrap()).unwrap().clone();
    assert!(
        shape.sheet.sections.iter().all(|s| s.name.as_str() != "User" && s.name.as_str() != "Geometry"),
        "no route marker and no route rows: {:?}",
        shape.sheet.sections.iter().map(|s| s.name.to_string()).collect::<Vec<_>>()
    );
    assert!(!has_own_cell(&doc, page, link, "EndArrow"));
    let zigzag = geometry_rows(&doc, &sheet);
    assert_eq!(zigzag.len(), 6, "the geometry is the master's zigzag");
    assert!((zigzag[1].1 - 0.2 * 2.0).abs() < 1e-9 && (zigzag[1].2 - 0.1).abs() < 1e-9 && (zigzag[2].2 + 0.1).abs() < 1e-9);
    // Both ends glue, as a connector's do.
    draw::reconnect_connector(&mut doc, page, link, Which::Begin, &End::Shape { id: left, point: None }).unwrap();
    draw::reconnect_connector(&mut doc, page, link, Which::End, &End::Shape { id: right, point: None }).unwrap();
    assert_eq!(doc.page(page).unwrap().connects.len(), 2, "glued at both ends");
    let glued = zigzag_after_glue(&doc, page, link);
    assert!(glued.0.len() == 6 && glued.0.iter().all(|r| r.0 == zigzag[0].0 || r.0 == "LineTo"), "still the master's six rows: {:?}", glued.0);
    // A move of a shape it is glued to carries the end and stretches the figure along the line, and lays no route over it.
    let (_, before_end_x) = (glued.1, at(&doc, "EndX"));
    draw::move_shapes(&mut doc, page, &[right], 1.5, 0.0).unwrap();
    let after = zigzag_after_glue(&doc, page, link);
    assert!((at(&doc, "EndX") - (before_end_x + 1.5)).abs() < 1e-6, "the end follows its target");
    assert_eq!(after.0.len(), 6, "no route was laid over the figure");
    assert!(
        after.0.iter().enumerate().all(|(i, r)| (r.1 - glued.0[i].1 * (after.1 / glued.1)).abs() < 1e-6 && (r.2 - glued.0[i].2).abs() < 1e-9),
        "stretched along the line, the same across it: {:?} against {:?}",
        after.0,
        glued.0
    );
    let shape = doc.shape(&doc.find_shape(Owner::Page(page), link).unwrap()).unwrap();
    // No route marker came back. (The instance may hold the values its inherited formulas give on its own width: they are cached results, not a route.)
    assert!(shape.sheet.sections.iter().all(|s| s.name.as_str() != "User"), "still no route marker");
    let kinds: Vec<&str> = after.0.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(kinds, ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo", "LineTo"], "the master's rows, not a route's");
}

/// The geometry rows of a Fixed1D shape, and its width.
fn zigzag_after_glue(doc: &Document, page: u32, id: u32) -> (Vec<(String, f64, f64)>, f64) {
    let sheet = sheet_of(doc, page, id);
    (geometry_rows(doc, &sheet), effective(doc, &sheet, "Width").unwrap())
}

fn sixty_five() -> Vec<OwnedPath> {
    (0..65)
        .map(|_| OwnedPath {
            fill: false,
            segs: vec![Seg::Move(fw(0.0), fh(0.0)), Seg::Line(fw(1.0), fh(1.0))],
            no_show: Flag::Off,
            no_line: Flag::Off,
        })
        .collect()
}

fn three_sections() -> Vec<OwnedPath> {
    (0..3)
        .map(|_| OwnedPath {
            fill: false,
            segs: vec![Seg::Move(fw(0.0), fh(0.0)), Seg::Line(fw(1.0), fh(1.0))],
            no_show: Flag::Off,
            no_line: Flag::Off,
        })
        .collect()
}

fn many_rows() -> Vec<OwnedPath> {
    let mut segs = vec![Seg::Move(fw(0.0), fh(0.0))];
    segs.extend((0..MAX_ROWS + 5).map(|i| Seg::Line(fw((i % 7) as f64 / 7.0), fh(1.0))));
    vec![OwnedPath { fill: false, segs, no_show: Flag::Off, no_line: Flag::Off }]
}

fn code_master(build: fn() -> Vec<OwnedPath>) -> MasterSpec {
    MasterSpec {
        id: "t.code",
        name: "Code",
        width: 1.0,
        height: 1.0,
        identity: (200, 30),
        outline: Outline::Code { build, text: TextBox::FULL },
        points: &SIDES,
        more: &More::NONE,
    }
}

#[test]
fn a_code_outline_past_the_caps_fails_the_catalogue_check() {
    let within = code_master(three_sections);
    assert_eq!(caps_problem(&within), None);
    let sections = code_master(sixty_five);
    let said = caps_problem(&sections).expect("65 sections are past 64");
    assert!(said.contains("t.code") && said.contains("65 Geometry sections, past 64"), "{said}");
    let rows = code_master(many_rows);
    let said = caps_problem(&rows).expect("rows past 1,024");
    assert!(said.contains("rows, past 1024"), "{said}");
    // A master with more than 128 connection points.
    static MANY_POINTS: [Point; 129] = [pt(0.5, 0.5, 1.0, 0.0); 129];
    let points = MasterSpec { points: &MANY_POINTS, ..code_master(three_sections) };
    assert!(caps_problem(&points).unwrap().contains("129 connection points, past 128"));
    // The build never goes past them, whatever the function made.
    let master = build_master(&sections, 1);
    let geometry = master.shapes.get(0).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").count();
    assert_eq!(geometry, MAX_SECTIONS, "a build is cut at the caps");
    let master = build_master(&rows, 1);
    let rows: usize = master.shapes.get(0).unwrap().sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").map(|s| s.rows.len()).sum();
    assert_eq!(rows, MAX_ROWS);
    // Every master of the catalogue is within them.
    for m in all_masters() {
        assert_eq!(caps_problem(m), None, "{}", m.id);
    }
}

static BOX_UNDER: [PathSpec; 1] = [PathSpec {
    fill: true,
    segs: &[
        Seg::Move(fw(0.0), fh(0.0)),
        Seg::Line(fw(1.0), fh(0.0)),
        Seg::Line(fw(1.0), fh(1.0)),
        Seg::Line(fw(0.0), fh(1.0)),
        Seg::Line(fw(0.0), fh(0.0)),
    ],
    no_show: Flag::Off,
    no_line: Flag::Off,
}];

static LABELLED: MasterSpec = MasterSpec {
    id: "t.labelled",
    name: "Labelled",
    width: 1.0,
    height: 0.8,
    identity: (200, 40),
    outline: Outline::Paths { paths: &BOX_UNDER, text: kit::LABEL_BELOW },
    points: &SIDES,
    more: &More::NONE,
};

#[test]
fn a_label_below_the_figure_has_its_text_cells_as_formulas_of_width_and_height() {
    let (mut doc, page, id) = dropped(&LABELLED, 4.0, 4.0);
    let master_id = doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().master.unwrap();
    let shape = doc.master(master_id).unwrap().shapes.get(0).unwrap().clone();
    let formula = |name: &str| match &shape.sheet.cell(name).unwrap().formula {
        FormulaSlot::Local(f) => {
            assert!(f.ast.is_ok(), "{name}: `{}` parses", f.source);
            f.source.to_string()
        }
        other => panic!("{name}: {other:?}"),
    };
    // Shares of the box: the block is 1.6 times as wide, centred, and below the figure.
    assert_eq!(formula("TxtPinX"), "Width*0.5");
    assert_eq!(formula("TxtWidth"), "Width*1.6");
    assert_eq!(formula("TxtHeight"), "Height*0.35");
    assert_eq!(formula("TxtPinY"), "Height*-0.225");
    assert_eq!(formula("TxtLocPinX"), "TxtWidth*0.5");
    assert_eq!(formula("TxtLocPinY"), "TxtHeight*0.5");
    // The values are what the formulas give at the default size.
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((report.written, report.refused), (0, 0), "the cached values are the formulas'");
    let sheet = sheet_of(&doc, page, id);
    let near = |name: &str, want: f64| assert!((effective(&doc, &sheet, name).unwrap() - want).abs() < 1e-9, "{name}");
    near("TxtPinX", 0.5);
    near("TxtPinY", -0.18);
    near("TxtWidth", 1.6);
    near("TxtHeight", 0.28);
    // And the block follows the figure when it is resized: it is a formula of the instance's own width and height.
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let dropped = s.execute("shape.drop", &json!({ "master": "rectangle", "page": 1, "x": 4.0, "y": 4.0 })).unwrap();
    assert!(dropped["id"].as_u64().is_some());
}

static TEST_DRESS: Dress = Dress {
    fill: Some("THEMEVAL(\"AccentColor\",RGB(58,127,192))"),
    fill_pattern: None,
    line_weight_pt: Some(2.0),
    line_pattern: Some(Flag::Formula("GUARD(2)")),
    text: TextSpec::Fields(&[
        FieldText { formula: "Prop.Name", shown: "Name", char_row: 0, line_after: true },
        FieldText { formula: "Prop.Kind", shown: "Box", char_row: 1, line_after: false },
    ]),
    chars: &[CharRow { size_pt: 11.0, style: 1, color: None }, CharRow { size_pt: 8.0, style: 0, color: Some("RGB(100,100,100)") }],
    data: &[
        FieldDef { row: "Name", label: "Name", kind: FieldKind::String, value: "Name", prompt: "The name" },
        FieldDef { row: "Kind", label: "Kind", kind: FieldKind::FixedList("Box;Ring"), value: "Box", prompt: "The kind" },
        FieldDef { row: "Abstract", label: "Abstract", kind: FieldKind::Boolean, value: "0", prompt: "" },
        FieldDef { row: "Count", label: "Count", kind: FieldKind::Number, value: "3", prompt: "" },
    ],
    user: &[("DiaCraftTest", "Marker")],
    locks: &[("LockHeight", 1.0), ("LockRotate", 1.0)],
    select_mode: Some(1),
    text_edit_target: Some(false),
    obj_type: Some(1),
    role: Role::Container { heading: Side::Left, margin_in: 0.3 },
};
static DRESSED_MORE: More = More { dress: Some(&TEST_DRESS), hidden: false, brings: &[] };
static DRESSED: MasterSpec = MasterSpec {
    id: "t.dressed",
    name: "Dressed",
    width: 1.5,
    height: 1.0,
    identity: (200, 50),
    outline: Outline::Polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
    points: &SIDES,
    more: &DRESSED_MORE,
};

#[test]
fn a_dress_writes_its_shape_data_fields_user_rows_and_locks_into_the_master() {
    let (mut doc, page, id) = dropped(&DRESSED, 4.0, 4.0);
    let master_id = doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().master.unwrap();
    let shape = doc.master(master_id).unwrap().shapes.get(0).unwrap().clone();
    let cell = |name: &str| shape.sheet.cell(name).unwrap_or_else(|| panic!("no cell {name}")).clone();
    let number = |name: &str| cell(name).value.as_ref().and_then(|v| v.number()).unwrap_or_else(|| panic!("no number in {name}"));
    // The look: a fill formula that is settled to its value, a weight, a guarded pattern.
    assert!(matches!(&cell("FillForegnd").formula, FormulaSlot::Local(f) if f.source.starts_with("THEMEVAL")));
    let fill = cell("FillForegnd").value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default();
    assert!(fill.starts_with('#') && fill.len() == 7, "the fill formula is settled to a colour: `{fill}`");
    assert_eq!(number("FillPattern"), 1.0, "a fill given and no pattern is solid");
    assert!((number("LineWeight") - 2.0 / 72.0).abs() < 1e-9);
    assert!(matches!(&cell("LinePattern").formula, FormulaSlot::Local(f) if &*f.source == "GUARD(2)"));
    assert_eq!(number("LinePattern"), 2.0, "a guard is settled to its value");
    // Protection, group properties.
    assert_eq!((number("LockHeight"), number("LockRotate")), (1.0, 1.0));
    assert_eq!((number("SelectMode"), number("IsTextEditTarget")), (1.0, 0.0));
    // The Shape Data rows, in order, typed as `shapedata.define` types them.
    let property = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Property").expect("Shape Data");
    let rows: Vec<(&str, String, String, String, Option<&str>)> = property
        .rows
        .iter()
        .map(|r| {
            let text = |n: &str| {
                r.cells.get(&diacraft_doc::CellName::new(n)).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default()
            };
            let unit = r.cells.get(&diacraft_doc::CellName::new("Value")).and_then(|c| c.unit.as_deref());
            (r.name.as_deref().unwrap(), text("Type"), text("Value"), text("Format"), unit)
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Name", "0".into(), "Name".into(), String::new(), Some("STR")),
            ("Kind", "1".into(), "Box".into(), "Box;Ring".into(), Some("STR")),
            ("Abstract", "3".into(), "0".into(), String::new(), Some("BOOL")),
            ("Count", "2".into(), "3".into(), String::new(), None),
        ]
    );
    let sort_keys: Vec<String> =
        property.rows.iter().map(|r| r.cells[&diacraft_doc::CellName::new("SortKey")].value.as_ref().unwrap().text().to_owned()).collect();
    assert_eq!(sort_keys, ["0001", "0002", "0003", "0004"]);
    assert_eq!(property.rows[0].cells[&diacraft_doc::CellName::new("Prompt")].value.as_ref().unwrap().text(), "The name");
    // The fields of the text, each the value of its Shape Data row, with the shown form they have at the master's data.
    let fields = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Field").expect("Field section");
    let shown: Vec<(String, String)> = fields
        .rows
        .iter()
        .map(|r| {
            let c = &r.cells[&diacraft_doc::CellName::new("Value")];
            (
                match &c.formula {
                    FormulaSlot::Local(f) => f.source.to_string(),
                    _ => String::new(),
                },
                c.value.as_ref().unwrap().text().to_owned(),
            )
        })
        .collect();
    assert_eq!(shown, [("Prop.Name".into(), "Name".into()), ("Prop.Kind".into(), "Box".into())]);
    // Character formats: size in inches, style bits, a colour formula that is settled.
    let chars = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").expect("Character section");
    assert_eq!(chars.rows.len(), 2);
    let num =
        |r: &std::sync::Arc<diacraft_doc::Row>, n: &str| r.cells[&diacraft_doc::CellName::new(n)].value.as_ref().and_then(|v| v.number()).unwrap();
    assert!((num(&chars.rows[0], "Size") - 11.0 / 72.0).abs() < 1e-9 && num(&chars.rows[0], "Style") == 1.0);
    assert!((num(&chars.rows[1], "Size") - 8.0 / 72.0).abs() < 1e-9);
    assert!(chars.rows[1].cells[&diacraft_doc::CellName::new("Color")].value.is_some(), "the colour formula is settled");
    assert!(!chars.rows[0].cells.contains_key(&diacraft_doc::CellName::new("Color")), "no colour formula: the style's");
    // The User rows: the container role and the rows of our own. A container master has its settings and no style row.
    let user = shape.sheet.sections.iter().find(|s| s.name.as_str() == "User").expect("User section");
    let named: Vec<&str> = user.rows.iter().filter_map(|r| r.name.as_deref()).collect();
    assert_eq!(
        named,
        [
            "msvStructureType",
            "DiaCraftContainerHeading",
            "DiaCraftContainerMargin",
            "DiaCraftContainerResize",
            "DiaCraftContainerLock",
            "DiaCraftTest"
        ]
    );
    let user_value = |n: &str| {
        user.rows.iter().find(|r| r.name.as_deref() == Some(n)).unwrap().cells[&diacraft_doc::CellName::new("Value")]
            .value
            .as_ref()
            .unwrap()
            .text()
            .to_owned()
    };
    assert_eq!(
        (user_value("msvStructureType").as_str(), user_value("DiaCraftContainerHeading").as_str(), user_value("DiaCraftTest").as_str()),
        ("Container", "3", "Marker")
    );
    assert!((user_value("DiaCraftContainerMargin").parse::<f64>().unwrap() - 0.3).abs() < 1e-9);
    // The master's own text shows the fields, a line break after the first.
    let text = shape.text.as_ref().expect("the master has a text");
    let master_text = shown_text(text);
    assert_eq!(master_text, "Name\nBox");
    // A shape dropped from it keeps the text that shows the fields (the shown form of a field lives in the text that holds it), and reads the master's Shape Data.
    let instance = doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().clone();
    assert_eq!(shown_text(instance.text.as_ref().expect("an own text")), "Name\nBox");
    let sheet = sheet_of(&doc, page, id);
    assert_eq!(effective(&doc, &sheet, "LockHeight"), Some(1.0), "the instance reads the master's locks");
    // Every cached value of the master is what its formula gives.
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((report.written, report.refused), (0, 0), "settled values are the formulas'");
    // The caps of the dress's master hold.
    assert_eq!(caps_problem(&DRESSED), None);
}

#[test]
fn a_dress_with_an_object_type_writes_it_into_the_master_and_a_dress_without_one_writes_none() {
    // The dress of a shape people join gives `ObjType` 1; the master's shape holds the cell and a shape dropped from it reads it, holding none of its own.
    let (doc, page, id) = dropped(&DRESSED, 4.0, 4.0);
    let master_id = doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().master.unwrap();
    let master = doc.master(master_id).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!(master.sheet.cell("ObjType").and_then(|c| c.value.as_ref()?.number()), Some(1.0));
    let instance = doc.shape(&doc.find_shape(Owner::Page(page), id).unwrap()).unwrap().clone();
    assert!(instance.sheet.cell("ObjType").is_none(), "the shape holds no cell of its own for it");
    assert_eq!(effective(&doc, &sheet_of(&doc, page, id), "ObjType"), Some(1.0), "and reads the master's");
    // A dress that gives none (`Dress::NONE`, a plain master) writes no cell: the shape reads none until a connector is glued to it.
    let (doc, page, id) = dropped(&SOLID_PLAIN, 4.0, 4.0);
    assert_eq!(effective(&doc, &sheet_of(&doc, page, id), "ObjType"), None);
}

#[test]
fn stencil_list_gives_a_connector_master_its_ends_and_pattern() {
    let listed = list();
    let gs = listed["stencils"].as_array().unwrap().iter().find(|s| s["id"] == "gane-sarson").unwrap();
    let flow = gs["masters"].as_array().unwrap().iter().find(|m| m["id"] == "gane-sarson.data-flow").unwrap();
    assert_eq!(flow["one_d"], true);
    assert_eq!(flow["ends"], json!({ "begin": 0, "end": 4, "pattern": 1 }), "the Data Flow: a solid line ending in a solid arrowhead");
    let process = gs["masters"].as_array().unwrap().iter().find(|m| m["id"] == "gane-sarson.process").unwrap();
    assert!(process.get("ends").is_none() && process.get("one_d").is_none(), "a two-dimensional master has no ends");
    // A test master with a begin end and a dash.
    let json = master_json(&DASHED_DIAMOND);
    assert_eq!(json["ends"], json!({ "begin": 22, "end": 0, "pattern": 2 }));
    assert_eq!(json["one_d"], true);
    // A one-dimensional figure is listed as one, with its ends.
    let json = master_json(&COMM_LINK);
    assert_eq!((json["one_d"].clone(), json["ends"].clone()), (json!(true), json!({ "begin": 0, "end": 0, "pattern": 1 })));
    // Every one-dimensional master of the catalogue is listed with ends that are numbers gives or the custom one.
    for m in all_masters().filter(|m| m.is_connector()) {
        let json = master_json(m);
        for key in ["begin", "end"] {
            let n = json["ends"][key].as_u64().unwrap();
            assert!(n <= 45 || n == 254, "{}: {key} {n}", m.id);
        }
    }
}

// ---- the invariants every shipped master keeps ----------------------------------------------------------------------------------------

/// The masters the invariants are about, with their stencil: every master of every shipped stencil.
fn shipped_masters() -> Vec<(&'static StencilSpec, &'static MasterSpec)> {
    stencils().iter().flat_map(|st| st.masters().iter().map(move |m| (st, m))).collect()
}

#[test]
fn ids_are_unique_and_namespaced_and_guids_are_unique_and_the_series_is_the_stencils() {
    let mut ids = std::collections::BTreeSet::new();
    let mut guids = std::collections::BTreeSet::new();
    for (stencil, master) in shipped_masters() {
        assert!(ids.insert(master.id), "{}: the id is another master's", master.id);
        assert!(guids.insert(master.guid()), "{}: the GUID is another master's", master.id);
        assert!(master.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b)), "{}", master.id);
        let series = SERIES.iter().find(|(id, _)| *id == stencil.id).map(|(_, s)| *s).unwrap_or_else(|| panic!("{} has a series", stencil.id));
        assert_eq!(master.identity.0, series, "{}: one series for the stencil", master.id);
        if master.identity.0 >= 9 {
            assert!(master.id.starts_with(&format!("{}.", stencil.id)), "{}: a master of the second batch is namespaced by its stencil", master.id);
        }
        // The id names the master and the name names it for a person: no two masters of a stencil are called the same.
        assert_eq!(stencil.masters().iter().filter(|m| m.name == master.name).count(), 1, "{}: a name twice in {}", master.name, stencil.id);
    }
    // The hidden line ends: ids of their own, GUIDs of their own.
    for m in line_ends::MASTERS.iter() {
        assert!(ids.insert(m.id) && guids.insert(m.guid()), "{}", m.id);
        assert!(m.id.starts_with("line-end."), "{}", m.id);
    }
}

#[test]
fn sizes_are_positive_and_at_most_three_inches_before_the_second_batch_and_ten_after() {
    for (_, m) in shipped_masters() {
        let most = if m.identity.0 >= 9 { 10.0 } else { 3.0 };
        assert!(m.width > 0.0 && m.width <= most, "{}: width {}", m.id, m.width);
        if m.is_connector() {
            assert_eq!(m.height, 0.0, "{}: a one-dimensional master has no height", m.id);
        } else {
            assert!(m.height > 0.0 && m.height <= most, "{}: height {}", m.id, m.height);
        }
    }
}

#[test]
fn every_formula_parses_and_every_cached_value_is_what_its_formula_gives_at_the_default_size() {
    for (_, m) in shipped_masters() {
        assert_formulas_hold(m);
    }
}

/// Asserts that every formula of `m` (and of each member of a group master) parses and holds a cached value, and that a recalculation writes nothing.
#[track_caller]
pub(super) fn assert_formulas_hold(m: &MasterSpec) {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, m).unwrap();
    let top = doc.master(id).unwrap().shapes.get(0).unwrap().clone();
    let mut unparsed = Vec::new();
    let mut check = |cell: &diacraft_doc::Cell| {
        if let FormulaSlot::Local(f) = &cell.formula {
            if f.ast.is_err() {
                unparsed.push(f.source.to_string());
            }
            assert!(cell.value.is_some(), "{}: `{}` has no cached value", m.id, f.source);
        }
    };
    // The members of a group master hold formulas too (their frames, the heights of their texts): the shape and each member are looked at.
    for shape in std::iter::once(&top).chain(top.children.iter()) {
        shape.sheet.cells.values().for_each(&mut check);
        for section in &shape.sheet.sections {
            section.cells.values().for_each(&mut check);
            section.rows.iter().for_each(|r| r.cells.values().for_each(&mut check));
        }
    }
    assert!(unparsed.is_empty(), "{}: {unparsed:?}", m.id);
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert!(!report.budget_exhausted, "{}", m.id);
    assert_eq!((report.written, report.refused), (0, 0), "{}: a saved value differs from its formula", m.id);
}

#[test]
fn every_path_closes_where_it_fills() {
    for (_, m) in shipped_masters().into_iter().filter(|(_, m)| !m.is_connector()) {
        assert_paths_close(m);
    }
}

/// Asserts that every path of `m` (and of each member of a group master) that fills is one run that comes back to where it started.
#[track_caller]
pub(super) fn assert_paths_close(m: &MasterSpec) {
    let mut doc = Document::new_blank();
    let id = ensure_master(&mut doc, m).unwrap();
    let res = Resolver::new(&doc);
    // The shape and, for a group master, each member (a group has no geometry of its own).
    let top = diacraft_doc::ShapeRef::top(Owner::Master(id), 0);
    let members = doc.shape(&top).map_or(0, |shape| shape.children.len());
    let sheets = std::iter::once(top.clone()).chain((0..members).filter_map(|k| u32::try_from(k).ok()).map(|k| top.child(k))).map(SheetRef::Shape);
    for sheet in sheets {
        for key in res.effective_sections(&sheet, "Geometry").unwrap_or_default() {
            let flag = |name: &str| effective_cell(&res, &sheet, &CellAddr::in_section("Geometry", key.ix, name)).unwrap_or(0.0);
            if flag("NoFill") == 1.0 {
                continue;
            }
            let rows = res.effective_rows(&sheet, &key).unwrap();
            let point = |row: &diacraft_doc::EffectiveRow<'_>| {
                let at = |name: &str| effective_cell(&res, &sheet, &CellAddr::in_row("Geometry", key.ix, row.key.clone(), name)).unwrap_or(f64::NAN);
                (at("X"), at("Y"))
            };
            if rows.first().and_then(|r| r.row_type.as_deref()) == Some("Ellipse") {
                continue;
            }
            let runs = rows.iter().filter(|r| r.row_type.as_deref() == Some("MoveTo")).count();
            let (first, last) = (point(rows.first().unwrap()), point(rows.last().unwrap()));
            // A filled path is one run that comes back to where it started (a pen lifted and put down is a path that is two).
            assert_eq!(runs, 1, "{}: section {:?} fills and is {runs} runs", m.id, key.ix);
            assert!(
                (first.0 - last.0).abs() < 1e-9 && (first.1 - last.1).abs() < 1e-9,
                "{}: section {:?} fills and does not close ({first:?} to {last:?})",
                m.id,
                key.ix
            );
        }
    }
}

fn effective_cell(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<f64> {
    res.resolve(sheet, addr)?.value.cell?.value.as_ref()?.number()
}

/// Asserts that every connection point of `m`, dropped on a page, lies on the outline it draws (within 0.001 inch of a path), leaves along an axis and faces away from the shape's centre.
#[track_caller]
pub(super) fn assert_points_on_the_outline(m: &MasterSpec) {
    let (doc, page, id) = dropped(m, 5.0, 5.0);
    let reader = draw::Reader::new(&doc);
    let at = doc.find_shape(Owner::Page(page), id).unwrap();
    // A group draws through its members, so its outline is theirs together.
    let members = doc.shape(&at).map_or(0, |shape| shape.children.len());
    let mut paths = Vec::new();
    for shape in std::iter::once(at.clone()).chain((0..members).filter_map(|k| u32::try_from(k).ok()).map(|k| at.child(k))) {
        paths.extend(crate::snap::outline_for(&reader, &shape, 1.0, &mut crate::snap::Budget::new(), crate::snap::Purpose::Box).paths);
    }
    assert!(!paths.is_empty(), "{}: it draws something", m.id);
    let points = draw::connection_points(&doc, page, id);
    assert_eq!(points.len(), m.point_count().min(MAX_POINTS), "{}", m.id);
    let centre = (5.0, 5.0);
    for p in &points {
        let nearest = paths
            .iter()
            .flat_map(|path| path.segments())
            .map(|segment| kurbo::ParamCurveNearest::nearest(&segment, kurbo::Point::new(p.x, p.y), 1e-6).distance_sq.sqrt())
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 0.001, "{}: point {} is {nearest:.4} inch from the outline", m.id, p.index);
        assert_eq!(p.dir_x.abs() + p.dir_y.abs(), 1.0, "{}: point {} leaves along an axis", m.id, p.index);
        assert!((p.x - centre.0) * p.dir_x + (p.y - centre.1) * p.dir_y >= -1e-9, "{}: point {} faces into the shape", m.id, p.index);
    }
}

#[test]
fn every_connection_point_lies_on_the_drawn_outline_and_faces_outward() {
    for (_, m) in shipped_masters().into_iter().filter(|(_, m)| !m.is_connector() && !m.points.is_empty()) {
        assert_points_on_the_outline(m);
    }
}

#[test]
fn every_master_has_the_points_the_catalogue_asks_four_on_a_two_dimensional_one_and_none_on_a_pointless_one() {
    for (_, m) in shipped_masters() {
        if m.is_connector() {
            assert_eq!(m.point_count(), 0, "{}: a one-dimensional master has no points of its own", m.id);
        } else if m.dress().is_some_and(|d| matches!(d.role, Role::Pointless)) {
            assert_eq!(m.point_count(), 0, "{}: a title or a key has no connection point", m.id);
        } else {
            assert!(m.point_count() >= 4, "{}: {} points", m.id, m.point_count());
        }
    }
}

#[test]
fn every_master_of_the_second_batch_has_search_words() {
    for (_, m) in shipped_masters().into_iter().filter(|(_, m)| m.identity.0 >= 9) {
        assert!(!keywords::of(m.id).is_empty(), "{} has no search words", m.id);
    }
}

#[test]
fn a_connector_masters_ends_are_numbers_of_the_set_or_the_custom_one_with_the_master_it_brings() {
    for (_, m) in shipped_masters() {
        let Some(look) = m.look() else { continue };
        for end in [look.begin, look.end] {
            assert!(end.is_known(), "{}: end {end:?} is no number", m.id);
            if let LineEnd::Master { id, .. } = end {
                assert!(m.more.brings.contains(&id), "{}: it uses `{id}` and does not bring it", m.id);
                let brought = find_any(id).unwrap_or_else(|| panic!("{}: `{id}` is no built-in master", m.id));
                assert!(brought.is_hidden() && brought.identity.0 == LINE_END_SERIES, "{id}: a hidden line end");
            }
        }
        assert!((1..=4).contains(&look.pattern), "{}: pattern {}", m.id, look.pattern);
    }
}

#[test]
fn a_master_made_in_code_reads_back_a_row_for_each_segment_and_the_kit_figures_stay_in_their_box() {
    // The figures of the kit as masters: each stays inside its box at the default size and at twice the width and half the height, and each filled path closes.
    /// A figure of the kit by name, and the size of the box it is tried in.
    type Figure = (&'static str, fn() -> Vec<OwnedPath>, f64, f64);
    let figures: [Figure; 9] = [
        ("note", kit::note, 1.4, 0.8),
        ("folded_document", kit::folded_document, 0.5, 0.65),
        ("rounded_box", || kit::rounded_box(0.15), 1.4, 0.7),
        ("cylinder_v", kit::cylinder_v, 0.8, 1.0),
        ("cylinder_h", kit::cylinder_h, 2.0, 0.8),
        ("stick_figure", kit::stick_figure, 0.5, 1.0),
        ("diamond", kit::diamond, 0.6, 0.6),
        ("box_3d", || kit::box_3d(0.2), 2.4, 1.6),
        ("code", three_sections, 1.0, 1.0),
    ];
    // The stick figure's points are the head's top, the hands and a foot: its sides' middles are not on it. The generic figure of three sections is a diagonal.
    static STICK: [Point; 4] = [pt(0.5, 1.0, 0.0, 1.0), pt(0.0, 0.62, -1.0, 0.0), pt(1.0, 0.62, 1.0, 0.0), pt(0.0, 0.0, 0.0, -1.0)];
    for (name, build, w, h) in figures {
        let master = MasterSpec {
            id: "t.figure",
            name: "Figure",
            width: w,
            height: h,
            identity: (200, 60),
            outline: Outline::Code { build, text: TextBox::FULL },
            points: if name == "stick_figure" { &STICK } else { &SIDES },
            more: &More::NONE,
        };
        if name != "code" {
            assert_points_on_the_outline(&master);
        }
        assert_eq!(caps_problem(&master), None, "{name}");
        let mut doc = Document::new_blank();
        let id = ensure_master(&mut doc, &master).unwrap();
        let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
        assert_eq!((report.written, report.refused), (0, 0), "{name}: the cached values are the formulas'");
        // Every vertex is in the box.
        let res = Resolver::new(&doc);
        let sheet = SheetRef::Shape(diacraft_doc::ShapeRef::top(Owner::Master(id), 0));
        for key in res.effective_sections(&sheet, "Geometry").unwrap() {
            for row in res.effective_rows(&sheet, &key).unwrap() {
                let at = |n: &str| effective_cell(&res, &sheet, &CellAddr::in_row("Geometry", key.ix, row.key.clone(), n));
                if let (Some(x), Some(y)) = (at("X"), at("Y")) {
                    assert!((-1e-9..=w + 1e-9).contains(&x) && (-1e-9..=h + 1e-9).contains(&y), "{name}: ({x}, {y}) is outside {w} by {h}");
                }
            }
        }
        // Dropped, resized to twice the width and half the height, it stays finite and draws.
        let (mut doc, page, shape) = dropped(&master, 5.0, 5.0);
        let at = doc.find_shape(Owner::Page(page), shape).unwrap();
        let frame = draw::shape_frame(&doc, page, shape).unwrap();
        let (nw, nh) = (frame.width * 2.0, frame.height * 0.5);
        draw::resize_shape(&mut doc, page, shape, frame.pin_x - nw / 2.0, frame.pin_y - nh / 2.0, nw, nh).unwrap();
        let sheet = SheetRef::Shape(at);
        for n in ["Width", "Height"] {
            assert!(effective(&doc, &sheet, n).is_some_and(f64::is_finite), "{name}: {n}");
        }
        let drawn = render(&doc, page);
        assert!(drawn.skipped.is_empty(), "{name}: {:?}", drawn.skipped);
    }
}
