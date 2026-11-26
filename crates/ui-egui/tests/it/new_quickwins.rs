//! File > New: the Templates and Sample Diagrams headings and the tiles of the engine's templates.
//!
//! The window is driven by the pointer as a person would, and what is checked is the drawing a tile makes and where the tiles lie.

use egui::Rect;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::edit_window;
use crate::stencil_files::rows;
use crate::support::{Rest, Window, click, press, run_command};

const WIDE: [f32; 2] = [1280.0, 900.0];
/// The templates the engine lists: the four of the first batches, and the ones of the second stencil batch as their stencils ship (STENCIL2).
fn templates() -> Vec<&'static str> {
    diacraft_engine::template::templates().iter().map(|t| t.id).collect()
}

/// A press on a template's tile, which opens its detail, and a press on Create there ("picking a tile opens a dialog ... and a large Create button").
fn create_from(w: &mut Window, id: &str) {
    press(w, &format!("backstage.tile.template.{id}"));
    w.rest();
    press(w, "backstage.detail.create");
}

fn new_page(size: [f32; 2]) -> Window {
    let mut w = edit_window(size);
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    w
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

/// A tile for each template of `template.list`, with the Blank drawing first, each named for the template; and the headings of the page.
#[test]
fn new_lists_a_tile_for_each_template_of_the_engine() {
    let mut w = new_page(WIDE);
    let listed = w.state_mut().execute("template.list", &json!({})).unwrap();
    let ids: Vec<&str> = listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, templates(), "the engine's templates");
    assert_eq!(&ids[..4], ["basic_diagram", "basic_flowchart", "cross_functional_flowchart", "organization_chart"]);
    assert!(w.state().element("backstage.tile.blank").unwrap().enabled);
    for t in listed["templates"].as_array().unwrap() {
        let tile = w.state().element(&format!("backstage.tile.template.{}", t["id"].as_str().unwrap())).unwrap_or_else(|| panic!("no tile for {t}"));
        assert!(tile.enabled);
        // The tile is a button named for the template, and its tooltip is the description.
        w.get_by_label(t["name"].as_str().unwrap());
    }
    // The blank tile comes first, then the templates in the engine's order, on one row in a wide window.
    let x = |n: &str| rect_of(&w, n).min.x;
    assert!(
        x("backstage.tile.blank") < x("backstage.tile.template.basic_diagram")
            && x("backstage.tile.template.basic_diagram") < x("backstage.tile.template.basic_flowchart")
    );
    // The headings are the ones the window layout names, Templates being the one on view.
    for (heading, enabled) in [("Personal", false), ("Templates", true), ("Sample Diagrams", true)] {
        assert_eq!(w.state().element(&format!("backstage.heading.{heading}")).unwrap().enabled, enabled, "{heading}");
    }
}

/// A tile makes a new drawing from its template: the page, the theme and the stencils the template names, in a drawing named for the window's next, and the File
/// view closes.
#[test]
fn a_template_tile_makes_a_new_drawing_from_the_template() {
    let mut w = new_page(WIDE);
    assert_eq!(w.state().session().stencils().open_builtins().len(), 7, "a blank drawing has all seven stencils");
    create_from(&mut w, "basic_flowchart");
    w.rest();
    assert!(w.state().window_state()["backstage"].is_null(), "the File view closes behind the new drawing");
    assert_eq!(w.state().window_title(), "Drawing2 - DiaCraft");
    // Basic Flowchart: portrait, the theme Fernwood, the flowchart shapes first.
    let setup = w.state_mut().execute("page.setup", &json!({})).unwrap();
    assert_eq!(setup["orientation"], "portrait", "{setup}");
    let listed = w.state_mut().execute("theme.list", &json!({})).unwrap();
    assert_eq!(listed["page"]["theme"], "fernwood", "{listed}");
    assert_eq!(w.state().session().stencils().open_builtins(), ["flowchart", "basic"], "the template opens its stencils");
    assert_eq!(rows(&mut w), ["Basic Flowchart", "Basic Shapes"], "and the Shapes window lists them");
    // Basic Diagram from a drawing with no changes: no question, landscape and no theme.
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    create_from(&mut w, "basic_diagram");
    w.rest();
    assert!(!w.state().prompt_open());
    assert_eq!(w.state().window_title(), "Drawing3 - DiaCraft");
    let listed = w.state_mut().execute("theme.list", &json!({})).unwrap();
    assert_eq!(listed["page"]["theme"], "none");
    assert_eq!(w.state_mut().execute("page.setup", &json!({})).unwrap()["orientation"], "landscape");
    assert_eq!(rows(&mut w), ["Basic Shapes", "Arrow Shapes"]);
    assert_eq!(w.state().ui_panics(), 0);
}

/// A drawing with changes that are not saved is asked about first, as Ctrl+N asks; Cancel leaves everything as it was and Don't Save goes on to the template.
#[test]
fn a_template_tile_asks_about_unsaved_changes_first() {
    let mut w = new_page(WIDE);
    w.state_mut().execute("page.add", &json!({})).unwrap();
    w.rest();
    create_from(&mut w, "basic_diagram");
    assert!(w.state().prompt_open(), "the drawing has a change that is not saved");
    assert_eq!(w.state().window_state()["backstage"], "New", "and nothing has happened yet");
    w.get_by_label("Cancel").click();
    w.rest();
    assert!(!w.state().prompt_open());
    assert_eq!(w.state().window_title(), "*Drawing1 - DiaCraft", "still the same drawing, with its change");
    press(&mut w, "backstage.detail.create");
    w.get_by_label("Don't Save").click();
    w.rest();
    assert!(w.state().window_state()["backstage"].is_null());
    assert_eq!(w.state().window_title(), "Drawing2 - DiaCraft");
    assert_eq!(rows(&mut w), ["Basic Shapes", "Arrow Shapes"], "the template's own stencils");
}

/// Sample Diagrams is a heading that can be opened, and says there are none: the engine has no samples. Personal has nothing behind it and is quiet.
#[test]
fn sample_diagrams_says_there_are_none_and_personal_is_quiet() {
    let mut w = new_page(WIDE);
    assert!(w.state().element("backstage.tile.blank").is_some());
    let at = rect_of(&w, "backstage.heading.Sample Diagrams").center();
    click(&mut w, at);
    w.rest();
    w.get_by_label("No sample diagrams are built yet.");
    assert!(w.state().element("backstage.tile.blank").is_none(), "the Templates tiles are not on this heading");
    // Personal does nothing.
    let at = rect_of(&w, "backstage.heading.Personal").center();
    click(&mut w, at);
    w.rest();
    w.get_by_label("No sample diagrams are built yet.");
    // And Templates brings the tiles back.
    let at = rect_of(&w, "backstage.heading.Templates").center();
    click(&mut w, at);
    w.rest();
    assert!(w.state().element("backstage.tile.template.basic_diagram").is_some());
}

/// The tiles and the headings wrap to the width: in a wide window the tiles are on one row, in a narrow one they stack, and nothing lies outside the window or over another tile.
#[test]
fn the_tiles_wrap_to_the_window_and_never_overlap() {
    for [width, height] in [[1280.0, 800.0], [700.0, 600.0], [480.0, 480.0], [320.0, 480.0]] {
        let w = new_page([width, height]);
        let names = ["backstage.tile.blank", "backstage.tile.template.basic_diagram", "backstage.tile.template.basic_flowchart"];
        let tiles: Vec<Rect> = names.iter().map(|n| rect_of(&w, n)).collect();
        // The headings are inside the window too, wrapped to a second row where one row is too long.
        for heading in ["Personal", "Templates", "Sample Diagrams"] {
            let r = rect_of(&w, &format!("backstage.heading.{heading}"));
            assert!(r.right() <= width + 0.5, "the heading {heading} runs off a {width} wide window: {r:?}");
        }
        for (n, t) in names.iter().zip(&tiles) {
            assert!(t.right() <= width + 0.5, "{n} runs off a {width} wide window: {t:?}");
        }
        for (i, a) in tiles.iter().enumerate() {
            for b in tiles.iter().skip(i + 1) {
                assert!(!a.shrink(0.5).intersects(*b), "tiles overlap at {width}: {a:?} {b:?}");
            }
        }
        if width >= 1000.0 {
            assert!(tiles.iter().all(|t| (t.top() - tiles[0].top()).abs() < 0.5), "one row when the window is wide: {tiles:?}");
        }
        if width <= 480.0 {
            assert!(tiles[1].top() > tiles[0].top() || tiles[1].left() > tiles[0].right() - 1.0, "wraps when the window is narrow: {tiles:?}");
        }
    }
}

// ---- the detail of a template -------------------------------------------------------------------------------------------------

/// "choosing a template on the File, New view and pressing Create gives a new drawing
/// set up for that kind of diagram". A press on a template's tile opens its detail: the name, the category, the description, a picture of its page and a Create button, and the
/// gallery steps back; Close (and Escape) go back to the gallery with nothing made; Create asks about unsaved changes as Ctrl+N does and then makes the drawing. **No Metric or US
/// choice** (the engine has no unit choice at the start of a drawing: P1 "Choose metric or US units when starting").
#[test]
fn a_template_tile_opens_its_detail_and_create_makes_the_drawing() {
    let mut w = new_page(WIDE);
    let title = w.state().window_title();
    press(&mut w, "backstage.tile.template.basic_flowchart");
    w.rest();
    assert_eq!(w.state().window_title(), title, "a press on a tile makes nothing yet");
    assert_eq!(w.state().window_state()["backstage"], "New");
    for name in ["backstage.detail", "backstage.detail.create", "backstage.detail.close"] {
        assert!(w.state().element(name).is_some_and(|e| e.enabled), "{name} is drawn and works");
    }
    assert!(w.state().element("backstage.tile.template.basic_diagram").is_none(), "the gallery steps back while the detail is open");
    // The template's name, its category and its description are on the page.
    w.get_by_label("Basic Flowchart");
    w.get_by_label("Flowchart");
    let described = w.state_mut().execute("template.list", &json!({})).unwrap()["templates"][1]["description"].as_str().unwrap().to_owned();
    assert!(!described.is_empty());
    w.get_by_label(described.as_str());
    // Close is back to the gallery, with nothing made.
    press(&mut w, "backstage.detail.close");
    w.rest();
    assert!(w.state().element("backstage.detail").is_none() && w.state().element("backstage.tile.template.basic_diagram").is_some());
    assert_eq!(w.state().window_title(), title);
    // Escape does the same, and a second Escape leaves the File view.
    press(&mut w, "backstage.tile.template.basic_diagram");
    w.rest();
    crate::editing::key(&mut w, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(w.state().element("backstage.detail").is_none() && w.state().window_state()["backstage"] == "New", "Escape closes the detail first");
    crate::editing::key(&mut w, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(w.state().window_state()["backstage"].is_null(), "and then the File view");
    // The File view opened again shows the gallery, not the detail left behind.
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    press(&mut w, "backstage.tile.template.basic_flowchart");
    w.rest();
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    run_command(&mut w, "window.backstage", json!({"page": "New"}));
    assert!(w.state().element("backstage.detail").is_none(), "the detail does not wait for the next time");
    // Create makes the drawing.
    create_from(&mut w, "basic_flowchart");
    w.rest();
    assert!(w.state().window_state()["backstage"].is_null());
    assert_eq!(w.state_mut().execute("theme.list", &json!({})).unwrap()["page"]["theme"], "fernwood");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The detail fits a narrow window and a wide one: the picture, the words and the two buttons are inside the window and none lies over another.
#[test]
fn the_detail_of_a_template_fits_narrow_and_wide_windows() {
    for [width, height] in [[1280.0, 800.0], [700.0, 600.0], [480.0, 480.0], [320.0, 480.0]] {
        let mut w = new_page([width, height]);
        // The gallery's tile may be scrolled out of a short window, so the detail is opened by its command (`window.templateDetail`).
        run_command(&mut w, "window.templateDetail", json!({"template": "basic_flowchart"}));
        w.rest();
        let names = ["backstage.detail.picture", "backstage.detail.create", "backstage.detail.close"];
        let rects: Vec<Rect> = names.iter().map(|n| rect_of(&w, n)).collect();
        for (n, r) in names.iter().zip(&rects) {
            assert!(r.left() >= 0.0 && r.right() <= width + 0.5, "{n} runs off the side of a {width} wide window: {r:?}");
            // A window too short for the picture, the words and the buttons scrolls (the page has a scroll area); a tall enough one shows all of it.
            if height >= 600.0 {
                assert!(r.bottom() <= height + 0.5, "{n} is below a {width} by {height} window: {r:?}");
            }
        }
        for (i, a) in rects.iter().enumerate() {
            for b in rects.iter().skip(i + 1) {
                assert!(!a.shrink(0.5).intersects(*b), "{a:?} overlaps {b:?} at {width}");
            }
        }
    }
}

/// `window.templateDetail` opens the File view on the detail of a template an id of `template.list` names (in any case) and `close` goes back to the gallery; a template there
/// is none of is refused in words and the window is as it was.
#[test]
fn the_template_detail_command_opens_and_closes_the_detail_and_refuses_what_it_does_not_know() {
    let mut w = edit_window(WIDE);
    run_command(&mut w, "window.templateDetail", json!({"template": "BASIC_DIAGRAM"}));
    w.rest();
    assert_eq!(w.state().window_state()["backstage"], "New");
    assert!(w.state().element("backstage.detail.create").is_some());
    run_command(&mut w, "window.templateDetail", json!({"close": true}));
    w.rest();
    assert!(w.state().element("backstage.detail").is_none() && w.state().element("backstage.tile.template.basic_diagram").is_some());
    let e = w.state_mut().execute("window.templateDetail", &json!({"template": "nope"})).unwrap_err().to_string();
    assert!(e.contains("no template `nope`") && e.contains("basic_diagram"), "{e}");
    let e = w.state_mut().execute("window.templateDetail", &json!({})).unwrap_err().to_string();
    assert!(e.contains("give `template`"), "{e}");
    assert!(w.state_mut().execute("window.templateDetail", &json!({"templat": "basic_diagram"})).is_err());
}
