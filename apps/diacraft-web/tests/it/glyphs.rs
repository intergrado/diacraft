//! Characters no loaded face has, in the window the web shell makes.
//!
//! The browser cannot unwind from a panic: the module aborts. So a character that no bundled face has must be a box in the browser's window, never a panic. eframe's web
//! runner keeps egui's default (`MissingGlyphPolicy::Tofu`) and adds a canvas rasterizer behind the faces, which draws what the browser can and leaves the rest to the box. The
//! rasterizer needs a browser, which these tests do not have, so they hold what does not: the faces the web build installs (the bundled two, `font_definitions()`; the browser's
//! branch adds only the optional faces of the `craft-fonts` build input) lay out every kind of text, and the window the shell makes takes such text in every place a file or a person
//! puts it.
//!
//! The test harness is stricter than the browser: it turns on the policy that panics (`egui_kittest`), so these windows are built `allow_missing_glyphs`, which is what the
//! browser runs.

use diacraft_browser::page::new_app;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::fonts::{font_definitions, strong_family};
use egui::{FontFamily, FontId, MissingGlyphPolicy};
use egui_kittest::Harness;
use serde_json::json;

/// One of each kind of text the bundled faces cannot draw, or that is hard for the text code under them.
fn hostile() -> Vec<(&'static str, String)> {
    vec![
        ("an arrow", "(1 \u{2192} 3)".to_owned()),
        ("emoji", "\u{1F600} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F1E8}\u{1F1ED} 1\u{FE0F}\u{20E3}".to_owned()),
        ("Han, kana and Hangul", "\u{6F22}\u{5B57}\u{304B}\u{306A}\u{D55C}\u{AE00}".to_owned()),
        ("Arabic among Latin", "abc \u{645}\u{631}\u{62D}\u{628}\u{627} 123".to_owned()),
        ("the planes beyond the first", "\u{1D11E} \u{10348} \u{20000} \u{E0001} \u{10FFFF}".to_owned()),
        ("noncharacters and marks", "\u{FDD0}\u{FFFE}\u{FFFF}\u{301}\u{302}\u{200D}".to_owned()),
        ("one letter under 300 marks", format!("a{}", "\u{301}".repeat(300))),
        ("controls", "\u{1}\u{7F}\u{85}\u{202E}\u{FEFF}a\tb\nc".to_owned()),
    ]
}

/// The faces the browser's window installs lay out each text in each family without a panic, and the arrows a notice uses are drawn by all of them.
#[test]
fn the_faces_the_web_build_installs_draw_a_box_for_what_they_lack_and_the_arrows_they_have() {
    let ctx = egui::Context::default();
    assert_eq!(ctx.missing_glyph_policy(), MissingGlyphPolicy::Tofu, "egui's default is no longer the one the browser's window relies on");
    ctx.set_fonts(font_definitions());
    let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
        for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
            for arrow in "\u{2190}\u{2191}\u{2192}\u{2193}\u{2194}".chars() {
                assert!(
                    ctx.fonts_mut(|f| f.has_glyph(&FontId::new(14.0, family.clone()), arrow)),
                    "{family:?} has no glyph for U+{:04X}",
                    arrow as u32
                );
            }
            for (what, text) in hostile() {
                for size in [0.5_f32, 14.0, 96.0] {
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

/// Runs `command` in the window and draws two frames; whether the engine took it.
fn run(window: &mut Harness<'_, DiacraftApp>, command: &str, params: serde_json::Value) -> bool {
    // A refusal is the engine's answer to a text it does not take (a control character in a name); the window is what is under test.
    let took = window.state_mut().execute(command, &params).is_ok();
    window.step();
    window.step();
    took
}

/// The window the shell makes shows such text wherever the window puts a name: the page tabs and their All menu, the layers, a shape's text, data and comments, the status bar's notice.
#[test]
fn the_window_the_shell_makes_shows_text_of_any_script_without_a_panic() {
    for (what, text) in hostile() {
        let mut window = Harness::builder().with_size([800.0, 500.0]).allow_missing_glyphs().build_eframe(|_| new_app().with_notice(&text));
        window.step();
        let name: String = text.chars().take(31).collect();
        let mut took = 0;
        took += usize::from(run(&mut window, "file.new", json!({})));
        let dropped = window
            .state_mut()
            .execute("shape.drop", &json!({"master": "process", "page": 1, "x": 4.0, "y": 5.0}))
            .ok()
            .and_then(|a| a["id"].as_u64());
        let id = dropped.expect("a process shape is dropped on the new drawing");
        for (command, params) in [
            ("page.add", json!({"name": name})),
            ("layer.add", json!({"name": text.chars().take(255).collect::<String>()})),
            ("comment.author", json!({"name": "Ada Lovelace", "initials": "AL"})),
            ("shape.set_text", json!({"id": id, "text": text})),
            ("comment.add", json!({"id": id, "text": text})),
            ("shapedata.define", json!({"id": id, "fields": [{"label": text, "name": "Owner", "type": "string"}]})),
            ("window.pane", json!({"pane": "drawing_explorer", "shown": true})),
            ("window.pane", json!({"pane": "shape_data", "shown": true})),
            ("window.layerProperties", json!({})),
        ] {
            took += usize::from(run(&mut window, command, params));
        }
        assert_eq!(window.state().ui_panics(), 0, "{what}: {:?}", window.state().notice());
        // The engine refuses a control character and an invisible mark in most names and texts, so such a text reaches the panels in fewer places (the five commands that carry
        // no text are always taken); the texts that are only letters and symbols are taken by most.
        let floor = if ["an arrow", "Han, kana and Hangul", "Arabic among Latin"].contains(&what) { 8 } else { 5 };
        assert!(took >= floor, "{what}: the engine took {took} of 10 commands, so the panels were not shown what the test means them to be shown");
    }
}
