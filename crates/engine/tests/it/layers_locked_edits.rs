//! What left. **A command that changes the drawing refuses a shape it names on a locked layer**,
//! found by the registry and not by hand.

use diacraft_engine::{EngineError, Session, command_specs, param_keys};
use serde_json::{Value as Json, json};

use crate::edit::drop_shape;
use crate::{blank_session, check_session, run, snapshot};

/// A page with the layers `Open` (0) and `Frozen` (1, locked) and three shapes: the first on `Frozen`, the second on `Open`, the third on both. → the session and the shapes.
fn locked_page() -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let ids = [(2.0, 3.0), (4.0, 3.0), (6.0, 3.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "layer.add", json!({"name": "Open"}));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [ids[0]], "layers": ["Frozen"]}));
    run(&mut s, "layer.assign", json!({"ids": [ids[1]], "layers": ["Open"]}));
    run(&mut s, "layer.assign", json!({"ids": [ids[2]], "layers": [0, 1]}));
    // A locked layer takes no new shapes, so it is locked last.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, ids)
}

/// Whether an error is the refusal of an edit of a shape on the locked layer `Frozen`: words that name the shape, the layer and the lock.
fn is_lock_refusal(e: &EngineError, id: u32) -> bool {
    matches!(e, EngineError::BadParams { msg, .. } if msg.contains(&format!("shape {id} ")) && msg.contains("\"Frozen\"") && msg.contains("locked"))
}

/// Whether the `id` a command takes names a shape: every `id` does but a page's and a stencil's, whose docs say "the id of".
fn id_names_a_shape(params: &str) -> bool {
    params.split(';').find_map(|e| e.trim().strip_prefix("id:")).is_some_and(|doc| !doc.contains("the id of"))
}

/// **The registry's walk.** Every registered command that is journaled and names shapes by `ids` or by an `id` that is a shape's is refused, before it runs, when a shape it
/// names is on a locked layer, in words that name the layer, and changes nothing; a query (not journaled) still answers about it, or refuses for a reason of its own. A command
/// added later is in the walk without a line here.
#[test]
fn every_command_that_changes_the_drawing_refuses_a_shape_it_names_on_a_locked_layer_and_a_query_does_not() {
    let (mut s, [frozen, _, both]) = locked_page();
    let (mut edits, mut queries) = (Vec::new(), Vec::new());
    for spec in command_specs() {
        let keys = param_keys(spec.params).unwrap();
        let by_ids = keys.contains(&"ids");
        let by_id = keys.contains(&"id") && id_names_a_shape(spec.params);
        if !by_ids && !by_id {
            continue;
        }
        for (shape, params) in [frozen, both].into_iter().flat_map(|id| {
            let mut tries = Vec::new();
            if by_ids {
                tries.push((id, json!({"ids": [id]})));
            }
            if by_id {
                tries.push((id, json!({"id": id})));
            }
            tries
        }) {
            let before = snapshot(&s);
            let got = s.execute(spec.id, &params);
            if spec.journal {
                let e = got.expect_err(&format!("{} {params}: an edit of a shape on a locked layer ran", spec.id));
                assert!(is_lock_refusal(&e, shape), "{} {params}: refused for another reason: {e}", spec.id);
                assert_eq!(snapshot(&s), before, "{} {params}: a refusal changes nothing", spec.id);
                edits.push(spec.id);
            } else {
                if let Err(e) = &got {
                    assert!(!is_lock_refusal(e, shape), "{} {params}: a query is not refused for a lock: {e}", spec.id);
                }
                queries.push(spec.id);
            }
            check_session(&s).unwrap();
        }
    }
    // The walk met the commands the brief names: a move, a resize, a format, a text, a delete, an arrange, the clipboard, the groups, the connectors and the layers.
    for id in [
        "shape.move",
        "shape.resize",
        "shape.format",
        "text.format",
        "shape.set_text",
        "text.insert",
        "shape.delete",
        "shape.rotate",
        "shape.align",
        "shape.order",
        "edit.cut",
        "edit.copy",
        "shape.duplicate",
        "shape.group",
        "connector.reroute",
        "layer.assign",
        "hyperlink.set",
        "shape.union",
        // The command, added after the rule: the walk holds it to it with no line of its own (this one only checks it was met).
        "shape.change",
        // Also added after the rule.
        "callout.insert",
        "callout.attach",
    ] {
        assert!(edits.contains(&id), "{id} was not walked as an edit: {edits:?}");
    }
    for id in ["shape.placement", "shape.formatting", "edit.find", "hyperlink.list", "text.block"] {
        assert!(queries.contains(&id), "{id} was not walked as a query: {queries:?}");
    }
}

/// A page's and a stencil's `id` is not a shape's: `page.rename` of page 1 runs though shape 1 is on a locked layer.
#[test]
fn an_id_that_is_a_pages_is_not_read_as_a_shapes() {
    let (mut s, [frozen, ..]) = locked_page();
    assert_eq!(frozen, 1, "the fixture's first shape has the first page's id");
    run(&mut s, "page.rename", json!({"id": 1, "name": "Plan"}));
}

/// A shape on a locked layer, alone or with an unlocked one, is not moved; one on an unlocked layer is; a query answers about the locked one; unlocked, it moves.
#[test]
fn a_shape_on_a_locked_layer_is_not_moved_until_the_layer_is_unlocked() {
    let (mut s, [frozen, open, both]) = locked_page();
    for id in [frozen, both] {
        let before = snapshot(&s);
        let e = s.execute("shape.move", &json!({"ids": [open, id], "dx": 1.0, "dy": 0.0})).unwrap_err();
        assert!(is_lock_refusal(&e, id), "{e}");
        assert!(e.to_string().contains("layer 1") && e.to_string().contains("unlock the layer"), "{e}");
        assert_eq!(snapshot(&s), before, "nothing moved, not even the shape on the open layer");
    }
    run(&mut s, "shape.move", json!({"ids": [open], "dx": 1.0, "dy": 0.0}));
    let placed = run(&mut s, "shape.placement", json!({"ids": [frozen]}));
    assert!(placed.get("shapes").is_some() || placed.is_object(), "a query answers about a locked shape: {placed}");
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    run(&mut s, "shape.move", json!({"ids": [frozen, both], "dx": 1.0, "dy": 0.0}));
}

/// A batch that names a shape on a locked layer in one of its steps is refused whole, and the steps before it are not kept.
#[test]
fn a_batch_with_a_step_on_a_locked_shape_is_refused_whole() {
    let (mut s, [frozen, open, _]) = locked_page();
    let before = snapshot(&s);
    let steps = json!({"commands": [
        {"command": "shape.move", "params": {"ids": [open], "dx": 1.0, "dy": 0.0}},
        {"command": "shape.format", "params": {"ids": [frozen], "fill": "#ff0000"}},
    ]});
    let e = s.execute("command.batch", &steps).unwrap_err();
    assert!(e.to_string().contains("\"Frozen\"") && e.to_string().contains("locked"), "{e}");
    assert_eq!(snapshot(&s), before);
}

/// Each shape goes by its own list, as the window's pointer does: a group on no layer moves though a member of it is on a locked layer, and the member named
/// on its own is refused.
#[test]
fn a_group_is_judged_by_its_own_list_and_a_member_by_its_own() {
    let mut s = blank_session();
    let ids = [(2.0, 3.0), (4.0, 3.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [ids[0]], "layers": ["Frozen"]}));
    let group = run(&mut s, "shape.group", json!({"ids": ids}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    run(&mut s, "shape.move", json!({"ids": [group], "dx": 1.0, "dy": 0.0}));
    let e = s.execute("shape.format", &json!({"ids": [ids[0]], "fill": "#00ff00"})).unwrap_err();
    assert!(is_lock_refusal(&e, ids[0]), "{e}");
    run(&mut s, "shape.format", json!({"ids": [ids[1]], "fill": "#00ff00"}));
}

/// A shape on a locked layer of another page is not read for the page a command works on: the page named.
#[test]
fn the_lock_is_read_on_the_page_the_command_works_on() {
    let (mut s, [frozen, ..]) = locked_page();
    let second = run(&mut s, "page.add", json!({"name": "Second"}))["id"].as_u64().unwrap() as u32;
    let other = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": second, "x": 2.0, "y": 2.0})));
    let moved: Json = run(&mut s, "shape.move", json!({"ids": [other], "page": second, "dx": 1.0, "dy": 0.0}));
    assert_eq!(moved["page"], second, "{moved}");
    let e = s.execute("shape.move", &json!({"ids": [frozen], "page": 1, "dx": 1.0, "dy": 0.0})).unwrap_err();
    assert!(is_lock_refusal(&e, frozen), "{e}");
}

// ----------: a layer whose Glue box is off takes no new glue ----------

/// Three rectangles in a row (1.5 by 1, at the height of 4, 3 inches apart) and the layers `Open` (0) and `Bare` (1, Glue off): the second rectangle on `Bare`, the third on
/// none. → the session and the shapes.
fn glue_row() -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let ids = [3.0, 6.0, 9.0].map(|x| drop_shape(&mut s, "rectangle", x, 4.0));
    run(&mut s, "layer.add", json!({"name": "Open"}));
    run(&mut s, "layer.add", json!({"name": "Bare", "glue": false}));
    run(&mut s, "layer.assign", json!({"ids": [ids[1]], "layers": ["Bare"]}));
    (s, ids)
}

/// The shapes the connector `id` is glued to, begin then end, from the page's `Connect` rows.
fn glued(s: &Session, id: u32) -> (Option<u32>, Option<u32>) {
    let page = s.document().unwrap().page(1).unwrap();
    let to = |cell: &str| page.connects.iter().find(|c| c.from_sheet == id && c.from_cell.as_deref() == Some(cell)).map(|c| c.to_sheet);
    (to("BeginX"), to("EndX"))
}

fn is_glue_refusal(e: &EngineError, id: u32) -> bool {
    matches!(e, EngineError::BadParams { msg, .. } if msg.contains(&format!("shape {id} ")) && msg.contains("\"Bare\"") && msg.contains("Glue box is off"))
}

/// A connector's end that names a shape whose layers allow no glue is refused in words, for `connector.add` and `connector.reconnect`; a shape on a layer that allows it, on
/// that layer and one that does not, or on no layer, is glued to (the project rule).
#[test]
fn a_connector_is_not_glued_to_a_shape_whose_layers_allow_no_glue() {
    let (mut s, [a, bare, none]) = glue_row();
    let before = snapshot(&s);
    let e = s.execute("connector.add", &json!({"from": {"id": a}, "to": {"id": bare}})).unwrap_err();
    assert!(is_glue_refusal(&e, bare), "{e}");
    let e = s.execute("connector.add", &json!({"from": {"id": bare, "point": 0}, "to": {"x": 1.0, "y": 1.0}})).unwrap_err();
    assert!(is_glue_refusal(&e, bare), "the begin too: {e}");
    assert_eq!(snapshot(&s), before, "a refusal changes nothing");
    // The other shapes are glued to as before.
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": none}})));
    assert_eq!(glued(&s, c), (Some(a), Some(none)));
    let e = s.execute("connector.reconnect", &json!({"id": c, "end": "end", "target": {"id": bare}})).unwrap_err();
    assert!(is_glue_refusal(&e, bare), "{e}");
    assert_eq!(glued(&s, c), (Some(a), Some(none)), "the end stays where it was");
    // On `Bare` and `Open` too: a layer it is on allows glue.
    run(&mut s, "layer.assign", json!({"ids": [bare], "layers": ["Open"], "mode": "add"}));
    run(&mut s, "connector.reconnect", json!({"id": c, "end": "end", "target": {"id": bare}}));
    assert_eq!(glued(&s, c), (Some(a), Some(bare)));
    check_session(&s).unwrap();
}

/// An end given as a place and snapped (`snap: true`) onto a connection point of a shape whose layers allow no glue is not glued: it stays free. The same by a shape on no layer
/// is glued.
#[test]
fn a_snapped_end_on_a_connection_point_of_a_shape_whose_layers_allow_no_glue_is_free() {
    let (mut s, [a, _, _]) = glue_row();
    // The left connection point of the second rectangle is at (5.25, 4); the end lands 0.04 off it.
    let c = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 5.29, "y": 4.03}, "snap": true}));
    assert_eq!(c["end"], Json::Null, "not glued: {c}");
    assert_eq!(c["snap"]["to"]["glue"], Json::Null, "{c}");
    // The first rectangle's right point, at (3.75, 4), on no layer: glued.
    let d = run(&mut s, "connector.add", json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 3.79, "y": 4.03}, "snap": true}));
    assert_eq!(d["end"]["id"], json!(a), "{d}");
}

/// AutoConnect offers no shape whose layers allow no glue: a click on an arrow passes over it to the next neighbour, the shape itself joins nothing, a drag onto it is refused,
/// and a shape that would be made onto active layers none of which allows glue is refused; all in words and with nothing changed.
#[test]
fn autoconnect_offers_no_shape_whose_layers_allow_no_glue() {
    let (mut s, [a, bare, none]) = glue_row();
    let d = s.document().unwrap();
    assert_eq!(diacraft_engine::draw::neighbour(d, 1, a, diacraft_engine::draw::route::Dir::Right), Some(none), "the no-glue shape is passed over");
    assert_eq!(run(&mut s, "connector.neighbour", json!({"from": a, "direction": "right"}))["neighbour"], json!(none));
    let before = snapshot(&s);
    for params in [
        json!({"from": bare, "direction": "right", "existing": true}),
        json!({"from": bare, "direction": "up"}),
        json!({"from": a, "direction": "right", "to": bare}),
    ] {
        let e = s.execute("connector.autoconnect", &params).unwrap_err();
        assert!(is_glue_refusal(&e, bare), "{params}: {e}");
        assert_eq!(snapshot(&s), before, "{params}");
    }
    let joined = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "existing": true}));
    assert_eq!(joined["id"], json!(none), "{joined}");
    // A new shape joins the active layers that are not locked: when none of them allows glue, AutoConnect would glue to a shape whose layers allow none.
    run(&mut s, "layer.set", json!({"layer": "Bare", "active": true}));
    let before = snapshot(&s);
    for params in [json!({"from": a, "direction": "down"}), json!({"from": a, "direction": "down", "master": "rectangle"})] {
        let e = s.execute("connector.autoconnect", &params).unwrap_err();
        assert!(e.to_string().contains("\"Bare\"") && e.to_string().contains("Glue box is off"), "{params}: {e}");
        assert_eq!(snapshot(&s), before);
    }
    // With an active layer that allows glue beside it, the new shape is on both and is glued to.
    run(&mut s, "layer.set", json!({"layer": "Open", "active": true}));
    let made = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "down", "master": "rectangle"}));
    assert_eq!(made["created"], json!(true), "{made}");
    check_session(&s).unwrap();
}

/// Glue a drawing holds is kept when the layer's Glue box is turned off: the `Connect` rows stay, the end follows its shape when the shape moves, and a save and a
/// reopen keep them.
#[test]
fn glue_that_is_there_is_kept_when_the_glue_box_is_turned_off() {
    let (mut s, [a, bare, _]) = glue_row();
    run(&mut s, "layer.set", json!({"layer": "Bare", "glue": true}));
    let c = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": bare}})));
    run(&mut s, "layer.set", json!({"layer": "Bare", "glue": false}));
    assert_eq!(glued(&s, c), (Some(a), Some(bare)), "the glue stays");
    run(&mut s, "shape.move", json!({"ids": [bare], "dx": 0.0, "dy": 1.0}));
    assert_eq!(glued(&s, c), (Some(a), Some(bare)), "and holds when the shape moves");
    let dir = crate::odg_build::Dir::new();
    for ext in ["vsdx", "diacraft"] {
        let path = dir.path(&format!("glue.{ext}"));
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut back = Session::new();
        run(&mut back, "file.open", json!({"path": path}));
        assert_eq!(glued(&back, c), (Some(a), Some(bare)), "{ext}: the glue is kept in the file");
    }
}

// ---------- An `.odg` saved in place declares a layer added in DiaCraft in its own layer set ----------

/// The `draw:layer` elements of the layer set of `office:master-styles`, each as its markup is written.
fn layer_set(bytes: &[u8]) -> Vec<String> {
    let styles = crate::odg_build::part_of(bytes, "styles.xml");
    let Some(start) = styles.find("<draw:layer-set") else { return Vec::new() };
    let end = styles[start..].find("</draw:layer-set>").map_or(styles.len(), |e| start + e);
    styles[start..end].split("<draw:layer ").skip(1).map(|piece| format!("<draw:layer {}", piece.split("/>").next().unwrap_or(""))).collect()
}

/// The start tag of the shape element of `content.xml` whose `draw:name` is `name`.
fn shape_tag(bytes: &[u8], name: &str) -> String {
    let content = crate::odg_build::content_of(bytes);
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name} in {content}"));
    let start = content[..at].rfind('<').unwrap();
    let end = content[at..].find('>').map_or(content.len(), |e| at + e);
    content[start..end].to_owned()
}

/// The package without DiaCraft's document file and its folder: what a program that drops DiaCraft's data (LibreOffice, without the folder's manifest row) reads.
fn without_diacraft(bytes: &[u8]) -> Vec<u8> {
    let parts = ["content.xml", "styles.xml", "meta.xml"].map(|name| (name.to_owned(), crate::odg_build::part_of(bytes, name).into_bytes()));
    crate::odg_build::package(crate::odg_build::DRAWING, parts.to_vec())
}

/// The `LayerMember` text of the shape called `name` on the first page of `doc`.
fn member_of(doc: &diacraft_doc::Document, name: &str) -> Option<String> {
    let page = doc.pages.get(0).unwrap();
    let shape = page.shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}"));
    shape.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The `LayerMember` text of the shape `id` of page `page` of `doc` (the first with that `ID`): `None` for a shape with no such cell.
fn member_on(doc: &diacraft_doc::Document, page: u32, id: u32) -> Option<String> {
    let at = doc.find_shape(diacraft_doc::Owner::Page(page), id)?;
    doc.shape(&at)?.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The `Name`, `Visible`, `Print` and `Lock` of each `Layer` row of the first page of `doc`.
fn rows_of(doc: &diacraft_doc::Document) -> Vec<(String, String, String, String)> {
    let page = doc.pages.get(0).unwrap();
    let Some(section) = page.sheet.section(&diacraft_doc::SectionKey::new("Layer", None)) else { return Vec::new() };
    let text =
        |row: &diacraft_doc::Row, cell: &str| row.cells.get(cell).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default();
    section.rows.iter().map(|r| (text(r, "Name"), text(r, "Visible"), text(r, "Print"), text(r, "Lock"))).collect()
}

/// A layer added to a drawing read from an `.odg` and saved in place is a `draw:layer` of the document's layer set, after the file's, with `draw:display` from `Visible` and
/// `Print` and `draw:protected` from `Lock`; a shape added on it is written on it, and so is a shape of the file put on it (
/// keep a shape's `draw:layer` for every other layer), its list riding in DiaCraft's data too. Read without DiaCraft's data, as another program reads it, the page has the layer
/// and the new shape is on it; read with it, the drawing is the model.
#[test]
fn an_odg_saved_in_place_declares_a_layer_added_in_its_layer_set_and_writes_a_new_shape_on_it() {
    let dir = crate::odg_build::Dir::new();
    let drawing = crate::odg_build::Drawing::new().page(&crate::odg_build::rect("A", (2.0, 2.0, 4.0, 2.0), ""));
    let mut s = crate::odg_build::open_drawing(&dir, &drawing);
    let a = crate::odg_build::id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "layer.add", json!({"name": "Wiring", "print": false, "active": true}));
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let new = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 5.0, "y": 5.0})));
    run(&mut s, "layer.assign", json!({"ids": [a], "layers": ["Wiring"], "page": page}));
    run(&mut s, "layer.add", json!({"name": "Frozen", "lock": true}));
    let want_lists: Vec<Option<String>> = [a, new].iter().map(|id| member_on(s.document().unwrap(), page, *id)).collect();
    let want_layers = run(&mut s, "layer.list", json!({"page": page}))["layers"].clone();
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    // The layer set: the file's five, then the two new ones, as the format can say them.
    let set = layer_set(&bytes);
    assert_eq!(set.len(), 7, "{set:?}");
    assert!(set[5].contains("draw:name=\"Wiring\"") && set[5].contains("draw:display=\"screen\"") && !set[5].contains("protected"), "{set:?}");
    assert!(set[6].contains("draw:name=\"Frozen\"") && set[6].contains("draw:protected=\"true\"") && !set[6].contains("display"), "{set:?}");
    // The new shape is written on its layer, and so is the shape of the file put on it.
    let new_shape = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|sh| sh.id == new).unwrap().name_u.clone().unwrap();
    assert!(shape_tag(&bytes, &new_shape).contains("draw:layer=\"Wiring\""), "{}", shape_tag(&bytes, &new_shape));
    assert!(shape_tag(&bytes, "A").contains("draw:layer=\"Wiring\""), "{}", shape_tag(&bytes, "A"));
    // Another program's reading: the markup alone.
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&bytes));
    assert_eq!(
        rows_of(&bare),
        [("Wiring".into(), "1".into(), "0".into(), "0".into()), ("Frozen".into(), "1".into(), "1".into(), "1".into())],
        "the markup gives the page both layers"
    );
    assert_eq!(member_of(&bare, &new_shape).as_deref(), Some("0"), "the new shape is on Wiring");
    // (The shape of the file is on `Wiring` in the markup, and its list rides on its own style's DiaCraft attributes too, which the reader puts over it.)
    assert_eq!(member_of(&bare, "A").as_deref(), Some("0"), "the list the shape's style carries");
    // DiaCraft's reading: the model.
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("a.odg")}));
    let page = back.document().unwrap().pages.get(0).unwrap().id;
    let lists: Vec<Option<String>> = [a, new].iter().map(|id| member_on(back.document().unwrap(), page, *id)).collect();
    assert_eq!(lists, want_lists);
    assert_eq!(run(&mut back, "layer.list", json!({"page": page}))["layers"], want_layers);
}

/// A file whose styles hold no layer set is given one holding the layer added, and a file with a page that has a layer set of its own declares nothing of that page's own in
/// the document's set: an edit that adds no layer leaves the set as it was, and the page's new shape, whose own set lacks the layer, is read back on it from DiaCraft's data.
#[test]
fn an_odg_with_no_layer_set_is_given_one_and_a_pages_own_set_is_not_copied_into_the_documents() {
    let dir = crate::odg_build::Dir::new();
    let drawing = crate::odg_build::Drawing::new().page(&crate::odg_build::rect("A", (2.0, 2.0, 4.0, 2.0), ""));
    let bytes = drawing.bytes();
    let styles = crate::odg_build::part_of(&bytes, "styles.xml");
    let start = styles.find("<draw:layer-set>").unwrap();
    let end = styles.find("</draw:layer-set>").unwrap() + "</draw:layer-set>".len();
    let stripped = crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![
            ("content.xml".to_owned(), crate::odg_build::part_of(&bytes, "content.xml").into_bytes()),
            ("styles.xml".to_owned(), format!("{}{}", &styles[..start], &styles[end..]).into_bytes()),
            ("meta.xml".to_owned(), crate::odg_build::part_of(&bytes, "meta.xml").into_bytes()),
        ],
    );
    let mut s = crate::odg_build::open_bytes_in(&dir, "none.odg", &stripped);
    run(&mut s, "layer.add", json!({"name": "Notes", "visible": false}));
    run(&mut s, "file.save", json!({}));
    let after = std::fs::read(dir.path("none.odg")).unwrap();
    assert_eq!(
        layer_set(&after),
        ["<draw:layer draw:name=\"Notes\" draw:display=\"printer\""],
        "{}",
        crate::odg_build::part_of(&after, "styles.xml")
    );
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&after));
    assert_eq!(rows_of(&bare), [("Notes".into(), "0".into(), "1".into(), "0".into())]);

    // A page with a layer set of its own: its layer is the page's, and an edit that adds none leaves the document's set as it was.
    let own =
        format!("<draw:layer-set><draw:layer draw:name=\"PageOnly\"/></draw:layer-set>{}", crate::odg_build::rect("A", (2.0, 2.0, 4.0, 2.0), ""));
    let drawing = crate::odg_build::Drawing::new().page(&own);
    let mut s = crate::odg_build::open_bytes_in(&dir, "own.odg", &drawing.bytes());
    let a = crate::odg_build::id_of(s.document().unwrap(), 0, "A");
    let page = s.document().unwrap().pages.get(0).unwrap().id;
    let before = layer_set(&std::fs::read(dir.path("own.odg")).unwrap());
    run(&mut s, "shape.move", json!({"ids": [a], "page": page, "dx": 1.0, "dy": 0.0}));
    run(&mut s, "file.save", json!({}));
    assert_eq!(layer_set(&std::fs::read(dir.path("own.odg")).unwrap()), before, "the page's own layer is not put in the document's set");
    // A layer added there is declared in the document's set; the page's new shape on it reads back on it.
    let mut s = crate::odg_build::open_bytes_in(&dir, "own.odg", &std::fs::read(dir.path("own.odg")).unwrap());
    run(&mut s, "layer.add", json!({"name": "Extra", "active": true, "page": page}));
    let new = crate::edit::id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": page, "x": 5.0, "y": 5.0})));
    let want = member_on(s.document().unwrap(), page, new);
    assert_eq!(want.as_deref(), Some("1"), "on the page's second row");
    run(&mut s, "file.save", json!({}));
    let saved = std::fs::read(dir.path("own.odg")).unwrap();
    assert!(layer_set(&saved).iter().any(|l| l.contains("draw:name=\"Extra\"")), "{:?}", layer_set(&saved));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("own.odg")}));
    assert_eq!(member_on(back.document().unwrap(), page, new), want);
}

/// A layer added to one page of an `.odg` is declared in the document's layer set, which every page that has no set of its own reads: the other page's rows ride in DiaCraft's
/// document file, so that opened again it has no layer it did not have (a page whose rows differ from the set is carried).
#[test]
fn a_layer_added_to_one_page_of_an_odg_is_not_read_on_the_others() {
    let dir = crate::odg_build::Dir::new();
    let drawing = crate::odg_build::Drawing::new().page(&crate::odg_build::rect("A", (2.0, 2.0, 4.0, 2.0), "")).page(&crate::odg_build::rect(
        "B",
        (2.0, 2.0, 4.0, 2.0),
        "",
    ));
    let mut s = crate::odg_build::open_drawing(&dir, &drawing);
    let pages: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    run(&mut s, "layer.add", json!({"name": "Second only", "page": pages[1]}));
    let want: Vec<Json> = pages.iter().map(|p| run(&mut s, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    assert_eq!(want[0], json!([]), "the first page has no layer");
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    assert!(layer_set(&bytes).iter().any(|l| l.contains("draw:name=\"Second only\"")), "{:?}", layer_set(&bytes));
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path("a.odg")}));
    let got: Vec<Json> = pages.iter().map(|p| run(&mut back, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    assert_eq!(got, want, "each page has the layers it had");
    // And saved again unedited, the file is as it was.
    run(&mut back, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path("a.odg")).unwrap(), bytes, "a second save writes nothing new");
}
