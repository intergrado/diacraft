//! (corrected 2026-11-20; the bullet on a page height change and the connectors no edit touched, the `svg:x1` and
//! `svg:d` rows): across a change of the page's height the splice writer keeps the path of a connector no edit touched, **shifted**.
//!
//! A height edit keeps `PinY`, so every shape and every connector end moves down the page by `dh` in the file's top-left frame. A connector no edit touched keeps its own path
//! through it: its `svg:d` is shifted by `dh` (in the relative form DiaCraft and LibreOffice write the first absolute `M` moves and the relative steps do not; in a path of
//! absolute points every point moves), its `svg:viewBox` is as it was, its `draw:line-skew` is kept, and its ends move as R8 says. Written anew from its cells, as
//! the writer did before, it lost the skew, and a program that works out a connector's legs from its type and skew drew them elsewhere. A connector whose route has a hop, kept
//! through a height change, keeps its path shifted too, with the word it was kept with while the hop is in the model. A width change moves nothing.
//!
//! The vectors' K2 (a connector glued by both ends whose file holds `draw:line-skew="0.5cm"`, `svg:viewBox="0 0 7001 6001"` and `svg:d="M 4000 10000 v 2000 h 5000 v -6000
//! h 2000 v 1000"`, begin (4, 10) cm and end (11, 7)) is built by hand, with the model's rows set by hand where a vector says so. Every case compares the bytes of the saved
//! file with the file's own, so a writer that echoes the file, or writes every connector anew, fails each of them.

use diacraft_doc::Document;
use diacraft_odg::SaveNote;

use crate::drawing::Drawing;
use crate::write_core::{Open, num_of, open, part, set_num, shape_mut};
use crate::write_untouched_routes::{
    K2, K2_FILE, PAGE_HEIGHT, corners_of, custom, open_of, page_taller, page_wider, same_corners, save, save_again, set_route, set_route_on, tag_of,
    w1_drawing,
};

/// K2's path in the vectors, its view box and its skew.
const PATH: &str = "M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000";

/// The shape id of K2 in [`vector_drawing`]: U is 1 and E is 2.
const KV: u32 = 3;

/// The vectors' drawing: U (x 3 to 5, y 9 to 10) holds K2's begin at its bottom, E (x 10 to 12, y 7 to 8) its end at its top; K2 has the path `d`.
fn vector_drawing(d: &str) -> Drawing {
    let k2 = format!(
        "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"11cm\" svg:y2=\"7cm\" svg:d=\"{d}\" svg:viewBox=\"0 0 7001 6001\"><text:p/></draw:connector>"
    );
    Drawing::a4().page(&[custom("U", "u", (3.0, 9.0, 2.0, 1.0)), custom("E", "e", (10.0, 7.0, 2.0, 1.0)), k2].concat())
}

/// The model's rows of K2 in the vectors, the points of the page in centimetres from its top left (the page the file has): the corners (4, 10), (4, 12), (9, 12), (9, 6),
/// (11, 6) and (11, 7), with a hop (an `ArcTo` row, as the rebuild of line jumps makes one) on the leg from (9, 12) to (9, 6) when `hop` says so.
fn vector_rows(hop: bool) -> Vec<(&'static str, (f64, f64))> {
    let mut rows = vec![("MoveTo", (4.0, 10.0)), ("LineTo", (4.0, 12.0)), ("LineTo", (9.0, 12.0))];
    if hop {
        rows.extend([("LineTo", (9.0, 8.6)), ("ArcTo", (9.0, 8.4))]);
    }
    rows.extend([("LineTo", (9.0, 6.0)), ("LineTo", (11.0, 6.0)), ("LineTo", (11.0, 7.0))]);
    rows
}

/// The model with the page `by_cm` taller and K2's rows as `vector_rows` gives them: **a height edit keeps `PinY`**, so the route's cells (inches, y up) are the same
/// whatever the page's height, and `set_route` measures them from the top of the page the file has.
fn vector_model(doc: &Document, by_cm: f64, hop: bool) -> Document {
    let mut doc = page_taller(doc, by_cm);
    set_route(shape_mut(&mut doc, 0, KV), &vector_rows(hop));
    doc
}

fn k2_tag(w: &diacraft_odg::Written) -> String {
    tag_of(&part(&w.bytes, "content.xml"), "K2").to_owned()
}

/// The text of the attribute `name` of a start tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(&tag[at..at + tag[at..].find('"')?])
}

/// An attribute holding a length in centimetres, as a number.
fn cm_of(tag: &str, name: &str) -> f64 {
    attr(tag, name).and_then(|v| v.strip_suffix("cm")).and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("no {name} in centimetres: {tag}"))
}

/// Whether the four ends of a start tag are `(x1, y1, x2, y2)` centimetres, to a thousandth of a millimetre.
fn ends_are(tag: &str, (x1, y1, x2, y2): (f64, f64, f64, f64)) -> bool {
    [("svg:x1", x1), ("svg:y1", y1), ("svg:x2", x2), ("svg:y2", y2)].iter().all(|(name, want)| (cm_of(tag, name) - want).abs() < 1e-4)
}

/// The tag with the attributes `names` taken out: what a save that touched only those must have left as it was.
fn without(tag: &str, names: &[&str]) -> String {
    let mut text = tag.to_owned();
    for name in names {
        let key = format!(" {name}=\"");
        if let Some(at) = text.find(&key) {
            let end = at + key.len() + text[at + key.len()..].find('"').map_or(0, |q| q + 1);
            text.replace_range(at..end, "");
        }
    }
    text
}

fn path_of(tag: &str) -> &str {
    attr(tag, "svg:d").unwrap_or("")
}

/// As the rule now has them (for R22, a page that grows or shrinks): K2 no edit touched, the page's `PageHeight` set `by` cm greater. Expect
/// `svg:d` shifted by `by` cm (**the first absolute `M` moves and the relative steps do not**), `svg:viewBox` and `draw:line-skew` as the file has them, the ends 2 cm lower
/// (`svg:y1` and `svg:y2`, the x values as they were), no word, and nothing else of the element changed. The model's corners are the file's own, so the file reads back
/// with them and a save of the same model after it is no edit. Before a change the connector was written anew from its cells: `svg:d` at the same corners and no skew.
#[test]
fn a_page_that_grows_or_shrinks_leaves_the_connector_no_edit_touched_its_path_shifted_with_its_view_box_and_skew() {
    for (by, y1, y2, start) in [(2.0, 12.0, 9.0, "M 4000 12000"), (-3.0, 7.0, 4.0, "M 4000 7000"), (0.5, 10.5, 7.5, "M 4000 10500")] {
        let o = open(&vector_drawing(PATH));
        let before = tag_of(&part(&o.bytes, "content.xml"), "K2").to_owned();
        let doc = page_taller(&o.doc, by);
        let saved = save(&doc, &o);
        assert!(saved.edited && saved.notes.is_empty(), "{by}: {:?}", saved.notes);
        let tag = k2_tag(&saved);
        assert_eq!(path_of(&tag), format!("{start} v 2000 h 5000 v -6000 h 2000 v 1000"), "{by}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 7001 6001"), "{by}: the view box is as it was");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{by}: the skew is kept");
        assert!(ends_are(&tag, (4.0, y1, 11.0, y2)), "{by}: the ends are {by} cm lower and the x values as they were: {tag}");
        assert_eq!(
            without(&tag, &["svg:y1", "svg:y2", "svg:d"]),
            without(&before, &["svg:y1", "svg:y2", "svg:d"]),
            "{by}: nothing else of K2 changed"
        );
        // The file reads back with the model's route, and the model saved again is no edit.
        let back = open_of(&saved);
        assert!(same_corners(&corners_of(&back.doc, KV), &corners_of(&doc, KV)), "{by}: K2 reopens with the model's route");
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{by}: {:?}", again.notes);
    }
}

/// A path of absolute points: every point moves (a `K2_FILE`: `M`, then `L` and absolute coordinates only). The view box and the skew are as they were.
#[test]
fn a_path_of_absolute_points_has_every_point_shifted() {
    let o = open(&w1_drawing(K2_FILE));
    let doc = page_taller(&o.doc, 2.0);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = tag_of(&part(&saved.bytes, "content.xml"), "K2").to_owned();
    assert_eq!(path_of(&tag), "M 9744 13192 L 9744 13500 L 9744 13804 L 7305 13804 L 7305 6172 L 7805 6172", "{tag}");
    assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 2501 7633"));
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0cm -0.612cm"));
    assert!(same_corners(&corners_of(&open_of(&saved).doc, K2), &corners_of(&doc, K2)));
    let again = save_again(&doc, &saved);
    assert!(!again.edited && again.notes.is_empty(), "{:?}", again.notes);
}

/// A connector whose path holds curves, arcs and every kind of command: what is absolute and stands for a `y` moves, and the rest is copied as the file wrote it (the
/// relative form: the `c` after the first `M` is untouched; absolute `C`, `S`, `Q`, `T`, `A` and `V` have each `y` shifted, `H` and the arc's radii and flags do not).
#[test]
fn a_curved_connector_has_its_path_shifted_by_the_commands_that_are_absolute() {
    let cases = [
        ("a relative cubic", "M 4000 10000 c 0 1000 5000 1000 5000 0", "M 4000 12000 c 0 1000 5000 1000 5000 0"),
        ("an absolute cubic", "M 4000 10000 C 4000 11000 9000 11000 9000 10000", "M 4000 12000 C 4000 13000 9000 13000 9000 12000"),
        ("a smooth cubic and a quadratic", "M 0 0 S 10 20 30 40 Q 5 6 7 8 T 9 10", "M 0 2000 S 10 2020 30 2040 Q 5 2006 7 2008 T 9 2010"),
        ("an arc", "M 1000 1000 A 500 300 0 0 1 2000 1000", "M 1000 3000 A 500 300 0 0 1 2000 3000"),
        ("horizontal and vertical lines, absolute and relative", "M 1 2 H 5 V 9 h 3 v 4", "M 1 2002 H 5 V 2009 h 3 v 4"),
        ("a closed subpath and a second one", "M 1 2 L 3 4 Z M 5 6 l 7 8 z", "M 1 2002 L 3 2004 Z M 5 2006 l 7 8 z"),
        ("numbers that run together and a sign that separates", "M 1,2 L3-4 5-6", "M 1,2002 L3 1996 5 1994"),
        ("pairs after a move", "M 1 2 3 4 5 6", "M 1 2002 3 2004 5 2006"),
        ("a relative first move is absolute, the pairs after it are relative", "m 1 2 3 4 5 6", "m 1 2002 3 4 5 6"),
        ("a fraction and an exponent", "M 0.5 1.25e2 L 2e1 3.5", "M 0.5 2125 L 2e1 2003.5"),
    ];
    for (what, d, shifted) in cases {
        let o = open(&Drawing::a4().page(&format!(
            "<draw:connector draw:name=\"C\" draw:type=\"curve\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"9cm\" svg:y2=\"10cm\" svg:d=\"{d}\" svg:viewBox=\"0 0 5001 751\"><text:p/></draw:connector>"
        )));
        let doc = page_taller(&o.doc, 2.0);
        let saved = save(&doc, &o);
        // A second subpath reads as a second `MoveTo` in the route, the form a hop has, and is no hop: the path is shifted, and nothing is said.
        assert!(saved.edited && saved.notes.is_empty(), "{what}: {:?}", saved.notes);
        let tag = tag_of(&part(&saved.bytes, "content.xml"), "C").to_owned();
        assert_eq!(path_of(&tag), shifted, "{what}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 5001 751"), "{what}");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{what}");
        assert!(ends_are(&tag, (4.0, 12.0, 9.0, 12.0)), "{what}: {tag}");
        // The file reads back with the route the model has, and the same model saved again is no edit.
        assert!(same_corners(&corners_of(&open_of(&saved).doc, 1), &corners_of(&doc, 1)), "{what}: the file reopens with the model's route");
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{what}: {:?}", again.notes);
    }
}

/// The owner's ruling of 2026-11-20: **a route kept through a change of the page's height is not named again**. A connector whose `svg:d` has two subpaths reads as a
/// route with a second `MoveTo`, the form a hop has, but it is no hop and the connector no edit touched: at +2 cm, then +3, then back at the file's own height, its path
/// is shifted by each change (and is the file's own bytes at 0), its view box and skew are as they were, the file reads back with the route the model has, and the save says
/// nothing at any of them. Before the ruling the save said the route could not be written at every one of them.
#[test]
fn a_path_of_two_subpaths_is_shifted_at_each_height_change_and_never_named() {
    let d = "M 2000 2000 L 4000 2000 M 4000 3000 L 6000 4000";
    let o = open(&Drawing::a4().page(&format!(
        "<draw:connector draw:name=\"C\" draw:line-skew=\"0.5cm\" svg:x1=\"2cm\" svg:y1=\"2cm\" svg:x2=\"6cm\" svg:y2=\"4cm\" svg:d=\"{d}\" svg:viewBox=\"0 0 4001 2001\"><text:p/></draw:connector>"
    )));
    let tag_of_c = |w: &diacraft_odg::Written| tag_of(&part(&w.bytes, "content.xml"), "C").to_owned();
    let mut last: Option<diacraft_odg::Written> = None;
    for (by, want) in [
        (2.0, "M 2000 4000 L 4000 4000 M 4000 5000 L 6000 6000"),
        (3.0, "M 2000 5000 L 4000 5000 M 4000 6000 L 6000 7000"),
        (0.0, d),
        (-2.0, "M 2000 0 L 4000 0 M 4000 1000 L 6000 2000"),
    ] {
        let doc = page_taller(&o.doc, by);
        let saved = match &last {
            None => save(&doc, &o),
            Some(last) => save_again(&doc, last),
        };
        assert!(saved.edited && saved.notes.is_empty(), "{by}: nothing is said: {:?}", saved.notes);
        let tag = tag_of_c(&saved);
        assert_eq!(path_of(&tag), want, "{by}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 4001 2001"), "{by}");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{by}");
        assert!(ends_are(&tag, (2.0, 2.0 + by, 6.0, 4.0 + by)), "{by}: {tag}");
        assert!(same_corners(&corners_of(&open_of(&saved).doc, 1), &corners_of(&doc, 1)), "{by}: the file reopens with the model's route");
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{by}: {:?}", again.notes);
        last = Some(saved);
    }
}

/// A connector with no `svg:d`: nothing to shift. The ends move (the `svg:x1` row), no path is made, and its skew is kept (nothing was routed anew), with no word.
#[test]
fn a_connector_with_no_path_has_its_ends_moved_and_no_path_made() {
    let o = open(&Drawing::a4().page(
        "<draw:connector draw:name=\"N\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"9cm\" svg:y2=\"12cm\"><text:p/></draw:connector>",
    ));
    let before = tag_of(&part(&o.bytes, "content.xml"), "N").to_owned();
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = tag_of(&part(&saved.bytes, "content.xml"), "N").to_owned();
    assert!(ends_are(&tag, (4.0, 12.0, 9.0, 14.0)), "{tag}");
    assert!(attr(&tag, "svg:d").is_none() && attr(&tag, "svg:viewBox").is_none(), "no path is made: {tag}");
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "the skew is kept");
    assert_eq!(without(&tag, &["svg:y1", "svg:y2"]), without(&before, &["svg:y1", "svg:y2"]), "nothing else of it changed");
    let again = save_again(&page_taller(&o.doc, 2.0), &saved);
    assert!(!again.edited && again.notes.is_empty(), "{:?}", again.notes);
}

/// V63 as the rule now has it (a route with a hop that is kept through a height change keeps its path shifted by `dh` as well): K2 holds a hop and the page is 2 cm
/// taller in one save. Expect save 1 to keep K2's `svg:d` **shifted** 2 cm, `svg:viewBox` and `draw:line-skew` as the file has them, to write the ends 2 cm lower, and to say
/// `RouteNotWritten` once. The hop then goes with no other change: expect save 2 to write nothing of K2 (the model's corners are the file's) and to say nothing. With the
/// page 3 cm shorter, the path starts `M 4000 7000`.
#[test]
fn a_route_with_a_hop_kept_through_a_height_change_has_its_path_shifted_too() {
    for (by, start, y1, y2) in [(2.0, "M 4000 12000", 12.0, 9.0), (-3.0, "M 4000 7000", 7.0, 4.0)] {
        let o = open(&vector_drawing(PATH));
        let first = save(&vector_model(&o.doc, by, true), &o);
        assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }], "{by}: the hop is said once");
        let kept = k2_tag(&first);
        assert_eq!(path_of(&kept), format!("{start} v 2000 h 5000 v -6000 h 2000 v 1000"), "{by}: the file's path, shifted: {kept}");
        assert_eq!(attr(&kept, "svg:viewBox"), Some("0 0 7001 6001"), "{by}");
        assert_eq!(attr(&kept, "draw:line-skew"), Some("0.5cm"), "{by}");
        assert!(ends_are(&kept, (4.0, y1, 11.0, y2)), "{by}: {kept}");
        // The file's path is where the model's corners are, so the file reads back with them (the hop being what it has no place for).
        assert!(same_corners(&corners_of(&open_of(&first).doc, KV), &corners_of(&vector_model(&o.doc, by, false), KV)), "{by}");
        // A save with no edit says nothing more.
        let again = save_again(&vector_model(&o.doc, by, true), &first);
        assert!(!again.edited && again.notes.is_empty(), "{by}: {:?}", again.notes);
        // The hop goes: the route is the file's, nothing is written and nothing said.
        let second = save_again(&vector_model(&o.doc, by, false), &first);
        assert!(!second.edited && second.notes.is_empty(), "{by}: {:?}", second.notes);
        assert_eq!(k2_tag(&second), kept, "{by}");
    }
}

/// V64 as the rule now has it (the kept route in the page of now; a control at the writer: no command makes this route). Given the save 1, then a model whose K2 rows
/// put the corners, in the top-left measure of the taller page, at (4, 12), (4, 14), (9, 14), (9, 8), (11, 8) and (11, 9), the file's path where save 1 shifted it to, with
/// no hop: expect save 2 to write nothing of K2 and to say nothing. The corners where the file's path stood before it was shifted, (4, 10) to (11, 7), are a route the file
/// does not have: they are written, and the skew goes.
#[test]
fn the_kept_route_in_the_page_of_now_is_where_the_save_shifted_the_path_to() {
    let o = open(&vector_drawing(PATH));
    let first = save(&vector_model(&o.doc, 2.0, true), &o);
    let kept = k2_tag(&first);
    let at = |dy: f64| -> Vec<(&'static str, (f64, f64))> { vector_rows(false).into_iter().map(|(kind, (x, y))| (kind, (x, y + dy))).collect() };
    let model = |dy: f64| {
        let mut doc = page_taller(&o.doc, 2.0);
        set_route_on(shape_mut(&mut doc, 0, KV), &at(dy), PAGE_HEIGHT + 2.0);
        doc
    };
    let same = save_again(&model(2.0), &first);
    assert!(!same.edited && same.notes.is_empty(), "the file's path is drawn here: {:?}", same.notes);
    assert_eq!(k2_tag(&same), kept);
    let moved = save_again(&model(0.0), &first);
    assert!(moved.edited && moved.notes.is_empty(), "{:?}", moved.notes);
    let tag = k2_tag(&moved);
    assert_eq!(path_of(&tag), "M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000", "the route written in the page of now: {tag}");
    assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 7001 6001"));
    assert!(attr(&tag, "draw:line-skew").is_none(), "routed anew: {tag}");
}

/// A chain of height changes and saves (heights as centimetres more than the file's, then back to it): after each save K2's path is the file's shifted by the sum, its view
/// box and skew as they were, no word; a save of the same model after each is no edit; and back at the file's own height K2's `svg:d` is the file's own bytes again. A shift
/// that is not a whole number of hundredths of a millimetre is written with the decimals it has (to four), and taken back to the digit.
#[test]
fn a_chain_of_height_changes_and_saves_shifts_the_path_by_each_and_back() {
    let o = open(&vector_drawing(PATH));
    let mut last = save(&page_taller(&o.doc, 2.0), &o);
    assert_eq!(path_of(&k2_tag(&last)), "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000");
    for (by, y) in [(5.0, "15000"), (-1.0, "9000"), (0.0, "10000"), (3.0, "13000"), (0.0123456, "10012.3456"), (0.0, "10000"), (-12.0, "-2000")] {
        let doc = page_taller(&o.doc, by);
        let saved = save_again(&doc, &last);
        assert!(saved.edited && saved.notes.is_empty(), "{by}: {:?}", saved.notes);
        let tag = k2_tag(&saved);
        assert_eq!(path_of(&tag), format!("M 4000 {y} v 2000 h 5000 v -6000 h 2000 v 1000"), "{by}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 7001 6001"), "{by}");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{by}");
        assert!(ends_are(&tag, (4.0, 10.0 + by, 11.0, 7.0 + by)), "{by}: {tag}");
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{by}: {:?}", again.notes);
        last = saved;
    }
    assert_eq!(
        attr(&k2_tag(&save_again(&page_taller(&o.doc, 0.0), &last)), "svg:d"),
        Some(PATH),
        "back at the file's height, K2's path is the file's bytes"
    );
}

/// The same chain with a hop in the model, which the `.odg` has no place for. **The hop is said once, in the save that keeps the file's path for it, and not again** (plan
/// the clause that said it at every save that changes the height): the saves after it that change the height only shift the file's path by
/// what the page grew, with the skew and view box as they were, and say nothing, and the file reads back with the route the file holds, which is the route the model has
/// with the hop left out. A save that changes nothing says nothing, and the hop going writes nothing (the corners are the file's). Then, at the file's own height, the
/// path is the file's bytes again; the hop coming back is said once and the path is kept.
#[test]
fn a_route_with_a_hop_is_kept_and_shifted_at_each_height_change_and_said_once() {
    let o = open(&vector_drawing(PATH));
    let mut last = save(&vector_model(&o.doc, 2.0, true), &o);
    assert_eq!(last.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }]);
    for (by, y, y1, y2) in [(5.0, "15000", 15.0, 12.0), (-1.0, "9000", 9.0, 6.0), (0.0, "10000", 10.0, 7.0), (3.0, "13000", 13.0, 10.0)] {
        let saved = save_again(&vector_model(&o.doc, by, true), &last);
        assert!(
            saved.edited && saved.notes.is_empty(),
            "{by}: the hop was said when it was kept, and a height change names it no more: {:?}",
            saved.notes
        );
        let tag = k2_tag(&saved);
        assert_eq!(path_of(&tag), format!("M 4000 {y} v 2000 h 5000 v -6000 h 2000 v 1000"), "{by}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 7001 6001"), "{by}");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{by}");
        assert!(ends_are(&tag, (4.0, y1, 11.0, y2)), "{by}: {tag}");
        // The geometry: the file reads back with the route it holds, the model's without the hop.
        assert!(same_corners(&corners_of(&open_of(&saved).doc, KV), &corners_of(&vector_model(&o.doc, by, false), KV)), "{by}: the file's route");
        let still = save_again(&vector_model(&o.doc, by, true), &saved);
        assert!(!still.edited && still.notes.is_empty(), "{by}: nothing said at a save that changes nothing: {:?}", still.notes);
        last = saved;
    }
    // The hop goes at the page's own height: the file has this route.
    let at_the_files = save_again(&vector_model(&o.doc, 0.0, true), &last);
    let gone = save_again(&vector_model(&o.doc, 0.0, false), &at_the_files);
    assert!(!gone.edited && gone.notes.is_empty(), "{:?}", gone.notes);
    // The hop goes at another height: the page's change is written, the path shifted, nothing said.
    let hopped = save_again(&vector_model(&o.doc, 4.0, true), &at_the_files);
    assert!(hopped.notes.is_empty(), "the hop stays and the height changes: {:?}", hopped.notes);
    let gone = save_again(&vector_model(&o.doc, 7.0, false), &hopped);
    assert!(gone.edited && gone.notes.is_empty(), "the height changed, the hop went: {:?}", gone.notes);
    let tag = k2_tag(&gone);
    assert_eq!(path_of(&tag), "M 4000 17000 v 2000 h 5000 v -6000 h 2000 v 1000", "{tag}");
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "the connector was never routed anew: {tag}");
    assert!(same_corners(&corners_of(&open_of(&gone).doc, KV), &corners_of(&vector_model(&o.doc, 7.0, false), KV)), "the file's route");
    // The file's own height again, the hop back: said, the path the file's own bytes.
    let back = save_again(&vector_model(&o.doc, 0.0, true), &gone);
    assert_eq!(back.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }]);
    assert_eq!(attr(&k2_tag(&back), "svg:d"), Some(PATH));
}

/// A width change moves nothing (`svg:x` is from the left edge): K2's bytes are the file's, the width is written, nothing is said, and a hop kept through it is not
/// shifted either. A width and a height in one save shift by the height alone.
#[test]
fn a_width_change_moves_nothing_and_a_height_change_with_it_shifts_by_the_height_alone() {
    for by in [3.0, -2.0] {
        let o = open(&vector_drawing(PATH));
        let before = tag_of(&part(&o.bytes, "content.xml"), "K2").to_owned();
        let saved = save(&page_wider(&o.doc, by), &o);
        assert!(saved.edited && saved.notes.is_empty(), "{by}: the width is written: {:?}", saved.notes);
        assert_eq!(k2_tag(&saved), before, "{by}: K2 is the file's, byte for byte");
        let hop = save(
            &{
                let mut doc = page_wider(&o.doc, by);
                set_route(shape_mut(&mut doc, 0, KV), &vector_rows(true));
                doc
            },
            &o,
        );
        assert_eq!(hop.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }], "{by}");
        assert_eq!(k2_tag(&hop), before, "{by}: a hop kept through a width change is not shifted");
        let both = page_taller(&page_wider(&o.doc, by), 2.0);
        let saved = save(&both, &o);
        assert_eq!(path_of(&k2_tag(&saved)), "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000", "{by}: the height alone moves the path");
        assert_eq!(attr(&k2_tag(&saved), "draw:line-skew"), Some("0.5cm"));
    }
}

/// A connector an edit touched is still routed anew: its ends moved in the same save as the page's height (its begin 2 cm further left, the route cells following it), so
/// the path is written from the cells in the page of now, the skew goes, and the save says nothing. The control of the cases above: the page's height alone keeps a path,
/// and a path is not kept for every connector of a page that changes.
#[test]
fn a_connector_whose_ends_moved_in_the_save_that_changed_the_height_is_written_anew() {
    let o = open(&vector_drawing(PATH));
    let mut doc = page_taller(&o.doc, 2.0);
    {
        let k2 = shape_mut(&mut doc, 0, KV);
        let x = num_of(k2, "BeginX");
        set_num(k2, "BeginX", x - 2.0 / 2.54);
    }
    let mut rows = vector_rows(false);
    rows[0] = ("MoveTo", (2.0, 10.0));
    rows[1] = ("LineTo", (2.0, 12.0));
    set_route(shape_mut(&mut doc, 0, KV), &rows);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = k2_tag(&saved);
    assert_eq!(path_of(&tag), "M 2000 12000 v 2000 h 7000 v -6000 h 2000 v 1000", "written from the cells in the page of now: {tag}");
    assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 9001 6001"));
    assert!(attr(&tag, "draw:line-skew").is_none(), "routed anew: {tag}");
}

/// A route that really moved while its ends did not (a leg dragged to another place by `connector.edit_path`, say), in the save that changes the page's height: it is the
/// model's route, written anew in the page of now, and the skew goes.
#[test]
fn a_route_that_really_changed_in_the_save_that_changed_the_height_is_written_anew() {
    let o = open(&vector_drawing(PATH));
    let mut doc = page_taller(&o.doc, 2.0);
    let mut rows = vector_rows(false);
    rows[2] = ("LineTo", (9.5, 12.0));
    rows[3] = ("LineTo", (9.5, 6.0));
    set_route(shape_mut(&mut doc, 0, KV), &rows);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = k2_tag(&saved);
    assert_eq!(path_of(&tag), "M 4000 12000 v 2000 h 5500 v -6000 h 1500 v 1000", "{tag}");
    assert!(attr(&tag, "draw:line-skew").is_none(), "{tag}");
    assert!(same_corners(&corners_of(&open_of(&saved).doc, KV), &corners_of(&doc, KV)));
    // A hop on that route is kept with a word, and the path is the file's shifted (the route cannot be written).
    let mut hopped = page_taller(&o.doc, 2.0);
    let mut rows = vector_rows(true);
    rows[2] = ("LineTo", (9.5, 12.0));
    set_route(shape_mut(&mut hopped, 0, KV), &rows);
    let kept = save(&hopped, &o);
    assert_eq!(kept.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }]);
    assert_eq!(path_of(&k2_tag(&kept)), "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000", "the file's own, shifted: {}", k2_tag(&kept));
}

/// A route kept for a hop **with the ends moved too** keeps the file's path shifted by the page's change (the ends are written from the cells, so the path and the ends
/// differ by the move and not by the page), and the route written when the hop goes is in the page of now.
#[test]
fn a_hop_kept_with_the_ends_moved_and_the_page_changed_is_the_files_path_shifted_and_the_route_is_written_when_the_hop_goes() {
    let o = open(&w1_drawing(K2_FILE));
    let moved = |hop: bool| {
        let mut doc = page_taller(&o.doc, 2.0);
        {
            let k2 = shape_mut(&mut doc, 0, K2);
            let x = num_of(k2, "BeginX");
            set_num(k2, "BeginX", x - 2.0 / 2.54);
        }
        set_route(shape_mut(&mut doc, 0, K2), &crate::write_untouched_routes::route_at(7.0, hop));
        doc
    };
    let first = save(&moved(true), &o);
    assert_eq!(first.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: K2 }]);
    let tag = tag_of(&part(&first.bytes, "content.xml"), "K2").to_owned();
    assert_eq!(path_of(&tag), "M 9744 13192 L 9744 13500 L 9744 13804 L 7305 13804 L 7305 6172 L 7805 6172", "{tag}");
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0cm -0.612cm"));
    assert!((cm_of(&tag, "svg:x1") - 7.7435).abs() < 1e-4 && (cm_of(&tag, "svg:y1") - 13.192).abs() < 1e-4, "the ends are the model's: {tag}");
    let second = save_again(&moved(false), &first);
    assert!(second.edited && second.notes.is_empty(), "{:?}", second.notes);
    let written = tag_of(&part(&second.bytes, "content.xml"), "K2").to_owned();
    assert!(path_of(&written).starts_with("M 9744 13192 "), "the route the model has, in the page of now: {written}");
    assert!(attr(&written, "draw:line-skew").is_none(), "routed anew: {written}");
}

/// A path the reader cannot read is **not touched** and the save says so: the file's own bytes stay (the path of a connector whose route the reader could not read, which
/// it read as the straight segment of its ends), the ends move, and the word is the one a route that cannot be written has. Four spellings of a path that cannot be moved: it
/// breaks SVG's syntax, a number past what a path can hold, a command letter SVG does not have, and numbers with no command. (A path the reader reads that begins with no
/// move is no such path: see the test below.)
#[test]
fn a_path_that_cannot_be_read_is_kept_as_it_is_and_the_save_says_so() {
    for d in ["M 4000 10000 v 2000 h", "4000 10000 v 2000", "M 4000 1e300 L 5000 2000", "M 4000 10000 X 5 6", "M 4000 10000 Z 5 6"] {
        let o = open(&vector_drawing(d));
        let saved = save(&page_taller(&o.doc, 2.0), &o);
        assert_eq!(saved.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }], "{d}");
        let tag = k2_tag(&saved);
        assert_eq!(path_of(&tag), d, "{d}: the file's own path, as it was");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{d}");
        assert!(ends_are(&tag, (4.0, 12.0, 11.0, 9.0)), "{d}: the ends move with the page: {tag}");
    }
}

/// The number of rows of the route the model holds for shape `id` of page 0.
fn route_rows(doc: &Document, id: u32) -> usize {
    let shape = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|s| s.id == id)).unwrap();
    shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)).map_or(0, |s| s.rows.len())
}

/// (a) (the owner's ruling on the review, 2026-11-20): a path **the reader reads** that begins with no move starts at the origin, so it is placed by a
/// `M 0 <by>` put in front of it, and every absolute `y` moves with it: the path moves exactly, and no later save is silently `dh` off. It is not said. The file reads back
/// with the route the model has, and a chain of height changes keeps it right. A path the reader cannot read is the test above's.
#[test]
fn a_path_the_reader_reads_that_begins_with_no_move_is_shifted_by_a_move_put_in_front_of_it() {
    for (d, at_2, at_5) in [
        ("L 4000 10000 v 2000", "M 0 2000 L 4000 12000 v 2000", "M 0 5000 L 4000 15000 v 2000"),
        ("l 4000 10000 v 2000", "M 0 2000 l 4000 10000 v 2000", "M 0 5000 l 4000 10000 v 2000"),
        ("H 4000 V 12000", "M 0 2000 H 4000 V 14000", "M 0 5000 H 4000 V 17000"),
        ("C 1000 2000 3000 4000 5000 6000", "M 0 2000 C 1000 4000 3000 6000 5000 8000", "M 0 5000 C 1000 7000 3000 9000 5000 11000"),
    ] {
        let o = open(&vector_drawing(d));
        let doc = page_taller(&o.doc, 2.0);
        let saved = save(&doc, &o);
        assert!(saved.edited && saved.notes.is_empty(), "{d}: {:?}", saved.notes);
        let tag = k2_tag(&saved);
        assert_eq!(path_of(&tag), at_2, "{d}: {tag}");
        assert_eq!(attr(&tag, "svg:viewBox"), Some("0 0 7001 6001"), "{d}");
        assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"), "{d}");
        assert!(ends_are(&tag, (4.0, 12.0, 11.0, 9.0)), "{d}: {tag}");
        assert!(same_corners(&corners_of(&open_of(&saved).doc, KV), &corners_of(&doc, KV)), "{d}: the file reads back with the route the model has");
        assert_eq!(route_rows(&open_of(&saved).doc, KV), route_rows(&doc, KV), "{d}");
        let again = save_again(&doc, &saved);
        assert!(!again.edited && again.notes.is_empty(), "{d}: {:?}", again.notes);
        // A later save: the path has its move now, and goes on by the next change, and back to the page the file had.
        let doc = page_taller(&o.doc, 5.0);
        let later = save_again(&doc, &saved);
        assert!(later.edited && later.notes.is_empty(), "{d}: {:?}", later.notes);
        assert_eq!(path_of(&k2_tag(&later)), at_5, "{d}");
        assert!(same_corners(&corners_of(&open_of(&later).doc, KV), &corners_of(&doc, KV)), "{d}: and again");
        let back = save_again(&page_taller(&o.doc, 0.0), &later);
        assert!(back.notes.is_empty(), "{d}: {:?}", back.notes);
        assert!(
            same_corners(&corners_of(&open_of(&back).doc, KV), &corners_of(&o.doc, KV)),
            "{d}: back at the file's height the route is the file's"
        );
    }
}

/// (b) (the ruling): a number of more than four places keeps its places through a shift. `M 0 0 l 1 0.00003 A 5 5 0 1 1 1 0.00003` has an arc from a point to itself, which
/// the reader leaves out; a shift that wrote the arc's end to four places, `100`, against a pen at `100.00003` would make it an arc between two points a hair apart, drawn as
/// a near-full circle of radius 5. The path is `M 0 2000 l 1 0.00003 A 5 5 0 1 1 1 2000.00003`, and the file reads back with as many rows as the model has.
#[test]
fn a_number_of_more_than_four_places_keeps_them_and_a_degenerate_arc_stays_one() {
    let d = "M 0 0 l 1 0.00003 A 5 5 0 1 1 1 0.00003";
    let o = open(&vector_drawing(d));
    let doc = page_taller(&o.doc, 2.0);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    assert_eq!(path_of(&k2_tag(&saved)), "M 0 2000 l 1 0.00003 A 5 5 0 1 1 1 2000.00003", "{}", k2_tag(&saved));
    let back = open_of(&saved);
    assert_eq!(route_rows(&back.doc, KV), route_rows(&doc, KV), "the arc from a point to itself is no row, as it was");
    assert!(same_corners(&corners_of(&back.doc, KV), &corners_of(&doc, KV)));
    // Taken back to the file's page: the numbers are the file's.
    let home = save_again(&page_taller(&o.doc, 0.0), &saved);
    assert_eq!(path_of(&k2_tag(&home)), d, "{}", k2_tag(&home));
}

/// A route the model has that the writer cannot write (a point past a trillion hundredths of a millimetre) is said once; and when the file's own path cannot be placed
/// either, in the same save that changes the page's height, it is **not said again**: one word for the one connector (the `said` of the first arm). K2's rows are set far off
/// the page (so the route is the model's to write, and the writer cannot) and the file's path ends in a command with no numbers.
#[test]
fn a_route_past_a_trillion_hundredths_meeting_a_path_that_cannot_be_placed_is_said_once() {
    let o = open(&vector_drawing("M 4000 10000 v 2000 h"));
    let mut doc = page_taller(&o.doc, 2.0);
    let mut rows = vector_rows(false);
    rows[2] = ("LineTo", (9.0, 2.0e9));
    rows[3] = ("LineTo", (9.0, 2.0e9 - 1.0));
    set_route_on(shape_mut(&mut doc, 0, KV), &rows, PAGE_HEIGHT + 2.0);
    let saved = save(&doc, &o);
    assert_eq!(saved.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }], "one word for the one connector");
    assert_eq!(path_of(&k2_tag(&saved)), "M 4000 10000 v 2000 h", "the file's own path, as it was");
}

/// A connector in a group: its path is in the page's own measure (a group has no frame of its own in `svg:d`), so it is shifted the same way.
#[test]
fn a_connector_in_a_group_has_its_path_shifted_as_well() {
    let k2 = "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"11cm\" svg:y2=\"7cm\" svg:d=\"M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000\" svg:viewBox=\"0 0 7001 6001\"><text:p/></draw:connector>";
    let group = format!("<draw:g draw:name=\"G\">{}{}{k2}</draw:g>", custom("U", "u", (3.0, 9.0, 2.0, 1.0)), custom("E", "e", (10.0, 7.0, 2.0, 1.0)));
    let o = open(&Drawing::a4().page(&group));
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = k2_tag(&saved);
    assert_eq!(path_of(&tag), "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000", "{tag}");
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"));
    assert!(ends_are(&tag, (4.0, 12.0, 11.0, 9.0)), "{tag}");
}

/// A part of ODF 1.1 has no place for a path on a connector: a connector with none keeps its ends alone through a height change, with no note (nothing is routed anew); one
/// that holds a path (a file that is not valid for its version) has it shifted as in any other part.
#[test]
fn a_part_of_odf_1_1_keeps_a_connector_with_no_path_by_its_ends_alone_and_shifts_one_that_has_a_path() {
    let mut drawing = vector_drawing(PATH);
    drawing.version = "1.1".to_owned();
    let o = open(&drawing);
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = k2_tag(&saved);
    assert_eq!(path_of(&tag), "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000", "{tag}");
    assert_eq!(attr(&tag, "draw:line-skew"), Some("0.5cm"));
    let mut plain = Drawing::a4().page(&[
        custom("U", "u", (3.0, 9.0, 2.0, 1.0)),
        custom("E", "e", (10.0, 7.0, 2.0, 1.0)),
        "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"11cm\" svg:y2=\"7cm\"><text:p/></draw:connector>".to_owned(),
    ]
    .concat());
    plain.version = "1.1".to_owned();
    let o = open(&plain);
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "no path to shift, none is made, and nothing is said: {:?}", saved.notes);
    let tag = k2_tag(&saved);
    assert!(attr(&tag, "svg:d").is_none() && ends_are(&tag, (4.0, 12.0, 11.0, 9.0)), "{tag}");
}

/// What a save writes is checked against the shapes it was made for: nothing of a connector other than the one the page's height changed is touched, and the bytes between
/// the shapes are the file's own. K2 is the only element the save changes besides the two shapes it is glued to (their `svg:y`, which a height edit moves) and the page's
/// layout.
#[test]
fn only_the_ends_and_the_path_of_the_connector_change_beside_the_shapes_the_height_moves() {
    let o = open(&vector_drawing(PATH));
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    let strip = |s: &str| -> String {
        let mut text = s.to_owned();
        for name in ["U", "E", "K2"] {
            let tag = tag_of(&text, name).to_owned();
            text = text.replace(&tag, "");
        }
        text
    };
    assert_eq!(strip(&before), strip(&after), "nothing but U, E and K2 changed in the content");
}

// ---- the cases the mutation run and the hand mutants asked for --------------------------------------------------------------------------------------

/// A curved connector whose ends moved in the save that changed the height: its rows are in its own frame, so they are the baseline's, and a comparison of its rows says
/// nothing moved; but the connector itself did (its begin and end 1 cm right), so it is routed anew, the path written from the cells in the page of now and the skew gone.
/// (A hand mutant that took the move out of the rule kept the file's path and its skew, 1 cm off its ends.)
#[test]
fn a_curved_connector_whose_ends_moved_in_the_save_that_changed_the_height_is_written_anew() {
    let o = open(&Drawing::a4().page(
        "<draw:connector draw:name=\"C\" draw:type=\"curve\" draw:line-skew=\"0.25cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"9cm\" svg:y2=\"10cm\" svg:d=\"M 4000 10000 c 0 1000 5000 1000 5000 0\" svg:viewBox=\"0 0 5001 751\"><text:p/></draw:connector>",
    ));
    let mut doc = page_taller(&o.doc, 2.0);
    {
        let c = shape_mut(&mut doc, 0, 1);
        for cell in ["BeginX", "EndX"] {
            let x = num_of(c, cell);
            set_num(c, cell, x + 1.0 / 2.54);
        }
    }
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = tag_of(&part(&saved.bytes, "content.xml"), "C").to_owned();
    assert!(path_of(&tag).starts_with("M 5000 12000 c "), "written from the cells, in the page of now, from the begin that moved: {tag}");
    assert!(attr(&tag, "draw:line-skew").is_none(), "routed anew: {tag}");
    assert!(ends_are(&tag, (5.0, 12.0, 10.0, 12.0)), "{tag}");
    // The control: the same connector with the page's height alone changed keeps its path and its skew.
    let kept = save(&page_taller(&o.doc, 2.0), &o);
    assert_eq!(path_of(tag_of(&part(&kept.bytes, "content.xml"), "C")), "M 4000 12000 c 0 1000 5000 1000 5000 0");
}

/// A curved connector whose route changed (its control points are others), the ends and the page as they were: the rows are not the baseline's, and a curve has no corners to
/// compare by, so the model's route is the one to write, with the skew gone. (A hand mutant that kept the file's path whenever a curve had no corners kept it here.)
#[test]
fn a_curved_connector_whose_route_changed_is_written_anew_with_the_page_as_it_was() {
    let o = open(&Drawing::a4().page(
        "<draw:connector draw:name=\"C\" draw:type=\"curve\" draw:line-skew=\"0.25cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"9cm\" svg:y2=\"10cm\" svg:d=\"M 4000 10000 c 0 1000 5000 1000 5000 0\" svg:viewBox=\"0 0 5001 751\"><text:p/></draw:connector>",
    ));
    let mut doc = o.doc.clone();
    {
        let sheet = std::sync::Arc::make_mut(&mut shape_mut(&mut doc, 0, 1).sheet);
        let section = sheet.sections.iter_mut().find(|x| x.name.as_str() == "Geometry" && x.ix == Some(0)).unwrap();
        let rows = &mut std::sync::Arc::make_mut(section).rows;
        let curve = std::sync::Arc::make_mut(rows.get_mut(1).unwrap());
        curve.cells.insert(
            diacraft_doc::CellName::new("E"),
            diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("NURBS(1,3,1,1,0.5,0.5,0,1,1,0.5,0,1)")),
        );
    }
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = tag_of(&part(&saved.bytes, "content.xml"), "C").to_owned();
    assert!(attr(&tag, "draw:line-skew").is_none(), "routed anew: {tag}");
    assert!(
        path_of(&tag).starts_with("M 4000 10000 c ") && path_of(&tag) != "M 4000 10000 c 0 1000 5000 1000 5000 0",
        "the model's route is written: {tag}"
    );
}

/// A path the writer cannot read is said only in a save that shifts it: an edit of the connector's own line weight, with the page's height as it was, leaves its path as it
/// is and says nothing of it. (A hand mutant that took the height out of the condition tried to shift the path of every connector a save changed, and said so of this one.)
#[test]
fn a_path_the_writer_cannot_read_is_said_of_in_no_save_that_changes_no_height() {
    let o = open(&vector_drawing("M 4000 10000 v 2000 h"));
    let before = tag_of(&part(&o.bytes, "content.xml"), "K2").to_owned();
    let mut doc = o.doc.clone();
    crate::write_core::set_num_new(shape_mut(&mut doc, 0, KV), "LineWeight", 0.05);
    let saved = save(&doc, &o);
    assert!(saved.edited && saved.notes.is_empty(), "nothing is said of K2: {:?}", saved.notes);
    assert_eq!(path_of(tag_of(&part(&saved.bytes, "content.xml"), "K2")), path_of(&before), "K2's path is the file's, as it was");
    // The same save with the page's height changed as well says it, once.
    let both = save(&page_taller(&doc, 2.0), &o);
    assert_eq!(both.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }]);
}

/// A route with a hop whose file path cannot be read is said **once**: the hop is the word, and the path that could not be shifted is not said again.
#[test]
fn a_hop_on_a_connector_whose_path_cannot_be_read_is_said_once() {
    let o = open(&vector_drawing("M 4000 10000 v 2000 h"));
    let saved = save(&vector_model(&o.doc, 2.0, true), &o);
    assert_eq!(saved.notes, vec![SaveNote::RouteNotWritten { page: 0, shape: KV }], "one word for the one connector");
    assert_eq!(path_of(&k2_tag(&saved)), "M 4000 10000 v 2000 h", "the file's own path, as it was");
}

/// A shape that is no connector keeps its `svg:d` through a change of the page's height: a `draw:path`'s is in its own view box, and only its frame moves with the page.
/// (A hand mutant that shifted whatever has a `svg:d` moved the path inside its frame as well.)
#[test]
fn a_path_shape_that_is_no_connector_keeps_its_svg_d_and_view_box_through_a_height_change() {
    let o = open(&Drawing::a4().page(
        "<draw:path draw:name=\"P\" svg:x=\"2cm\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"3cm\" svg:viewBox=\"0 0 4000 3000\" svg:d=\"M 0 0 L 4000 3000 L 0 3000 Z\"><text:p/></draw:path>",
    ));
    let before = tag_of(&part(&o.bytes, "content.xml"), "P").to_owned();
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let tag = tag_of(&part(&saved.bytes, "content.xml"), "P").to_owned();
    assert_eq!(
        (attr(&tag, "svg:d"), attr(&tag, "svg:viewBox")),
        (attr(&before, "svg:d"), attr(&before, "svg:viewBox")),
        "its path and view box are its own: {tag}"
    );
    assert!((cm_of(&tag, "svg:y") - 5.0).abs() < 1e-4, "its frame is 2 cm further down: {tag}");
}

/// The page's height set a billionth of an inch greater, in the model of `o`.
fn a_hair_taller(o: &Open) -> Document {
    let mut doc = o.doc.clone();
    let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let sheet = std::sync::Arc::make_mut(&mut page.sheet);
    let height = sheet.cell("PageHeight").and_then(|c| c.value.as_ref()?.number()).unwrap();
    sheet.cells.get_mut("PageHeight").unwrap().value = diacraft_doc::Value::from_number(height + 1.0e-9);
    doc
}

/// A shift that changes nothing writes nothing: the page's height edited by a billionth of an inch moves a path by less than the four decimals it is written to, so the
/// connector's `svg:d` is not written again (a counted test: the attributes the save writes are those of the same save of a drawing whose connector has no path).
#[test]
fn a_shift_too_small_to_change_the_path_writes_no_attribute() {
    let plain = "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"10cm\" svg:x2=\"11cm\" svg:y2=\"7cm\"><text:p/></draw:connector>";
    let no_path = Drawing::a4().page(&[custom("U", "u", (3.0, 9.0, 2.0, 1.0)), custom("E", "e", (10.0, 7.0, 2.0, 1.0)), plain.to_owned()].concat());
    let (with_path, with_none) = (open(&vector_drawing(PATH)), open(&no_path));
    let (a, b) = (save(&a_hair_taller(&with_path), &with_path), save(&a_hair_taller(&with_none), &with_none));
    assert_eq!(a.work.attributes_written, b.work.attributes_written, "a path that does not change is not written again");
    // (The ends are written against the new height, to the last digit; the path is the file's, byte for byte.)
    let (now, was) = (k2_tag(&a), tag_of(&part(&with_path.bytes, "content.xml"), "K2").to_owned());
    assert_eq!(
        without(&now, &["svg:y1", "svg:y2"]),
        without(&was, &["svg:y1", "svg:y2"]),
        "K2's path, view box and skew are the file's, byte for byte"
    );
}

/// The least `Limits::max_write_memory` that lets a save of `doc` from `o` through, by bisection, and the words of the refusal one byte below it.
fn least_memory(o: &Open, doc: &Document) -> (u64, String) {
    use diacraft_odg::{Limits, OriginalPackage, WriteOptions, write_with};
    let with = |limit: u64| {
        let options = WriteOptions { limits: Limits { max_write_memory: limit, ..Limits::default() }, ..WriteOptions::default() };
        write_with(doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options)
    };
    let (mut low, mut high) = (0u64, Limits::default().max_write_memory);
    assert!(with(high).is_ok());
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if with(mid).is_ok() { high = mid } else { low = mid }
    }
    (high, with(high - 1).map(|_| String::new()).unwrap_or_else(|e| e.to_string()))
}

/// The text a save makes of the paths it shifts is charged to the memory a save may hold, in the words of the charge, **as the splicer keeps it: escaped** (
/// the charge was the length of the text made, and `escape_value` writes a line feed as `&#10;`, five bytes). The path's separators are line feeds, so the text kept is
/// far more than the text made: the least limit that lets the save through is more than the least for the same save with a shift too small to change the path, by exactly the
/// bytes of the new `svg:d` as they stand in the file; and the room the save leaves is less by the same.
#[test]
fn the_paths_a_save_shifts_are_charged_to_the_memory_a_save_may_hold_as_they_are_written() {
    let long: String = std::iter::once("M 4000 10000".to_owned()).chain((0..20_000).map(|n| format!("&#10;v {}", 1 + n % 7))).collect();
    let o = open(&vector_drawing(&long));
    let taller = page_taller(&o.doc, 2.0);
    let ((with_shift, said), (without, _)) = (least_memory(&o, &taller), least_memory(&o, &a_hair_taller(&o)));
    let saved = save(&taller, &o);
    let written = path_of(&k2_tag(&saved)).len() as u64;
    let moved = unescaped_len(path_of(&k2_tag(&saved)));
    assert!(written >= moved + 4 * 20_000, "the separators are written as references: {written} bytes for {moved}");
    assert_eq!(
        with_shift - without,
        written,
        "the text kept is the memory charged: {with_shift} against {without} for {written} bytes written ({moved} made)"
    );
    assert!(said.contains("memory for the paths a save shifted"), "{said}");
    let hair = save(&a_hair_taller(&o), &o);
    assert_eq!(hair.work.memory_room - saved.work.memory_room, written, "the room the save leaves is less by the text kept");
}

/// The bytes of an attribute's value with its references resolved, for the references this file's paths use (`&#10;` for a line feed).
fn unescaped_len(raw: &str) -> u64 {
    raw.replace(
        "&#10;", "
",
    )
    .len() as u64
}

/// The memory a save holds for a shifted path is taken **before the text is made**, as an upper bound of it: a save whose room cannot hold what the pass may make is
/// refused in the charge's words **whether or not the path changes**, and the bound is given back once the text is known (the room left is less only by the text kept). A
/// path of 300,000 closes (`Z`), which the reader keeps no rows of, so the model holds nothing of it; a shift too small to change it needs the room for the bound all the same.
#[test]
fn the_text_a_shift_may_make_is_charged_before_it_is_made_and_given_back_after() {
    let closes: String = std::iter::once("M 4000 10000 v 2000".to_owned()).chain((0..300_000).map(|_| " Z".to_owned())).collect();
    let o = open(&vector_drawing(&closes));
    let (least, said) = least_memory(&o, &a_hair_taller(&o));
    assert!(said.contains("memory for the paths a save shifted"), "the bound is what a save with the path unchanged needs room for: {said}");
    // The least for the same drawing with a path of one close needs far less: the bound is a share of the path's length.
    let short = open(&vector_drawing("M 4000 10000 v 2000 Z"));
    let (least_short, _) = least_memory(&short, &a_hair_taller(&short));
    assert!(least > least_short + 5_000_000, "{least} for 300,000 closes and {least_short} for one");
    // Given back: the room the save leaves is what the unchanged path leaves, to the byte, bound or no bound.
    let (a, b) = (save(&a_hair_taller(&o), &o), save(&page_taller(&o.doc, 0.0), &o));
    assert_eq!(a.work.memory_room, b.work.memory_room, "nothing is kept for a path that did not change");
}

/// A page of `n` connectors that no glue holds, each a path of `steps` relative steps with a skew.
fn connectors_page(n: usize, steps: usize) -> Drawing {
    let path: String = std::iter::once("M 1000 1000".to_owned()).chain((0..steps).map(|k| format!(" v {}", 1 + k % 5))).collect();
    let body: String = (0..n)
        .map(|k| {
            format!(
                "<draw:connector draw:name=\"c{k}\" draw:line-skew=\"0.25cm\" svg:x1=\"1cm\" svg:y1=\"1cm\" svg:x2=\"1cm\" svg:y2=\"2cm\" svg:d=\"{path}\" svg:viewBox=\"0 0 1 {}\"><text:p/></draw:connector>",
                steps + 1
            )
        })
        .collect();
    Drawing::a4().page(&body)
}

/// What a save of `connectors_page(n, steps)` costs when the page grows 2 cm, in the counters the save keeps: the cells compared, the bytes of start tags looked at, the
/// attributes written, the bytes the edits wrote and the bytes copied.
fn cost_of(n: usize, steps: usize) -> [u64; 5] {
    let o = open(&connectors_page(n, steps));
    let saved = save(&page_taller(&o.doc, 2.0), &o);
    assert!(saved.edited && saved.notes.is_empty(), "{:?}", saved.notes);
    let w = &saved.work;
    [w.cells_compared, w.attribute_bytes, w.attributes_written, w.bytes_written, w.bytes_copied]
}

/// The cost of a page that grows is **linear** in its connectors and in the steps of their paths, counted and never timed: four times the connectors cost about four times
/// the work in each counter, four times the steps of each path at most about four times, and each connector's share of the rows looked at is a bounded number of looks.
#[test]
fn the_cost_of_a_page_that_grows_is_linear_in_its_connectors_and_their_paths() {
    let (small, big) = (cost_of(200, 8), cost_of(800, 8));
    for (k, (a, b)) in small.iter().zip(&big).enumerate() {
        assert!(*b <= 4 * a + 4096, "counter {k}: {a} for 200 connectors and {b} for 800 is more than four times");
        assert!(
            *b >= 3 * a,
            "counter {k}: {a} for 200 connectors and {b} for 800 is less than three times (every connector of the page is looked at)"
        );
    }
    let (short, long) = (cost_of(100, 20), cost_of(100, 80));
    for (k, (a, b)) in short.iter().zip(&long).enumerate() {
        assert!(*b <= 4 * a + 4096, "counter {k}: {a} for paths of 20 steps and {b} for 80 is more than four times");
    }
    // Each connector costs a bounded number of looks at each of its rows (about five were measured: the comparison of the rows, the route, the corners of the file's and of the
    // model's), and three attributes written: the two ends and the path.
    assert!(big[0] <= 800 * (8 + 1) * 8, "cells compared: {}", big[0]);
    assert!(long[0] <= 100 * (80 + 1) * 8, "cells compared: {}", long[0]);
    assert_eq!((small[2], big[2]), (3 * 200 + 1, 3 * 800 + 1), "the ends and the path of each connector, and the page's height");
}

/// The files for the black-box LibreOffice check of the rulings (a) and (b), written by hand: `ODG_PAGEKEEP_F1_DIR=<dir> cargo test -p diacraft-odg --test it
/// -- --ignored lo_pagekeep_write`. For each case (K2 with a path that begins with no move, `L` and `l`; K2 with an arc from a point to itself) the file as it is, after
/// +2 cm and -3 cm by the writer, and the file as the writer made it before the rulings (the path as it was, the ends moved, which is what a writer that left the path alone
/// writes; the arc's end cut to four places). LibreOffice converts each; the legs of K2 it writes are compared by `lo-legs.py`.
#[test]
#[ignore = "writes files for the LibreOffice check by hand"]
fn lo_pagekeep_write() {
    let Ok(dir) = std::env::var("ODG_PAGEKEEP_F1_DIR") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let cases = [
        ("a-L", "L 4000 10000 v 2000", "M 0 2000 L 4000 12000 v 2000", "M 0 -3000 L 4000 7000 v 2000"),
        ("a-l", "l 4000 10000 v 2000", "M 0 2000 l 4000 10000 v 2000", "M 0 -3000 l 4000 10000 v 2000"),
        (
            "b-arc",
            "M 0 0 l 1 0.00003 A 5 5 0 1 1 1 0.00003",
            "M 0 2000 l 1 0.00003 A 5 5 0 1 1 1 2000.00003",
            "M 0 -3000 l 1 0.00003 A 5 5 0 1 1 1 -2999.99997",
        ),
    ];
    // The same paths on a connector with no glue (LibreOffice routes a glued standard connector from its glue and ignores its path; one with none it may draw from the path).
    let unglued = |d: &str, by: f64| {
        let k2 = format!(
            "<draw:connector draw:name=\"K2\" draw:type=\"standard\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"{}cm\" svg:x2=\"11cm\" svg:y2=\"{}cm\" svg:d=\"{d}\" svg:viewBox=\"0 0 7001 6001\"><text:p/></draw:connector>",
            10.0 + by,
            7.0 + by
        );
        let mut drawing = Drawing::a4().page(&k2);
        drawing.styles_auto = drawing.styles_auto.replace("29.7cm", &format!("{}cm", 29.7 + by));
        drawing
    };
    for (name, d, plus2, minus3) in [("u-L", cases[0].1, cases[0].2, cases[0].3), ("u-arc", cases[2].1, cases[2].2, cases[2].3)] {
        let o = open(&unglued(d, 0.0));
        std::fs::write(dir.join(format!("{name}-input.odg")), unglued(d, 0.0).bytes()).unwrap();
        for (by, label, want) in [(2.0, "plus2", plus2), (-3.0, "minus3", minus3)] {
            let saved = save(&page_taller(&o.doc, by), &o);
            assert_eq!(path_of(&k2_tag(&saved)), want, "{name} {label}");
            std::fs::write(dir.join(format!("{name}-{label}.odg")), &saved.bytes).unwrap();
        }
        let old_d = if name == "u-arc" { "M 0 2000 l 1 0.00003 A 5 5 0 1 1 1 2000" } else { d };
        std::fs::write(dir.join(format!("{name}-old-plus2.odg")), unglued(old_d, 2.0).bytes()).unwrap();
    }
    for (name, d, plus2, minus3) in cases {
        let input = vector_drawing(d);
        let o = open(&input);
        std::fs::write(dir.join(format!("{name}-input.odg")), input.bytes()).unwrap();
        for (by, label, want) in [(2.0, "plus2", plus2), (-3.0, "minus3", minus3)] {
            let saved = save(&page_taller(&o.doc, by), &o);
            assert_eq!(path_of(&k2_tag(&saved)), want, "{name} {label}");
            std::fs::write(dir.join(format!("{name}-{label}.odg")), &saved.bytes).unwrap();
        }
        // The file a writer that left the path alone would make after +2 cm (a), or cut a number to four places (b): the page 2 cm taller, the shapes and K2's ends 2 cm lower.
        let old_d = if name == "b-arc" { "M 0 2000 l 1 0.00003 A 5 5 0 1 1 1 2000" } else { d };
        let k2 = format!(
            "<draw:connector draw:name=\"K2\" draw:start-shape=\"u\" draw:start-glue-point=\"2\" draw:end-shape=\"e\" draw:end-glue-point=\"0\" draw:line-skew=\"0.5cm\" svg:x1=\"4cm\" svg:y1=\"12cm\" svg:x2=\"11cm\" svg:y2=\"9cm\" svg:d=\"{old_d}\" svg:viewBox=\"0 0 7001 6001\"><text:p/></draw:connector>"
        );
        let mut old = Drawing::a4().page(&[custom("U", "u", (3.0, 11.0, 2.0, 1.0)), custom("E", "e", (10.0, 9.0, 2.0, 1.0)), k2].concat());
        old.styles_auto = old.styles_auto.replace("29.7cm", "31.7cm");
        std::fs::write(dir.join(format!("{name}-old-plus2.odg")), old.bytes()).unwrap();
    }
}
