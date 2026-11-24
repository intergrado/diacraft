//! The layer gaps left (Lock "stops shapes on the layer from
//! being selected, moved or edited").
//!
//! 1. A replace across a page or the drawing passes over the shapes on locked layers and counts them; Find still finds them.
//! 2. A connector on a locked layer keeps its route when a shape it is glued to moves: its glued ends follow, as they follow a fixed route's.
//! 3. A group is not formatted, retexted or resized through a member on a locked layer: refused in words.
//! 4. An `.odg` saved in place writes a file layer's new `draw:display` and `draw:protected`.
//! 5. A shape of the file put on a layer the save declares gets its `draw:layer` written.

use diacraft_doc::{Owner, SectionKey, TextPart};
use diacraft_engine::{EngineError, Session, command_specs, param_keys};
use serde_json::{Value as Json, json};

use crate::edit::drop_shape;
use crate::{blank_session, check_session, run, snapshot};

/// The characters of the text of shape `id` of page `page`.
fn text_of(s: &Session, page: u32, id: u32) -> String {
    let doc = s.document().unwrap();
    let at = doc.find_shape(Owner::Page(page), id).unwrap_or_else(|| panic!("no shape {id} on page {page}"));
    let Some(text) = doc.shape(&at).unwrap().text.as_ref() else { return String::new() };
    text.parts
        .iter()
        .filter_map(|p| match p {
            TextPart::Chars(c) => Some(c.to_string()),
            _ => None,
        })
        .collect()
}

/// The shape `id` of page `page` as it is (a copy), or `None` when the page holds none.
fn shape_on(s: &Session, page: u32, id: u32) -> Option<diacraft_doc::Shape> {
    let doc = s.document()?;
    let at = doc.find_shape(Owner::Page(page), id)?;
    doc.shape(&at).map(|v| (**v).clone())
}

fn is_lock_refusal(e: &EngineError, id: u32) -> bool {
    matches!(e, EngineError::BadParams { msg, .. } if msg.contains(&format!("shape {id} ")) && msg.contains("\"Frozen\"") && msg.contains("locked"))
}

// ---------- 1. Replace passes over the shapes on locked layers ----------

/// The shapes of the replace tests, each with the text `Alpha`: on the first page `open` (on `Open`), `frozen` (on `Frozen`, locked), `both` (on both), `plain` (on no layer),
/// and a group on no layer whose members are `member_open` (on `Open`) and `member_frozen` (on `Frozen`); on a second page `ice` (on its own locked layer `Frozen`) and `free`.
struct Replaces {
    open: u32,
    frozen: u32,
    both: u32,
    plain: u32,
    group: u32,
    member_open: u32,
    member_frozen: u32,
    second: u32,
    ice: u32,
    free: u32,
}

fn replace_drawing() -> (Session, Replaces) {
    let mut s = blank_session();
    let ids = [(1.0, 2.0), (3.0, 2.0), (5.0, 2.0), (7.0, 2.0), (1.0, 6.0), (3.0, 6.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    for id in ids {
        run(&mut s, "shape.set_text", json!({"id": id, "text": "Alpha"}));
    }
    let [open, frozen, both, plain, member_open, member_frozen] = ids;
    run(&mut s, "layer.add", json!({"name": "Open"}));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    for (id, layers) in [
        (open, json!(["Open"])),
        (frozen, json!(["Frozen"])),
        (both, json!([0, 1])),
        (member_open, json!(["Open"])),
        (member_frozen, json!(["Frozen"])),
    ] {
        run(&mut s, "layer.assign", json!({"ids": [id], "layers": layers}));
    }
    let group = run(&mut s, "shape.group", json!({"ids": [member_open, member_frozen]}))["id"].as_u64().unwrap() as u32;
    let second = run(&mut s, "page.add", json!({"name": "Second"}))["id"].as_u64().unwrap() as u32;
    let mut on_second = |x: f64| crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": second, "x": x, "y": 2.0})));
    let (ice, free) = (on_second(2.0), on_second(5.0));
    for id in [ice, free] {
        run(&mut s, "shape.set_text", json!({"id": id, "page": second, "text": "Alpha"}));
    }
    run(&mut s, "layer.add", json!({"name": "Frozen", "page": second}));
    run(&mut s, "layer.assign", json!({"ids": [ice], "layers": ["Frozen"], "page": second}));
    // A locked layer takes no new shapes, so the layers are locked last.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true, "page": second}));
    (s, Replaces { open, frozen, both, plain, group, member_open, member_frozen, second, ice, free })
}

/// The words a replace gives a match it left on a locked layer.
const LOCKED_WHY: &str = "on a locked layer";

/// A Replace All across a page replaces the text of the shapes on no locked layer and passes over the rest, a member of a group by its own list, and says how many
/// matches it left on locked layers, among the matches it left and with their reason; the texts on locked layers are as they were. Across the drawing, the second page's are
/// passed over too. Find still finds every one of them. One match on a locked shape, replaced on its own, is refused in words.
#[test]
fn a_replace_across_a_page_or_the_drawing_passes_over_the_shapes_on_locked_layers_and_counts_them() {
    let (mut s, f) = replace_drawing();
    let found = run(&mut s, "edit.find", json!({"what": "Alpha", "scope": "all", "limit": 100}));
    assert_eq!(found["matches"].as_array().unwrap().len(), 8, "Find finds the shapes on locked layers: {found}");
    let r = run(&mut s, "edit.replace", json!({"what": "Alpha", "with": "Omega", "scope": "page"}));
    assert_eq!((r["replaced"].clone(), r["locked"].clone(), r["skipped_total"].clone()), (json!(3), json!(3), json!(3)), "{r}");
    let left: Vec<u64> = r["skipped"].as_array().unwrap().iter().map(|m| m["id"].as_u64().unwrap()).collect();
    assert_eq!(left, [f.frozen, f.both, f.member_frozen].map(u64::from), "{r}");
    assert!(r["skipped"].as_array().unwrap().iter().all(|m| m["why"].as_str().unwrap().contains(LOCKED_WHY)), "{r}");
    for id in [f.open, f.plain, f.member_open] {
        assert_eq!(text_of(&s, 1, id), "Omega", "shape {id}");
    }
    for id in [f.frozen, f.both, f.member_frozen] {
        assert_eq!(text_of(&s, 1, id), "Alpha", "shape {id} is on a locked layer");
    }
    let found = run(&mut s, "edit.find", json!({"what": "Alpha", "scope": "page"}));
    assert_eq!(found["matches"].as_array().unwrap().len(), 3, "{found}");
    // Across the drawing: the second page's locked shape is passed over as well, the other replaced.
    let r = run(&mut s, "edit.replace", json!({"what": "Alpha", "with": "Omega", "scope": "all"}));
    assert_eq!((r["replaced"].clone(), r["locked"].clone()), (json!(1), json!(4)), "{r}");
    assert_eq!((text_of(&s, f.second, f.ice), text_of(&s, f.second, f.free)), ("Alpha".to_owned(), "Omega".to_owned()));
    // One match, named by its place: refused in words, nothing changed.
    let before = snapshot(&s);
    let at = json!({"page": 1, "id": f.frozen, "place": "text", "start": 0, "end": 5});
    let e = s.execute("edit.replace", &json!({"what": "Alpha", "with": "Omega", "at": at})).unwrap_err();
    assert!(e.to_string().contains(LOCKED_WHY), "{e}");
    assert_eq!(snapshot(&s), before);
    // Unlocked, they are replaced.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    let r = run(&mut s, "edit.replace", json!({"what": "Alpha", "with": "Omega", "scope": "page"}));
    assert_eq!((r["replaced"].clone(), r["locked"].clone()), (json!(3), json!(0)), "{r}");
    check_session(&s).unwrap();
}

/// A replace in a selection that holds a group passes over the group's member on a locked layer and replaces the other member's text.
#[test]
fn a_replace_in_a_selected_group_passes_over_its_member_on_a_locked_layer() {
    let (mut s, f) = replace_drawing();
    let r = run(&mut s, "edit.replace", json!({"what": "Alpha", "with": "Omega", "scope": "selection", "ids": [f.group]}));
    assert_eq!((r["replaced"].clone(), r["locked"].clone()), (json!(1), json!(1)), "{r}");
    assert_eq!((text_of(&s, 1, f.member_open), text_of(&s, 1, f.member_frozen)), ("Omega".to_owned(), "Alpha".to_owned()));
}

// ---------- 2. A connector on a locked layer keeps its route ----------

/// The rows of the route of connector `id` of page 1: its `Geometry` section 0.
fn route_of(s: &Session, id: u32) -> Option<std::sync::Arc<diacraft_doc::Section>> {
    shape_on(s, 1, id)?.sheet.section(&SectionKey::new("Geometry", Some(0))).cloned()
}

fn number_of(s: &Session, id: u32, cell: &str) -> f64 {
    shape_on(s, 1, id).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
}

/// Two rectangles and a connector DiaCraft routed between them, put on the layer `Frozen`, locked when `lock`. → the session, the two rectangles and the connector.
fn wired(lock: bool) -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 7.0, 6.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}})));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [c], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": lock}));
    (s, [a, b, c])
}

/// A connector on a locked layer is not routed again when a shape it is glued to moves: its route keeps its rows (a locked shape's geometry is not edited), its glued end
/// follows the shape, as an end of a fixed route does, and its glue stays. On a layer that is not locked, the same move routes it again.
#[test]
fn a_connector_on_a_locked_layer_keeps_its_route_when_a_shape_it_is_glued_to_moves_and_its_ends_follow() {
    let (mut s, [a, b, c]) = wired(true);
    let (route, begin_y) = (route_of(&s, c).unwrap(), number_of(&s, c, "BeginY"));
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": -2.0}));
    assert_eq!(route_of(&s, c).unwrap(), route, "the route of a connector on a locked layer is not made again");
    assert!((number_of(&s, c, "BeginY") - (begin_y - 2.0)).abs() < 1.0e-6, "the glued end follows the shape");
    let page = s.document().unwrap().page(1).unwrap();
    let glued: Vec<u32> = page.connects.iter().filter(|g| g.from_sheet == c).map(|g| g.to_sheet).collect();
    assert!(glued.contains(&a) && glued.contains(&b), "the glue stays: {glued:?}");
    check_session(&s).unwrap();
    // The control: unlocked, the move routes it again.
    let (mut s, [a, _, c]) = wired(false);
    let route = route_of(&s, c).unwrap();
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": -2.0}));
    assert_ne!(route_of(&s, c).unwrap(), route, "a connector on a layer that is not locked is routed again");
}

/// The kinds of the rows of the route of connector `id` of page 1.
fn kinds(s: &Session, id: u32) -> Vec<String> {
    route_of(s, id).unwrap().rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()).collect()
}

/// A connector on a locked layer is given no line jump when another is routed across it: a jump is rows of the connector that carries it, which a locked shape's
/// route does not take. Unlocked, the level connector carries the jump (the root's code 1: the flatter leg).
#[test]
fn a_connector_on_a_locked_layer_is_given_no_line_jump() {
    for lock in [true, false] {
        let mut s = blank_session();
        let level = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"x": 2.0, "y": 4.0}, "to": {"x": 8.0, "y": 4.0}})));
        run(&mut s, "layer.add", json!({"name": "Frozen"}));
        run(&mut s, "layer.assign", json!({"ids": [level], "layers": ["Frozen"]}));
        run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": lock}));
        let before = route_of(&s, level).unwrap();
        run(&mut s, "connector.add", json!({"from": {"x": 5.0, "y": 1.0}, "to": {"x": 5.0, "y": 7.0}}));
        if lock {
            assert_eq!(route_of(&s, level).unwrap(), before, "no jump on the locked connector: {:?}", kinds(&s, level));
        } else {
            assert_eq!(kinds(&s, level), ["MoveTo", "LineTo", "ArcTo", "LineTo"], "the control: the level connector jumps");
        }
    }
}

// ---------- 3. A group is not edited through a member on a locked layer ----------

/// A group on no layer of a rectangle `a` (on no layer) and a rectangle `b` on `Frozen` (locked), with the text `Beta` each, and a rectangle `c` beside it.
struct Group {
    a: u32,
    b: u32,
    c: u32,
    g: u32,
}

fn group_page() -> (Session, Group) {
    let mut s = blank_session();
    let [a, b, c] = [(2.0, 3.0), (4.0, 3.0), (8.0, 3.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Alpha Beta"}));
    run(&mut s, "shape.set_text", json!({"id": b, "text": "Beta"}));
    run(&mut s, "layer.add", json!({"name": "Open"}));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [b], "layers": ["Frozen"]}));
    let g = run(&mut s, "shape.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, Group { a, b, c, g })
}

/// The format, text and geometry commands that reach a group's members refuse a group whose member is on a locked layer, in words that name the member, the group and the
/// layer, and change nothing; the commands that edit only the group's own cells (a move, a turn, a flip, its own text) still run, and the member is as it was. Unlocked, the
/// group is formatted.
#[test]
fn a_group_is_not_formatted_retexted_resized_or_given_layers_through_a_member_on_a_locked_layer() {
    let (mut s, f) = group_page();
    for (id, params) in [
        ("shape.format", json!({"ids": [f.g], "fill": "#ff0000"})),
        ("text.format", json!({"ids": [f.g], "bold": true})),
        ("shape.resize", json!({"id": f.g, "x": 1.0, "y": 1.0, "width": 3.0, "height": 2.0})),
        ("shape.resize", json!({"ids": [f.g], "x": 1.0, "y": 1.0, "width": 5.0, "height": 3.0})),
        ("shape.resize", json!({"ids": [f.c, f.g], "x": 1.0, "y": 1.0, "width": 9.0, "height": 3.0})),
        ("layer.assign", json!({"ids": [f.g], "layers": ["Open"], "preserve_members": false})),
    ] {
        let before = snapshot(&s);
        let e = s.execute(id, &params).expect_err(&format!("{id} {params}: a group was edited through its locked member"));
        assert!(is_lock_refusal(&e, f.b), "{id} {params}: {e}");
        assert!(e.to_string().contains(&format!("group {}", f.g)), "{id} {params}: the words name the group: {e}");
        assert_eq!(snapshot(&s), before, "{id} {params}: nothing changed");
    }
    let member = shape_on(&s, 1, f.b);
    for (id, params) in [
        ("shape.move", json!({"ids": [f.g], "dx": 1.0, "dy": 0.5})),
        ("shape.rotate", json!({"ids": [f.g], "by": 30.0})),
        ("shape.flip", json!({"ids": [f.g], "axis": "horizontal"})),
        ("shape.set_text", json!({"id": f.g, "text": "Gamma"})),
        ("layer.assign", json!({"ids": [f.g], "layers": ["Open"]})),
    ] {
        run(&mut s, id, params);
        assert_eq!(shape_on(&s, 1, f.b), member, "{id}: the member on the locked layer is as it was");
    }
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    run(&mut s, "shape.format", json!({"ids": [f.g], "fill": "#ff0000"}));
    assert_ne!(shape_on(&s, 1, f.b), member, "unlocked, the member is formatted with the group");
    let _ = f.a;
    check_session(&s).unwrap();
}

/// **The registry's walk.** No command that changes the drawing, given a group whose member is on a locked layer, changes that member: it is refused for the lock, or the member
/// is as it was. A delete, a cut and an ungroup of the group are refused too (before it they acted on the group as one). Each command is given the
/// params of the table, or the group's `ids` (or `id`) alone, and a command whose params the table does not give is walked too, with what it refuses for its own reasons passed
/// over.
#[test]
fn no_command_changes_a_groups_member_on_a_locked_layer() {
    let (_, f) = group_page();
    let (g, c) = (f.g, f.c);
    let table: Vec<(&str, Json)> = vec![
        ("shape.move", json!({"ids": [g], "dx": 1.0, "dy": 0.5})),
        ("shape.resize", json!({"id": g, "x": 1.0, "y": 1.0, "width": 3.0, "height": 2.0})),
        ("shape.resize", json!({"ids": [g], "x": 1.0, "y": 1.0, "width": 5.0, "height": 3.0})),
        ("shape.size", json!({"ids": [g], "width": 3.0})),
        ("shape.format", json!({"ids": [g], "fill": "#ff0000"})),
        ("text.format", json!({"ids": [g], "bold": true})),
        ("shape.rotate", json!({"ids": [g], "by": 30.0})),
        ("shape.flip", json!({"ids": [g], "axis": "horizontal"})),
        ("shape.order", json!({"ids": [g], "to": "back"})),
        ("shape.align", json!({"ids": [c, g], "how": "top"})),
        ("shape.place", json!({"ids": [g], "x": 5.0})),
        ("shape.pin", json!({"ids": [g], "at": "bottom-left"})),
        ("shape.set_text", json!({"id": g, "text": "Gamma"})),
        ("text.insert", json!({"id": g, "text": "!"})),
        ("edit.replace", json!({"scope": "selection", "ids": [g], "what": "Beta", "with": "Zeta"})),
        ("layer.assign", json!({"ids": [g], "layers": ["Open"], "preserve_members": false})),
        ("layer.assign", json!({"ids": [g], "layers": ["Open"]})),
        ("shape.duplicate", json!({"ids": [g]})),
        ("shape.group", json!({"ids": [g, c]})),
        ("shape.change", json!({"ids": [g], "master": "ellipse"})),
        ("hyperlink.set", json!({"id": g, "links": [{"address": "https://example.org"}]})),
        ("text.insert_field", json!({"id": g, "category": "geometry", "field": "width"})),
        ("text.update_fields", json!({"id": g})),
        ("callout.insert", json!({"ids": [g]})),
        ("text.block_move", json!({"id": g, "dx": 0.5, "dy": 0.0})),
        ("text.block_resize", json!({"id": g, "x": 1.0, "y": 1.0, "width": 2.0, "height": 1.0})),
        ("text.block_rotate", json!({"id": g, "by": 30.0})),
    ];
    let mut walked = Vec::new();
    let mut tries: Vec<(&str, Json)> = table.clone();
    for spec in command_specs() {
        let keys = param_keys(spec.params).unwrap();
        if !spec.journal || table.iter().any(|(id, _)| *id == spec.id) {
            continue;
        }
        if keys.contains(&"ids") {
            tries.push((spec.id, json!({"ids": [g]})));
        } else if keys.contains(&"id") && params_id_is_a_shapes(spec.params) {
            tries.push((spec.id, json!({"id": g})));
        }
    }
    for (id, params) in tries {
        let (mut s, f) = group_page();
        let before = shape_on(&s, 1, f.b);
        match s.execute(id, &params) {
            Ok(_) => {
                assert_eq!(shape_on(&s, 1, f.b), before, "{id} {params}: the member on the locked layer changed");
                walked.push(id);
            }
            Err(e) => {
                assert_eq!(shape_on(&s, 1, f.b), before, "{id} {params}");
                if is_lock_refusal(&e, f.b) {
                    walked.push(id);
                }
            }
        }
    }
    for id in [
        "shape.format",
        "text.format",
        "shape.resize",
        "layer.assign",
        "edit.replace",
        "shape.move",
        "shape.set_text",
        "shape.delete",
        "edit.cut",
        "shape.ungroup",
    ] {
        assert!(walked.contains(&id), "{id} was not walked: {walked:?}");
    }
}

/// Whether a command's `id` names a shape: every `id` does but a page's and a stencil's, whose docs say "the id of".
fn params_id_is_a_shapes(params: &str) -> bool {
    params.split(';').find_map(|e| e.trim().strip_prefix("id:")).is_some_and(|doc| !doc.contains("the id of"))
}

// ---------- 4 and 5. An `.odg` saved in place: a file layer's new flags, and a shape of the file put on a layer the save declares ----------

/// The `draw:layer` start tags of the layer set of `office:master-styles`, each as its markup is written.
fn layer_set(bytes: &[u8]) -> Vec<String> {
    let styles = crate::odg_build::part_of(bytes, "styles.xml");
    let Some(start) = styles.find("<draw:layer-set") else { return Vec::new() };
    let end = styles[start..].find("</draw:layer-set>").map_or(styles.len(), |e| start + e);
    styles[start..end].split("<draw:layer ").skip(1).map(|piece| format!("<draw:layer {}", piece.split("/>").next().unwrap_or(""))).collect()
}

/// The start tag of the `draw:layer` named `name` in the layer set.
fn layer_tag(bytes: &[u8], name: &str) -> String {
    layer_set(bytes)
        .into_iter()
        .find(|l| l.contains(&format!("draw:name=\"{name}\"")))
        .unwrap_or_else(|| panic!("no layer {name}: {:?}", layer_set(bytes)))
}

/// The start tag of the shape element of `content.xml` whose `draw:name` is `name`.
fn shape_tag(bytes: &[u8], name: &str) -> String {
    let content = crate::odg_build::content_of(bytes);
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name} in {content}"));
    let start = content[..at].rfind('<').unwrap();
    let end = content[at..].find('>').map_or(content.len(), |e| at + e);
    content[start..end].to_owned()
}

/// The package without DiaCraft's document file and its folder: what a program that drops DiaCraft's data reads.
fn without_diacraft(bytes: &[u8]) -> Vec<u8> {
    let parts = ["content.xml", "styles.xml", "meta.xml"].map(|name| (name.to_owned(), crate::odg_build::part_of(bytes, name).into_bytes()));
    crate::odg_build::package(crate::odg_build::DRAWING, parts.to_vec())
}

/// The `Name`, `Visible`, `Print` and `Lock` of each `Layer` row of the page at `page` of `doc`, the flags as numbers.
fn rows_of(doc: &diacraft_doc::Document, page: usize) -> Vec<(String, f64, f64, f64)> {
    let page = doc.pages.get(page).unwrap();
    let Some(section) = page.sheet.section(&SectionKey::new("Layer", None)) else { return Vec::new() };
    let number =
        |row: &diacraft_doc::Row, cell: &str| row.cells.get(cell).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(f64::NAN);
    let name = |row: &diacraft_doc::Row| row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default();
    section.rows.iter().map(|r| (name(r), number(r, "Visible"), number(r, "Print"), number(r, "Lock"))).collect()
}

/// A drawing of `pages` pages, each with a rectangle (`A`, `B`, ...), whose layer set declares after LibreOffice's five `Notes` (shown on paper only), `Pins` (written
/// unprotected) and `Plain` (no flag written).
fn layered_drawing(pages: usize) -> Vec<u8> {
    let mut drawing = crate::odg_build::Drawing::new();
    for (k, name) in ["A", "B", "C"].iter().take(pages).enumerate() {
        drawing = drawing.page(&crate::odg_build::rect(name, (2.0 + k as f64, 2.0, 4.0, 2.0), ""));
    }
    let bytes = drawing.bytes();
    let styles = crate::odg_build::part_of(&bytes, "styles.xml").replace(
        "</draw:layer-set>",
        "<draw:layer draw:name=\"Notes\" draw:display=\"printer\"/><draw:layer draw:name=\"Pins\" draw:protected=\"false\"/><draw:layer draw:name=\"Plain\"/></draw:layer-set>",
    );
    crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![
            ("content.xml".to_owned(), crate::odg_build::part_of(&bytes, "content.xml").into_bytes()),
            ("styles.xml".to_owned(), styles.into_bytes()),
            ("meta.xml".to_owned(), crate::odg_build::part_of(&bytes, "meta.xml").into_bytes()),
        ],
    )
}

/// A layer of the file whose `Visible`, `Print` or `Lock` changed has its `draw:display` and `draw:protected` written from the cells, in their
/// place when the layer has them and added when it has not; another program reads the new flags from the markup alone; DiaCraft reads the drawing back as it was saved;
/// and saved again unedited, the file is as it was.
#[test]
fn an_odg_saved_in_place_writes_a_file_layers_new_display_and_protection() {
    let dir = crate::odg_build::Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &layered_drawing(1));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    assert_eq!(rows_of(s.document().unwrap(), 0), [("Notes".into(), 0.0, 1.0, 0.0), ("Pins".into(), 1.0, 1.0, 0.0), ("Plain".into(), 1.0, 1.0, 0.0)]);
    run(&mut s, "layer.set", json!({"layer": "Notes", "visible": true, "page": page}));
    run(&mut s, "layer.set", json!({"layer": "Pins", "lock": true, "page": page}));
    run(&mut s, "layer.set", json!({"layer": "Plain", "print": false, "visible": false, "page": page}));
    let want = run(&mut s, "layer.list", json!({"page": page}))["layers"].clone();
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    assert!(layer_tag(&bytes, "Notes").contains("draw:display=\"always\""), "{}", layer_tag(&bytes, "Notes"));
    assert!(
        layer_tag(&bytes, "Pins").contains("draw:protected=\"true\"") && !layer_tag(&bytes, "Pins").contains("display"),
        "{}",
        layer_tag(&bytes, "Pins")
    );
    assert!(
        layer_tag(&bytes, "Plain").contains("draw:display=\"none\"") && !layer_tag(&bytes, "Plain").contains("protected"),
        "{}",
        layer_tag(&bytes, "Plain")
    );
    assert_eq!(layer_set(&bytes).len(), 8, "no layer is added or lost: {:?}", layer_set(&bytes));
    // Another program's reading: the markup alone gives the new flags.
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&bytes));
    assert_eq!(rows_of(&bare, 0), [("Notes".into(), 1.0, 1.0, 0.0), ("Pins".into(), 1.0, 1.0, 1.0), ("Plain".into(), 0.0, 0.0, 0.0)]);
    // DiaCraft's reading: the model.
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("a.odg")}));
    assert_eq!(run(&mut back, "layer.list", json!({"page": page}))["layers"], want);
    run(&mut back, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path("a.odg")).unwrap(), bytes, "a second save writes nothing new");
    // Back to the flags the file had: written again, in place.
    run(&mut back, "layer.set", json!({"layer": "Notes", "visible": false, "page": page}));
    run(&mut back, "layer.set", json!({"layer": "Pins", "lock": false, "page": page}));
    run(&mut back, "file.save", json!({}));
    let again = std::fs::read(dir.path("a.odg")).unwrap();
    assert!(layer_tag(&again, "Notes").contains("draw:display=\"printer\""), "{}", layer_tag(&again, "Notes"));
    assert!(layer_tag(&again, "Pins").contains("draw:protected=\"false\""), "{}", layer_tag(&again, "Pins"));
}

/// The document's layer set is every page's: a layer's flags are decided by the row that changed (before it, by the first page that has the
/// layer), and another page whose rows then differ from the set keeps its own in DiaCraft's document file.
#[test]
fn a_file_layers_flags_are_the_changed_rows_and_another_page_keeps_its_own() {
    let dir = crate::odg_build::Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &layered_drawing(2));
    let pages: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    run(&mut s, "layer.set", json!({"layer": "Pins", "lock": true, "page": pages[0]}));
    run(&mut s, "layer.set", json!({"layer": "Notes", "visible": true, "page": pages[1]}));
    let want: Vec<Json> = pages.iter().map(|p| run(&mut s, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    assert!(layer_tag(&bytes, "Pins").contains("draw:protected=\"true\""), "the first page's change: {}", layer_tag(&bytes, "Pins"));
    assert!(
        layer_tag(&bytes, "Notes").contains("draw:display=\"always\""),
        "the second page's change, though the first has Notes as the file had it: {}",
        layer_tag(&bytes, "Notes")
    );
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("a.odg")}));
    let got: Vec<Json> = pages.iter().map(|p| run(&mut back, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    assert_eq!(got, want, "each page has the layers it had");
    run(&mut back, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path("a.odg")).unwrap(), bytes, "a second save writes nothing new");
}

/// A shape of the file that is put on a layer the save declares gets its `draw:layer` written, so that another program shows it there; a shape of the file
/// put on a layer the file already declares keeps the `draw:layer` it had, its list riding in DiaCraft's data. Opened again, both are on their layers.
#[test]
fn a_shape_of_the_file_put_on_a_layer_the_save_declares_is_written_on_it() {
    let dir = crate::odg_build::Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &layered_drawing(1));
    let doc = s.document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let a = crate::odg_build::id_of(doc, 0, "A");
    run(&mut s, "layer.add", json!({"name": "Wiring", "page": page}));
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Wiring"], "page": page}));
    let b = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 5.0, "y": 5.0})));
    run(&mut s, "layer.assign", json!({"ids": [b], "layers": ["Notes"], "page": page}));
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    assert!(shape_tag(&bytes, "A").contains("draw:layer=\"Wiring\""), "{}", shape_tag(&bytes, "A"));
    assert!(layer_set(&bytes).iter().any(|l| l.contains("draw:name=\"Wiring\"")), "{:?}", layer_set(&bytes));
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&bytes));
    assert_eq!(rows_of(&bare, 0).last().map(|r| r.0.clone()).as_deref(), Some("Wiring"));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("a.odg")}));
    let member =
        |s: &Session, id: u32| shape_on(s, page, id).unwrap().sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    assert_eq!(member(&back, a), member(&s, a));
    assert_eq!(member(&back, a).as_deref(), Some("3"), "on Wiring, the row after the file's three");
    run(&mut back, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path("a.odg")).unwrap(), bytes, "a second save writes nothing new");

    // Still holds for a layer the file declares: the shape keeps the layer it named.
    let mut s = crate::odg_build::open_bytes_in(&dir, "b.odg", &layered_drawing(1));
    let a = crate::odg_build::id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Notes"], "page": page}));
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("b.odg")).unwrap();
    assert!(shape_tag(&bytes, "A").contains("draw:layer=\"layout\""), "{}", shape_tag(&bytes, "A"));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("b.odg")}));
    assert_eq!(member(&back, a).as_deref(), Some("0"), "on Notes, by DiaCraft's data");
}

// ---------- Containers and callouts: what a shape takes along ----------

fn pin_of(s: &Session, id: u32) -> (f64, f64) {
    (number_of(s, id, "PinX"), number_of(s, id, "PinY"))
}

/// A container round a rectangle `a` (on no layer) and a rectangle `b` (on `Frozen`, locked once the container holds it). → the session, `a`, `b` and the container.
fn locked_member_of_a_container() -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [b], "layers": ["Frozen"]}));
    let c = crate::edit::id_of(&run(&mut s, "container.add", json!({"ids": [a, b]})));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, [a, b, c])
}

/// A member on a locked layer does not move with its container, and is not deleted or cut with it: the move, the place, the delete and the cut of the container are refused in
/// words that name the member and the layer, and nothing changes. Unlocked, the container moves and takes it along.
#[test]
fn a_member_on_a_locked_layer_does_not_move_or_go_with_its_container() {
    let (mut s, [a, b, c]) = locked_member_of_a_container();
    for (id, params) in [
        ("shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.0})),
        ("shape.place", json!({"ids": [c], "x": 1.0})),
        ("shape.align", json!({"ids": [a, c], "how": "top"})),
        ("shape.delete", json!({"ids": [c]})),
        ("edit.cut", json!({"ids": [c]})),
    ] {
        let before = snapshot(&s);
        let e = s.execute(id, &params).expect_err(&format!("{id} {params}: a member on a locked layer went with its container"));
        assert!(is_lock_refusal(&e, b), "{id} {params}: {e}");
        assert_eq!(snapshot(&s), before, "{id} {params}: nothing changed");
    }
    // The member of the open layer is moved on its own (it leaves the container's box), as before.
    let before = pin_of(&s, b);
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.0}));
    assert!((pin_of(&s, b).0 - before.0 - 1.0).abs() < 1.0e-6, "unlocked, the member goes with its container");
    check_session(&s).unwrap();
}

/// A connector on a locked layer glued to a member of a container keeps its route when the container moves the member: its glued end follows.
#[test]
fn a_connector_on_a_locked_layer_keeps_its_route_when_a_container_moves_the_shape_it_is_glued_to() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let far = drop_shape(&mut s, "rectangle", 9.0, 7.0);
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": far}})));
    let box_ = crate::edit::id_of(&run(&mut s, "container.add", json!({"ids": [a]})));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [c], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let (route, begin_y) = (route_of(&s, c).unwrap(), number_of(&s, c, "BeginY"));
    run(&mut s, "shape.move", json!({"ids": [box_], "dx": 0.0, "dy": -2.0}));
    assert_eq!(route_of(&s, c).unwrap(), route, "the route of the locked connector is not made again");
    assert!((number_of(&s, c, "BeginY") - (begin_y - 2.0)).abs() < 1.0e-6, "its glued end followed the member");
}

/// A callout on a locked layer does not move with the shape it is tied to and is not deleted with it: both are refused in words; a callout on no locked layer follows as before.
#[test]
fn a_callout_on_a_locked_layer_does_not_move_or_go_with_its_shape() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let callout = run(&mut s, "callout.insert", json!({"ids": [t]}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [callout], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    for (id, params) in
        [("shape.move", json!({"ids": [t], "dx": 1.0, "dy": 0.0})), ("shape.delete", json!({"ids": [t]})), ("edit.cut", json!({"ids": [t]}))]
    {
        let before = snapshot(&s);
        let e = s.execute(id, &params).expect_err(&format!("{id}: a callout on a locked layer went with its shape"));
        assert!(is_lock_refusal(&e, callout), "{id}: {e}");
        assert_eq!(snapshot(&s), before, "{id}: nothing changed");
    }
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    let before = pin_of(&s, callout);
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.0, "dy": 0.0}));
    assert!((pin_of(&s, callout).0 - before.0 - 1.0).abs() < 1.0e-6, "unlocked, the callout follows");
}
