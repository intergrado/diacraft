//! The small follow-ups the review parked. Each case runs with the real writer.
//!
//! * A paragraph put in on a page **after the first** is read back against that page's shapes (hand mutant A2: the read-back looked every shape up on page 0 and no test
//!   noticed).
//! * The index of a part's styles is never walked for an edit in place, and a walk that is put back is counted (hand mutant B4: the old `index.retain`, uncharged).

use std::sync::Arc;

use diacraft_doc::{Document, Text, TextPart};
use diacraft_odg::Limits;

use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real, shape_mut};
use crate::write_layers_parked::{Edit, in_place_page};
use crate::write_layers_review::save_with;
use crate::write_runs::{chars, shape};

/// The characters of each top-level shape of page `page`, one text each.
fn texts_of(doc: &Document, page: usize) -> Vec<String> {
    doc.pages
        .get(page)
        .map(|p| {
            p.shapes
                .iter()
                .map(|s| {
                    s.text
                        .iter()
                        .flat_map(|t| t.parts.iter())
                        .filter_map(|part| if let TextPart::Chars(c) = part { Some(c.to_string()) } else { None })
                        .collect::<String>()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The id of the first shape of page `page`.
fn first_id(doc: &Document, page: usize) -> u32 {
    doc.pages.get(page).and_then(|p| p.shapes.get(0)).map(|s| s.id).unwrap_or_else(|| panic!("no shape on page {page}"))
}

/// Gives the first shape of page `page` the paragraphs `parts` (characters, one text part each).
fn set_text(doc: &mut Document, page: usize, parts: &[&str]) {
    let id = first_id(doc, page);
    shape_mut(doc, page, id).text = Some(Arc::new(Text { parts: parts.iter().map(|p| chars(p)).collect(), ..Default::default() }));
}

/// Two pages of one shape each, with the same ordinal (the first of its page) and one paragraph: `zz` on the first page and `ab` on the second.
fn two_pages() -> Drawing {
    Drawing::a4().page(&shape("w", "0cm", "<text:p>zz</text:p>")).page(&shape("x", "6cm", "<text:p>ab</text:p>"))
}

/// The reviewer of (MEDIUM): the read-back of a paragraph put in looks the shape up on the page it is on. A paragraph put in on the **second** page is saved, and
/// the file read again has it there and the first page as it was. Hand mutant A2 (`by_page.get(&rb.page_at)` to `.get(&0)` in `check_read_back`) looked the second page's
/// shape up on the first page: the page has no read-back map of its own, and the save is refused as "a shape whose text was written is not read again".
#[test]
fn a_paragraph_put_in_on_the_second_page_is_saved_and_read_back_against_that_page() {
    let mut o = open(&two_pages());
    assert_eq!(o.doc.pages.len(), 2);
    set_text(&mut o.doc, 1, &["ab\n", "new"]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a paragraph put in on page 2 was refused: {e}"));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(texts_of(&back.doc, 1), ["ab\nnew"], "the second page has the paragraph put in");
    assert_eq!(texts_of(&back.doc, 0), ["zz"], "the first page is as it was");
}

/// A paragraph put in on **each** page in one save: both pages are read back, each shape against its own page's map. The two shapes are the first of their pages, so a lookup
/// that keys the shapes by the element's ordinal alone, or by the page 0 for all of them, finds the other page's shape and the texts differ (`zz` has one paragraph where the
/// model has two, and the second page's shape is read as the first's).
#[test]
fn a_paragraph_put_in_on_each_of_two_pages_is_read_back_against_its_own_page() {
    let mut o = open(&two_pages());
    set_text(&mut o.doc, 0, &["zz\n", "one"]);
    set_text(&mut o.doc, 1, &["ab\n", "two\n", "three"]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a paragraph put in on each page was refused: {e}"));
    assert!(saved.edited);
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(texts_of(&back.doc, 0), ["zz\none"]);
    assert_eq!(texts_of(&back.doc, 1), ["ab\ntwo\nthree"]);
    assert!(part(&saved.bytes, "content.xml").contains("three"), "the paragraph is in the part");
}

/// The page after the first, alone: a paragraph taken out (the model has fewer paragraphs than the file) is read back against the second page too.
#[test]
fn a_paragraph_taken_out_on_the_second_page_is_read_back_against_that_page() {
    let drawing = Drawing::a4().page(&shape("w", "0cm", "<text:p>zz</text:p>")).page(&shape(
        "x",
        "6cm",
        "<text:p>ab</text:p><text:p>cd</text:p><text:p>ef</text:p>",
    ));
    let mut o = open(&drawing);
    assert_eq!(texts_of(&o.doc, 1), ["ab\ncd\nef"]);
    set_text(&mut o.doc, 1, &["ab\n", "ef"]);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save of a paragraph taken out on page 2 was refused: {e}"));
    let back = open_bytes(saved.bytes);
    assert_eq!(texts_of(&back.doc, 1), ["ab\nef"]);
    assert_eq!(texts_of(&back.doc, 0), ["zz"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The index of a part's styles: a walk that is put back is counted.

/// What a save of [`in_place_page`] of `n` edits in place took of steps.
fn steps_of(n: usize) -> u64 {
    let o = in_place_page(n, Edit::InPlace);
    let (saved, work) = save_with(&o, &Limits::default());
    assert!(saved.is_ok_and(|w| w.edited), "{n} edits: saved");
    work.order_steps
}

/// The steps of a page of `n` edits in place are a few for each edit and **no more**: 3,608 for 400 edits (nine each), 14,408 for 1,600. A walk of the index of the part's
/// styles for each edit (the old `index.retain`: a step for each of the `n + 1` keys, so about `n` squared) takes 83,407 steps at 400 and 1,293,607 at 1,600, which this bound
/// refuses at once whatever the ratio between sizes is.
#[test]
fn a_page_of_edits_in_place_takes_a_few_steps_for_each_edit_and_no_walk_of_the_index() {
    for n in [400usize, 1_600] {
        let steps = steps_of(n);
        eprintln!("F3 steps of {n} edits in place: {steps}");
        assert!(steps <= 12 * n as u64, "{n} edits take {steps} steps: more than twelve for each edit, the index of the styles is walked");
    }
}
