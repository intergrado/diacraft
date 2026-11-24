//! Of the review: three hand mutants of the engine that survived every test, each killed by a case that only it
//! gets wrong. The settings are the spec's "Settings for vectors"; paper inches, y up, 96 screen points to an inch, on a 20 by 20 inch page.

use diacraft_doc::RowKey;
use diacraft_engine::snap::{Extras, OwnFrame, Query, SnapPage, Subject, Target, Use, bounds_of, frame_of_shapes, own_points, snap_ext};
use diacraft_testkit::build::ShapeBuilder;
use serde_json::json;

use super::snap_library_settings::{at, corners, guide_at, page_of_shapes, rect, session_of, turned};
use super::snap_review_fixes::vector_settings;

/// **The set of targets a page keeps is keyed by whether the subject has sides of its own** (`c.slanted == slanted` of the key). A subject with a frame is told the guides that
/// lie along no axis, and one without is not: a set built for the one is not the other's. A guide along 30 degrees and a 2 by 1 shape turned 30, its bottom side 0.03 above the
/// guide, asked first as a box with no frame (the slanted guide is no target of it) and then with its frame: the second asks another set, and snaps its side to the guide.
#[test]
fn a_set_of_targets_built_for_a_subject_with_no_frame_is_not_the_one_a_framed_subject_asks_for() {
    let start = at((0.0, 0.0), 30.0, 5.0, 0.53);
    let doc = page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0), turned(2, start, 2.0, 1.0, 30.0)]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let b = bounds_of(&doc, 1, &[2]).unwrap().rect;
    let frame: OwnFrame = frame_of_shapes(&doc, 1, &[2]).expect("the shape is turned");
    let ask = |frame: Option<OwnFrame>| {
        let query = Query { subject: Subject::Box(b), using: Use::Place, moving: &[2], zoom: 96.0, own_points: &[] };
        snap_ext(&doc, &page, &vector_settings(), &query, Extras { frame, ..Extras::default() })
    };
    let plain = ask(None);
    assert!(plain.hits.iter().all(|h| h.target != Target::Guide), "a subject with no frame has no side along the guide: {plain:?}");
    assert_eq!(page.target_builds(), 1);
    let framed = ask(Some(frame));
    assert_eq!(page.target_builds(), 2, "a framed subject asks for the set with the slanted guides, which is another one");
    assert!(framed.hits.iter().any(|h| h.target == Target::Guide), "its side comes to the slanted guide: {framed:?}");
    // And back: the set kept for the framed subject is not the plain one's.
    let again = ask(None);
    assert_eq!(page.target_builds(), 3, "a subject with no frame asks again for the set without");
    assert!(again.hits.iter().all(|h| h.target != Target::Guide), "{again:?}");
    // Asked again the same way, the kept set is the one found.
    ask(None);
    assert_eq!(page.target_builds(), 3);
}

/// **A hit along a direction of the subject's own is no place** (`Hit::is_place`: no axis and no normal). A box whose shapes have places of their own is asked without the
/// outlines first, and, when that found no place, with the outlines alone (a place of another kind beats an outline, and a place beats a line): a line along the box's own side
/// that the first query found is no place, so the outlines are still asked. Shape 2 is turned like shape 1 and 0.03 inch off its own lines, and its box's bottom middle is 0.03 inch
/// above the top edge of a wide box (3), which no line of the other shapes meets: the answer is the outline's, and not the own line the first query found.
#[test]
fn a_line_along_the_subjects_own_side_is_no_place_so_the_outline_is_still_asked() {
    let a = (3.0, 3.0);
    let pin = at(a, 30.0, 4.0, 0.03);
    // The subject has a connection point of its own, at the middle of its right side, which makes the box's own places worth asking about apart.
    let subject = || -> ShapeBuilder {
        turned(2, pin, 2.0, 1.0, 30.0).row_cell("Connection", None, RowKey::Ix(0), "X", "2").row_cell("Connection", None, RowKey::Ix(0), "Y", "0.5")
    };
    // The wide box's top edge is 0.03 inch under the bottom of the subject's box.
    let probe = page_of_shapes(vec![subject()]);
    let top = bounds_of(&probe, 1, &[2]).unwrap().rect.y0 - 0.03;
    // The wide box draws its outline: five rows, a closed rectangle 10 by 3 in its own frame.
    let mut wide = rect(3, 2.0, top - 3.0, 12.0, top);
    for (i, (x, y)) in [(0.0, 0.0), (10.0, 0.0), (10.0, 3.0), (0.0, 3.0), (0.0, 0.0)].iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        wide = wide.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &y.to_string(),
        );
    }
    let doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), subject(), wide]);
    let page = SnapPage::read(&doc, 1).unwrap();
    let b = bounds_of(&doc, 1, &[2]).unwrap().rect;
    let own = own_points(&doc, 1, &[2], (0.0, 0.0));
    assert_eq!(own.len(), 1, "the subject has one place of its own: {own:?}");
    let query = Query { subject: Subject::Box(b), using: Use::Place, moving: &[2], zoom: 96.0, own_points: &own };
    let extras = Extras { frame: frame_of_shapes(&doc, 1, &[2]), ..Extras::default() };
    let out = snap_ext(&doc, &page, &vector_settings(), &query, extras);
    eprintln!("E07 {out:?}");
    assert!(
        out.hits.iter().any(|h| h.target == Target::Outline && h.is_place()),
        "the outline of the wide box is asked, and a place of the box comes to it: {out:?}"
    );
}

/// **A kept resize of a turned shape is refused by the page, in the words of `keep`**, as an upright shape's is. A shape 2 inches wide and a hundred thousandth of an inch
/// high, turned 30 degrees, resized to 6 inches high with its proportions: its width comes to 1,200,000 inches, past the page, and the refusal says `snap.keep`, not the edit's words
/// ("must be a number within"); the drawing is as it was.
#[test]
fn a_kept_resize_of_a_turned_shape_is_refused_by_the_page_in_the_words_of_keep() {
    let mut s = session_of(page_of_shapes(vec![turned(1, (5.0, 5.0), 2.0, 0.00001, 30.0)]));
    let corner = corners(&s, 1)[0];
    let before = s.document().cloned();
    let params = json!({"id": 1, "x": corner.0, "y": corner.1, "width": 2.0, "height": 6.0, "snap": {"zoom": 96.0, "keep": "proportions"}});
    let err = s.execute("shape.resize", &params).unwrap_err().to_string();
    assert!(err.contains("snap.keep") && !err.contains("must be a number within"), "{err}");
    assert_eq!(s.document().cloned(), before);
}
