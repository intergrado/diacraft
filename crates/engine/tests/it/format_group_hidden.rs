//! A format put on a figure that is a group reaches its members, and **a member that draws no line or no fill keeps it so** (of `docs/editing-commands.md`).
//!
//! The notation stencils build a figure as a group: the outline in a member, each marker in a member of its own, and the name in the last member, which draws no fill and no line (it is only
//! its text). A line colour put on the figure used to make that member's pattern solid (is for a shape a person names), which drew a square round an event and a gateway and a second
//! rectangle in a task; a fill colour filled the square and hid the markers under it. **The envelope of a send task is a mark that is filled with its own line colour**, which the format has to
//! leave to the line.
//!
//! Each test says what it looks at by pixels, in paper inches on a 200 pixels to the inch picture, and by the cells the members hold.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, ShapeRef, SheetRef};
use diacraft_engine::Session;
use diacraft_engine::stencil::builtin_stencil;
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value, json};

use crate::run;

const PPI: f64 = 200.0;

/// Where the three shapes of the figure are dropped (their centres, on page 1 that is 11 by 8.5 inches): a start event, a gateway and a task.
const EVENT: (f64, f64) = (2.0, 6.0);
const GATEWAY: (f64, f64) = (4.0, 6.0);
const TASK: (f64, f64) = (6.5, 6.0);

fn new_session() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({}));
    s
}

fn drop_master(s: &mut Session, stencil: &str, master: &str, (x, y): (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": master, "stencil": stencil, "page": 1, "x": x, "y": y }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

/// A start event, a gateway and a send task: their `ID`s.
fn the_three(s: &mut Session) -> [u32; 3] {
    let event = drop_master(s, "bpmn", "bpmn.start-event", EVENT);
    let gateway = drop_master(s, "bpmn", "bpmn.gateway", GATEWAY);
    let task = drop_master(s, "bpmn", "bpmn.task", TASK);
    run(s, "shapedata.set", json!({ "id": task, "row": "BpmnTaskType", "value": "Send" }));
    [event, gateway, task]
}

fn doc(s: &Session) -> &Document {
    s.document().unwrap()
}

/// The shapes with no members at or below the shape `id` of page 1, in order.
fn leaves(d: &Document, id: u32) -> Vec<ShapeRef> {
    fn walk(d: &Document, at: ShapeRef, out: &mut Vec<ShapeRef>) {
        let members = d.shape(&at).map_or(0, |shape| shape.children.len());
        if members == 0 {
            out.push(at);
            return;
        }
        for k in 0..members {
            walk(d, at.child(u32::try_from(k).unwrap()), out);
        }
    }
    let mut out = Vec::new();
    walk(d, d.find_shape(Owner::Page(1), id).unwrap(), &mut out);
    out
}

fn id_of(d: &Document, at: &ShapeRef) -> u32 {
    d.shape(at).unwrap().id
}

/// What a cell of a shape reads by inheritance: a number, or `None`.
fn read(d: &Document, at: &ShapeRef, name: &str) -> Option<f64> {
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at.clone()), &CellAddr::direct(name))?;
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
}

/// The line and fill patterns every leaf below the shapes reads, with the leaf's `ID`: `(id, LinePattern, FillPattern)`.
fn patterns(d: &Document, ids: &[u32]) -> Vec<(u32, f64, f64)> {
    ids.iter()
        .flat_map(|id| leaves(d, *id))
        .map(|at| (id_of(d, &at), read(d, &at, "LinePattern").unwrap_or(1.0), read(d, &at, "FillPattern").unwrap_or(1.0)))
        .collect()
}

/// Asserts that no leaf that drew no line (no fill) before draws one now.
#[track_caller]
fn assert_hidden_stay_hidden(before: &[(u32, f64, f64)], after: &[(u32, f64, f64)], what: &str) {
    assert_eq!(before.len(), after.len(), "{what}: the same shapes");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(b.0, a.0, "{what}: the same shape");
        if b.1 == 0.0 {
            assert_eq!(a.1, 0.0, "{what}: shape {} drew no line and now has the line pattern {}", a.0, a.1);
        }
        if b.2 == 0.0 {
            assert_eq!(a.2, 0.0, "{what}: shape {} drew no fill and now has the fill pattern {}", a.0, a.2);
        }
    }
}

fn picture(s: &Session) -> Rendered {
    let d = doc(s);
    let view = View::fit_page(d, 1, PPI).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::WHITE));
    // A hatch is drawn approximately and says so in `skipped`: what the pictures look at is where the ink is.
    renderer.render(d, 1, &view)
}

/// The pixel at (`x`, `y`) inches from the page's left and bottom edges.
fn pixel(r: &Rendered, (x, y): (f64, f64)) -> [u8; 3] {
    let col = ((x * PPI) as usize).min(r.width as usize - 1);
    let row = (((f64::from(r.height) - y * PPI).max(0.0)) as usize).min(r.height as usize - 1);
    let at = (row * r.width as usize + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2]]
}

/// Whether the picture holds ink (a pixel that is not paper) within two pixels of the place.
fn ink_near(r: &Rendered, (x, y): (f64, f64)) -> bool {
    let d = 2.0 / PPI;
    [(0.0, 0.0), (d, 0.0), (-d, 0.0), (0.0, d), (0.0, -d)].iter().any(|(dx, dy)| pixel(r, (x + dx, y + dy)).iter().any(|c| *c < 235))
}

fn is_dark(p: [u8; 3]) -> bool {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 300
}

fn is_red(p: [u8; 3]) -> bool {
    p[0] > 150 && p[1] < 80 && p[2] < 80
}

/// The places on the edges of the boxes the names of the three shapes have, which no outline of the figure passes through: the square of the event's box, the gateway's box and the
/// inner rectangle of the task's name (0.05 inch in from the body).
fn box_edges() -> [(f64, f64); 3] {
    [(EVENT.0 - 0.2, EVENT.1 + 0.15), (GATEWAY.0 - 0.3, GATEWAY.1 + 0.25), (TASK.0 - 0.7 + 0.05, TASK.1 - 0.1)]
}

/// The lower part of the envelope in the corner of a send task: 0.2 inch wide, 0.07 inch in from the corner.
const ENVELOPE: (f64, f64) = (TASK.0 - 0.53, TASK.1 + 0.224);

#[test]
fn a_line_colour_put_on_a_bpmn_figure_leaves_the_boxes_of_its_names_hidden() {
    let mut s = new_session();
    let ids = the_three(&mut s);
    let before = patterns(doc(&s), &ids);
    assert!(before.iter().any(|b| b.1 == 0.0), "the names draw no line: {before:?}");
    assert!(!box_edges().iter().any(|at| ink_near(&picture(&s), *at)), "no box before");
    run(&mut s, "shape.format", json!({ "ids": ids, "line": "#c00000" }));
    assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), "a line colour");
    let r = picture(&s);
    for at in box_edges() {
        assert!(!ink_near(&r, at), "a box is drawn at {at:?}: {:?}", pixel(&r, at));
    }
    // The outlines did change colour: the ring of the event, on its left side.
    assert!(is_red(pixel(&r, (EVENT.0 - 0.2, EVENT.1))) || ink_near(&r, (EVENT.0 - 0.2, EVENT.1)), "the ring is drawn");
}

#[test]
fn the_envelope_of_a_send_task_takes_the_line_colour_put_on_the_task() {
    let mut s = new_session();
    let [_, _, task] = the_three(&mut s);
    assert!(is_dark(pixel(&picture(&s), ENVELOPE)), "dark to begin with: {:?}", pixel(&picture(&s), ENVELOPE));
    run(&mut s, "shape.format", json!({ "ids": [task], "line": "#c00000" }));
    let p = pixel(&picture(&s), ENVELOPE);
    assert!(is_red(p), "the envelope is filled with the line colour: {p:?}");
}

#[test]
fn a_fill_put_on_a_bpmn_figure_fills_the_figure_and_no_box_and_the_dark_envelope_stays_dark() {
    let mut s = new_session();
    let ids = the_three(&mut s);
    let before = patterns(doc(&s), &ids);
    run(&mut s, "shape.format", json!({ "ids": ids, "fill": "#ffe699" }));
    assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), "a fill colour");
    let r = picture(&s);
    for at in [(EVENT.0 - 0.18, EVENT.1 + 0.18), (GATEWAY.0 - 0.28, GATEWAY.1 + 0.28)] {
        assert_eq!(pixel(&r, at), [255, 255, 255], "a filled box is drawn at {at:?}");
    }
    assert_eq!(pixel(&r, (TASK.0, TASK.1 - 0.3)), [255, 230, 153], "the task's body is filled");
    assert!(is_dark(pixel(&r, ENVELOPE)), "the envelope is a dark mark and a fill does not change that: {:?}", pixel(&r, ENVELOPE));
}

#[test]
fn a_fill_or_a_line_put_on_a_member_named_directly_replaces_what_it_held() {
    // The rule is for a member reached through a group. A shape a person names is formatted as say, and so is the dark envelope.
    let mut s = new_session();
    let [event, _, task] = the_three(&mut s);
    let name = leaves(doc(&s), event).last().map(|at| id_of(doc(&s), at)).unwrap();
    assert_eq!(read(doc(&s), &doc(&s).find_shape(Owner::Page(1), name).unwrap(), "LinePattern"), Some(0.0), "the name draws no line");
    run(&mut s, "shape.format", json!({ "ids": [name], "line": "#c00000" }));
    let at = doc(&s).find_shape(Owner::Page(1), name).unwrap();
    assert_eq!(read(doc(&s), &at, "LinePattern"), Some(1.0), "a line colour on a shape named shows its line");
    // The task's members are its body, the type marker, the dark type marker (the send envelope), the loop and compensation markers and the name.
    let envelope = id_of(doc(&s), &leaves(doc(&s), task)[2]);
    run(&mut s, "shape.format", json!({ "ids": [envelope], "fill": "#00ff00" }));
    assert!(pixel(&picture(&s), ENVELOPE)[1] > 200, "a fill put on the envelope itself is its fill: {:?}", pixel(&picture(&s), ENVELOPE));
}

#[test]
fn a_pattern_a_fill_pattern_and_a_gradient_put_on_a_figure_leave_the_hidden_members_hidden() {
    // What is looked at in the picture is a line: the edge of a box that a name draws is on no outline, and a fill covers the figure, so for a fill it is the cells that are looked at.
    for (what, command, params, lines_only) in [
        ("a dashed line", "shape.format", json!({ "pattern": 2 }), true),
        ("a hatch", "shape.format", json!({ "fill_pattern": 3 }), false),
        ("a fill gradient", "shape.gradient", json!({ "kind": "linear" }), false),
        ("a line gradient", "shape.gradient", json!({ "target": "line", "kind": "linear" }), true),
        ("a line colour from the theme", "shape.format", json!({ "line": "theme:accent2" }), true),
        ("a weight", "shape.format", json!({ "weight": 3 }), true),
        ("use default for the line", "shape.format", json!({ "line": "default" }), true),
    ] {
        let mut s = new_session();
        let ids = the_three(&mut s);
        let before = patterns(doc(&s), &ids);
        let mut params = params;
        params["ids"] = json!(ids);
        run(&mut s, command, params);
        assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), what);
        if lines_only {
            let r = picture(&s);
            for at in box_edges() {
                assert!(!ink_near(&r, at), "{what}: a box is drawn at {at:?}");
            }
        }
    }
}

#[test]
fn a_quick_style_put_on_a_figure_leaves_the_hidden_members_hidden() {
    let mut s = new_session();
    let ids = the_three(&mut s);
    run(&mut s, "theme.apply", json!({ "theme": "tidepool", "page": 1 }));
    let before = patterns(doc(&s), &ids);
    run(&mut s, "shape.quick_style", json!({ "ids": ids, "row": 3, "column": 2 }));
    assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), "a quick style");
}

#[test]
fn a_theme_leaves_the_hidden_members_hidden() {
    let mut s = new_session();
    let ids = the_three(&mut s);
    let before = patterns(doc(&s), &ids);
    for theme in ["tidepool", "fernwood"] {
        run(&mut s, "theme.apply", json!({ "theme": theme, "page": 1 }));
        assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), theme);
    }
}

/// Puts `params` of `shape.format` on the three figures of a page that has a theme, and checks that no member that drew no line or no fill draws one.
#[track_caller]
fn themed_page_keeps_hidden(what: &str, mut params: Value) {
    let mut s = new_session();
    let ids = the_three(&mut s);
    run(&mut s, "theme.apply", json!({ "theme": "tidepool", "page": 1 }));
    let before = patterns(doc(&s), &ids);
    params["ids"] = json!(ids);
    run(&mut s, "shape.format", params);
    assert_hidden_stay_hidden(&before, &patterns(doc(&s), &ids), what);
}

#[test]
fn a_line_colour_of_the_theme_put_on_a_figure_of_a_themed_page_leaves_the_hidden_members_hidden() {
    themed_page_keeps_hidden("a theme colour for the line", json!({ "line": "theme:accent2" }));
}

#[test]
fn a_fill_colour_of_the_theme_put_on_a_figure_of_a_themed_page_leaves_the_hidden_members_hidden() {
    themed_page_keeps_hidden("a theme colour for the fill", json!({ "fill": "theme:accent2:40" }));
}

#[test]
fn use_default_put_on_a_figure_of_a_themed_page_leaves_the_hidden_members_hidden() {
    themed_page_keeps_hidden("use default for the line and the fill", json!({ "line": "default", "fill": "default" }));
}

#[test]
fn a_group_a_person_made_keeps_its_member_with_no_line_hidden_too() {
    let mut s = new_session();
    let body = id_of_drop(&mut s, "rectangle", (3.0, 4.0));
    let text = id_of_drop(&mut s, "rectangle", (3.0, 4.0));
    run(&mut s, "shape.format", json!({ "ids": [text], "line": "none", "fill": "none" }));
    let group = u32::try_from(run(&mut s, "shape.group", json!({ "ids": [body, text] }))["id"].as_u64().unwrap()).unwrap();
    run(&mut s, "shape.format", json!({ "ids": [group], "line": "#c00000", "fill": "#ffe699" }));
    let d = doc(&s);
    let (b, t) = (d.find_shape(Owner::Page(1), body).unwrap(), d.find_shape(Owner::Page(1), text).unwrap());
    assert_eq!(read(d, &b, "LinePattern"), Some(1.0));
    assert_eq!(read(d, &t, "LinePattern"), Some(0.0), "the member with no line keeps none");
    assert_eq!(read(d, &t, "FillPattern"), Some(0.0), "the member with no fill keeps none");
    // Its colours are written all the same: the figure is one colour whichever member later shows a line.
    let colour = Resolver::new(d)
        .resolve(&SheetRef::Shape(t), &CellAddr::direct("LineColor"))
        .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref().map(|v| v.text().to_owned())));
    assert_eq!(colour.as_deref(), Some("#c00000"));
}

fn id_of_drop(s: &mut Session, master: &str, at: (f64, f64)) -> u32 {
    let dropped = run(s, "shape.drop", json!({ "master": master, "page": 1, "x": at.0, "y": at.1 }));
    u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
}

/// Every stencil that is made of group masters.
const STENCILS: [&str; 13] = [
    "bpmn",
    "c4",
    "chen",
    "cloud",
    "computers",
    "crows-foot",
    "network",
    "uml-activity",
    "uml-class",
    "uml-component",
    "uml-sequence",
    "uml-state",
    "uml-usecase",
];

#[test]
fn no_group_master_of_any_notation_stencil_draws_a_line_or_a_fill_it_did_not_after_a_format() {
    let commands: [(&str, Value); 6] = [
        ("shape.format", json!({ "line": "#c00000" })),
        ("shape.format", json!({ "fill": "#ffe699" })),
        ("shape.format", json!({ "pattern": 2, "fill_pattern": 3 })),
        ("shape.gradient", json!({ "kind": "radial" })),
        ("shape.gradient", json!({ "target": "line", "kind": "linear" })),
        ("shape.format", json!({ "line": "theme:accent3", "fill": "theme:accent1" })),
    ];
    let (mut groups, mut refused) = (0, Vec::new());
    for stencil in STENCILS {
        let masters = builtin_stencil(stencil).map_or(&[][..], |st| st.masters());
        for master in masters.iter().filter(|m| !m.is_connector() && !m.makes_structure()) {
            for (command, params) in &commands {
                let mut s = new_session();
                let id = drop_master(&mut s, stencil, master.id, (5.5, 4.25));
                if leaves(doc(&s), id).len() == 1 && leaves(doc(&s), id).first().map(|l| id_of(doc(&s), l)) == Some(id) {
                    break;
                }
                let before = patterns(doc(&s), &[id]);
                let mut params = params.clone();
                params["ids"] = json!([id]);
                // A command a master's cells refuse in words (a guarded pattern) changes nothing, and nothing then is a hidden member made to draw.
                if let Err(e) = s.execute(command, &params) {
                    refused.push(format!("{} {command} {params}: {e}", master.id));
                }
                assert_hidden_stay_hidden(&before, &patterns(doc(&s), &[id]), &format!("{} {command} {params}", master.id));
                groups += 1;
            }
        }
    }
    assert!(groups >= 120, "the sweep went over the group masters, six commands each: {groups}");
    assert!(refused.is_empty(), "and every command was done and none refused: {refused:#?}");
}
