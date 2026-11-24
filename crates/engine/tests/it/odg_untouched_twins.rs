//! The **engine-reachable twins** of three vectors of (the check, question 2). Those vectors state their
//! numbers as the writer is given them (the model's rows are set by hand); here the same chains are driven by the commands a person has, on a drawing built by hand with
//! the vectors' K2: a connector glued by both ends whose file holds `draw:line-skew="0.5cm"`, `svg:viewBox="0 0 7001 6001"` and
//! `svg:d="M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000"`, so that its begin is at (4, 10) cm, its end at (11, 7) and its corners at (4, 10), (4, 12), (9, 12), (9, 6),
//! (11, 6) and (11, 7).
//!
//! **A hop on an upright leg takes `connector.line_jumps` with `code` 2**: by the page's default (code 1) the flatter of two crossing legs carries the hop, and K1, a level
//! connector, would carry it and K2 none. And **the engine routes a connector whose end moved by its own router**, not along the file's waypoints, so the corners after U
//! has moved are the engine's, not the vector's: this twin asserts what the vector states of the saves (the file's path and skew kept with one word, then the route written,
//! beginning where the begin now is, with no skew and no word, and nothing written by a save after it), and the vector's own numbers where the commands reach them (
//! `connector.edit_path` moves a leg to the corners the vector states; a page's height moves no corner of the route the file holds, so the starts are the vector's).
//!
//! Every drawing is built by hand from the standard (`odg_build.rs`); what is read back is read by the `odg` crate's reader.

use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, content_of, id_of, open_drawing, read_back};
use crate::odg_file::{assert_reopens_as_saved, assert_saves_again_as_no_edit, save};
use crate::odg_untouched_routes::{
    HOP_WORDS, Told, check_told, connector_named, glue_rect, has_hop, number_of, page_cell, page_corners, same_route, size_page, warnings,
};
use crate::run;

/// A centimetre, in inches (the engine's lengths are paper inches).
pub(crate) fn inches(cm: f64) -> f64 {
    cm / 2.54
}

/// The four glue points a shape of `w` by `h` centimetres lists of its own, in the view box [`glue_rect`] gives it (ids 4 top, 5 left, 6 bottom, 7 right: the middle of each side).
pub(crate) fn four_glue(w: f64, h: f64) -> String {
    let (x, y) = ((w * 1000.0).round(), (h * 1000.0).round());
    format!("{x} 0 0 {y} {x} {} {} {y}", 2.0 * y, 2.0 * x)
}

/// K2 as the vectors have it, and the shapes around it. U (x 3 to 5, y 9 to 10) holds K2's begin at its bottom, glue point 2 (the default one), (4, 10); E (x 10 to 12,
/// y 7 to 8) holds its end at its top, glue point 0, (11, 7). K1 is a level connector from T's right side (2.5, 8.5) to S's left side (14, 8.5), which crosses K2's leg
/// from (9, 12) to (9, 6) at (9, 8.5). Shape ids 1 to 4 for U, E, T and S, then K1 and K2.
pub(crate) fn twin_drawing() -> Drawing {
    let shape = |name: &str, id: &str, frame: (f64, f64, f64, f64)| glue_rect(name, id, frame, &four_glue(frame.2, frame.3));
    let k1 = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"K1\" draw:type=\"line\" draw:start-shape=\"t\" draw:start-glue-point=\"1\" draw:end-shape=\"s\" draw:end-glue-point=\"3\" svg:x1=\"2.5cm\" svg:y1=\"8.5cm\" svg:x2=\"14cm\" svg:y2=\"8.5cm\" svg:d=\"M 2500 8500 L 14000 8500\" svg:viewBox=\"0 0 11501 1\"><text:p/></draw:connector>";
    let k2 = "<draw:connector draw:style-name=\"gr1\" draw:text-style-name=\"P1\" draw:layer=\"layout\" draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"11cm\" svg:y2=\"7cm\" svg:d=\"M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000\" svg:viewBox=\"0 0 7001 6001\"><text:p/></draw:connector>";
    Drawing::new().page(
        &[
            shape("U", "u", (3.0, 9.0, 2.0, 1.0)),
            shape("E", "e", (10.0, 7.0, 2.0, 1.0)),
            shape("T", "t", (0.5, 8.0, 2.0, 1.0)),
            shape("S", "s", (14.0, 8.0, 2.0, 1.0)),
            k1.to_owned(),
            k2.to_owned(),
        ]
        .concat(),
    )
}

/// The attributes K2 holds in the file as the vectors give them.
pub(crate) const K2_PATH: &str = "svg:d=\"M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000\"";
pub(crate) const K2_BOX: &str = "svg:viewBox=\"0 0 7001 6001\"";
pub(crate) const K2_SKEW: &str = "draw:line-skew=\"0.5cm\"";

/// K2's start tag in the file the session last saved (`a.odg`).
pub(crate) fn k2_tag(dir: &Dir) -> String {
    connector_named(&content_of(&dir.read("a.odg")), "K2")
}

/// The length an attribute of a start tag holds, in centimetres (`svg:y1="12.000000000940004cm"` is 12.000000000940004): the engine writes a place as the number it has, with
/// the noise of the page's height in inches (here 1E-9 cm), so a place is compared to a thousandth of a millimetre and not by its spelling.
pub(crate) fn cm_of(tag: &str, attr: &str) -> f64 {
    let at = tag.find(&format!(" {attr}=\"")).unwrap_or_else(|| panic!("no {attr}: {tag}")) + attr.len() + 3;
    let text = &tag[at..];
    text[..text.find("cm\"").unwrap_or_else(|| panic!("{attr} is not in centimetres: {tag}"))]
        .parse()
        .unwrap_or_else(|e| panic!("{attr}: {e}: {tag}"))
}

/// K2's ends as a start tag has them, in centimetres from the page's top left: `(x1, y1, x2, y2)`.
pub(crate) fn tag_ends(tag: &str) -> (f64, f64, f64, f64) {
    (cm_of(tag, "svg:x1"), cm_of(tag, "svg:y1"), cm_of(tag, "svg:x2"), cm_of(tag, "svg:y2"))
}

/// Whether the ends of a start tag are `wanted`, to a thousandth of a millimetre.
pub(crate) fn ends_are(tag: &str, wanted: (f64, f64, f64, f64)) -> bool {
    let got = tag_ends(tag);
    [(got.0, wanted.0), (got.1, wanted.1), (got.2, wanted.2), (got.3, wanted.3)].iter().all(|(a, b)| (a - b).abs() < 1e-4)
}

/// Whether K2's tag is the file's own: its path, its view box and its skew.
pub(crate) fn is_the_files(tag: &str) -> bool {
    [K2_PATH, K2_BOX, K2_SKEW].iter().all(|a| tag.contains(a))
}

/// The `ID` of K2.
pub(crate) fn k2_of(s: &Session) -> u32 {
    id_of(s.document().unwrap(), 0, "K2")
}

/// K2's begin and end in the model, centimetres from the page's top left.
pub(crate) fn ends_in_cm(s: &Session) -> ((f64, f64), (f64, f64)) {
    let doc = s.document().unwrap();
    let height = page_cell(s, "PageHeight");
    let k2 = doc.pages.get(0).unwrap().shapes.iter().find(|x| x.name_u.as_deref() == Some("K2")).unwrap();
    let at = |x: &str, y: &str| (number_of(k2, x) * 2.54, (height - number_of(k2, y)) * 2.54);
    (at("BeginX", "BeginY"), at("EndX", "EndY"))
}

/// Two points the same to a tenth of a millimetre.
pub(crate) fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
}

/// `connector.line_jumps` with the vertical lines carrying the hop of two crossing legs.
pub(crate) fn vertical_lines_hop(s: &mut Session) {
    run(s, "connector.line_jumps", json!({ "code": 2 }));
}

/// T moved 10 cm right: K1 runs from T's right side to S's left, 1.5 cm, and crosses no leg of K2 (which stands between x 4 and x 11, from y 6 to y 12).
pub(crate) fn move_t_off_the_leg(s: &mut Session) {
    let t = id_of(s.document().unwrap(), 0, "T");
    run(s, "shape.move", json!({ "ids": [t], "dx": inches(10.0), "dy": 0.0 }));
}

/// The way the hop goes: no jumps at all (`connector.line_jumps` code 0), or K1 moved off the leg. Neither moves an end of K2.
pub(crate) fn take_the_hop_away(s: &mut Session, way: &str) {
    if way == "jumps off" {
        run(s, "connector.line_jumps", json!({ "code": 0 }));
    } else {
        move_t_off_the_leg(s);
    }
    assert!(!has_hop(s.document().unwrap(), "K2"), "{way}: the hop is gone");
}

/// The route the saved file holds of K2 is the model's, read by the real reader: the same corners on the page.
pub(crate) fn assert_the_file_holds_the_models_route(dir: &Dir, s: &Session, what: &str) {
    let (back, _) = read_back(&dir.read("a.odg"));
    let (model, file) = (page_corners(s.document().unwrap(), "K2"), page_corners(&back, "K2"));
    assert!(same_route(&model, &file), "{what}: K2 reopens with {file:?} for the model's {model:?}");
}

/// The drawing as built is what the vectors say: K2 is read with the corners (4, 10), (4, 12), (9, 12), (9, 6), (11, 6) and (11, 7), no hop, and K1 crosses its leg at (9, 8.5).
#[test]
fn the_twin_drawing_is_the_vectors_k2() {
    let dir = Dir::new();
    let s = open_drawing(&dir, &twin_drawing());
    let tag = k2_tag(&dir);
    assert!(is_the_files(&tag) && ends_are(&tag, (4.0, 10.0, 11.0, 7.0)), "{tag}");
    assert!(!has_hop(s.document().unwrap(), "K2"), "the file's own rows have no hop");
    let corners: Vec<(f64, f64)> =
        page_corners(s.document().unwrap(), "K2").iter().map(|p| (p.0 * 2.54, (page_cell(&s, "PageHeight") - p.1) * 2.54)).collect();
    let wanted = [(4.0, 10.0), (4.0, 12.0), (9.0, 12.0), (9.0, 6.0), (11.0, 6.0), (11.0, 7.0)];
    assert_eq!(corners.len(), wanted.len(), "{corners:?}");
    for (c, w) in corners.iter().zip(wanted) {
        assert!(near(*c, w), "{corners:?}");
    }
    let (begin, end) = ends_in_cm(&s);
    assert!(near(begin, (4.0, 10.0)) && near(end, (11.0, 7.0)), "{begin:?} {end:?}");
    // Moved no further than the edits below: nothing is written for a save with no edit.
    let mut s = s;
    assert_eq!(save(&mut s)["edited"], false);
}

/// V58, through the commands (for R22, a hop made and then taken away; "V-a"). Edit 1: U is moved 1 cm right (K2's begin goes with it and the engine routes
/// K2 anew) and the page's jumps are the vertical lines' (`connector.line_jumps` code 2), so that K2 holds a hop on an upright leg where K1 crosses it, in either order. Save
/// 1 keeps K2's `svg:d`, `svg:viewBox` and `draw:line-skew` as the file has them, writes `svg:x1="5cm"` and says the route could not be written, once, of K2. A save after it
/// is no edit and says nothing more. Edit 2 takes the hop away with K2's corners as they are (the jumps off, or K1 moved off the leg). Save 2 writes K2's route: a path that
/// begins at (5, 10), where U's bottom is now, ends at (11, 7), and reads back as the model's, with no `draw:line-skew` and no word; a save after it is no edit. A writer that
/// compares the model of save 2 only with the model as save 1 saw it finds the corners equal and writes nothing, and K2 reads back with its old path, which begins 1 cm left of U.
#[test]
fn a_hop_the_engine_made_on_k2_is_kept_with_a_word_and_the_route_is_written_when_it_goes() {
    for order in ["jumps, then U moved", "U moved, then jumps"] {
        for way in ["jumps off", "K1 moved off"] {
            let label = format!("{order}, then {way}");
            let dir = Dir::new();
            let mut s = open_drawing(&dir, &twin_drawing());
            let u = id_of(s.document().unwrap(), 0, "U");
            let move_u = |s: &mut Session| drop(run(s, "shape.move", json!({ "ids": [u], "dx": inches(1.0), "dy": 0.0 })));
            if order.starts_with("jumps") {
                vertical_lines_hop(&mut s);
                move_u(&mut s);
            } else {
                move_u(&mut s);
                vertical_lines_hop(&mut s);
            }
            assert!(has_hop(s.document().unwrap(), "K2"), "{label}: K2 holds a hop");
            assert!(!has_hop(s.document().unwrap(), "K1"), "{label}: K1, the level one, holds none");
            let (begin, end) = ends_in_cm(&s);
            assert!(near(begin, (5.0, 10.0)) && near(end, (11.0, 7.0)), "{label}: K2's begin goes with U: {begin:?} {end:?}");
            let mut told = Told::new();
            let first = save(&mut s);
            assert_eq!(first["edited"], true, "{label}: {first}");
            assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, &label), Vec::<String>::new());
            let said = warnings(&first);
            assert_eq!(said.len(), 1, "{label}: the hop is said once, of K2: {first}");
            assert!(said[0].contains(&format!("the route of connector {} ", k2_of(&s))) && said[0].contains(HOP_WORDS), "{label}: {}", said[0]);
            let kept = k2_tag(&dir);
            assert!(is_the_files(&kept), "{label}: K2's path, view box and skew are the file's: {kept}");
            assert!(ends_are(&kept, (5.0, 10.0, 11.0, 7.0)), "{label}: K2's begin is 1 cm further right and its end where it was: {kept}");
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
            // Edit 2: the hop goes, no end of K2 moves.
            take_the_hop_away(&mut s, way);
            let second = save(&mut s);
            assert_eq!(warnings(&second), Vec::<String>::new(), "{label}: the route can be written now: {second}");
            assert_eq!(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, &label), Vec::<String>::new(), "{label}");
            let tag = k2_tag(&dir);
            assert!(!tag.contains("draw:line-skew"), "{label}: routed anew, so no skew: {tag}");
            eprintln!("V58 {label}: save 2 wrote {}", tag.split(" svg:d=").nth(1).unwrap_or(""));
            assert!(tag.contains("svg:d=\"M 5000 10000 "), "{label}: the route begins where U's bottom is now, 1 cm right of the file's path: {tag}");
            assert!(!tag.contains(K2_PATH) && !tag.contains(K2_BOX), "{label}: the file's path is gone: {tag}");
            assert_the_file_holds_the_models_route(&dir, &s, &label);
            let (back, _) = read_back(&dir.read("a.odg"));
            let corners = page_corners(&back, "K2");
            let (first_corner, last_corner) = (corners[0], corners[corners.len() - 1]);
            let height = page_cell(&s, "PageHeight");
            assert!(
                near((first_corner.0 * 2.54, (height - first_corner.1) * 2.54), (5.0, 10.0)),
                "{label}: the file's K2 begins at {first_corner:?}"
            );
            assert!(near((last_corner.0 * 2.54, (height - last_corner.1) * 2.54), (11.0, 7.0)), "{label}: and ends at {last_corner:?}");
            assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
            assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        }
    }
}

/// V59, through the commands (for R22, a waypoint moved by hand; "V-b"). Edit 1: `connector.edit_path` moves K2's leg from (9, 12) to (9, 6) to x = 9.5
/// (K2's ends do not move) and the page's jumps are the vertical lines', so that K2 holds a hop on the moved leg where K1 crosses it. The corners are those of the vector:
/// (4, 10), (4, 12), (9.5, 12), (9.5, 6), (11, 6) and (11, 7). Save 1 keeps K2's path, view box and skew as the file has them, writes no end, and says the route could not
/// be written, once. Edit 2 takes the hop away (the jumps off, or K1 moved off the leg), the corners as they are. Save 2 writes `svg:d="M 4000 10000 v 2000 h 5500 v -6000
/// h 1500 v 1000"` and `svg:viewBox="0 0 7001 6001"` for K2, no skew and no word. A writer that compares with the baseline of save 1 writes nothing.
#[test]
fn a_waypoint_moved_by_a_command_is_kept_with_a_word_and_the_route_is_written_when_the_hop_goes() {
    for way in ["jumps off", "K1 moved off"] {
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &twin_drawing());
        let k2 = k2_of(&s);
        assert_eq!(run(&mut s, "connector.edit_path", json!({ "id": k2, "leg": 2, "to": inches(9.5) }))["changed"], true, "{way}");
        vertical_lines_hop(&mut s);
        assert!(has_hop(s.document().unwrap(), "K2"), "{way}: K2 holds a hop on the moved leg");
        let height = page_cell(&s, "PageHeight");
        let corners: Vec<(f64, f64)> = page_corners(s.document().unwrap(), "K2").iter().map(|p| (p.0 * 2.54, (height - p.1) * 2.54)).collect();
        let wanted = [(4.0, 10.0), (4.0, 12.0), (9.5, 12.0), (9.5, 6.0), (11.0, 6.0), (11.0, 7.0)];
        assert_eq!(corners.len(), wanted.len(), "{way}: {corners:?}");
        for (c, w) in corners.iter().zip(wanted) {
            assert!(near(*c, w), "{way}: the corners of the vector: {corners:?}");
        }
        let (begin, end) = ends_in_cm(&s);
        assert!(near(begin, (4.0, 10.0)) && near(end, (11.0, 7.0)), "{way}: K2's ends do not move: {begin:?} {end:?}");
        let mut told = Told::new();
        let first = save(&mut s);
        assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, way), Vec::<String>::new());
        let said = warnings(&first);
        assert_eq!(said.len(), 1, "{way}: the hop is said once, of K2: {first}");
        assert!(said[0].contains(&format!("the route of connector {k2} ")) && said[0].contains(HOP_WORDS), "{way}: {}", said[0]);
        let kept = k2_tag(&dir);
        assert!(is_the_files(&kept), "{way}: K2's path, view box and skew are the file's: {kept}");
        assert!(ends_are(&kept, (4.0, 10.0, 11.0, 7.0)), "{way}: K2's ends are as they were: {kept}");
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
        take_the_hop_away(&mut s, way);
        let second = save(&mut s);
        assert_eq!(warnings(&second), Vec::<String>::new(), "{way}: the route can be written now: {second}");
        assert_eq!(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, way), Vec::<String>::new(), "{way}");
        let tag = k2_tag(&dir);
        assert!(tag.contains("svg:d=\"M 4000 10000 v 2000 h 5500 v -6000 h 1500 v 1000\""), "{way}: the vector's path: {tag}");
        assert!(tag.contains("svg:viewBox=\"0 0 7001 6001\""), "{way}: the vector's view box: {tag}");
        assert!(!tag.contains("draw:line-skew"), "{way}: routed anew, so no skew: {tag}");
        assert_the_file_holds_the_models_route(&dir, &s, way);
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// V46 through the commands, as the rule now has it (for R22, a page that grows or shrinks): the page's height is set 2 cm greater, or 3 cm
/// less, with `page.size`, which keeps every shape's distance from the page's bottom. No edit touches K1 or K2, so each keeps its own path **shifted** by the change (K2's `M`
/// moves and its relative steps do not: `M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000`; K1's absolute points all move), its `svg:viewBox` as it was, and K2 its
/// `draw:line-skew`; the ends are written against the new height (K2's `svg:y1="12cm"` and `svg:y2="9cm"`, the x values as they were), nothing is said, and the file reads
/// back with the routes the model has. A save after it is no edit. (Before a change both were written anew from their cells, and K2's skew went.)
#[test]
fn a_page_that_grows_or_shrinks_leaves_the_connectors_no_edit_touched_their_paths_shifted_and_k2_its_skew() {
    for (by_cm, y1, y2, path, k1_path) in [
        (2.0, 12.0, 9.0, "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000", "M 2500 10500 L 14000 10500"),
        (-3.0, 7.0, 4.0, "M 4000 7000 v 2000 h 5000 v -6000 h 2000 v 1000", "M 2500 5500 L 14000 5500"),
    ] {
        let label = format!("the page {by_cm} cm");
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &twin_drawing());
        let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
        let original = content_of(&dir.read("a.odg"));
        size_page(&mut s, width, was + inches(by_cm));
        let saved = save(&mut s);
        assert_eq!(saved["edited"], true, "{label}: {saved}");
        assert_eq!(warnings(&saved), Vec::<String>::new(), "{label}: nothing is said: {saved}");
        let tag = k2_tag(&dir);
        assert!(tag.contains(&format!("svg:d=\"{path}\"")), "{label}: K2's path, shifted: {tag}");
        assert!(tag.contains(K2_BOX) && tag.contains(K2_SKEW), "{label}: K2's view box and skew are the file's: {tag}");
        assert!(ends_are(&tag, (4.0, y1, 11.0, y2)), "{label}: K2's ends are {by_cm} cm lower and the x values as they were: {tag}");
        let k1 = connector_named(&content_of(&dir.read("a.odg")), "K1");
        assert!(k1.contains(&format!("svg:d=\"{k1_path}\"")) && k1.contains("svg:viewBox=\"0 0 11501 1\""), "{label}: K1's path, shifted: {k1}");
        assert!(ends_are(&k1, (2.5, 8.5 + by_cm, 14.0, 8.5 + by_cm)), "{label}: K1's ends: {k1}");
        // Nothing but the ends and the path of each connector, and the shapes' own places, changed in the part.
        let (before, after) = (connector_named(&original, "K2"), k2_tag(&dir));
        let strip = |tag: &str| {
            ["svg:y1=\"", "svg:y2=\"", "svg:d=\""].iter().fold(tag.to_owned(), |t, a| match t.find(a) {
                Some(at) => format!("{}{}", &t[..at], &t[at + a.len() + t[at + a.len()..].find('"').map_or(0, |q| q + 1)..]),
                None => t,
            })
        };
        assert_eq!(strip(&before), strip(&after), "{label}: nothing else of K2 changed");
        let (back, _) = read_back(&dir.read("a.odg"));
        let height = back.pages.get(0).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap();
        assert!((height - (was + inches(by_cm))).abs() < 1e-6, "{label}: the page's new height is written: {height}");
        for name in ["K1", "K2"] {
            let (model, file) = (page_corners(s.document().unwrap(), name), page_corners(&back, name));
            assert!(same_route(&model, &file), "{label}: {name} reopens with {file:?} for the model's {model:?}");
        }
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}

/// V63 through the commands, as the rule now has it (for R22, a page that grows while a route with a hop is kept;
/// "V-e"). The page's height is set 2 cm greater with `page.size` (the shapes keep their distance from the page's bottom, so each point stands 2 cm further down the page) and
/// the jumps are the vertical lines', so that K1 crossing K2's leg gives K2 a hop. Save 1 keeps K2's `svg:d` **shifted 2 cm** (`M 4000 12000 v 2000 h 5000 v -6000 h 2000 v
/// 1000`), `svg:viewBox` and `draw:line-skew` as the file has them, writes K2's ends 2 cm lower (`svg:y1="12cm"` and `svg:y2="9cm"`, the x values as they were) and says the
/// route could not be written, once; the page's height is written. K1 then moves off the leg (the hop goes), with no other change. Save 2 writes nothing of K2 (the file holds
/// the route the model has, with its skew) and says nothing. With the height 3 cm less, the path starts `M 4000 7000`.
#[test]
fn a_page_that_grows_or_shrinks_while_a_route_with_a_hop_is_kept_has_its_path_shifted_with_it() {
    for (by_cm, y1, y2, path) in
        [(2.0, 12.0, 9.0, "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000"), (-3.0, 7.0, 4.0, "M 4000 7000 v 2000 h 5000 v -6000 h 2000 v 1000")]
    {
        let label = format!("the page {by_cm} cm");
        let dir = Dir::new();
        let mut s = open_drawing(&dir, &twin_drawing());
        let (width, was) = (page_cell(&s, "PageWidth"), page_cell(&s, "PageHeight"));
        size_page(&mut s, width, was + inches(by_cm));
        vertical_lines_hop(&mut s);
        assert!(has_hop(s.document().unwrap(), "K2"), "{label}: K1 crosses K2's leg, which holds a hop");
        let mut told = Told::new();
        let first = save(&mut s);
        assert_eq!(first["edited"], true, "{label}: {first}");
        assert_eq!(check_told(&s, &dir, "a.odg", &first, &["K1", "K2"], &mut told, &label), Vec::<String>::new());
        let said = warnings(&first);
        assert_eq!(said.len(), 1, "{label}: the hop is said once, of K2: {first}");
        assert!(said[0].contains(&format!("the route of connector {} ", k2_of(&s))) && said[0].contains(HOP_WORDS), "{label}: {}", said[0]);
        let kept = k2_tag(&dir);
        assert!(
            kept.contains(&format!("svg:d=\"{path}\"")) && kept.contains(K2_BOX) && kept.contains(K2_SKEW),
            "{label}: K2's path is the file's, shifted {by_cm} cm, with its view box and skew: {kept}"
        );
        assert!(ends_are(&kept, (4.0, y1, 11.0, y2)), "{label}: K2's ends are {by_cm} cm lower and the x values as they were: {kept}");
        let (back, _) = read_back(&dir.read("a.odg"));
        let height = back.pages.get(0).unwrap().sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap();
        assert!((height - (was + inches(by_cm))).abs() < 1e-6, "{label}: the page's new height is written: {height}");
        // The path is where the model's corners are, so the file reads back with them (the hop being what it has no place for).
        let (model, file) = (page_corners(s.document().unwrap(), "K2"), page_corners(&back, "K2"));
        assert!(same_route(&model, &file), "{label}: K2 reopens with {file:?} for the model's {model:?}");
        // K1 moves off the leg: the hop goes, K2's ends and corners as they are.
        take_the_hop_away(&mut s, "K1 moved off");
        let second = save(&mut s);
        assert_eq!(warnings(&second), Vec::<String>::new(), "{label}: nothing is said: {second}");
        assert_eq!(check_told(&s, &dir, "a.odg", &second, &["K1", "K2"], &mut told, &label), Vec::<String>::new(), "{label}");
        assert_eq!(k2_tag(&dir), kept, "{label}: nothing of K2 is written: the file holds the route the model has");
        assert_the_file_holds_the_models_route(&dir, &s, &label);
        assert_reopens_as_saved(&dir, &mut s, "a.odg", 0);
        assert_saves_again_as_no_edit(&dir, &mut s, "a.odg");
    }
}
