//! (corrected 2026-11-20; the bullet on a page height change and the connectors no edit touched, the `svg:x1` and `svg:d`
//! rows), driven by the commands a person has: **a change of the page's height keeps `PinY`, so every shape and every connector end moves down the page
//! by what the page grew, and a connector no edit touched keeps its own path through it, shifted** (the first absolute `M` of LibreOffice's form moves and its relative steps do
//! not; in a path of absolute points every point moves), its `svg:viewBox` as it was, its `draw:line-skew` kept, and its ends moved as R8 says. A width change moves nothing.
//!
//! The vectors' K2 and the shapes around it come from `odg_untouched_twins.rs` (U, E, T, S, K1 a level connector crossing K2's long leg, and K2 with the skew `0.5cm`); the chains
//! here are the ones the rule's own words name: several height changes and saves, undo across them, an absolute-form path, a curved connector, a connector with no `svg:d`, a hop
//! kept through a change. Every drawing is built by hand from the standard (`odg_build.rs`); what is read back is read by the `odg` crate's reader.

use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing, read_back};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, save};
use crate::odg_untouched_routes::{
    Told, attr_of, check_told, connector_named, has_hop, page_cell, page_corners, path_number, same_route, size_page, warnings,
};
use crate::odg_untouched_twins::{K2_BOX, K2_SKEW, ends_are, inches, k2_tag, move_t_off_the_leg, twin_drawing, vertical_lines_hop};
use crate::run;

/// K2's path in the vectors (its begin at (4, 10) cm) moved `dh` inches down the page, in LibreOffice's form: the `M` moves, the relative steps do not.
fn k2_vector_path(dh: f64) -> String {
    format!("M 4000 {} v 2000 h 5000 v -6000 h 2000 v 1000", path_number(10000.0 + dh * 2540.0))
}

/// K1's path in the twin drawing (a level connector from T to S at y 8.5 cm, absolute points) moved `dh` inches down the page: every point moves.
fn k1_path(dh: f64) -> String {
    let y = path_number(8500.0 + dh * 2540.0);
    format!("M 2500 {y} L 14000 {y}")
}

/// The page's height now less the file's, inches.
fn grew(s: &Session, was: f64) -> f64 {
    page_cell(s, "PageHeight") - was
}

/// What every save of a chain must have left, after a change of the page's height by `dh` inches from the file's (the page's own is the model's): K1 and K2 hold their own
/// paths shifted by it, K2 its view box and skew, K1 its view box; K2's ends are `dh` lower than the vectors', the file reads back with the routes the model has and with the
/// page's height, and nothing is said of a connector (no hop is in the model).
fn assert_the_paths_went_with_the_page(dir: &Dir, s: &Session, was: f64, saved: &serde_json::Value, label: &str) {
    let dh = grew(s, was);
    let content = content_of(&dir.read("a.odg"));
    let (k1, k2) = (connector_named(&content, "K1"), connector_named(&content, "K2"));
    assert_eq!(attr_of(&k2, "svg:d").as_deref(), Some(k2_vector_path(dh).as_str()), "{label}: K2's path, shifted {dh} inches: {k2}");
    assert_eq!(attr_of(&k1, "svg:d").as_deref(), Some(k1_path(dh).as_str()), "{label}: K1's path, shifted {dh} inches: {k1}");
    assert!(k2.contains(K2_BOX) && k2.contains(K2_SKEW), "{label}: K2's view box and skew are the file's: {k2}");
    assert!(k1.contains("svg:viewBox=\"0 0 11501 1\"") && !k1.contains("draw:line-skew"), "{label}: K1's view box is the file's: {k1}");
    let down = dh * 2.54;
    assert!(ends_are(&k2, (4.0, 10.0 + down, 11.0, 7.0 + down)), "{label}: K2's ends: {k2}");
    assert!(ends_are(&k1, (2.5, 8.5 + down, 14.0, 8.5 + down)), "{label}: K1's ends: {k1}");
    assert_eq!(warnings(saved), Vec::<String>::new(), "{label}: nothing is said: {saved}");
    let (back, _) = read_back(&dir.read("a.odg"));
    for name in ["K1", "K2"] {
        let (model, file) = (page_corners(s.document().unwrap(), name), page_corners(&back, name));
        assert!(same_route(&model, &file), "{label}: {name} reopens with {file:?} for the model's {model:?}");
    }
    let height = back.pages.get(0).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((height - page_cell(s, "PageHeight")).abs() < 1e-6, "{label}: the page's height is written: {height}");
}

/// Several height changes and saves, then undo across them, then the file's own height again: after each save the paths are the file's shifted by what the page is taller
/// than the file's (a save shifts by the change since the last, and the sum is the page's own), the skew and the view boxes are as they were, and at the file's own height
/// K2's and K1's `svg:d` are the file's own bytes again. A save after each is no edit.
#[test]
fn chains_of_height_changes_and_saves_and_undos_shift_the_paths_by_each_and_back_to_the_files_own() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &twin_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    let original = content_of(&dir.read("a.odg"));
    let steps: [(&str, Option<f64>); 10] = [
        ("size +2", Some(2.0)),
        ("size +5", Some(5.0)),
        ("size -1", Some(-1.0)),
        ("undo", None),
        ("undo", None),
        ("redo", None),
        ("size +0.01234", Some(0.01234)),
        ("undo", None),
        ("undo", None),
        ("undo", None),
    ];
    let mut told = Told::new();
    for (n, (step, size)) in steps.iter().enumerate() {
        let label = format!("step {n}: {step}");
        match (size, *step) {
            (Some(cm), _) => size_page(&mut s, width, was + inches(*cm)),
            (None, "redo") => drop(run(&mut s, "edit.redo", json!({}))),
            (None, _) => drop(run(&mut s, "edit.undo", json!({}))),
        }
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{label}: the height changed: {saved}");
        assert_eq!(check_told(&s, &dir, "a.odg", &saved, &["K1", "K2"], &mut told, &label), Vec::<String>::new(), "{label}");
        assert_the_paths_went_with_the_page(&dir, &s, was, &saved, &label);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    // Three undos from +0.01234 (the last of the sizes) take the page back to the file's own height after the earlier undos: the paths are the file's own bytes.
    assert!((grew(&s, was)).abs() < 1e-9, "the page is the file's again: {}", grew(&s, was));
    let content = content_of(&dir.read("a.odg"));
    for name in ["K1", "K2"] {
        assert_eq!(
            attr_of(&connector_named(&content, name), "svg:d"),
            attr_of(&connector_named(&original, name), "svg:d"),
            "{name}: the file's own path again"
        );
    }
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// A hop that K1 gives K2 (`connector.line_jumps` with `code` 2), kept through several changes of the height, an undo across them, the hop going and coming back: **the save
/// that keeps the file's path for the hop says the route could not be written, once, and the saves after it that change the page's height say nothing** (a
/// route kept through a height change is not named again); K2's path is the file's, shifted by what the page is taller than the file's, with its skew and view box, after every
/// save; a save that takes the hop away writes nothing of K2 and says nothing (the file holds the route the model has); and the hop coming back is said once. The property
/// `check_told` holds every save to the route the file reopens with.
#[test]
fn a_hop_kept_through_height_changes_saves_and_undos_has_its_path_shifted_each_time_and_is_said_once() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &twin_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    vertical_lines_hop(&mut s);
    assert!(has_hop(s.document().unwrap(), "K2"), "K1 crosses K2's leg: K2 holds a hop");
    let mut told = Told::new();
    let check = |s: &Session, dir: &Dir, told: &mut Told, saved: &serde_json::Value, label: &str, said: usize| {
        assert_eq!(check_told(s, dir, "a.odg", saved, &["K1", "K2"], told, label), Vec::<String>::new(), "{label}");
        assert_eq!(warnings(saved).len(), said, "{label}: {saved}");
        let k2 = k2_tag(dir);
        let dh = grew(s, was);
        assert_eq!(attr_of(&k2, "svg:d").as_deref(), Some(k2_vector_path(dh).as_str()), "{label}: the path, shifted {dh} inches: {k2}");
        assert!(k2.contains(K2_BOX) && k2.contains(K2_SKEW), "{label}: view box and skew are the file's: {k2}");
        let down = dh * 2.54;
        assert!(ends_are(&k2, (4.0, 10.0 + down, 11.0, 7.0 + down)), "{label}: the ends: {k2}");
    };
    // The jumps alone are no edit of the page, and give K2 a hop: the first save keeps its path and says so.
    let first = save(&mut s);
    check(&s, &dir, &mut told, &first, "the hop made", 1);
    for (cm, label) in [(2.0, "size +2"), (5.0, "size +5"), (-1.0, "size -1")] {
        size_page(&mut s, width, was + inches(cm));
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{label}: the page's height is written: {saved}");
        check(&s, &dir, &mut told, &saved, label, 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
    // Undo takes the last size back: the path goes back with the page, and nothing is said (the hop was said when it was kept).
    run(&mut s, "edit.undo", json!({}));
    let back = save(&mut s);
    assert_eq!(back["edited"], true, "{back}");
    check(&s, &dir, &mut told, &back, "undo", 0);
    // The hop goes (K1 moves off the leg): nothing of K2 is written, and nothing said.
    move_t_off_the_leg(&mut s);
    assert!(!has_hop(s.document().unwrap(), "K2"));
    let gone = save(&mut s);
    check(&s, &dir, &mut told, &gone, "the hop gone", 0);
    // A change of the height with the hop gone: the path goes with the page, nothing is said.
    size_page(&mut s, width, was + inches(7.0));
    let taller = save(&mut s);
    check(&s, &dir, &mut told, &taller, "size +7", 0);
    // The undo of the page's change and of K1's move brings the hop back: said.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(has_hop(s.document().unwrap(), "K2"), "the hop is back");
    let again = save(&mut s);
    check(&s, &dir, &mut told, &again, "the hop back", 1);
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// A connector the page's height edit and an edit of its own both touch is routed anew, and the skew goes with the route: U moved 1 cm right and the page 2 cm taller in one
/// save. The path is written from the cells in the page of now, which begins where U's bottom is, 2 cm further down the page; K1, which no edit touched, keeps its path shifted.
#[test]
fn a_connector_an_edit_touched_is_written_anew_in_the_save_that_changes_the_height_and_the_others_keep_their_paths_shifted() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &twin_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    let u = id_of(s.document().unwrap(), 0, "U");
    run(&mut s, "shape.move", json!({ "ids": [u], "dx": inches(1.0), "dy": 0.0 }));
    size_page(&mut s, width, was + inches(2.0));
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    let content = content_of(&dir.read("a.odg"));
    let k2 = connector_named(&content, "K2");
    assert!(!k2.contains("draw:line-skew"), "routed anew, so no skew: {k2}");
    assert!(
        attr_of(&k2, "svg:d").is_some_and(|d| d.starts_with("M 5000 12000 ")),
        "the route begins where U's bottom is now, 2 cm further down: {k2}"
    );
    let k1 = connector_named(&content, "K1");
    assert_eq!(attr_of(&k1, "svg:d").as_deref(), Some(k1_path(grew(&s, was)).as_str()), "K1 no edit touched: {k1}");
    let (back, _) = read_back(&dir.read("a.odg"));
    assert!(same_route(&page_corners(s.document().unwrap(), "K2"), &page_corners(&back, "K2")), "K2 reopens with the model's route");
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
}

/// A width change moves nothing: K1 and K2 keep their bytes, the width is written, nothing is said; a width and a height in one save shift by the height alone.
#[test]
fn a_width_change_moves_no_path_and_a_height_change_with_it_shifts_by_the_height_alone() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &twin_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    let before = content_of(&dir.read("a.odg"));
    size_page(&mut s, width + inches(3.0), was);
    let saved = save(&mut s);
    assert_eq!(saved["edited"], true, "{saved}");
    assert_eq!(warnings(&saved), Vec::<String>::new());
    let after = content_of(&dir.read("a.odg"));
    for name in ["K1", "K2"] {
        assert_eq!(connector_named(&after, name), connector_named(&before, name), "{name} keeps its bytes");
    }
    size_page(&mut s, width - inches(2.0), was + inches(2.0));
    let both = save(&mut s);
    assert_eq!(warnings(&both), Vec::<String>::new());
    let content = content_of(&dir.read("a.odg"));
    assert_eq!(attr_of(&connector_named(&content, "K2"), "svg:d").as_deref(), Some(k2_vector_path(grew(&s, was)).as_str()));
    assert_eq!(attr_of(&connector_named(&content, "K1"), "svg:d").as_deref(), Some(k1_path(grew(&s, was)).as_str()));
    assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
}

/// A drawing of three connectors no glue holds, built by hand: C1 a curve in relative form (`c`) with a skew at y 10 cm, C2 a curve in absolute form (`C`) at y 14 cm, with a
/// view box, N1 with no `svg:d` at y 18 cm (and a skew), and P1 a path of absolute `H` and `V` lines.
fn loose_connectors() -> Drawing {
    let connector = |name: &str, kind: &str, skew: &str, (x1, y1, x2, y2): (f64, f64, f64, f64), path: &str| {
        format!(
            "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"{name}\" draw:type=\"{kind}\" {skew} svg:x1=\"{x1}cm\" svg:y1=\"{y1}cm\" svg:x2=\"{x2}cm\" svg:y2=\"{y2}cm\" {path}><text:p/></draw:connector>"
        )
    };
    Drawing::new().page(
        &[
            connector(
                "C1",
                "curve",
                "draw:line-skew=\"0.25cm\"",
                (4.0, 10.0, 9.0, 10.0),
                "svg:d=\"M 4000 10000 c 0 1000 5000 1000 5000 0\" svg:viewBox=\"0 0 5001 751\"",
            ),
            connector(
                "C2",
                "curve",
                "",
                (4.0, 14.0, 9.0, 14.0),
                "svg:d=\"M 4000 14000 C 4000 15000 9000 15000 9000 14000\" svg:viewBox=\"0 0 5001 751\"",
            ),
            connector("N1", "standard", "draw:line-skew=\"0.5cm\"", (4.0, 18.0, 9.0, 21.0), ""),
            connector("P1", "line", "", (2.0, 2.0, 8.0, 6.0), "svg:d=\"M 2000 2000 H 8000 V 6000\" svg:viewBox=\"0 0 6001 4001\""),
        ]
        .concat(),
    )
}

/// A curved connector, one with a path of absolute `C`, one with `H` and `V`, and one with no `svg:d`, through the page's height change by +2 and by -3 cm, and back: the
/// relative `c` after the first `M` is untouched; absolute `C` has each `y` moved, `H` and `V` as the commands say; the connector with no path has its ends moved and no path
/// made; the skews and the view boxes are as they were; nothing is said. The file reads back with the routes the model has, and a save after each is no edit.
#[test]
fn a_curved_connector_one_with_no_path_and_absolute_commands_go_with_the_page() {
    for (by, c1, c2, p1, ys) in [
        (
            2.0,
            "M 4000 12000 c 0 1000 5000 1000 5000 0",
            "M 4000 16000 C 4000 17000 9000 17000 9000 16000",
            "M 2000 4000 H 8000 V 8000",
            (12.0, 16.0, 20.0, 23.0),
        ),
        (
            -3.0,
            "M 4000 7000 c 0 1000 5000 1000 5000 0",
            "M 4000 11000 C 4000 12000 9000 12000 9000 11000",
            "M 2000 -1000 H 8000 V 3000",
            (7.0, 11.0, 15.0, 18.0),
        ),
    ] {
        let label = format!("the page {by} cm");
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &loose_connectors());
        let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
        let original = content_of(&dir.read("a.odg"));
        size_page(&mut s, width, was + inches(by));
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{label}: {saved}");
        assert_eq!(warnings(&saved), Vec::<String>::new(), "{label}: nothing is said: {saved}");
        let content = content_of(&dir.read("a.odg"));
        let tag = |name: &str| connector_named(&content, name);
        assert_eq!(attr_of(&tag("C1"), "svg:d").as_deref(), Some(c1), "{label}: the relative curve");
        assert_eq!(attr_of(&tag("C2"), "svg:d").as_deref(), Some(c2), "{label}: the absolute curve");
        assert_eq!(attr_of(&tag("P1"), "svg:d").as_deref(), Some(p1), "{label}: absolute H and V");
        assert!(
            attr_of(&tag("N1"), "svg:d").is_none() && attr_of(&tag("N1"), "svg:viewBox").is_none(),
            "{label}: no path is made for N1: {}",
            tag("N1")
        );
        for (name, y) in [("C1", ys.0), ("C2", ys.1)] {
            assert!(ends_are(&tag(name), (4.0, y, 9.0, y)), "{label}: {name}'s ends: {}", tag(name));
        }
        assert!(ends_are(&tag("N1"), (4.0, ys.2, 9.0, ys.3)), "{label}: N1's ends: {}", tag("N1"));
        assert!(ends_are(&tag("P1"), (2.0, 2.0 + by, 8.0, 6.0 + by)), "{label}: P1's ends: {}", tag("P1"));
        for (name, view_box) in [("C1", Some("0 0 5001 751")), ("C2", Some("0 0 5001 751")), ("P1", Some("0 0 6001 4001")), ("N1", None)] {
            assert_eq!(attr_of(&tag(name), "svg:viewBox").as_deref(), view_box, "{label}: {name}'s view box");
        }
        for (name, skew) in [("C1", Some("0.25cm")), ("C2", None), ("P1", None), ("N1", Some("0.5cm"))] {
            assert_eq!(attr_of(&tag(name), "draw:line-skew").as_deref(), skew, "{label}: {name}'s skew");
        }
        let (back, _) = read_back(&dir.read("a.odg"));
        for name in ["C1", "C2", "N1", "P1"] {
            let shape =
                |d: &diacraft_doc::Document| d.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some(name)).unwrap().clone();
            assert_eq!(shape(&back).sheet.sections.len(), shape(s.document().unwrap()).sheet.sections.len(), "{label}: {name}'s sections");
        }
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        // Back to the file's own height: every path is the file's own bytes again.
        run(&mut s, "edit.undo", json!({}));
        let undone = save(&mut s);
        assert_eq!(undone["edited"], true, "{label}: {undone}");
        let content = content_of(&dir.read("a.odg"));
        for name in ["C1", "C2", "N1", "P1"] {
            assert_eq!(
                attr_of(&connector_named(&content, name), "svg:d"),
                attr_of(&connector_named(&original, name), "svg:d"),
                "{label}: {name}'s path is the file's own"
            );
        }
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
    }
}

// ---- the black-box LibreOffice check -----------------------------------------------------------------------------------------------------------------

/// Writes what the vectors save, for the LibreOffice check by hand (`ODG_PAGEKEEP_DIR/in/*.odg`; the script converts each to an `.odg` in a profile of its own, and compares
/// the legs of K2 that LibreOffice draws, from the path it writes, between the input and each save): the vectors' drawing as it is, saved after the page's height set 2 cm
/// greater and 3 cm less, each with and without the hop K1 gives K2, and the chain of three saves.
#[test]
#[ignore = "writes files for the LibreOffice check by hand"]
fn lo_pagekeep_write() {
    let Some(base) = std::env::var_os("ODG_PAGEKEEP_DIR") else { return };
    let dir = std::path::Path::new(&base).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let to = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    std::fs::write(to("v-input.odg"), twin_drawing().bytes()).unwrap();
    let scratch = Dir::new();
    for (name, by, hop) in [("plus2", 2.0, false), ("minus3", -3.0, false), ("plus2-hop", 2.0, true), ("minus3-hop", -3.0, true)] {
        let mut s = open_drawing(&scratch, &twin_drawing());
        let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
        size_page(&mut s, width, was + inches(by));
        if hop {
            vertical_lines_hop(&mut s);
        }
        run(&mut s, "file.save_as", json!({ "path": to(&format!("v-{name}.odg")) }));
    }
    let mut s = open_drawing(&scratch, &twin_drawing());
    let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
    for (n, cm) in [2.0, 5.0, -1.0].iter().enumerate() {
        size_page(&mut s, width, was + inches(*cm));
        run(&mut s, "file.save_as", json!({ "path": to(&format!("v-chain-{n}.odg")) }));
    }
}
