//! A page's layers and the shapes' lists **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model
//! writers) and from a drawing read from a file and saved in its own format (the splice). A VSDX file holds the `Layer` section of the page and the `LayerMember` cells; an
//! `.odg` holds the layer set (`draw:layer-set`: `draw:name`, `draw:display`, `draw:protected`) and what it cannot say in DiaCraft's document file.

use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::layers::{listed, member_text};
use crate::odg_build::{Dir, Drawing, part_of};
use crate::{blank_session, run};

/// Opens the file `path` in a session of its own.
fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// A drawing of three shapes and three layers: `Notes` (hidden on screen, red, half clear, never snapped to), `Guides` (locked, not glued to, not printed) and `Live`
/// (active). The first shape is on `Notes`, the second on `Guides` and `Live`, the third on none. → the shapes.
fn made() -> (Session, [u32; 3]) {
    let mut s = blank_session();
    let ids = [(2.0, 3.0), (4.0, 3.0), (6.0, 3.0)].map(|(x, y)| crate::edit::drop_shape(&mut s, "rectangle", x, y));
    run(&mut s, "layer.add", json!({"name": "Notes", "visible": false, "color": "#ff0000", "transparency": 0.5, "snap": false}));
    run(&mut s, "layer.add", json!({"name": "Guides", "glue": false, "print": false}));
    run(&mut s, "layer.add", json!({"name": "Live", "active": true}));
    run(&mut s, "layer.assign", json!({"ids": [ids[0]], "layers": ["Notes"]}));
    run(&mut s, "layer.assign", json!({"ids": [ids[1]], "layers": [1, 2]}));
    // A locked layer takes no new shapes, so it is locked last.
    run(&mut s, "layer.set", json!({"layer": "Guides", "lock": true}));
    (s, ids)
}

/// The page's layers and the lists of the three shapes, as one value to compare.
fn state(s: &mut Session, ids: &[u32; 3]) -> Json {
    let lists: Vec<Json> = ids.iter().map(|id| json!(member_text(s.document().unwrap(), *id))).collect();
    json!({"layers": listed(s), "lists": lists})
}

/// The shapes' `ID`s in the first page of a session, in drawing order.
fn shapes_of(s: &Session) -> [u32; 3] {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    let ids: Vec<u32> = page.shapes.iter().map(|sh| sh.id).collect();
    [ids[0], ids[1], ids[2]]
}

#[test]
fn layers_and_the_lists_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut s, ids) = made();
        let want = state(&mut s, &ids);
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let at = shapes_of(&back);
        let got = state(&mut back, &at);
        assert_eq!(got["lists"], want["lists"], "{ext}: the lists");
        assert_eq!(got["layers"], want["layers"], "{ext}: the layers");
    }
}

#[test]
fn a_drawing_read_from_a_file_and_edited_keeps_its_layers_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut first, _) = made();
        let dir = Dir::new();
        let path = dir.path(&format!("source.{ext}"));
        run(&mut first, "file.save_as", json!({ "path": path }));
        // Read from the file: a layer is added, one is changed, a shape is put on the new layer, and the drawing is saved in place.
        let mut s = reopened(&path);
        let ids = shapes_of(&s);
        run(&mut s, "layer.add", json!({"name": "Extra", "color": "#00ff7f"}));
        run(&mut s, "layer.set", json!({"layer": "Notes", "visible": true, "print": false}));
        run(&mut s, "layer.assign", json!({"ids": [ids[2]], "layers": ["Extra"]}));
        let want = state(&mut s, &ids);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        let at = shapes_of(&back);
        let got = state(&mut back, &at);
        assert_eq!(got["layers"], want["layers"], "{ext}: the layers after an edit in place");
        assert_eq!(got["lists"], want["lists"], "{ext}: the lists after an edit in place");
        // And a layer removed in a later edit: the numbers close up in the file too.
        let mut s = reopened(&path);
        run(&mut s, "layer.delete", json!({"layer": "Notes"}));
        let at = shapes_of(&s);
        let want = state(&mut s, &at);
        run(&mut s, "file.save", json!({}));
        let mut back = reopened(&path);
        let at = shapes_of(&back);
        let got = state(&mut back, &at);
        assert_eq!(got, want, "{ext}: a layer removed and saved in place");
    }
}

/// The `draw:layer` elements of the `draw:layer-set` of a package's `styles.xml`, each as the markup is written.
fn layer_set(bytes: &[u8]) -> Vec<String> {
    let styles = part_of(bytes, "styles.xml");
    let Some(start) = styles.find("<draw:layer-set") else { return Vec::new() };
    let end = styles[start..].find("</draw:layer-set>").map(|e| start + e).unwrap_or(styles.len());
    styles[start..end].split("<draw:layer ").skip(1).map(|piece| format!("<draw:layer {}", piece.split("/>").next().unwrap_or(""))).collect()
}

#[test]
fn an_odg_declares_the_layers_a_drawing_has_in_its_layer_set_and_puts_a_new_shape_on_the_first() {
    let (mut s, ids) = made();
    let dir = Dir::new();
    let path = dir.path("layers.odg");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let bytes = std::fs::read(&path).unwrap();
    let set = layer_set(&bytes);
    let named = |name: &str| {
        set.iter().find(|l| l.contains(&format!("draw:name=\"{name}\""))).cloned().unwrap_or_else(|| panic!("{name} is not declared: {set:?}"))
    };
    // The five layers of the template stay; each layer of the page is added after them, with what the format can say of it (`draw:display` from `Visible` and
    // `Print`, `draw:protected` from `Lock`).
    for five in ["layout", "background", "backgroundobjects", "controls", "measurelines"] {
        named(five);
    }
    assert!(named("Notes").contains("draw:display=\"printer\""), "{}", named("Notes"));
    assert!(named("Guides").contains("draw:protected=\"true\"") && named("Guides").contains("draw:display=\"screen\""), "{}", named("Guides"));
    assert!(!named("Live").contains("draw:display=\"none\""), "{}", named("Live"));
    // The shape on `Notes` is written on it, the one on two layers on the first of them, and the one on none on `layout`.
    let content = part_of(&bytes, "content.xml");
    let layer_of = |n: usize| -> String {
        let at = content.match_indices("<draw:custom-shape").nth(n).map(|(i, _)| i).unwrap_or_else(|| panic!("no shape {n} in {content}"));
        let tag = &content[at..content[at..].find('>').map(|e| at + e).unwrap_or(content.len())];
        tag.split("draw:layer=\"").nth(1).and_then(|t| t.split('"').next()).unwrap_or("").to_owned()
    };
    assert_eq!([layer_of(0), layer_of(1), layer_of(2)], ["Notes", "Guides", "layout"], "ids {ids:?}");
}

#[test]
fn an_odg_with_no_layer_set_is_not_given_one_by_an_edit_that_uses_no_layer() {
    // A file whose styles hold no `draw:layer-set` (a producer may leave it out: the layer set is optional in the standard).
    let drawing = Drawing::new().page(&crate::odg_build::rect("A", (1.0, 1.0, 3.0, 2.0), ""));
    let bytes = drawing.bytes();
    let stripped = {
        let styles = part_of(&bytes, "styles.xml");
        let start = styles.find("<draw:layer-set>").unwrap();
        let end = styles.find("</draw:layer-set>").unwrap() + "</draw:layer-set>".len();
        let without = format!("{}{}", &styles[..start], &styles[end..]);
        crate::odg_build::package(
            "application/vnd.oasis.opendocument.graphics",
            vec![
                ("content.xml".to_owned(), part_of(&bytes, "content.xml").into_bytes()),
                ("styles.xml".to_owned(), without.into_bytes()),
                ("meta.xml".to_owned(), part_of(&bytes, "meta.xml").into_bytes()),
            ],
        )
    };
    assert!(!part_of(&stripped, "styles.xml").contains("draw:layer-set"), "the fixture has none");
    let dir = Dir::new();
    let path = dir.write("none.odg", &stripped);
    let mut s = reopened(&path);
    let id = shapes_of_first(&s);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.5}));
    run(&mut s, "file.save", json!({}));
    let after = std::fs::read(&path).unwrap();
    assert!(!part_of(&after, "styles.xml").contains("draw:layer-set"), "an edit that uses no layer adds none: {}", part_of(&after, "styles.xml"));
    // Opened and saved again as a new `.odg` (the model writer), a drawing that has no layer adds none beyond what the package is made with.
    let mut s = reopened(&path);
    let other = dir.path("again.odg");
    run(&mut s, "file.save_as", json!({ "path": other }));
    let again = std::fs::read(&other).unwrap();
    assert!(layer_set(&again).is_empty(), "a drawing that has no layer set is not given one: {:?}", layer_set(&again));
}

fn shapes_of_first(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().shapes.get(0).unwrap().id
}
