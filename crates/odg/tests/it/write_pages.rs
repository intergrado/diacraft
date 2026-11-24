//! A page's size, margins and orientation: they live in the
//! page layout of the page's master page. A page that shares its master page or layout with another gets a copy of both, and the edit is made in the
//! copy; a page that does not has its layout edited where it is. The shapes of a page whose height changed are written against the new height.

use std::sync::Arc;

use diacraft_doc::{Document, Value};
use diacraft_odg::{OriginalPackage, WriteError, WriteOptions, WriteWork, write_counted};

use crate::drawing::{Drawing, cm};
use crate::write_core::{Saver, compare, custom, open, open_bytes, part, real};

/// Sets a page cell's number (its formula and unit stay).
fn set_page_cell(doc: &mut Document, page: usize, cell: &str, value: f64) {
    let p = Arc::make_mut(doc.pages.get_mut(page).expect("no such page"));
    let sheet = Arc::make_mut(&mut p.sheet);
    sheet.cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}")).value = Value::from_number(value);
}

fn number_of(doc: &Document, page: usize, cell: &str) -> f64 {
    doc.pages.get(page).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

/// The element of `part` that starts with `open` and ends at the first `close` after it.
fn element<'a>(part: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = part.find(open)?;
    let end = part[start..].find(close)? + close.len();
    Some(&part[start..start + end])
}

fn two_pages() -> Drawing {
    Drawing::a4().page(&custom("s1", "1cm")).page(&custom("s2", "2cm"))
}

/// Two pages on `Default`, the second resized to A3. A new master page `Default_2` (display name `Default 2`) and a new layout
/// `PM2`, the second page naming the copy; the first page and `Default` untouched; the second page's shape written against the new height.
fn page_resize_case(save: &Saver) {
    let mut o = open(&two_pages());
    set_page_cell(&mut o.doc, 1, "PageWidth", cm(29.7));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before_styles, styles) = (part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"));
    // The layout's copy, with the new size, follows the layout; the master page's copy follows the master page.
    let old_layout = element(&before_styles, "<style:page-layout style:name=\"PM1\"", "</style:page-layout>").unwrap();
    let new_layout = element(&styles, "<style:page-layout style:name=\"PM2\"", "</style:page-layout>").unwrap_or_else(|| panic!("no PM2: {styles}"));
    assert_eq!(
        new_layout,
        old_layout
            .replace("PM1", "PM2")
            .replace("fo:page-width=\"21cm\"", "fo:page-width=\"29.7cm\"")
            .replace("fo:page-height=\"29.7cm\"", "fo:page-height=\"42cm\"")
    );
    assert!(styles.contains(&format!("{old_layout}{new_layout}")), "the copy follows the original: {styles}");
    let master = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#;
    assert!(
        styles
            .contains(&format!(r#"{master}<style:master-page style:name="Default_2" style:page-layout-name="PM2" style:display-name="Default 2"/>"#)),
        "{styles}"
    );
    assert_eq!(
        styles
            .replace(new_layout, "")
            .replace(r#"<style:master-page style:name="Default_2" style:page-layout-name="PM2" style:display-name="Default 2"/>"#, ""),
        before_styles,
        "nothing else of styles.xml changed"
    );
    // The second page names the copy; the first is as it was; the second page's shape is lower by what the page grew.
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(after.contains(r#"draw:name="page2" draw:master-page-name="Default_2""#), "{after}");
    assert!(after.contains(r#"draw:name="page1" draw:master-page-name="Default""#));
    assert_eq!(
        after.replace("Default_2", "Default").replace("svg:y=\"15.3cm\"", "svg:y=\"3cm\""),
        before,
        "only the page's master and the shape's y changed"
    );
    let diff = compare(&o.bytes, &saved.bytes);
    let styles_diff = diff.parts.iter().find(|p| p.part == "styles.xml").unwrap();
    assert_eq!((styles_diff.counts.added, styles_diff.counts.removed, styles_diff.counts.changed), (2, 0, 0), "{diff:?}");
    let content_diff = diff.parts.iter().find(|p| p.part == "content.xml").unwrap();
    assert_eq!(content_diff.changed.len(), 2, "the page and its shape: {diff:?}");
    // Read back: the second page has the new size and its shape keeps its distance from the bottom, the first page is as it was.
    let back = open_bytes(saved.bytes.clone());
    for (cell, now) in [("PageWidth", cm(29.7)), ("PageHeight", cm(42.0))] {
        assert!((number_of(&back.doc, 1, cell) - now).abs() < 1e-9, "{cell}");
    }
    assert!((number_of(&back.doc, 0, "PageHeight") - cm(29.7)).abs() < 1e-9);
    let pin_y = |doc: &Document, page: usize| {
        doc.pages.get(page).unwrap().shapes.get(0).unwrap().sheet.cell("PinY").and_then(|c| c.value.as_ref()?.number()).unwrap()
    };
    assert!((pin_y(&back.doc, 1) - pin_y(&o.doc, 1)).abs() < 1e-9, "PinY is kept");
}

/// A page whose master page and layout no other page uses has its layout edited where it is: the margin and the orientation, the
/// size unchanged, no new master page.
fn a_page_that_shares_nothing_has_its_layout_edited_in_place(save: &Saver) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    set_page_cell(&mut o.doc, 0, "PageLeftMargin", cm(1.5));
    set_page_cell(&mut o.doc, 0, "PrintPageOrientation", 2.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"));
    let expected = before.replacen("fo:margin-left=\"1cm\"", "fo:margin-left=\"1.5cm\"", 1).replacen(
        "style:print-orientation=\"portrait\"",
        "style:print-orientation=\"landscape\"",
        1,
    );
    assert_ne!(expected, before);
    assert_eq!(after, expected, "the layout where it is, the size and everything else as it was");
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml"), "the page is not touched");
}

/// A page that is the only one on its master page and changes height: the shapes are rewritten against the new height; its layout is edited.
fn a_single_page_made_taller_rewrites_the_y_of_its_shapes(save: &Saver) {
    let mut o = open(&Drawing::a4().page(&custom("s1", "1cm")));
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(21.0));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    // `svg:y="-5.7cm"` and the layout's `fo:page-height="21cm"`.
    let (after, styles) = (part(&saved.bytes, "content.xml"), part(&saved.bytes, "styles.xml"));
    assert!(after.contains("svg:y=\"-5.7cm\""), "{after}");
    assert!(styles.contains("fo:page-height=\"21cm\"") && !styles.contains("PM2"), "{styles}");
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 3] = [
    ("page_resize_splits_a_shared_master_page", page_resize_case),
    ("a_page_that_shares_nothing_has_its_layout_edited_in_place", a_page_that_shares_nothing_has_its_layout_edited_in_place),
    ("a_single_page_made_taller_rewrites_the_y_of_its_shapes", a_single_page_made_taller_rewrites_the_y_of_its_shapes),
];

#[test]
fn page_resize_splits_a_shared_master_page() {
    page_resize_case(&real);
}

#[test]
fn a_page_that_shares_nothing_has_its_layout_edited_in_place_test() {
    a_page_that_shares_nothing_has_its_layout_edited_in_place(&real);
}

#[test]
fn a_single_page_made_taller_rewrites_the_y_of_its_shapes_test() {
    a_single_page_made_taller_rewrites_the_y_of_its_shapes(&real);
}

#[test]
fn echo_control_fails_every_page_case() {
    crate::write_core::echo_control(&CASES);
}

#[test]
fn a_second_resize_of_the_page_that_was_split_edits_its_own_layout() {
    // The split leaves the second page alone on `Default_2` and `PM2`: another resize of it goes into them where they are.
    let mut o = open(&two_pages());
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let mut second = open_bytes(first.bytes.clone());
    set_page_cell(&mut second.doc, 1, "PageHeight", cm(30.0));
    let saved = real(&second.doc, &second.bytes, &second.index).unwrap();
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains("fo:page-height=\"30cm\"") && !styles.contains("PM3") && !styles.contains("Default_3"), "{styles}");
}

#[test]
fn a_split_of_a_master_page_that_holds_shapes_and_the_size_of_a_page_the_file_does_not_describe_are_refused_in_words() {
    // A master page with a shape reads as a background page: its copy would be another the model lacks.
    let mut d = two_pages();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/></style:master-page>"#,
    );
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("holds shapes"), "{words}"),
        other => panic!("expected a refusal: {other:?}"),
    }
    // A page that names no master page.
    let mut d = Drawing::a4().page(&custom("s1", "1cm"));
    d.pages = d.pages.replace(" draw:master-page-name=\"Default\"", "");
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(42.0));
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(words)) if words.contains("master page")));
    // A cell that holds a formula, an orientation that is neither 1 nor 2, and a size that is not a number.
    let mut o = open(&two_pages());
    set_page_cell(&mut o.doc, 0, "PrintPageOrientation", 3.0);
    assert!(matches!(real(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(words)) if words.contains("orientation")));
}

/// Security finding (MEDIUM): the new master's name was tried from `_2` upward against the names in use, for each split, with no charge: a part of
/// master pages `Default_2` to `Default_k` made each of s splits walk k names. The number is now a cursor that only moves on, found once, and each name
/// looked at and each number tried is a step. Chained sizes (k = 200, s = 40 and k = 800, s = 160): the steps follow k + s, which is 4 times, where the
/// walk was s * k, which is 16 times.
#[test]
fn the_name_of_a_copied_master_page_costs_steps_that_follow_the_names_in_use_and_the_pages_split() {
    let steps = |k: usize, s: usize| {
        let mut d = Drawing::a4();
        for n in 2..=k {
            d.masters.push_str(&format!(r#"<style:master-page style:name="Default_{n}" style:page-layout-name="PM1"/>"#));
        }
        for _ in 0..s {
            d = d.page("");
        }
        let mut o = open(&d);
        // All the pages but the last are resized: each is a split of `Default`, which the last page still shares until the end.
        for page in 0..s - 1 {
            set_page_cell(&mut o.doc, page, "PageHeight", cm(42.0));
        }
        let mut work = diacraft_odg::WriteWork::default();
        let options = diacraft_odg::WriteOptions::default();
        let saved =
            diacraft_odg::write_counted(&o.doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work)
                .unwrap_or_else(|e| panic!("the save of {s} splits was refused: {} ({e})", e.class()));
        let styles = part(&saved.bytes, "styles.xml");
        // The first free number above the k in use, and each copy a number apart.
        assert!(
            styles.contains(&format!(r#"style:name="Default_{}""#, k + 1)) && styles.contains(&format!(r#"style:name="Default_{}""#, k + s - 1)),
            "k={k} s={s}"
        );
        // The names looked at once, and the numbers tried by a cursor: about k + s, and the same names are steps of the save's own count too.
        assert!(work.scope_steps >= work.master_name_tries, "each try is a step");
        work.master_name_tries
    };
    let (small, large) = (steps(200, 40), steps(800, 160));
    println!("COST copied master names: {small} tries at k = 200, s = 40, {large} at k = 800, s = 160");
    // The counted units, exactly: each of the k names is looked at once, the first split steps over the k - 1 numbers in use, and each of the s - 1
    // splits takes the number it has.
    assert_eq!(small, 2 * 200 + 40 - 2, "the names looked at and the numbers tried");
    assert_eq!(large, 2 * 800 + 160 - 2);
    assert!(large <= small * 5, "k + s grows 4 times, k * s 16 times: {small} then {large}");
}

/// The step cap is held inside both walks of the names: each stops where the cap is passed, with the save refused, and does not go on to the end of the
/// names and be refused after (the names are what a file makes, and the cap is what a save may spend).
#[test]
fn the_walks_of_master_page_names_stop_at_the_step_cap() {
    let tries = |k: usize, cap: u64| {
        let mut d = Drawing::a4();
        for n in 2..=k {
            d.masters.push_str(&format!(r#"<style:master-page style:name="Default_{n}" style:page-layout-name="PM1"/>"#));
        }
        let mut o = open(&d.page("").page(""));
        set_page_cell(&mut o.doc, 0, "PageHeight", cm(42.0));
        let mut work = diacraft_odg::WriteWork::default();
        let options =
            diacraft_odg::WriteOptions { limits: diacraft_odg::Limits { max_write_steps: cap, ..Default::default() }, ..Default::default() };
        let refused =
            diacraft_odg::write_counted(&o.doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work)
                .err()
                .map(|e| e.class());
        assert_eq!(refused, Some("write_limit"), "k={k} cap={cap}");
        work.master_name_tries
    };
    // 3,000 names and a cap of 1,000: the first walk, over the names in use, stops at the cap.
    let first = tries(3_000, 1_000);
    println!("COST master names, cap 1,000: {first} tries of 3,000 names");
    assert!((900..=1_001).contains(&first), "{first}");
    // 1,000 names and a cap of 1,500: the first walk is done, and the second, over the numbers in use, stops half way.
    let second = tries(1_000, 1_500);
    println!("COST master names, cap 1,500: {second} tries of 1,000 names and 999 numbers");
    assert!((1_400..=1_501).contains(&second), "{second}");
    // 1,000 pages that share `Default`, all but the last resized, and a cap 300 steps above what the comparison of the pages costs: the number each
    // split takes is a step, and the walk over the splits stops where the cap is passed.
    let mut d = Drawing::a4();
    for _ in 0..1_000 {
        d = d.page("");
    }
    let mut o = open(&d);
    for page in 0..999 {
        set_page_cell(&mut o.doc, page, "PageHeight", cm(42.0));
    }
    let run = |cap: u64| {
        let mut work = diacraft_odg::WriteWork::default();
        let options =
            diacraft_odg::WriteOptions { limits: diacraft_odg::Limits { max_write_steps: cap, ..Default::default() }, ..Default::default() };
        let saved =
            diacraft_odg::write_counted(&o.doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
        (saved.err().map(|e| e.class()), work)
    };
    let (done, whole) = run(u64::MAX);
    assert_eq!(done, None);
    let compared = whole.shapes_compared + whole.cells_compared;
    let (refused, work) = run(compared + 300);
    assert_eq!(refused, Some("write_limit"));
    println!("COST master names, 999 splits, cap {} above the comparison: {} tries", 300, work.master_name_tries);
    assert!(work.master_name_tries <= 300, "{}", work.master_name_tries);
}

/// What each split costs in steps, exactly: the master page's number is one, and writing the copies looks up the layout and the master page, one
/// step each. Pages are added at 11 and 31 and the cost of the 20 more is divided by 20.
#[test]
fn each_split_of_a_master_page_costs_the_same_steps() {
    let steps = |pages: usize| {
        let mut d = Drawing::a4();
        for _ in 0..pages {
            d = d.page("");
        }
        let mut o = open(&d);
        for page in 0..pages - 1 {
            set_page_cell(&mut o.doc, page, "PageHeight", cm(42.0));
        }
        let mut work = diacraft_odg::WriteWork::default();
        diacraft_odg::write_counted(
            &o.doc,
            Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &diacraft_odg::WriteOptions::default(),
            &mut work,
        )
        .unwrap();
        (work.scope_steps, work.master_name_tries)
    };
    let ((a, tries_a), (b, tries_b)) = (steps(11), steps(31));
    println!("COST per split: {a} steps and {tries_a} tries at 11 pages, {b} and {tries_b} at 31");
    assert_eq!(tries_b - tries_a, 20, "one try for each split");
    assert_eq!((b - a) % 20, 0, "{a} then {b}");
    // 10: one for the number, two for the lookups of the layout and the master page, and the rest are the steps of the namespace prefixes looked up
    // for the attributes of the two copies and of the page that names the copy.
    assert_eq!((b - a) / 20, 10, "steps for each split");
}

/// A layout that two master pages name is copied for the page that is resized, though its own master page is used by no other page: the other master
/// page would be moved to the new size too.
#[test]
fn a_layout_that_two_master_pages_share_is_copied_when_the_one_page_on_either_is_resized() {
    let mut d = two_pages();
    d.masters.push_str(r#"<style:master-page style:name="Other" style:page-layout-name="PM1"/>"#);
    d.pages = d.pages.replace(r#"draw:name="page2" draw:master-page-name="Default""#, r#"draw:name="page2" draw:master-page-name="Other""#);
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let (styles, content) = (part(&saved.bytes, "styles.xml"), part(&saved.bytes, "content.xml"));
    assert!(styles.contains(r#"<style:page-layout style:name="PM2""#), "{styles}");
    assert!(styles.contains(r#"style:name="Other_2" style:page-layout-name="PM2""#), "{styles}");
    assert_eq!(styles.matches("fo:page-height=\"42cm\"").count(), 1, "only the copy has the new height: {styles}");
    assert!(content.contains(r#"draw:name="page2" draw:master-page-name="Other_2""#), "{content}");
    assert!(content.contains(r#"draw:name="page1" draw:master-page-name="Default""#), "{content}");
    let back = open_bytes(saved.bytes.clone());
    assert!((number_of(&back.doc, 1, "PageHeight") - cm(42.0)).abs() < 1e-9);
    assert!((number_of(&back.doc, 0, "PageHeight") - cm(29.7)).abs() < 1e-9, "the page on the other master page is as it was");
}

/// Masters `Default` and `Other` both name `PM1`; the pages named in `on` are on them, in that order. The reviewer's input is `["Default", "Default", "Other"]`.
fn shared_layout_drawing(on: &[&str]) -> Drawing {
    let mut d = Drawing::a4();
    d.masters.push_str(r#"<style:master-page style:name="Other" style:page-layout-name="PM1"/>"#);
    for (k, master) in on.iter().enumerate() {
        d = d.page(&custom(&format!("s{}", k + 1), "1cm"));
        d.pages = d.pages.replace(
            &format!(r#"draw:name="page{}" draw:master-page-name="Default""#, k + 1),
            &format!(r#"draw:name="page{}" draw:master-page-name="{master}""#, k + 1),
        );
    }
    d
}

/// A page that moved to a copy was taken from the count of the layout by the master pages that name it, not by the pages that reach
/// it, so a layout two master pages name was edited in place under a page that was not edited (page 2 read back 50 cm). Pages 1 and 3 resized in one
/// save, in both orders of the master pages: page 2 is as it was, and each of the two has its own size.
#[test]
fn a_layout_two_master_pages_name_is_not_edited_in_place_under_a_page_that_was_not_edited() {
    for on in [["Default", "Default", "Other"], ["Other", "Default", "Default"], ["Default", "Other", "Default"]] {
        let mut o = open(&shared_layout_drawing(&on));
        // The pages edited are the first and the last; the one in the middle is not.
        set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
        set_page_cell(&mut o.doc, 2, "PageHeight", cm(50.0));
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{on:?}: {e}"));
        let back = open_bytes(saved.bytes.clone());
        let heights: Vec<f64> = (0..3).map(|p| number_of(&back.doc, p, "PageHeight") / cm(1.0)).collect();
        assert!((heights[0] - 40.0).abs() < 1e-6 && (heights[2] - 50.0).abs() < 1e-6, "{on:?}: {heights:?}");
        assert!((heights[1] - 29.7).abs() < 1e-6, "{on:?}: the page nobody edited is as it was: {heights:?}");
        // Each edited page names a copy of its own, and the page that was not edited the master page and layout it had.
        let content = part(&saved.bytes, "content.xml");
        assert!(content.contains(&format!(r#"draw:name="page2" draw:master-page-name="{}""#, on[1])), "{on:?}: {content}");
        assert!(!content.contains(&format!(r#"draw:name="page1" draw:master-page-name="{}""#, on[0])), "{on:?}: {content}");
    }
}

/// A master page that a page named before the save and no page names after is taken out, and one that nothing named stays. `Default` and
/// `Other` share `PM1`, and `Spare`, which no page names, too; page 2 on `Other` is resized and moves to a copy: `Other` is unnamed now and goes,
/// `PM1` is still `Default`'s and `Spare`'s and stays, `Spare` was never named and stays.
#[test]
fn a_master_page_the_save_leaves_with_no_page_is_taken_out_and_one_no_page_named_stays() {
    let mut d = shared_layout_drawing(&["Default", "Other"]);
    d.masters.push_str(r#"<style:master-page style:name="Spare" style:page-layout-name="PM1"/>"#);
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(42.0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let styles = part(&saved.bytes, "styles.xml");
    assert!(!styles.contains(r#"style:name="Other""#), "no page names Other any more: {styles}");
    assert!(styles.contains(r#"style:name="Other_2" style:page-layout-name="PM2""#), "{styles}");
    assert!(styles.contains(r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#), "{styles}");
    assert!(styles.contains(r#"<style:master-page style:name="Spare" style:page-layout-name="PM1"/>"#), "no page ever named it: {styles}");
    assert!(styles.contains(r#"<style:page-layout style:name="PM1""#), "{styles}");
    let back = open_bytes(saved.bytes);
    assert!((number_of(&back.doc, 1, "PageHeight") - cm(42.0)).abs() < 1e-9);
    assert!((number_of(&back.doc, 0, "PageHeight") - cm(29.7)).abs() < 1e-9);
}

/// A master page that holds shapes, named by one page: the model has it as a background page (page 1, after the foreground page 0).
fn background_drawing() -> Drawing {
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{}</style:master-page>"#, custom("bg", "1cm")),
    );
    d.page(&custom("s1", "1cm"))
}

/// The background page: the layout of a page whose master page holds shapes is the background page's too. Resizing the page alone is
/// refused in words (the first save made the background page 40 cm while the model kept it at 29.7, and the second save was refused for a change
/// the user had not made); resizing the background page alone is refused; resizing both to the same size edits the layout once, and the background
/// page's shapes are written against the new height.
#[test]
fn a_page_whose_master_page_holds_shapes_is_resized_only_with_its_background_page() {
    let refused = |o: &crate::write_core::Open, words: &str| match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(what)) => assert!(what.contains(words), "{what}"),
        other => panic!("expected a refusal of {words}: {other:?}"),
    };
    let mut o = open(&background_drawing());
    assert_eq!(o.doc.pages.len(), 2);
    assert!(o.doc.pages.get(1).unwrap().background, "the master page that holds a shape is a background page");
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    refused(&o, "master page holds shapes");
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(40.0));
    refused(&o, "background page");
    // The two together: the layout is edited where it is, and the background page's shape is lower by what the page grew.
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(40.0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let (before, after) = (part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"));
    assert_eq!(after.matches("fo:page-height=\"40cm\"").count(), 1, "{after}");
    assert!(!after.contains("PM2") && !after.contains("Default_2"), "no copy: {after}");
    assert!(before.contains("svg:y=\"3cm\"") && after.contains("svg:y=\"13.3cm\""), "the background page's shape: {after}");
    let back = open_bytes(saved.bytes);
    for p in 0..2 {
        assert!((number_of(&back.doc, p, "PageHeight") - cm(40.0)).abs() < 1e-9, "page {p}");
    }
}

/// "the same new size" is the same cells with the same numbers. A background page resized to another height, or in another cell, or in
/// more cells than the page is, is refused in words (each, since the layout is one: whichever size the file got, the other page would not have it).
#[test]
fn a_page_and_its_background_page_resized_to_different_sizes_are_refused_each_difference_by_itself() {
    let refused = |o: &crate::write_core::Open, what: &str| match real(&o.doc, &o.bytes, &o.index) {
        Err(e @ WriteError::Unsupported(words)) => {
            assert_eq!(e.class(), "write_unsupported", "{what}");
            assert!(words.contains("master page holds shapes"), "{what}: {words}");
        }
        other => panic!("{what}: expected a refusal: {other:?}"),
    };
    // The same cell, another number.
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(50.0));
    refused(&o, "another height");
    // The same number of cells, but not the same cell.
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageWidth", cm(40.0));
    refused(&o, "another cell");
    // The background page is resized in one cell more than the page.
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 1, "PageWidth", cm(30.0));
    refused(&o, "one cell more");
    // And the page in one cell more than the background page.
    let mut o = open(&background_drawing());
    set_page_cell(&mut o.doc, 0, "PageHeight", cm(40.0));
    set_page_cell(&mut o.doc, 0, "PageWidth", cm(30.0));
    set_page_cell(&mut o.doc, 1, "PageHeight", cm(40.0));
    refused(&o, "one cell less");
    // The same two cells with the same numbers are written.
    let mut o = open(&background_drawing());
    for p in 0..2 {
        set_page_cell(&mut o.doc, p, "PageHeight", cm(40.0));
        set_page_cell(&mut o.doc, p, "PageWidth", cm(30.0));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let back = open_bytes(saved.bytes);
    for p in 0..2 {
        assert!(
            (number_of(&back.doc, p, "PageHeight") - cm(40.0)).abs() < 1e-9 && (number_of(&back.doc, p, "PageWidth") - cm(30.0)).abs() < 1e-9,
            "page {p}"
        );
    }
}

/// A part no page of it changes, and that no page layout or marker is wanted in, is left alone: a shape moved in `content.xml` does not copy a
/// `styles.xml` many times the size of the page (the guard that skips a part).
#[test]
fn a_part_that_nothing_is_changed_in_is_not_copied() {
    let big: String = (0..3_000).map(|k| format!("<style:style style:name=\"Big{k}\" style:family=\"graphic\"/>")).collect();
    let mut o = open(&Drawing::a4().common(&big).page(&custom("s1", "1cm")));
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    crate::write_core::move_by(&mut o.doc, 0, id, 2.0, 0.0);
    let styles = part(&o.bytes, "styles.xml").len() as u64;
    let mut work = WriteWork::default();
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(saved.edited);
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "styles.xml is as it was");
    assert!(styles > 100_000, "{styles}");
    assert!(work.bytes_copied < styles / 4, "styles.xml ({styles} bytes) is not copied: {} bytes were", work.bytes_copied);
}
