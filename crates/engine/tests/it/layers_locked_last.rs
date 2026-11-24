//! The last layer gaps left, and (Lock "stops shapes on the layer from being selected, moved or edited").
//!
//! 1. In both `.odg` writers a layer's flags come from the row the person changed; with none changed the first page decides; of two changed rows, the later page's.
//! 2. A group with a member on a locked layer refuses a delete, a cut and an ungroup, and a container with one refuses a disband; a copy of such a group is made, and
//!    the copy's member is on the same layer.
//! 3. A container on a locked layer is not fitted, takes no member and loses none: a move, a drop, a paste or a resize passes over it and says so; a command that would take a
//!    member out of it is refused in words.
//! 4. `theme.apply` writes no cell into a shape on a locked layer, and counts the shapes it passed over.
//! 5. A page's own `.odg` layer set has its flags written on a save in place, as the document's set has.

use diacraft_doc::{Owner, SectionKey};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::container::{boxed, members};
use crate::edit::{doc, drop_shape, id_of};
use crate::{blank_session, check_session, run, snapshot};

/// The shape `id` of page `page` as it is (a copy), or `None` when the page holds none.
fn shape_on(s: &Session, page: u32, id: u32) -> Option<diacraft_doc::Shape> {
    let doc = s.document()?;
    let at = doc.find_shape(Owner::Page(page), id)?;
    doc.shape(&at).map(|v| (**v).clone())
}

/// The words of a refusal, which must be a bad-parameters one.
fn words(e: &EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg.clone(),
        other => panic!("expected a refusal of the parameters, got {other:?}"),
    }
}

/// The notes of an answer, joined.
fn notes(answer: &Json) -> String {
    answer["notes"].as_array().map(|n| n.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" | ")).unwrap_or_default()
}

/// Whether the notes of an answer say that the container `id` on the layer `Frozen` was left as it was.
fn says_left(answer: &Json, id: u32) -> bool {
    let n = notes(answer);
    n.contains(&format!("container {id} is on layer")) && n.contains("\"Frozen\"") && n.contains("left as it was")
}

// ---------- 2. A group with a member on a locked layer ----------

/// A group `g` on no layer of a rectangle `a` (on no layer) and a rectangle `b` on `Frozen` (locked), and a rectangle `c` beside it.
fn group_page() -> (Session, [u32; 4]) {
    let mut s = blank_session();
    let [a, b, c] = [(2.0, 3.0), (4.0, 3.0), (8.0, 3.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [b], "layers": ["Frozen"]}));
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, [a, b, c, g])
}

/// Whether `e` is the refusal of for the member `member` of the group `group`.
fn is_group_refusal(e: &EngineError, member: u32, group: u32, what: &str) -> bool {
    let w = words(e);
    w.contains(&format!("shape {member} in group {group} is on layer")) && w.contains("\"Frozen\"") && w.contains(&format!("is not {what}"))
}

/// A delete, a cut and an ungroup of a group with a member on a locked layer are refused in words that name the member, the group and the layer, and nothing changes;
/// so is a delete of a group that holds that group, and of a container that holds it. Unlocked, the group is deleted.
#[test]
fn a_group_with_a_member_on_a_locked_layer_is_not_deleted_cut_or_ungrouped() {
    let (mut s, [_, b, c, g]) = group_page();
    for (id, params, what) in [
        ("shape.delete", json!({"ids": [g]}), "deleted"),
        ("shape.delete", json!({"ids": [c, g]}), "deleted"),
        ("edit.cut", json!({"ids": [g]}), "cut"),
        ("shape.ungroup", json!({"ids": [g]}), "ungrouped"),
    ] {
        let before = snapshot(&s);
        let e = s.execute(id, &params).expect_err(&format!("{id} {params}: a group was taken whole with its member on a locked layer"));
        assert!(is_group_refusal(&e, b, g, what), "{id} {params}: {e}");
        assert_eq!(snapshot(&s), before, "{id} {params}: nothing changed");
    }
    // A group that holds that group, and a container that holds it: the member at any depth.
    let outer = id_of(&run(&mut s, "shape.group", json!({"ids": [g, c]})));
    for (id, what) in [("shape.delete", "deleted"), ("edit.cut", "cut"), ("shape.ungroup", "ungrouped")] {
        let before = snapshot(&s);
        let e = s.execute(id, &json!({"ids": [outer]})).expect_err(&format!("{id}: the outer group went"));
        assert!(is_group_refusal(&e, b, outer, what), "{id}: {e}");
        assert_eq!(snapshot(&s), before, "{id}: nothing changed");
    }
    let (mut s, [_, b, _, g]) = group_page();
    let k = id_of(&run(&mut s, "container.add", json!({"ids": [g]})));
    let before = snapshot(&s);
    let e = s.execute("shape.delete", &json!({"ids": [k]})).expect_err("a container took a group with a locked member with it");
    assert!(is_group_refusal(&e, b, g, "deleted"), "{e}");
    assert_eq!(snapshot(&s), before);
    // Unlocked, the group goes.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    run(&mut s, "shape.delete", json!({"ids": [g]}));
    assert!(shape_on(&s, 1, b).is_none(), "unlocked, the member goes with its group");
    check_session(&s).unwrap();
}

/// A disband of a container that holds a shape on a locked layer would rewrite that shape's ties, and is refused in words that name it, the container and the layer.
/// Unlocked, the container is disbanded and the shape stays.
#[test]
fn a_container_with_a_member_on_a_locked_layer_is_not_disbanded() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [b], "layers": ["Frozen"]}));
    let k = id_of(&run(&mut s, "container.add", json!({"ids": [a, b]})));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let before = snapshot(&s);
    let e = s.execute("container.disband", &json!({"ids": [k]})).expect_err("a container was disbanded through its member on a locked layer");
    let w = words(&e);
    assert!(w.contains(&format!("shape {b} is on layer")) && w.contains("\"Frozen\"") && w.contains(&format!("container {k}")), "{w}");
    assert_eq!(snapshot(&s), before, "nothing changed");
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    run(&mut s, "container.disband", json!({"ids": [k]}));
    assert!(shape_on(&s, 1, k).is_none() && shape_on(&s, 1, b).is_some(), "unlocked, the container goes and the shape stays");
    check_session(&s).unwrap();
}

/// A copy of a group with a member on a locked layer is made (copy, paste, duplicate), and the copy of the member is on the same layer.
#[test]
fn a_copy_of_a_group_with_a_member_on_a_locked_layer_keeps_the_member_on_its_layer() {
    let (mut s, [_, b, _, g]) = group_page();
    let member =
        |s: &Session, id: u32| shape_on(s, 1, id).unwrap().sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned());
    let held = member(&s, b);
    assert_eq!(held.as_deref(), Some("0"));
    run(&mut s, "edit.copy", json!({"ids": [g]}));
    let pasted = run(&mut s, "edit.paste", json!({"x": 5.0, "y": 7.0}));
    let duplicated = run(&mut s, "shape.duplicate", json!({"ids": [g]}));
    for answer in [pasted, duplicated] {
        let copy = u32::try_from(answer["ids"][0].as_u64().unwrap()).unwrap();
        let group = shape_on(&s, 1, copy).unwrap();
        let copied = group
            .children
            .iter()
            .map(|m| m.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()))
            .collect::<Vec<_>>();
        assert!(copied.contains(&held), "the copy's member is on the locked layer: {copied:?}");
    }
    check_session(&s).unwrap();
}

// ---------- 3. A container on a locked layer ----------

/// A container `k` round a rectangle `a` (on no layer), `k` on `Frozen` (locked), and a rectangle `d` far from it. → the session and `[a, d, k]`.
fn frozen_container() -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let d = drop_shape(&mut s, "rectangle", 9.0, 9.0);
    let k = id_of(&run(&mut s, "container.add", json!({"ids": [a]})));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [k], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    (s, [a, d, k])
}

/// The middle of the box of shape `id` of page 1.
fn middle(s: &Session, id: u32) -> (f64, f64) {
    let (x0, y0, x1, y1) = boxed(doc(s), id);
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

/// A container on a locked layer is left as it was by the automatic steps, which say so in a note naming it and its layer: a member moved past its edge does not grow it, a
/// member moved out of it stays a member, a shape moved, dropped or pasted inside it does not join it, and a member made larger does not grow it. Unlocked, it follows again.
#[test]
fn a_container_on_a_locked_layer_takes_and_loses_no_member_and_is_not_resized() {
    let (mut s, [a, d, k]) = frozen_container();
    let held = boxed(doc(&s), k);
    let (kx, ky) = middle(&s, k);
    // A member moved past the edge: the container would grow.
    let out = run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.6, "dy": 0.0}));
    assert_eq!(boxed(doc(&s), k), held, "the container on the locked layer is not grown");
    assert!(says_left(&out, k), "the move says so: {out}");
    // Moved back, and then a shape moved into it does not join it.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": -0.6, "dy": 0.0}));
    let (dx, dy) = middle(&s, d);
    let out = run(&mut s, "shape.move", json!({"ids": [d], "dx": kx - dx + 0.1, "dy": ky - dy - 0.1}));
    assert_eq!(members(&mut s, k), [a], "the shape moved in did not join: {out}");
    assert!(says_left(&out, k), "the move says so: {out}");
    // A shape dropped inside it, and one pasted inside it.
    let out = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": kx, "y": ky - 0.1}));
    let dropped = id_of(&out);
    assert_eq!(members(&mut s, k), [a], "the shape dropped in did not join");
    assert!(says_left(&out, k), "the drop says so: {out}");
    run(&mut s, "edit.copy", json!({"ids": [dropped]}));
    let out = run(&mut s, "edit.paste", json!({"x": kx, "y": ky - 0.1}));
    assert_eq!(members(&mut s, k), [a], "the shape pasted in did not join");
    assert!(says_left(&out, k), "the paste says so: {out}");
    // A member made larger than the container holds.
    let (x0, y0, x1, y1) = boxed(doc(&s), a);
    let out = run(&mut s, "shape.resize", json!({"id": a, "x": x0, "y": y0, "width": (x1 - x0) * 2.0, "height": y1 - y0}));
    assert_eq!(boxed(doc(&s), k), held, "a member made larger does not grow it");
    assert!(says_left(&out, k), "the resize says so: {out}");
    let out = run(&mut s, "shape.resize", json!({"ids": [a], "x": x0, "y": y0, "width": (x1 - x0) * 2.5, "height": y1 - y0}));
    assert_eq!(boxed(doc(&s), k), held);
    assert!(says_left(&out, k), "the resize of `ids` says so: {out}");
    // A member moved out stays a member: the container loses none.
    let out = run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": -6.0}));
    assert_eq!(members(&mut s, k), [a], "the member moved out stays in the container on the locked layer");
    assert!(says_left(&out, k), "the move says so: {out}");
    assert_eq!(boxed(doc(&s), k), held);
    // Fit to Contents of it is refused for the lock, with nothing changed.
    let before = snapshot(&s);
    let e = s.execute("container.fit", &json!({"ids": [k]})).expect_err("a container on a locked layer was fitted");
    assert!(words(&e).contains(&format!("shape {k} is on layer")), "{e}");
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
    // Unlocked, the member moved back in and out again leaves it.
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    let out = run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": -0.5}));
    assert!(notes(&out).is_empty(), "unlocked, nothing is passed over: {out}");
    assert!(members(&mut s, k).is_empty(), "unlocked, the member outside leaves");
}

/// The commands that would take a member out of a container on a locked layer, or put one in it, are refused in words that name the container and its layer, and change
/// nothing; one that would put a shape in the container that holds it passes over it and says so.
#[test]
fn a_command_that_would_take_a_member_out_of_a_container_on_a_locked_layer_is_refused() {
    let (mut s, [a, d, k]) = frozen_container();
    let other = id_of(&run(&mut s, "container.add", json!({"ids": [d]})));
    for (id, params) in [
        ("container.add", json!({"ids": [a]})),
        ("container.add_member", json!({"ids": [a], "container": other})),
        ("container.attach_edge", json!({"id": a, "container": other, "edge": "left"})),
    ] {
        let before = snapshot(&s);
        let e = s.execute(id, &params).expect_err(&format!("{id} {params}: a member left the container on a locked layer"));
        let w = words(&e);
        assert!(w.contains(&format!("container {k} is on layer")) && w.contains("\"Frozen\"") && w.contains(&format!("shape {a}")), "{id}: {w}");
        assert_eq!(snapshot(&s), before, "{id}: nothing changed");
    }
    // A shape put inside it while it is locked is in no container; Add to Underlying Container finds none to take it, and says why.
    let (kx, ky) = middle(&s, k);
    let x = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": kx, "y": ky - 0.1})));
    let before = snapshot(&s);
    let e = s.execute("container.add_member", &json!({"ids": [x]})).expect_err("a shape joined the container on a locked layer");
    let w = format!("{e}");
    assert!(w.contains(&format!("container {k} is on layer")) && w.contains("\"Frozen\""), "{w}");
    assert_eq!(snapshot(&s), before);
    // A container added round it would be inside the one on the locked layer: it is not put in it, and the answer says so.
    let out = run(&mut s, "container.add", json!({"ids": [x]}));
    assert!(out["inside"].is_null(), "{out}");
    assert!(says_left(&out, k), "{out}");
    assert_eq!(members(&mut s, k), [a]);
    check_session(&s).unwrap();
}

/// Nesting: a container on a locked layer that holds an open one is not grown when the open one grows, and the members of the open one disbanded do not join it; both say so.
#[test]
fn a_container_on_a_locked_layer_is_not_grown_by_the_one_it_holds_nor_given_its_members() {
    let mut s = blank_session();
    let outer = id_of(&run(&mut s, "container.add", json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 6.0})));
    let inner = id_of(&run(&mut s, "container.add", json!({"x": 4.25, "y": 5.0, "width": 3.0, "height": 2.0})));
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.0);
    assert_eq!(members(&mut s, inner), [a]);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [outer], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    assert_eq!(members(&mut s, outer), [inner]);
    let (held, small) = (boxed(doc(&s), outer), boxed(doc(&s), inner));
    let out = run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.4, "dy": 0.0}));
    assert_ne!(boxed(doc(&s), inner), small, "the open container grows");
    assert_eq!(boxed(doc(&s), outer), held, "the one on the locked layer does not");
    assert!(says_left(&out, outer), "{out}");
    let out = run(&mut s, "container.disband", json!({"ids": [inner]}));
    assert!(members(&mut s, outer).is_empty(), "the members of the container disbanded do not join the one on the locked layer: {out}");
    assert!(says_left(&out, outer), "{out}");
    check_session(&s).unwrap();
}

// ---------- 4. A theme and the shapes on locked layers ----------

/// The style links of shape `id` of page 1.
fn links(s: &Session, id: u32) -> (Option<u32>, Option<u32>, Option<u32>) {
    let l = shape_on(s, 1, id).unwrap().sheet.links;
    (l.line, l.fill, l.text)
}

/// A theme is the drawing's: its page cells are written, but no cell of a shape on a locked layer (a member of a group by its own list), and the answer counts the shapes it
/// passed over. Unlocked, they follow the theme too.
#[test]
fn a_theme_writes_no_cell_into_a_shape_on_a_locked_layer_and_counts_them() {
    let mut s = blank_session();
    let [a, b, m1, m2] = [(2.0, 3.0), (4.0, 3.0), (6.0, 3.0), (8.0, 3.0)].map(|(x, y)| drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [b, m2], "layers": ["Frozen"]}));
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [m1, m2]})));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let (b_was, m2_was) = (shape_on(&s, 1, b).unwrap(), shape_on(&s, 1, m2).unwrap());
    let out = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(out["locked"], json!(2), "{out}");
    assert_eq!(out["changed_pages"], json!([1]), "the theme is the page's: {out}");
    let style = links(&s, a).1;
    assert!(style.is_some_and(|id| id != 0), "a shape on no layer follows the theme");
    assert_eq!(links(&s, m1), (style, style, style), "a member on no layer follows it");
    assert_eq!(shape_on(&s, 1, b).unwrap().sheet.links, b_was.sheet.links, "no link is written into the shape on the locked layer");
    assert_eq!(shape_on(&s, 1, m2).unwrap().sheet.links, m2_was.sheet.links, "nor into the member on it");
    let _ = g;
    check_session(&s).unwrap();
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": false}));
    let out = run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    assert_eq!(out["locked"], json!(0), "{out}");
    assert_eq!(links(&s, b), (style, style, style), "unlocked, it follows the theme");
}

// ---------- 1 and 5. `.odg` layer flags ----------

/// The `draw:layer` start tags of the first layer set in `part` (`styles.xml` or `content.xml`), each as its markup is written.
fn layer_tags(part: &str) -> Vec<String> {
    let Some(start) = part.find("<draw:layer-set") else { return Vec::new() };
    let end = part[start..].find("</draw:layer-set>").map_or(part.len(), |e| start + e);
    part[start..end].split("<draw:layer ").skip(1).map(|piece| format!("<draw:layer {}", piece.split("/>").next().unwrap_or(""))).collect()
}

/// The start tag of the `draw:layer` named `name` among `tags`.
fn tag_named(tags: &[String], name: &str) -> String {
    tags.iter().find(|l| l.contains(&format!("draw:name=\"{name}\""))).cloned().unwrap_or_else(|| panic!("no layer {name}: {tags:?}"))
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

/// The package without DiaCraft's document file and its folder: what a program that drops DiaCraft's data reads.
fn without_diacraft(bytes: &[u8]) -> Vec<u8> {
    let parts = ["content.xml", "styles.xml", "meta.xml"].map(|name| (name.to_owned(), crate::odg_build::part_of(bytes, name).into_bytes()));
    crate::odg_build::package(crate::odg_build::DRAWING, parts.to_vec())
}

/// The layers of the drawings below: after LibreOffice's five, `Notes` (shown on paper only), `Pins` (written unprotected) and `Plain` (no flag written).
const LAYERS: &str = "<draw:layer draw:name=\"Notes\" draw:display=\"printer\"/><draw:layer draw:name=\"Pins\" draw:protected=\"false\"/><draw:layer draw:name=\"Plain\"/>";

/// A drawing of `pages` pages, each with a rectangle (`A`, `B`, `C`), whose document's layer set declares [`LAYERS`]; with `own`, the first page has a layer set of its own
/// that declares them too, and the document's set none of them.
fn layered(pages: usize, own: bool) -> Vec<u8> {
    let mut drawing = crate::odg_build::Drawing::new();
    for (k, name) in ["A", "B", "C"].iter().take(pages).enumerate() {
        drawing = drawing.page(&crate::odg_build::rect(name, (2.0 + k as f64, 2.0, 4.0, 2.0), ""));
    }
    let bytes = drawing.bytes();
    let mut styles = crate::odg_build::part_of(&bytes, "styles.xml");
    let mut content = crate::odg_build::part_of(&bytes, "content.xml");
    if own {
        let at = content.find("<draw:page ").unwrap();
        let open = at + content[at..].find('>').unwrap() + 1;
        content.insert_str(open, &format!("<draw:layer-set>{LAYERS}</draw:layer-set>"));
    } else {
        styles = styles.replace("</draw:layer-set>", &format!("{LAYERS}</draw:layer-set>"));
    }
    crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![
            ("content.xml".to_owned(), content.into_bytes()),
            ("styles.xml".to_owned(), styles.into_bytes()),
            ("meta.xml".to_owned(), crate::odg_build::part_of(&bytes, "meta.xml").into_bytes()),
        ],
    )
}

/// The layer tags of the document's set of a saved package.
fn doc_tags(bytes: &[u8]) -> Vec<String> {
    layer_tags(&crate::odg_build::part_of(bytes, "styles.xml"))
}

/// Opens `name` in `dir` again and checks that each page has the layers `want` says and that a second save writes the same bytes.
fn reopens_as(dir: &crate::odg_build::Dir, name: &str, pages: &[u32], want: &[Json]) {
    let bytes = std::fs::read(dir.path(name)).unwrap();
    let mut back = Session::new();
    run(&mut back, "file.open", json!({"path": dir.path(name)}));
    let got: Vec<Json> = pages.iter().map(|p| run(&mut back, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    assert_eq!(got, want, "each page has the layers it had");
    run(&mut back, "file.save", json!({}));
    assert_eq!(std::fs::read(dir.path(name)).unwrap(), bytes, "a second save writes nothing new");
}

/// The splice writer: the row the person changed decides a layer's flags, not the first page that has the layer. Page 2's `Notes` made visible is written though page 1
/// has `Notes` as the file had it (page 1 then keeps its own rows in DiaCraft's document file); `Pins` locked on page 1 is written; and of two pages that both changed `Plain`,
/// the later decides. Reopened, each page has its layers, and a second save writes the same bytes.
#[test]
fn an_odg_layers_flags_come_from_the_row_that_changed_and_of_two_from_the_later_page() {
    let dir = crate::odg_build::Dir::new();
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &layered(3, false));
    let pages: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    run(&mut s, "layer.set", json!({"layer": "Pins", "lock": true, "page": pages[0]}));
    run(&mut s, "layer.set", json!({"layer": "Notes", "visible": true, "page": pages[1]}));
    run(&mut s, "layer.set", json!({"layer": "Plain", "print": false, "page": pages[1]}));
    run(&mut s, "layer.set", json!({"layer": "Plain", "visible": false, "page": pages[2]}));
    let want: Vec<Json> = pages.iter().map(|p| run(&mut s, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    let tags = doc_tags(&bytes);
    assert!(tag_named(&tags, "Pins").contains("draw:protected=\"true\""), "page 1's change: {tags:?}");
    assert!(tag_named(&tags, "Notes").contains("draw:display=\"always\""), "page 2's change, though page 1 has Notes as it was: {tags:?}");
    assert!(tag_named(&tags, "Plain").contains("draw:display=\"printer\""), "of pages 2 and 3, which both changed Plain, page 3: {tags:?}");
    assert_eq!(tags.len(), 8, "no layer is added or lost: {tags:?}");
    // Another program reads the markup alone: every page has the set's flags.
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&bytes));
    assert_eq!(rows_of(&bare, 0), [("Notes".into(), 1.0, 1.0, 0.0), ("Pins".into(), 1.0, 1.0, 1.0), ("Plain".into(), 0.0, 1.0, 0.0)]);
    reopens_as(&dir, "a.odg", &pages, &want);
    // No row changed: the file's flags stay, bytes and all.
    let mut s = crate::odg_build::open_bytes_in(&dir, "b.odg", &layered(2, false));
    let a = crate::odg_build::id_of(s.document().unwrap(), 0, "A");
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.5, "dy": 0.0, "page": pages[0]}));
    run(&mut s, "file.save", json!({}));
    let tags = doc_tags(&std::fs::read(dir.path("b.odg")).unwrap());
    assert_eq!(tags, doc_tags(&layered(2, false)), "an edit that changes no layer leaves the set as the file had it");
}

/// The model writer: a drawing with no `.odg` file has no row that changed (no layer set says what the rows were), so the first page that has a layer decides its flags,
/// as before; a page whose rows differ keeps them in DiaCraft's document file. Reopened, each page has its layers.
#[test]
fn a_drawing_with_no_odg_file_gives_each_layer_the_first_pages_flags() {
    let dir = crate::odg_build::Dir::new();
    let mut s = blank_session();
    let second = run(&mut s, "page.add", json!({"name": "Second"}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "layer.add", json!({"name": "Notes", "print": false, "page": 1}));
    run(&mut s, "layer.add", json!({"name": "Notes", "visible": false, "lock": true, "page": second}));
    let want: Vec<Json> = [1, second].iter().map(|p| run(&mut s, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    run(&mut s, "file.save_as", json!({"path": dir.path("m.odg")}));
    let bytes = std::fs::read(dir.path("m.odg")).unwrap();
    let notes = tag_named(&doc_tags(&bytes), "Notes");
    assert!(notes.contains("draw:display=\"screen\"") && !notes.contains("protected"), "the first page's flags: {notes}");
    reopens_as(&dir, "m.odg", &[1, second], &want);
}

/// Gap 5: a page with a layer set of its own (a `draw:layer-set` in its `draw:page`) has a changed `Visible`, `Print` or `Lock` written in that set, in place or added, as the
/// document's set has; the document's set is left as it was. Another program reads the new flags from the markup alone; reopened, the page has its layers, and a
/// second save writes the same bytes.
#[test]
fn a_pages_own_layer_set_has_its_new_flags_written() {
    let dir = crate::odg_build::Dir::new();
    let file = layered(2, true);
    let mut s = crate::odg_build::open_bytes_in(&dir, "a.odg", &file);
    let pages: Vec<u32> = s.document().unwrap().pages.iter().map(|p| p.id).collect();
    assert_eq!(rows_of(s.document().unwrap(), 0), [("Notes".into(), 0.0, 1.0, 0.0), ("Pins".into(), 1.0, 1.0, 0.0), ("Plain".into(), 1.0, 1.0, 0.0)]);
    run(&mut s, "layer.set", json!({"layer": "Notes", "visible": true, "page": pages[0]}));
    run(&mut s, "layer.set", json!({"layer": "Pins", "lock": true, "page": pages[0]}));
    run(&mut s, "layer.set", json!({"layer": "Plain", "print": false, "visible": false, "page": pages[0]}));
    let want: Vec<Json> = pages.iter().map(|p| run(&mut s, "layer.list", json!({"page": p}))["layers"].clone()).collect();
    run(&mut s, "file.save", json!({}));
    let bytes = std::fs::read(dir.path("a.odg")).unwrap();
    let own = layer_tags(&crate::odg_build::content_of(&bytes));
    assert!(tag_named(&own, "Notes").contains("draw:display=\"always\""), "{own:?}");
    assert!(tag_named(&own, "Pins").contains("draw:protected=\"true\"") && !tag_named(&own, "Pins").contains("display"), "{own:?}");
    assert!(tag_named(&own, "Plain").contains("draw:display=\"none\"") && !tag_named(&own, "Plain").contains("protected"), "{own:?}");
    assert_eq!(own.len(), 3, "no layer is added or lost: {own:?}");
    assert_eq!(doc_tags(&bytes), doc_tags(&file), "the document's set is left as it was");
    let (bare, _) = crate::odg_build::read_back(&without_diacraft(&bytes));
    assert_eq!(rows_of(&bare, 0), [("Notes".into(), 1.0, 1.0, 0.0), ("Pins".into(), 1.0, 1.0, 1.0), ("Plain".into(), 0.0, 0.0, 0.0)]);
    reopens_as(&dir, "a.odg", &pages, &want);
}
