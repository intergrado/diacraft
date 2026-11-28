//! Characters no loaded face has.
//!
//! **What was found.** The window does not panic on a character the bundled faces lack. egui lays such a character out as a box (the `.notdef` glyph of the family's first
//! face, `MissingGlyphPolicy::Tofu`), and Tofu is egui's own default, which eframe keeps on the desktop (the real window was driven with the control channel, typing and
//! reading these texts: no panic) and, by its source, in the browser, which these tests have no way to run; nothing of ours sets the other policy. The panic belongs to the test harness: `egui_kittest` builds its context with `MissingGlyphPolicy::Panic` so that a label of ours with a character no face
//! draws fails a test. That is a good net for our own words (it found the arrow of a notice, which a person with none of the open sans faces on the machine would have seen as a box) and a wrong one for a person's:
//! text typed, pasted or read from a file in any script. So a test that feeds the window such text lays it out as shipped ([`as_shipped`]), and these tests do that for
//! every kind of box the window has and every panel that shows what a file holds.
//!
//! **What was mended.** The proportional faces ended at Ubuntu Light, which has no arrows, so a notice that said "1 → 3" shows a box for the arrow where no machine face fills
//! in. Hack, which the window carries already for its monospace text and has the arrows, the box-drawing and the block characters, now stands behind it in every proportional chain.

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::fonts::{font_definitions, strong_family};
use egui::{FontFamily, FontId, Key, MissingGlyphPolicy, Modifiers};
use serde_json::json;

use crate::support::{Rest, Window, as_shipped, shipped_window, window};

/// What a person may type, paste or find in a file, named by what makes each hard for a face or for the text code under it: scripts the bundled faces lack, characters
/// beyond the first plane, emoji that join and combine, marks with no base, noncharacters, controls, and text that runs the other way.
pub fn hostile() -> Vec<(&'static str, String)> {
    vec![
        ("an arrow", "(1 \u{2192} 3)".to_owned()),
        (
            "emoji, a family joined, a flag, a keycap and a skin tone",
            "\u{1F600} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} \u{1F1E8}\u{1F1ED} 1\u{FE0F}\u{20E3} \u{1F44D}\u{1F3FD}"
                .to_owned(),
        ),
        ("Han, kana and Hangul", "\u{6F22}\u{5B57}\u{304B}\u{306A}\u{D55C}\u{AE00}".to_owned()),
        ("Arabic among Latin and digits", "abc \u{645}\u{631}\u{62D}\u{628}\u{627} 123".to_owned()),
        ("Hebrew", "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D0}\u{5B8}".to_owned()),
        ("Thai and Devanagari", "\u{E2A}\u{E27}\u{E31}\u{E2A}\u{E14}\u{E35} \u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947}".to_owned()),
        ("the planes beyond the first", "\u{1D11E} \u{10348} \u{20000} \u{E0001} \u{10FFFF}".to_owned()),
        ("private use and noncharacters", "\u{E000}\u{F8FF}\u{FDD0}\u{FFFE}\u{FFFF}\u{FFFD}".to_owned()),
        ("marks with no base", "\u{301}\u{302}\u{200D}\u{20E3}".to_owned()),
        ("one letter under 300 marks", format!("a{}", "\u{301}".repeat(300))),
        ("controls and format characters", "\u{1}\u{7F}\u{85}\u{200B}\u{202E}\u{2066}\u{FEFF}".to_owned()),
        ("a tab and line breaks", "a\tb\nc\r\nd".to_owned()),
        ("text that turns round in the middle", "abc \u{202E}def\u{202C} \u{5D0}\u{5D1}\u{5D2} 12".to_owned()),
    ]
}

fn has_glyph(window: &Window, family: FontFamily, c: char) -> bool {
    window.ctx.fonts_mut(|fonts| fonts.has_glyph(&FontId::new(14.0, family), c))
}

/// A window on a new drawing, at rest: strict (as the harness makes it) or as shipped.
fn drawing(size: [f32; 2], shipped: bool) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer());
    let mut w = if shipped { shipped_window(app, size) } else { window(app, size) };
    w.rest();
    w
}

// ---- what the window draws ---------------------------------------------------------------------------------------------------------------

/// A notice that says "1 → 3" (the status bar's words after copying a shape three times) needs the arrow in the proportional text: the harness made the first test that
/// held it fail with "No glyph for '\u{2192}'", and the shipped window would have shown a box.
#[test]
fn every_family_of_the_window_draws_the_arrows_a_notice_uses() {
    let mut w = drawing([800.0, 500.0], false);
    w.step();
    for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
        for arrow in "\u{2190}\u{2191}\u{2192}\u{2193}\u{2194}\u{2195}".chars() {
            assert!(has_glyph(&w, family.clone(), arrow), "{family:?} has no glyph for U+{:04X}", arrow as u32);
        }
    }
}

/// The notice itself, in the strict window of a test: an arrow in the status bar is drawn, the frame is not cut short by a panic the window then reports in its own
/// words, and the notice stays the notice that was set.
#[test]
fn a_notice_with_an_arrow_is_drawn_and_stays_the_notice() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let text = "Copied 1 \u{2192} 3 (the shape was made three times)";
    let mut w = window(
        DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()).with_notice(text),
        [900.0, 500.0],
    );
    w.rest();
    assert_eq!(w.state().ui_panics(), 0, "the frame was cut short: {:?}", w.state().notice());
    assert_eq!(w.state().notice(), Some(text));
}

// ---- where the panic comes from ----------------------------------------------------------------------------------------------------------

/// The window's own context is egui's default, which eframe makes for the desktop window and for the browser's: a character no face has is a box. If an upgrade of egui
/// changed that default, the browser's window, which cannot unwind, would abort on the first emoji a person typed: this test is the guard for that, and it lays out every
/// kind of hostile text in each family at several sizes.
#[test]
fn a_plain_context_draws_a_box_for_a_character_no_face_has_and_never_panics() {
    let ctx = egui::Context::default();
    assert_eq!(ctx.missing_glyph_policy(), MissingGlyphPolicy::Tofu, "egui's default is no longer the one eframe's windows rely on");
    ctx.set_fonts(font_definitions());
    let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
        for (what, text) in hostile() {
            for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
                for size in [0.5_f32, 9.0, 14.0, 96.0] {
                    let id = FontId::new(size, family.clone());
                    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ctx.fonts_mut(|f| {
                            f.layout_no_wrap(text.clone(), id.clone(), Default::default());
                            f.layout(text.clone(), id.clone(), Default::default(), 60.0);
                        });
                    }));
                    assert!(drawn.is_ok(), "{what} in {family:?} at {size}");
                }
            }
        }
    });
    out.textures_delta.clear();
}

/// The replacement exists and is used: with the bundled faces alone (the browser's, and a desktop machine with no open sans face), a character none of them has comes out as a glyph
/// with a width and ink in the atlas, the `.notdef` box of the family's first face, and not as nothing; so a letter that is missing is seen to be missing in every family.
#[test]
fn a_character_no_bundled_face_has_is_drawn_as_a_box_with_a_width_and_ink() {
    let ctx = egui::Context::default();
    ctx.set_fonts(font_definitions());
    let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
        for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
            for missing in ['\u{6F22}', '\u{645}', '\u{1F600}', '\u{1D11E}', '\u{10FFFF}'] {
                assert!(
                    !ctx.fonts_mut(|f| f.has_glyph(&FontId::new(14.0, family.clone()), missing)),
                    "a bundled face has U+{:04X}: pick a letter none has",
                    missing as u32
                );
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(missing.to_string(), FontId::new(14.0, family.clone()), Default::default()));
                let glyphs: Vec<_> = galley.rows.iter().flat_map(|row| row.row.glyphs.iter()).collect();
                assert_eq!(glyphs.len(), 1, "{family:?} U+{:04X}", missing as u32);
                assert!(glyphs[0].advance_width > 1.0, "{family:?} U+{:04X} has no width: {}", missing as u32, glyphs[0].advance_width);
                assert!(!glyphs[0].uv_rect.is_nothing(), "{family:?} U+{:04X} is drawn as nothing", missing as u32);
            }
        }
    });
    out.textures_delta.clear();
}

/// The panic is the harness's: the same layout under the policy `egui_kittest` turns on panics, naming the character, and under the shipped one does not. This is the
/// whole of the difference between a test window and the app, and the reason [`as_shipped`] exists.
#[test]
fn the_panic_is_the_strict_harness_and_not_the_window() {
    let lays_out = |policy: MissingGlyphPolicy| {
        let ctx = egui::Context::default();
        ctx.set_fonts(font_definitions());
        ctx.set_missing_glyph_policy(policy);
        let mut said = None;
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
            let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ctx.fonts_mut(|f| f.layout_no_wrap("\u{6F22}".to_owned(), FontId::proportional(14.0), Default::default()));
            }));
            if let Err(payload) = drawn {
                said = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()));
            }
        });
        out.textures_delta.clear();
        said
    };
    let strict = lays_out(MissingGlyphPolicy::Panic).expect("the strict policy panics for a Han letter, which no bundled face has");
    assert!(strict.contains("No glyph") && strict.contains("U+6F22"), "{strict}");
    assert_eq!(lays_out(MissingGlyphPolicy::Tofu), None);
}

/// A notice can hold a person's text (an error that names the file or the layer it is about), so the status bar draws any of them as shipped.
#[test]
fn a_notice_of_any_script_is_drawn_by_a_window_laid_out_as_shipped() {
    for (what, text) in hostile() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let app = DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()).with_notice(&text);
        let mut w = shipped_window(app, [900.0, 500.0]);
        w.rest();
        quiet(&w, what);
    }
}

// ---- what a person types ---------------------------------------------------------------------------------------------------------------

/// The window carried on: no frame was cut short by a panic (the window's guard counts those and says them in the status bar), and what it says is not one.
fn quiet(w: &Window, what: &str) {
    assert_eq!(w.state().ui_panics(), 0, "{what}: the window caught a panic: {:?}", w.state().notice());
    assert!(!w.state().notice().is_some_and(|n| n.contains("internal error")), "{what}: {:?}", w.state().notice());
}

/// The caret moved, a selection made and taken out, across what was typed: by character, by word, to the ends, and the whole of it at once.
fn walk(w: &mut Window, what: &str) {
    let shift = Modifiers::SHIFT;
    let word = Modifiers::CTRL;
    for (k, m) in [
        (Key::Home, Modifiers::NONE),
        (Key::ArrowRight, Modifiers::NONE),
        (Key::ArrowRight, Modifiers::NONE),
        (Key::ArrowRight, word),
        (Key::End, Modifiers::NONE),
        (Key::ArrowLeft, Modifiers::NONE),
        (Key::ArrowLeft, word),
        (Key::ArrowLeft, shift),
        (Key::Backspace, Modifiers::NONE),
        (Key::Delete, Modifiers::NONE),
        (Key::ArrowRight, shift),
        (Key::Delete, Modifiers::NONE),
        (Key::Backspace, word),
        (Key::A, crate::saving::ctrl()),
    ] {
        crate::editing::key(w, k, m);
        quiet(w, what);
    }
}

/// A click in the box named `name`, and the whole of what it held selected, so that the text typed next replaces it.
fn focus(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    crate::support::click(w, at);
    w.rest();
    crate::editing::key(w, Key::A, crate::saving::ctrl());
}

/// What an editor gives back: the window, and the text the box (or the drawing, once the box has sent it) held after the typing; empty for a run that walked the caret over the text.
type Held = (Window, String);

/// One kind of box. It opens its window as shipped, types `text` as a keyboard sends it, and then either leaves the box and reads back what it held (`walked` false: the text typed
/// is the text kept; a box that has no drawing to send it to is read before it is left, `read_first`) or walks the caret over the text, takes some out and leaves the box.
fn typed_into(
    box_name: &str,
    w: &mut Window,
    text: &str,
    (walked, read_first): (bool, bool),
    leave: impl FnOnce(&mut Window),
    held: impl FnOnce(&mut Window) -> String,
) -> String {
    crate::editing::type_text(w, text);
    quiet(w, box_name);
    if walked {
        walk(w, box_name);
        leave(w);
        String::new()
    } else if read_first {
        let held = held(w);
        leave(w);
        held
    } else {
        leave(w);
        held(w)
    }
}

/// A rectangle with a Shape Data field named Owner, the pane open, a click in the field.
fn shape_data_field(text: &str, walked: bool) -> Held {
    let mut w = crate::editing::edit_window([1280.0, 900.0]);
    as_shipped(&w);
    crate::editing::drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = crate::editing::shape_ids(&w)[0];
    let define = json!({"id": id, "fields": [{"label": "Owner", "name": "Owner", "type": "string", "prompt": "Who owns it"}]});
    crate::support::run_command(&mut w, "shapedata.define", define);
    crate::support::run_command(&mut w, "window.pane", json!({"pane": "shape_data", "shown": true}));
    focus(&mut w, "pane.data.Owner");
    let held = typed_into(
        "a Shape Data field",
        &mut w,
        text,
        (walked, false),
        |w| crate::editing::key(w, Key::Enter, Modifiers::NONE),
        |w| {
            let list = w.state_mut().execute("shapedata.list", &json!({"id": id, "hidden": true})).unwrap();
            list["fields"][0]["value"].as_str().unwrap_or_default().to_owned()
        },
    );
    (w, held)
}

/// Page Setup, Page Properties: the page's name.
fn page_name(text: &str, walked: bool) -> Held {
    let mut w = drawing([1280.0, 900.0], true);
    crate::support::run_command(&mut w, "window.pageSetup", json!({"tab": "Page Properties"}));
    focus(&mut w, "pagesetup.name");
    let held = typed_into(
        "the page's name",
        &mut w,
        text,
        (walked, false),
        |w| {
            crate::support::press(w, "pagesetup.ok");
            w.rest();
        },
        |w| w.state_mut().execute("page.setup", &json!({})).unwrap()["name"].as_str().unwrap_or_default().to_owned(),
    );
    (w, held)
}

/// The Find dialog's box.
fn find_box(text: &str, walked: bool) -> Held {
    let mut w = drawing([1280.0, 900.0], true);
    crate::support::run_command(&mut w, "window.find", json!({"mode": "find"}));
    focus(&mut w, "find.what");
    let held = typed_into(
        "the Find box",
        &mut w,
        text,
        (walked, false),
        |w| crate::editing::key(w, Key::Enter, Modifiers::NONE),
        |w| w.state().window_state()["find"]["what"].as_str().unwrap_or_default().to_owned(),
    );
    (w, held)
}

/// The Shapes window's search box.
fn stencil_search(text: &str, walked: bool) -> Held {
    let mut w = drawing([1280.0, 900.0], true);
    focus(&mut w, "shapes.filter");
    let held = typed_into(
        "the stencil search",
        &mut w,
        text,
        (walked, false),
        |w| crate::editing::key(w, Key::Enter, Modifiers::NONE),
        |w| w.state().window_state()["shapeFilter"].as_str().unwrap_or_default().to_owned(),
    );
    (w, held)
}

/// Layer Properties: the name of a new layer.
fn layer_name(text: &str, walked: bool) -> Held {
    let mut w = drawing([1280.0, 900.0], true);
    crate::support::run_command(&mut w, "window.layerProperties", json!({}));
    crate::support::press(&mut w, "layers.new");
    let held = typed_into(
        "a layer's name",
        &mut w,
        text,
        (walked, false),
        |w| {
            crate::support::press(w, "layers.add");
            w.rest();
        },
        |w| w.state().layer_properties_names().last().cloned().unwrap_or_default(),
    );
    (w, held)
}

/// The comment pane's box.
fn comment(text: &str, walked: bool) -> Held {
    let mut w = crate::editing::edit_window([1280.0, 900.0]);
    as_shipped(&w);
    crate::editing::drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    crate::support::run_command(&mut w, "comment.author", json!({"name": "Ada Lovelace", "initials": "AL"}));
    crate::support::run_command(&mut w, "window.newComment", json!({}));
    let held = typed_into(
        "a comment",
        &mut w,
        text,
        (walked, false),
        |w| {
            crate::support::press(w, "comments.post");
            w.rest();
        },
        |w| w.state().session().document().and_then(|doc| doc.comments.iter().next_back().map(|c| c.text.to_string())).unwrap_or_default(),
    );
    (w, held)
}

/// The text box over a shape, opened with F2.
fn text_box(text: &str, walked: bool) -> Held {
    let mut w = crate::editing::edit_window([1280.0, 900.0]);
    as_shipped(&w);
    crate::editing::drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    crate::editing::key(&mut w, Key::F2, Modifiers::NONE);
    let held = typed_into(
        "the text box",
        &mut w,
        text,
        (walked, false),
        |w| crate::editing::key(w, Key::Escape, Modifiers::NONE),
        |w| {
            // The box has sent its text and closed: the shape's text is what the box held.
            let id = crate::editing::shape_ids(w)[0];
            let page = 1;
            diacraft_engine::draw::text_view(w.state().session().document().unwrap(), page, id).map(|view| view.chars).unwrap_or_default()
        },
    );
    (w, held)
}

/// A ShapeSheet cell's editor.
fn sheet_cell(text: &str, walked: bool) -> Held {
    let mut w = sheet_window();
    crate::support::run_command(&mut w, "window.sheetSelect", json!({"cell": "Width"}));
    let held =
        typed_into("a ShapeSheet cell", &mut w, text, (walked, true), |w| crate::editing::key(w, Key::Escape, Modifiers::NONE), live_sheet_text);
    (w, held)
}

/// The ShapeSheet's formula bar, with the formula it held selected.
fn formula_bar(text: &str, walked: bool) -> Held {
    let mut w = sheet_window();
    crate::support::run_command(&mut w, "window.sheetSelect", json!({"cell": "Width"}));
    crate::support::press(&mut w, "sheet.bar.formula");
    w.rest();
    crate::editing::key(&mut w, Key::A, crate::saving::ctrl());
    let held = typed_into("the formula bar", &mut w, text, (walked, true), |w| crate::editing::key(w, Key::Escape, Modifiers::NONE), live_sheet_text);
    (w, held)
}

/// What the ShapeSheet's open editor holds.
fn live_sheet_text(w: &mut Window) -> String {
    crate::sheet_window::sheet(w)["edit"]["text"].as_str().unwrap_or_default().to_owned()
}

/// A window on a drawing whose one shape has a Width and a Height, with its ShapeSheet open.
fn sheet_window() -> Window {
    use diacraft_testkit::build::ShapeBuilder;
    let shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN").cell_u("Height", "1", "IN").cell("PinX", "4").cell("PinY", "4");
    let mut w = crate::sheet_window::window_on(crate::sheet_window::one_page(shape), [1400.0, 900.0]);
    as_shipped(&w);
    crate::sheet_window::open_shape(&mut w, 10);
    w
}

/// The texts of [`hostile`] the engine takes as they are (no control character, no invisible mark, nothing it normalises): what a box can be seen to hold after it sent them.
fn plain() -> Vec<String> {
    vec![
        "(1 \u{2192} 3)".to_owned(),
        "\u{6F22}\u{5B57}\u{304B}\u{306A}\u{D55C}\u{AE00}".to_owned(),
        "abc \u{645}\u{631}\u{62D}\u{628}\u{627} 123".to_owned(),
        "\u{5E9}\u{5DC}\u{5D5}\u{5DD}".to_owned(),
        "\u{1F600} \u{1D11E} \u{10348} \u{20000}".to_owned(),
    ]
}

/// **No character a person can type, or paste, makes a box of the window panic**, as shipped: each kind of text is typed in, the caret walked over it and the text taken out again;
/// and, so that the test cannot pass by typing into nothing, the texts the engine takes are found in the box, or in the drawing the box sent them to, as they were typed. One test
/// for each kind of box, so that each is held to nextest's two minutes and the failure names the box.
macro_rules! no_text_panics {
    ($($test:ident: $box_name:literal, $editor:ident;)*) => {$(
        #[test]
        fn $test() {
            for (what, text) in hostile() {
                let (w, _) = $editor(&text, true);
                quiet(&w, &format!("{what} in {}", $box_name));
            }
            for text in plain() {
                let (w, held) = $editor(&text, false);
                quiet(&w, &format!("{text:?} in {}", $box_name));
                assert_eq!(held, text, "{} did not hold what was typed", $box_name);
            }
        }
    )*};
}

no_text_panics! {
    no_text_typed_into_a_shape_data_field_panics_the_window: "a Shape Data field", shape_data_field;
    no_text_typed_as_a_pages_name_panics_the_window: "the page's name", page_name;
    no_text_typed_into_the_find_box_panics_the_window: "the Find box", find_box;
    no_text_typed_into_the_stencil_search_panics_the_window: "the stencil search", stencil_search;
    no_text_typed_as_a_layers_name_panics_the_window: "a layer's name", layer_name;
    no_text_typed_as_a_comment_panics_the_window: "a comment", comment;
    no_text_typed_into_the_text_box_panics_the_window: "the text box", text_box;
    no_text_typed_into_a_shapesheet_cell_panics_the_window: "a ShapeSheet cell", sheet_cell;
    no_text_typed_into_the_formula_bar_panics_the_window: "the formula bar", formula_bar;
}

// ---- what a file holds ------------------------------------------------------------------------------------------------------------------

/// A drawing whose every name and text the window shows is `text`: the page's name (as a file may name it, 255 characters), a shape's text, title and description, a layer, a Shape
/// Data field (caption, prompt and value), a comment and a hyperlink. Every panel that shows them is open, and the dialogs that list them. The number of those the engine took is
/// returned, since a text it refuses (a control character in a layer's name) never reaches the window and says nothing of it.
fn hostile_drawing(text: &str, size: [f32; 2]) -> (Window, usize) {
    let page_name: String = text.chars().take(255).collect();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    // The first page's name as a file may hold it, which no command can make; the window starts with it. The model refuses a control character in a name, so
    // a text that holds one leaves the page as it was: the other panels still get the text.
    session
        .edit("a name a file held", |doc| {
            doc.rename_page(1, &page_name).map_err(|e| diacraft_engine::EngineError::Internal { cmd: "test".to_owned(), msg: e.to_string() })
        })
        .ok();
    let mut w = shipped_window(DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()), size);
    w.rest();
    let id = {
        let dropped = w.state_mut().execute("shape.drop", &json!({"master": "process", "page": 1, "x": 4.0, "y": 5.0})).unwrap();
        u32::try_from(dropped["id"].as_u64().unwrap()).unwrap()
    };
    let mut took = 0;
    let mut give = |w: &mut Window, command: &str, params: serde_json::Value| {
        // A refusal is the engine's answer to a text it does not take; the window is what is under test.
        if w.state_mut().execute(command, &params).is_ok() {
            took += 1;
        }
        w.rest();
    };
    give(&mut w, "shape.set_text", json!({"id": id, "text": text}));
    give(&mut w, "shape.alt_text", json!({"id": id, "title": text, "description": text}));
    give(&mut w, "layer.add", json!({"name": page_name}));
    give(&mut w, "layer.assign", json!({"ids": [id], "layers": [page_name]}));
    give(&mut w, "shapedata.define", json!({"id": id, "fields": [{"label": text, "name": "Owner", "type": "string", "prompt": text}]}));
    give(&mut w, "shapedata.set", json!({"id": id, "row": "Owner", "value": text}));
    give(&mut w, "comment.author", json!({"name": "Ada Lovelace", "initials": "AL"}));
    give(&mut w, "comment.add", json!({"id": id, "text": text}));
    give(&mut w, "hyperlink.set", json!({"id": id, "links": [{"description": text, "address": "https://example.com/a"}]}));
    give(&mut w, "page.add", json!({"name": text.chars().take(31).collect::<String>()}));
    for pane in ["size_position", "format_shape", "pan_zoom", "drawing_explorer", "shape_data"] {
        crate::support::run_command(&mut w, "window.pane", json!({"pane": pane, "shown": true}));
    }
    crate::support::run_command(&mut w, "window.newComment", json!({}));
    crate::support::run_command(&mut w, "window.layerProperties", json!({}));
    (w, took)
}

/// Every kind of hostile text shown by every panel, in a window of `size`.
fn panels_show_every_hostile_text(size: [f32; 2]) {
    for (what, text) in hostile() {
        let (mut w, took) = hostile_drawing(&text, size);
        w.step();
        quiet(&w, &format!("{what} at {size:?}"));
        // The engine refuses a control character in most names and texts: such a text reaches the panels in fewer places.
        let floor = if text.chars().any(char::is_control) { 1 } else { 3 };
        assert!(took >= floor, "{what}: the engine took only {took} of the texts, so the panels were not shown what the test means them to be shown");
    }
}

/// **No character a file can hold makes any panel of the window panic**, as shipped, in a narrow window: the page tabs and the status bar, the panes, the layers, the comments and
/// the data a shape carries.
#[test]
fn no_text_a_file_holds_makes_a_panel_of_a_narrow_window_panic() {
    panels_show_every_hostile_text([640.0, 480.0]);
}

/// The same in a wide window.
#[test]
fn no_text_a_file_holds_makes_a_panel_of_a_wide_window_panic() {
    panels_show_every_hostile_text([1600.0, 900.0]);
}
