//! Colours come from theme tokens and nowhere else.

use std::path::Path;

use diacraft_ui_egui::theme::Tokens;
use egui::Color32;

/// Every `.rs` file under `dir`, with its text.
fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push((path.display().to_string(), std::fs::read_to_string(&path).unwrap()));
        }
    }
}

/// The ways a colour is written in egui code.
const COLOUR_SPELLINGS: &[&str] =
    &["Color32::", "Rgba::", "Hsva::", "from_rgb", "from_gray", "from_black_alpha", "from_white_alpha", "gamma_multiply"];

#[test]
fn only_the_theme_names_a_colour() {
    let mut files = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    // The scan must see the interface's files, or a moved folder would make it pass by finding nothing.
    for expected in ["lib.rs", "app.rs", "control.rs", "theme.rs"] {
        assert!(
            files.iter().any(|(p, _)| p.ends_with(expected)),
            "{expected} is not among the scanned files: {:?}",
            files.iter().map(|f| &f.0).collect::<Vec<_>>()
        );
    }
    let theme = files.iter().find(|(p, _)| p.ends_with("theme.rs")).unwrap();
    assert!(COLOUR_SPELLINGS.iter().any(|s| theme.1.contains(s)), "theme.rs writes no colour: the scan would find nothing to tell apart");
    for (path, text) in files.iter().filter(|(p, _)| !p.ends_with("theme.rs")) {
        for (n, line) in text.lines().enumerate() {
            // `ColorImage::from_rgba_premultiplied` makes an image from the renderer's pixel bytes (the picture of a page): it is not a
            // colour written in code, though its name begins as one does.
            let code = line.split("//").next().unwrap_or_default().replace("ColorImage::from_rgba_premultiplied", "");
            for spelling in COLOUR_SPELLINGS {
                assert!(!code.contains(spelling), "{path}:{}: `{spelling}` outside theme.rs: take the colour from `theme::Tokens`", n + 1);
            }
        }
    }
}

#[test]
fn the_light_and_dark_tokens_differ_and_are_opaque_where_they_fill() {
    let (light, dark) = (Tokens::light(), Tokens::dark());
    assert_ne!(light, dark);
    for tokens in [light, dark] {
        for fill in [tokens.top_bar_fill, tokens.status_bar_fill, tokens.canvas_fill] {
            assert_eq!(fill.a(), 255, "a panel fill must be opaque");
        }
        // The text of a bar is not the colour of the bar.
        assert_ne!(tokens.top_bar_text, tokens.top_bar_fill);
        assert_ne!(tokens.status_bar_text, tokens.status_bar_fill);
        assert_ne!(tokens.notice_text, tokens.status_bar_fill);
    }
}

#[test]
fn tokens_follow_the_egui_theme() {
    assert_eq!(Tokens::for_theme(egui::Theme::Dark), Tokens::dark());
    assert_eq!(Tokens::for_theme(egui::Theme::Light), Tokens::light());
}

fn luminance(c: egui::Color32) -> f64 {
    let channel = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}
fn contrast(a: egui::Color32, b: egui::Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text on bars, selected controls and rulers must be readable: at least 4.5 (WCAG AA for body text).
#[test]
fn text_tokens_are_readable_on_their_bars() {
    for tokens in [Tokens::light(), Tokens::dark()] {
        for (what, text, fill) in [
            ("top bar", tokens.top_bar_text, tokens.top_bar_fill),
            ("status bar", tokens.status_bar_text, tokens.status_bar_fill),
            ("notice", tokens.notice_text, tokens.status_bar_fill),
            ("ribbon", tokens.text, tokens.ribbon_fill),
            ("ribbon secondary", tokens.text_dim, tokens.ribbon_fill),
            ("accent", tokens.accent_text, tokens.accent),
            ("accent hover", tokens.accent_text, tokens.accent_hover),
            ("file rail", tokens.rail_text, tokens.rail_fill),
            ("file rail hover", tokens.rail_text, tokens.rail_hover),
            ("ruler", tokens.ruler_text, tokens.ruler_fill),
            ("ruler over paper", tokens.ruler_text, tokens.ruler_page_fill),
        ] {
            let ratio = contrast(text, fill);
            assert!(ratio >= 4.5, "{what}: contrast {ratio:.2} is under 4.5");
        }
    }
}

/// The ShapeSheet window's four tokens (design 10): the text of an own cell, of a cell that reads its default and of an error read at 4.5 to 1 or more on the sheet's ground in
/// both themes, and the background of a cell the last commit changed holds all three and the ordinary text at that ratio too.
#[test]
fn the_sheets_tokens_read_on_its_ground_in_both_themes() {
    for (theme, t) in [("light", Tokens::light()), ("dark", Tokens::dark())] {
        for (what, ink) in [("own", t.sheet_local), ("default", t.sheet_default), ("error", t.sheet_error), ("inherited", t.text)] {
            let ratio = contrast(ink, t.panel_fill);
            assert!(ratio >= 4.5, "{theme} {what} on the sheet's ground: {ratio:.2} is under 4.5");
        }
        assert_eq!(t.sheet_changed.a(), 255, "{theme}: a changed cell's background is opaque");
        for (what, ink) in [("own", t.sheet_local), ("error", t.sheet_error), ("inherited", t.text)] {
            let ratio = contrast(ink, t.sheet_changed);
            assert!(ratio >= 4.5, "{theme} {what} on a changed cell: {ratio:.2} is under 4.5");
        }
        assert_ne!(t.sheet_changed, t.panel_fill, "{theme}: a changed cell shows");
        assert_ne!(t.sheet_local, t.text, "{theme}: own and inherited cells differ in colour");
    }
}

/// The close button's hover colour is DiaCraft's own: not the red another craft app uses for it (196, 43, 28), which is also
/// the red Windows 11 shows there, nor Windows 10's (232, 17, 35).
#[test]
fn the_close_hover_colour_is_not_another_products() {
    for tokens in [Tokens::light(), Tokens::dark()] {
        for (name, other) in [("sibling and Windows 11", Color32::from_rgb(196, 43, 28)), ("Windows 10", Color32::from_rgb(232, 17, 35))] {
            assert_ne!(tokens.close_hover_fill, other, "the close hover is the {name} red");
        }
        // And its cross, drawn over it, can be seen (3:1 is the WCAG minimum for a graphic).
        let ratio = contrast(tokens.close_hover_icon, tokens.close_hover_fill);
        assert!(ratio >= 3.0, "the cross on the close hover has contrast {ratio:.2}");
    }
}

/// The hue of an opaque colour, in degrees from 0 to 360, and its HSV saturation from 0 to 1 (a colour with no hue has hue 0 and saturation 0).
fn hue_and_saturation(c: egui::Color32) -> (f64, f64) {
    let (r, g, b) = (f64::from(c.r()), f64::from(c.g()), f64::from(c.b()));
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    let spread = hi - lo;
    if spread == 0.0 {
        return (0.0, 0.0);
    }
    let sector = if hi == r {
        ((g - b) / spread).rem_euclid(6.0)
    } else if hi == g {
        (b - r) / spread + 2.0
    } else {
        (r - g) / spread + 4.0
    };
    (60.0 * sector, spread / hi)
}

/// The colour paragraph as corrected them: the AutoConnect arrows are the desktop
/// app's, **pale sky blue at rest and a deeper blue under the pointer**, and not gray (the gray was the web app's, and the circle handle with it). Blue here is a hue from
/// 195 to 235 degrees with a saturation of 0.4 or more, and the one under the pointer is the deeper (darker) of the two. They sit on the page, white in both themes, and at
/// its edge on the canvas, so each must be seen on both: 3:1, the WCAG minimum for a graphic. The page's white caps how pale the sky blue can be.
#[test]
fn the_autoconnect_arrows_are_sky_blue_a_deeper_blue_under_the_pointer_and_read_on_the_page_and_the_canvas_in_both_themes() {
    for (theme, tokens) in [("light", Tokens::light()), ("dark", Tokens::dark())] {
        for (what, colour) in [("arrow", tokens.auto_arrow), ("arrow under the pointer", tokens.auto_arrow_hot)] {
            assert_eq!(colour.a(), 255, "{theme} {what}: opaque");
            let (hue, saturation) = hue_and_saturation(colour);
            assert!((195.0..=235.0).contains(&hue), "{theme} {what}: {colour:?} has hue {hue:.0}, which is not a sky blue");
            assert!(saturation >= 0.4, "{theme} {what}: {colour:?} has saturation {saturation:.2}, which is gray rather than blue");
            for (ground, fill) in [("page", tokens.page_fill), ("canvas", tokens.canvas_fill)] {
                let ratio = contrast(colour, fill);
                assert!(ratio >= 3.0, "{theme} {what} on the {ground}: contrast {ratio:.2} is under 3");
            }
        }
        // Under the pointer it is the deeper of the two, and the two can be told apart (3:1 would be too much to ask of two blues that both read on both grounds).
        let (rest, hot) = (luminance(tokens.auto_arrow), luminance(tokens.auto_arrow_hot));
        assert!(hot < rest, "{theme}: the arrow under the pointer is deeper ({hot:.3} against {rest:.3})");
        assert!(contrast(tokens.auto_arrow, tokens.auto_arrow_hot) >= 1.3, "{theme}: the two blues differ");
    }
}
