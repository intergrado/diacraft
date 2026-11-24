//! The pages of a drawing added, renamed, taken away and put in another order (
//! "Pages, layers, package"): the writer splices each change into `content.xml` (and `styles.xml` for a new size) and nothing else; a page nothing
//! changed keeps its bytes, and a second save of what a save wrote is no edit.
//!
//! The model's pages are matched with the file's by `ID` (the index records each entry's), so every test here edits the model the way the engine does: a page is added under
//! the highest `ID` plus one, a page that is taken away or moved keeps the others' `ID`s, and a rename leaves the page's universal name.

use std::sync::Arc;

use diacraft_doc::{Document, Page, ShapeList};
use diacraft_odg::{WriteError, Written};

use crate::drawing::{Drawing, without_package};
use crate::write_core::{Open, compare, custom, open, open_bytes, part, real};
use crate::write_new::rectangle;

/// A drawing of `n` pages `page1` to `pagen`, each holding the custom shapes `p<k>a` and `p<k>b`.
pub fn pages_of(n: usize) -> Drawing {
    let mut d = Drawing::a4();
    for k in 1..=n {
        d = d.page(&format!("{}{}", custom(&format!("p{k}a"), "1cm"), custom(&format!("p{k}b"), "5cm")));
    }
    d
}

pub fn names(doc: &Document) -> Vec<String> {
    doc.pages.iter().map(|p| p.name.to_string()).collect()
}

pub fn ids(doc: &Document) -> Vec<u32> {
    doc.pages.iter().map(|p| p.id).collect()
}

pub fn page_mut(doc: &mut Document, at: usize) -> &mut Page {
    Arc::make_mut(doc.pages.get_mut(at).expect("no such page"))
}

/// The page of the model with the shapes `shapes` and the layout cells of the first page, under `id` and `name`, as the engine's `page.add` makes one.
pub fn new_page(doc: &Document, id: u32, name: &str, shapes: Vec<diacraft_doc::Shape>) -> Page {
    let mut page = Page::new(id, name);
    page.sheet = Arc::clone(&doc.pages.get(0).expect("a page").sheet);
    page.shapes = ShapeList::new();
    for s in shapes {
        page.shapes.push(Arc::new(s));
    }
    page
}

fn save(o: &Open) -> Written {
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()))
}

fn refused(o: &Open) -> String {
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

/// The saved package as an open drawing again: what the next session holds, with the index the save gave.
fn reopen(saved: &Written) -> Open {
    open_bytes(saved.bytes.clone())
}

/// A second save of the saved drawing, from the model and the index the first gave: no edit, the same bytes.
#[track_caller]
fn assert_second_save_is_no_edit(o: &Open, saved: &Written) {
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("the second save was refused: {} ({e})", e.class()));
    assert!(!again.edited, "a second save of what a save wrote is no edit");
    assert_eq!(again.bytes, saved.bytes, "and the bytes are the same");
}

/// The element of `part` that starts with `open` and ends at the first `close` after it.
fn element<'a>(part: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = part.find(open)?;
    let end = part[start..].find(close)? + close.len();
    Some(&part[start..start + end])
}

/// The `draw:page` element named `name`, to its end.
fn page_element<'a>(content: &'a str, name: &str) -> &'a str {
    element(content, &format!("<draw:page draw:name=\"{name}\""), "</draw:page>").unwrap_or_else(|| panic!("no page {name} in {content}"))
}

#[test]
fn a_page_is_renamed_where_it_stands_and_only_its_name_changes() {
    let mut o = open(&pages_of(3));
    page_mut(&mut o.doc, 1).name = Arc::from("The \"second\" & <last> one");
    let saved = save(&o);
    assert!(saved.edited);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(
        after,
        before.replace("<draw:page draw:name=\"page2\"", "<draw:page draw:name=\"The &quot;second&quot; &amp; &lt;last> one\""),
        "the name of the page is the only change of content.xml"
    );
    assert_eq!(part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"), "the master pages are the file's");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "The \"second\" & <last> one", "page3"]);
    // A rename leaves the page's universal name, which the document file carries.
    assert_eq!(back.doc.pages.get(1).unwrap().name_u.as_deref(), Some("page2"));
    assert_eq!(without_package(&back.doc), without_package(&saved_model(&o.doc)), "the drawing reads back as the model was");
    assert_second_save_is_no_edit(&o, &saved);
}

/// The model as a session that saved holds it: what the reader gives for the saved file is the model, but for what it adds to a page it reads (the page's markup).
fn saved_model(doc: &Document) -> Document {
    doc.clone()
}

#[test]
fn a_page_is_taken_away_with_its_shapes_and_the_other_pages_keep_their_bytes() {
    let mut o = open(&pages_of(3));
    o.doc.pages.remove(1);
    let saved = save(&o);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(page_element(&after, "page1"), page_element(&before, "page1"));
    assert_eq!(page_element(&after, "page3"), page_element(&before, "page3"));
    assert!(!after.contains("page2") && !after.contains("p2a"), "the page and its shapes are gone: {after}");
    assert_eq!(after, before.replace(page_element(&before, "page2"), ""), "and nothing else changed");
    assert_eq!(
        part(&o.bytes, "styles.xml"),
        part(&saved.bytes, "styles.xml"),
        "the master pages and layouts stay: a page taken away does not take them"
    );
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page3"]);
    assert_eq!(saved.index.pages(), 2, "the index holds the pages that are left");
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn pages_are_put_in_another_order_by_moving_their_elements_whole() {
    let mut o = open(&pages_of(4));
    // The last page first: one element moves.
    let last = o.doc.pages.remove(3).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    let saved = save(&o);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    for k in 1..=4 {
        assert_eq!(page_element(&after, &format!("page{k}")), page_element(&before, &format!("page{k}")), "page {k} is the bytes it was");
    }
    let order: Vec<usize> = (1..=4).map(|k| after.find(&format!("draw:name=\"page{k}\"")).unwrap()).collect();
    assert!(order[3] < order[0] && order[0] < order[1] && order[1] < order[2], "{order:?}");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page4", "page1", "page2", "page3"]);
    assert_eq!(ids(&back.doc), ids(&o.doc), "the pages keep the IDs the model gave them");
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 2);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_is_added_with_its_new_shapes_on_the_master_page_of_a_page_that_has_its_size() {
    let mut o = open(&pages_of(2));
    let page = new_page(&o.doc, 2, "Page-3", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    for name in ["page1", "page2"] {
        assert_eq!(page_element(&after, name), page_element(&before, name), "the pages the file had are the bytes they were");
    }
    let added = page_element(&after, "Page-3");
    assert!(added.contains("draw:master-page-name=\"Default\"") && added.contains("draw:name=\"Box\""), "{added}");
    assert_eq!(part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"), "the page's size is the file's: no master page or layout is made");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page2", "Page-3"]);
    assert_eq!(ids(&back.doc), [0, 1, 2]);
    let new = back.doc.pages.get(2).unwrap();
    assert_eq!(new.shapes.len(), 1);
    assert_eq!(new.shapes.get(0).unwrap().name.as_deref(), Some("Box"));
    assert_eq!(saved.index.pages(), 3);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn the_second_save_after_a_page_change_still_writes_the_next_edit_into_the_page() {
    let mut o = open(&pages_of(2));
    let page = new_page(&o.doc, 2, "Page-3", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let first = save(&o);
    // A shape added to the page that was added, in the next save from the index the first gave.
    let shape = rectangle(2, "Second", (8.0, 8.0, 2.0, 2.0));
    Arc::make_mut(o.doc.pages.get_mut(2).unwrap()).shapes.push(Arc::new(shape));
    let second = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("refused: {e}"));
    assert!(second.edited);
    let back = reopen(&second);
    assert_eq!(back.doc.pages.get(2).unwrap().shapes.len(), 2);
    // Only that page's element changed (and the styles the new shape needed).
    let changes = compare(&first.bytes, &second.bytes);
    let content = changes.parts.iter().find(|p| p.part == "content.xml").expect("content.xml changed");
    assert!(content.changed.iter().all(|c| c.path.contains("page")) || content.counts.added > 0, "{content:?}");
    assert_second_save_is_no_edit(&o, &second);
}

/// A page of two rectangles `a` and `b` and a connector glued from the first to the second, as the engine's pages hold them.
fn glued_page(doc: &Document, id: u32, name: &str) -> Page {
    let shapes = vec![
        rectangle(1, "a", (2.0, 3.0, 4.0, 2.0)),
        rectangle(2, "b", (12.0, 3.0, 4.0, 2.0)),
        crate::write_new::connector(3, (6.0, 22.7, 12.0, 22.7)),
    ];
    let mut page = new_page(doc, id, name, shapes);
    let connect = |begin: bool, to: u32, row: u32| diacraft_doc::Connect {
        from_sheet: 3,
        from_cell: Some(Arc::from(if begin { "BeginX" } else { "EndX" })),
        from_part: Some(if begin { 9 } else { 12 }),
        to_sheet: to,
        to_cell: Some(Arc::from("PinX")),
        to_part: Some(100 + row),
        ..diacraft_doc::Connect::default()
    };
    page.connects = diacraft_doc::Connects::from_vec(vec![connect(true, 1, 3), connect(false, 2, 1)]);
    page
}

#[test]
fn the_ids_new_connectors_give_their_targets_are_one_series_across_the_pages_the_save_writes() {
    // Two pages added in one save, each with a connector glued to its two shapes: `xml:id` is unique in the part, so the second page's ids go on from the first's.
    let mut o = open(&pages_of(1));
    for (id, name) in [(1, "one"), (2, "two")] {
        let page = glued_page(&o.doc, id, name);
        o.doc.pages.push(Arc::new(page));
    }
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    let ids: Vec<&str> = after.split("xml:id=\"").skip(1).filter_map(|t| t.split('"').next()).collect();
    assert_eq!(ids.len(), 4, "two targets for each of the two connectors: {after}");
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 4, "no id twice: {ids:?}");
    let back = reopen(&saved);
    assert_eq!(back.doc.pages.get(1).unwrap().connects.len(), 2);
    assert_eq!(back.doc.pages.get(2).unwrap().connects.len(), 2);
    assert_second_save_is_no_edit(&o, &saved);
}

/// Sets a page cell's number (its formula and unit stay).
fn set_page_cell(doc: &mut Document, page: usize, cell: &str, value: f64) {
    let p = page_mut(doc, page);
    let sheet = Arc::make_mut(&mut p.sheet);
    sheet.cells.get_mut(cell).unwrap_or_else(|| panic!("no cell {cell}")).value = diacraft_doc::Value::from_number(value);
}

/// The page's number of a cell, in inches.
fn number_of(doc: &Document, page: usize, cell: &str) -> f64 {
    doc.pages.get(page).unwrap().sheet.cell(cell).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN)
}

#[test]
fn a_page_that_moves_keeps_the_edits_made_inside_it_in_the_same_save() {
    let mut o = open(&pages_of(3));
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    // The moved page's first shape (now on page 0) is moved by 1 cm, and a shape of the page after it is taken away.
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    Arc::make_mut(o.doc.pages.get_mut(2).unwrap()).shapes.remove(1);
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    let moved = page_element(&after, "page3");
    assert!(moved.contains("draw:name=\"p3a\" svg:x=\"2cm\""), "the edit inside the page that moved is written in its new place: {moved}");
    assert!(!page_element(&after, "page2").contains("p2b"), "and the shape taken away from another page is gone");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page3", "page1", "page2"]);
    assert_eq!(back.doc.pages.get(2).unwrap().shapes.len(), 1);
    assert_eq!(
        back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).map(|v| (v * 1e6).round()),
        o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()?.number()).map(|v| (v * 1e6).round())
    );
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_whose_size_the_file_has_names_the_master_page_of_that_size_and_one_that_is_new_names_a_copy() {
    // A page the size of the others names the master page they have: no master page or layout is made (the test above); a page of another size gets the copy R13 gives
    // a page resized, with the layout edited to the new size, and the first page and `Default` are untouched.
    let mut o = open(&pages_of(2));
    let mut page = new_page(&o.doc, 2, "Wide", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    {
        let sheet = Arc::make_mut(&mut page.sheet);
        sheet.cells.get_mut("PageWidth").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(29.7));
        sheet.cells.get_mut("PageHeight").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(42.0));
    }
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let (before, styles) = (part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"));
    let old_layout = element(&before, "<style:page-layout style:name=\"PM1\"", "</style:page-layout>").unwrap();
    let new_layout = element(&styles, "<style:page-layout style:name=\"PM2\"", "</style:page-layout>").unwrap_or_else(|| panic!("no PM2: {styles}"));
    assert_eq!(
        new_layout,
        old_layout
            .replace("PM1", "PM2")
            .replace("fo:page-width=\"21cm\"", "fo:page-width=\"29.7cm\"")
            .replace("fo:page-height=\"29.7cm\"", "fo:page-height=\"42cm\"")
    );
    assert!(
        styles.contains(r#"<style:master-page style:name="Default_2" style:page-layout-name="PM2" style:display-name="Default 2"/>"#),
        "{styles}"
    );
    let content = part(&saved.bytes, "content.xml");
    assert!(page_element(&content, "Wide").contains("draw:master-page-name=\"Default_2\""), "{content}");
    assert!(page_element(&content, "page1").contains("draw:master-page-name=\"Default\""));
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page2", "Wide"]);
    assert!((number_of(&back.doc, 2, "PageWidth") - crate::drawing::cm(29.7)).abs() < 1e-9);
    assert!((number_of(&back.doc, 2, "PageHeight") - crate::drawing::cm(42.0)).abs() < 1e-9);
    assert!((number_of(&back.doc, 0, "PageHeight") - crate::drawing::cm(29.7)).abs() < 1e-9, "the other pages are the size they were");
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn new_pages_of_one_size_share_the_master_page_made_for_the_first_of_them() {
    let mut o = open(&pages_of(1));
    for (id, name) in [(1, "A"), (2, "B"), (3, "C")] {
        let mut page = new_page(&o.doc, id, name, Vec::new());
        {
            let sheet = Arc::make_mut(&mut page.sheet);
            sheet.cells.get_mut("PageWidth").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(29.7));
            sheet.cells.get_mut("PageHeight").unwrap().value = diacraft_doc::Value::from_number(crate::drawing::cm(42.0));
        }
        o.doc.pages.push(Arc::new(page));
    }
    let saved = save(&o);
    let styles = part(&saved.bytes, "styles.xml");
    assert_eq!(styles.matches("<style:master-page ").count(), 2, "one copy for the three pages: {styles}");
    assert_eq!(styles.matches("<style:page-layout ").count(), 2, "{styles}");
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("draw:master-page-name=\"Default_2\"").count(), 3, "{content}");
    // Empty pages are written as LibreOffice writes them, as empty elements.
    assert!(content.contains("<draw:page draw:name=\"A\" draw:master-page-name=\"Default_2\"/>"), "{content}");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "A", "B", "C"]);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_added_beside_a_page_resized_in_the_same_save_names_the_master_page_the_resized_page_has() {
    let mut o = open(&pages_of(1));
    set_page_cell(&mut o.doc, 0, "PageWidth", crate::drawing::cm(29.7));
    set_page_cell(&mut o.doc, 0, "PageHeight", crate::drawing::cm(42.0));
    // The page that is new is the page the first now is (as `page.add` with `like` makes one).
    let mut page = new_page(&o.doc, 1, "Copy", Vec::new());
    page.sheet = Arc::clone(&o.doc.pages.get(0).unwrap().sheet);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let styles = part(&saved.bytes, "styles.xml");
    assert_eq!(
        styles.matches("<style:master-page ").count(),
        1,
        "the layout of `Default` is edited where it is, and the new page names it: {styles}"
    );
    assert!(styles.contains("fo:page-width=\"29.7cm\"") && styles.contains("fo:page-height=\"42cm\""), "{styles}");
    let back = reopen(&saved);
    assert!((number_of(&back.doc, 1, "PageHeight") - crate::drawing::cm(42.0)).abs() < 1e-9);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_added_in_the_size_of_a_page_taken_away_does_not_name_a_layout_the_save_resized_for_the_page_that_stays() {
    // `page1` and `page2` share `Default`. The first is taken away, the second resized (the only page that reaches the layout now, so the layout is edited where it is), and a page
    // added in the first's size, which is the size the layout had: it names a copy of the layout as it was, and never the one that is the second page's size now.
    let mut o = open(&pages_of(2));
    let first = new_page(&o.doc, 2, "Back to A4", Vec::new());
    o.doc.pages.remove(0);
    set_page_cell(&mut o.doc, 0, "PageWidth", crate::drawing::cm(29.7));
    set_page_cell(&mut o.doc, 0, "PageHeight", crate::drawing::cm(42.0));
    o.doc.pages.push(Arc::new(first));
    let saved = save(&o);
    let styles = part(&saved.bytes, "styles.xml");
    assert!(
        styles.contains("<style:page-layout style:name=\"PM1\"") && styles.contains("fo:page-width=\"29.7cm\""),
        "the layout of `Default` is the second page's size now: {styles}"
    );
    let content = part(&saved.bytes, "content.xml");
    assert!(page_element(&content, "page2").contains("draw:master-page-name=\"Default\""), "{content}");
    // (The page holds no shape: it is written as an empty element.)
    assert!(content.contains("<draw:page draw:name=\"Back to A4\" draw:master-page-name=\"Default_2\"/>"), "the new page names a copy: {content}");
    let back = reopen(&saved);
    assert!((number_of(&back.doc, 0, "PageHeight") - crate::drawing::cm(42.0)).abs() < 1e-9);
    assert!((number_of(&back.doc, 1, "PageHeight") - crate::drawing::cm(29.7)).abs() < 1e-9, "the page added is the size it has in the model");
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn the_styles_and_the_files_only_a_page_taken_away_named_go_with_it() {
    // A style only the shapes of the second page name, and an object only its frame names, go with the page; a style and a picture the first page names stay.
    let style = |n: u32, color: &str| {
        format!(r#"<style:style style:name="gr{n}" style:family="graphic"><style:graphic-properties draw:fill-color="{color}"/></style:style>"#)
    };
    let frame = |name: &str, style: &str| {
        format!(
            r#"<draw:frame draw:name="{name}" draw:style-name="{style}" svg:x="2cm" svg:y="2cm" svg:width="5cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#
        )
    };
    let shape = |name: &str, style: &str| {
        format!(
            r#"<draw:custom-shape draw:name="{name}" draw:style-name="{style}" svg:x="1cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#
        )
    };
    let mut d = Drawing::a4().auto(&format!("{}{}", style(1, "#ff0000"), style(2, "#00ff00"))).page(&shape("one", "gr1")).page(&format!(
        "{}{}",
        shape("two", "gr2"),
        frame("ole", "gr2")
    ));
    for (name, bytes) in [
        ("Object 1/content.xml", &b"<office:document-content/>"[..]),
        ("ObjectReplacements/Object 1", &crate::write_core::PNG[..]),
        ("Pictures/keep.png", &crate::write_core::PNG[..]),
    ] {
        d.files.push((name.to_owned(), bytes.to_vec()));
    }
    let mut o = open_bytes(d.bytes());
    o.doc.pages.remove(1);
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("style:name=\"gr1\"") && !after.contains("style:name=\"gr2\""), "{after}");
    let diff = compare(&o.bytes, &saved.bytes);
    let mut removed = diff.removed.clone();
    removed.sort_unstable();
    assert_eq!(removed, ["Object 1/content.xml", "ObjectReplacements/Object 1"], "{diff:?}");
    assert!(crate::write_core::has_part(&saved.bytes, "Pictures/keep.png"));
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn the_last_page_in_order_is_taken_away_and_the_only_page_is_not() {
    let mut o = open(&pages_of(3));
    o.doc.pages.remove(2);
    let saved = save(&o);
    assert_eq!(names(&reopen(&saved).doc), ["page1", "page2"]);
    assert_second_save_is_no_edit(&o, &saved);
    // A drawing of no page is no drawing: the file would hold no page the reader finds.
    let mut one = open(&pages_of(1));
    one.doc.pages.remove(0);
    let words = refused(&one);
    assert!(words.contains("no page"), "{words}");
}

#[test]
fn a_page_renamed_to_the_name_another_page_has_is_refused_and_a_name_the_file_already_repeats_is_not() {
    let mut o = open(&pages_of(2));
    page_mut(&mut o.doc, 1).name = Arc::from("page1");
    let words = refused(&o);
    assert!(words.contains("under one name") && words.contains("rename"), "{words}");
    // Two pages of one name in a file that has them already (the document file keeps them apart), saved with another edit, are saved.
    let twins = Drawing::a4().page(&custom("a", "1cm"));
    let mut twins = twins;
    twins.pages.push_str(r#"<draw:page draw:name="page1" draw:master-page-name="Default"></draw:page>"#);
    let mut o = open(&twins);
    assert_eq!(names(&o.doc), ["page1", "page1"]);
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    let saved = save(&o);
    assert!(saved.edited);
    assert_eq!(names(&reopen(&saved).doc), ["page1", "page1"]);
}

/// What a VSDX file held of a page (its attributes and elements, the relationship and the part it came from) is no reason to refuse it: it rides in DiaCraft's document file.
#[test]
fn a_new_page_that_holds_what_a_vsdx_page_holds_is_saved_and_the_data_is_read_back() {
    let mut o = open(&pages_of(1));
    let mut page = new_page(&o.doc, 1, "New", Vec::new());
    page.rel_id = Some(Arc::from("rId1"));
    page.raw.push("ViewScale", "-1");
    page.part_name = Some(Arc::from("/visio/pages/page2.xml"));
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let (back, _) = diacraft_odg::read(&saved.bytes, &diacraft_odg::Limits::default()).unwrap();
    let page = back.pages.iter().find(|p| &*p.name == "New").unwrap();
    assert_eq!(
        (page.rel_id.as_deref(), page.raw.get("ViewScale"), page.part_name.as_deref()),
        (Some("rId1"), Some("-1"), Some("/visio/pages/page2.xml"))
    );
}

#[test]
fn a_new_page_that_holds_what_a_new_element_cannot_is_refused_in_words() {
    for (what, change) in [
        ("a background page", Box::new(|p: &mut Page| p.background = true) as Box<dyn Fn(&mut Page)>),
        ("a page naming a background page", Box::new(|p: &mut Page| p.back_page = Some(0))),
    ] {
        let mut o = open(&pages_of(1));
        let mut page = new_page(&o.doc, 1, "New", Vec::new());
        change(&mut page);
        o.doc.pages.push(Arc::new(page));
        let words = refused(&o);
        assert!(words.contains("new page") || words.contains("background page"), "{what}: {words}");
    }
}

#[test]
fn an_empty_page_written_as_an_empty_element_gains_its_shapes_and_moves_whole() {
    // LibreOffice writes a page that holds nothing as `<draw:page .../>`: it is opened and closed by the same edit, which goes with the page when the page is put elsewhere.
    let mut d = Drawing::a4().page(&custom("a", "1cm"));
    d.pages.push_str(r#"<draw:page draw:name="empty" draw:master-page-name="Default"/>"#);
    d = d.page(&custom("c", "1cm"));
    let mut o = open(&d);
    assert_eq!(names(&o.doc), ["page1", "empty", "page3"]);
    let page = o.doc.pages.remove(1).unwrap();
    let mut page = (*page).clone();
    page.shapes.push(Arc::new(rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))));
    o.doc.pages.insert(0, Arc::new(page)).unwrap();
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    let moved = page_element(&after, "empty");
    assert!(moved.starts_with("<draw:page draw:name=\"empty\" draw:master-page-name=\"Default\">") && moved.contains("draw:name=\"Box\""), "{moved}");
    assert!(after.find("draw:name=\"empty\"").unwrap() < after.find("draw:name=\"page1\"").unwrap(), "{after}");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["empty", "page1", "page3"]);
    assert_eq!(back.doc.pages.get(0).unwrap().shapes.len(), 1);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_without_a_name_that_moves_is_given_the_name_the_reader_gave_it() {
    // The reader calls a page that has no `draw:name` `Page-` and its place from 1; moved, the page would be read under another name, so its name is written.
    let mut d = Drawing::a4();
    for k in 1..=3 {
        d.pages.push_str(&format!(r#"<draw:page draw:master-page-name="Default">{}</draw:page>"#, custom(&format!("s{k}"), "1cm")));
    }
    let mut o = open(&d);
    assert_eq!(names(&o.doc), ["Page-1", "Page-2", "Page-3"]);
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    let saved = save(&o);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["Page-3", "Page-1", "Page-2"], "every page is read under the name the model has");
    assert_eq!(ids(&back.doc), ids(&o.doc));
    assert_second_save_is_no_edit(&o, &saved);
    // A page without a name that stays where it is is not given one.
    let mut o = open(&d);
    crate::write_core::move_by(&mut o.doc, 1, 1, 1.0, 0.0);
    let after = part(&save(&o).bytes, "content.xml");
    assert!(!after.contains("draw:name=\"Page-"), "{after}");
}

#[test]
fn a_page_name_is_written_whole_whatever_it_holds_and_one_xml_cannot_hold_is_refused() {
    for name in [
        "a_20_b",
        "a b",
        "\"quoted\" 'single' & <angle> ]]> done",
        "two\nlines",
        "tab\there",
        "Ünïcödé — 日本語 🙂",
        "  padded  ",
        "#anchor",
        "Page-1",
    ] {
        let mut o = open(&pages_of(2));
        page_mut(&mut o.doc, 1).name = Arc::from(name);
        let saved = save(&o);
        let back = reopen(&saved);
        assert_eq!(back.doc.pages.get(1).unwrap().name.as_ref(), name, "{name:?}");
        assert_second_save_is_no_edit(&o, &saved);
    }
    // A control character XML 1.0 does not allow, and a carriage return that reads back as white space: refused, in words, and nothing is written.
    for name in ["bad\u{1}name", "bad\u{ffff}", "cr\rcr"] {
        let mut o = open(&pages_of(2));
        page_mut(&mut o.doc, 1).name = Arc::from(name);
        match real(&o.doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(words)) => assert!(words.contains("character") || words.contains("carriage"), "{name:?}: {words}"),
            Ok(saved) => assert_eq!(reopen(&saved).doc.pages.get(1).unwrap().name.as_ref(), name, "{name:?} was written, so it must read back"),
            other => panic!("{name:?}: {other:?}"),
        }
    }
}

#[test]
fn a_page_added_after_the_last_was_taken_away_under_the_same_id_is_a_new_page() {
    // The engine gives a page the highest `ID` plus one, so a page added after the last page was taken away has the `ID` the last had. It is not that page: the file's is
    // taken away, the model's is written.
    let mut o = open(&pages_of(3));
    o.doc.pages.remove(2);
    let page = new_page(&o.doc, 2, "Page-3", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("p3a"), "the shapes of the page the file had are gone: {after}");
    assert!(page_element(&after, "Page-3").contains("draw:name=\"Box\""), "{after}");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page2", "Page-3"]);
    assert_eq!(back.doc.pages.get(2).unwrap().shapes.len(), 1);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_carries_its_cells_the_markup_has_no_place_for_through_the_document_file() {
    // The page's scale, a user cell and its universal name are the document file's, keyed by the page's name: a new page's, a renamed page's, and the pages after a page
    // taken away (whose `ID`s are not their places).
    let mut o = open(&pages_of(3));
    o.doc.pages.remove(0);
    let mut page = new_page(&o.doc, 3, "Fourth", Vec::new());
    {
        let sheet = Arc::make_mut(&mut page.sheet);
        sheet.cells.insert(diacraft_doc::CellName::new("Tag"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("on the new page")));
    }
    o.doc.pages.push(Arc::new(page));
    page_mut(&mut o.doc, 0).name = Arc::from("Renamed");
    let saved = save(&o);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["Renamed", "page3", "Fourth"]);
    assert_eq!(ids(&back.doc), [1, 2, 3], "the pages keep the IDs the model gave them");
    assert_eq!(back.doc.pages.get(0).unwrap().name_u.as_deref(), Some("page2"), "and a rename leaves the universal name");
    assert_eq!(
        back.doc.pages.get(2).unwrap().sheet.cell("Tag").map(|c| c.value.clone()),
        o.doc.pages.get(2).unwrap().sheet.cell("Tag").map(|c| c.value.clone())
    );
    for at in 0..2 {
        assert_eq!(back.doc.pages.get(at), o.doc.pages.get(at), "page {at} reads back as the model had it");
    }
    // The page the save added is read back with what its element says besides (the master page it names), and with every cell the model gave it.
    assert_eq!(back.doc.pages.get(2).unwrap().sheet, o.doc.pages.get(2).unwrap().sheet);
    assert_second_save_is_no_edit(&o, &saved);
    let file = part(&saved.bytes, "DiaCraft/document.xml");
    assert!(file.contains("name=\"Fourth\"") && file.contains("name=\"Renamed\""), "{file}");
}

#[test]
fn a_model_whose_pages_were_put_in_another_order_and_edited_again_is_saved_from_the_index_the_save_gave() {
    let mut o = open(&pages_of(3));
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    let first = save(&o);
    // More page changes on the model, from the file and the index the first save gave: a page taken away, one renamed, one added.
    o.doc.pages.remove(1);
    page_mut(&mut o.doc, 1).name = Arc::from("Second now");
    let page = new_page(&o.doc, 3, "Added", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let second = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("the second save was refused: {e}"));
    let back = reopen(&second);
    assert_eq!(names(&back.doc), ["page3", "Second now", "Added"]);
    assert_eq!(ids(&back.doc), ids(&o.doc));
    assert_second_save_is_no_edit(&o, &second);
}

/// A master page that holds shapes, named by one page: the model has it as a background page (page 2, after the foreground pages 0 and 1).
fn background_drawing() -> Drawing {
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{}</style:master-page>"#, custom("bg", "1cm")),
    );
    d.page(&custom("s1", "1cm")).page(&custom("s2", "1cm"))
}

#[test]
fn a_background_page_is_not_added_taken_away_renamed_or_put_before_a_page_and_the_others_are_moved_around_it() {
    // A file reads its pages first and its background pages after them, so the model's must stand so for a save that changes the pages; and a background page is a master page
    // that holds shapes, which this save neither writes new nor takes away.
    let o = open(&background_drawing());
    assert_eq!(o.doc.pages.len(), 3);
    assert!(o.doc.pages.get(2).unwrap().background);
    // The two foreground pages exchanged: moved around the background page, which stays.
    let mut moved = open(&background_drawing());
    let last = moved.doc.pages.remove(1).unwrap();
    moved.doc.pages.insert(0, last).unwrap();
    let saved = save(&moved);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc)[..2], ["page2", "page1"]);
    assert!(back.doc.pages.get(2).unwrap().background);
    assert_second_save_is_no_edit(&moved, &saved);
    // A page taken away with a background page in the file: the background page's `ID` is no longer the place it is read at, and the document file says so.
    let mut fewer = open(&background_drawing());
    fewer.doc.pages.remove(0);
    let saved = save(&fewer);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc)[0], "page2");
    assert_eq!(ids(&back.doc), ids(&fewer.doc), "the pages and the background page keep the IDs the model has");
    assert!(back.doc.pages.get(1).unwrap().background);
    assert_eq!(
        back.doc.pages.get(0).unwrap().back_page,
        Some(back.doc.pages.get(1).unwrap().id),
        "a page names its background page by the ID the document file gave that page"
    );
    assert_eq!(back.doc.pages.get(0).unwrap().back_page, fewer.doc.pages.get(0).unwrap().back_page, "as the model had it");
    assert_second_save_is_no_edit(&fewer, &saved);
    // The background page taken away, renamed, or put before a page that is not one.
    let mut gone = open(&background_drawing());
    gone.doc.pages.remove(2);
    assert!(refused(&gone).contains("background page"), "taken away");
    let mut renamed = open(&background_drawing());
    page_mut(&mut renamed.doc, 2).name = Arc::from("Another master");
    assert!(refused(&renamed).contains("own fields"), "renamed");
    let mut before = open(&background_drawing());
    let background = before.doc.pages.remove(2).unwrap();
    before.doc.pages.insert(0, background).unwrap();
    assert!(refused(&before).contains("background"), "put before a page that is not one");
    // A page added after the background page, as the engine adds one (at the end of the list).
    let mut added = open(&background_drawing());
    let page = new_page(&added.doc, 3, "Page-4", Vec::new());
    added.doc.pages.push(Arc::new(page));
    assert!(refused(&added).contains("background"), "added after a background page");
    // And one put among the foreground pages has no master page of its own without shapes to take its size from: the pages all name the one that holds shapes.
    let mut among = open(&background_drawing());
    let page = new_page(&among.doc, 3, "Page-4", Vec::new());
    among.doc.pages.insert(2, Arc::new(page)).unwrap();
    assert!(refused(&among).contains("master page"), "no master page without shapes");
}

#[test]
fn the_index_says_which_page_of_the_model_each_entry_is_and_is_read_back_in_the_models_order() {
    let o = open(&pages_of(3));
    for at in 0..3 {
        assert_eq!(o.index.page(at).unwrap().page_id(), Some(o.doc.pages.get(at).unwrap().id), "a read gives each entry its page's ID");
    }
    // After a save that put the pages in another order, the entries are in the model's order, each under its page's ID and at the place its element now has.
    let mut m = open(&pages_of(3));
    let last = m.doc.pages.remove(2).unwrap();
    m.doc.pages.insert(0, last).unwrap();
    let saved = save(&m);
    let held: Vec<(Option<u32>, diacraft_odg::PageSource)> =
        (0..3).map(|k| (saved.index.page(k).unwrap().page_id(), saved.index.page(k).unwrap().source)).collect();
    assert_eq!(
        held,
        [(Some(2), diacraft_odg::PageSource::Page(0)), (Some(0), diacraft_odg::PageSource::Page(1)), (Some(1), diacraft_odg::PageSource::Page(2))]
    );
    // An index aligned to the model's pages: the entries in the model's order, an entry with no shapes for a page the index lacks, and nothing when they stand so already.
    assert!(o.index.aligned_to(&[0, 1, 2]).is_none(), "the entries stand as the IDs say");
    let aligned = o.index.aligned_to(&[2, 0, 7]).expect("another order");
    assert_eq!(aligned.pages(), 3);
    assert_eq!(aligned.page(0).unwrap().page_id(), Some(2));
    assert_eq!(aligned.page(0).unwrap().ordinal(1), o.index.page(2).unwrap().ordinal(1));
    assert_eq!(aligned.page(1).unwrap().page_id(), Some(0));
    assert_eq!(aligned.page(2).unwrap().modelled(), 0, "a page the index lacks holds no shape");
    assert_eq!(aligned.page(2).unwrap().page_id(), Some(7));
}

#[test]
fn a_shape_that_carries_tier_2_on_a_page_that_was_added_is_read_back_with_what_it_carries() {
    let mut o = open(&pages_of(1));
    let mut shape = rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet)
        .cells
        .insert(diacraft_doc::CellName::new("Comment"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("on the new page")));
    let page = new_page(&o.doc, 1, "Notes", vec![shape]);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let back = reopen(&saved);
    let read = back.doc.pages.get(1).unwrap().shapes.get(0).unwrap().clone();
    assert_eq!(read.sheet.cell("Comment").and_then(|c| c.value.as_ref()).map(diacraft_doc::Value::text), Some("on the new page"));
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn pages_are_renamed_taken_away_and_moved_in_a_package_with_no_styles_part_and_around_what_stands_between_them() {
    // A package with no `styles.xml` has no master page to touch: a rename, a delete and a move need none. What is between the pages (a comment, a processing
    // instruction) stays where it was.
    let mut d = Drawing::a4();
    d.no_styles = true;
    d.pages = format!(
        "<!-- first -->{}<?keep this?>{}<!-- last -->",
        r#"<draw:page draw:name="a" draw:master-page-name="Default"></draw:page>"#,
        r#"<draw:page draw:name="b" draw:master-page-name="Default"></draw:page><draw:page draw:name="c" draw:master-page-name="Default"></draw:page>"#
    );
    let mut o = open(&d);
    assert_eq!(names(&o.doc), ["a", "b", "c"]);
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    page_mut(&mut o.doc, 1).name = Arc::from("a2");
    o.doc.pages.remove(2);
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<!-- first -->") && after.contains("<?keep this?>") && after.contains("<!-- last -->"), "{after}");
    assert!(!after.contains("draw:name=\"b\""), "{after}");
    assert_eq!(names(&reopen(&saved).doc), ["c", "a2"]);
    assert_second_save_is_no_edit(&o, &saved);
    // A page added needs a master page to name, and the package has none: refused in words.
    let mut o = open(&d);
    let page = new_page(&o.doc, 3, "Page-4", Vec::new());
    o.doc.pages.push(Arc::new(page));
    assert!(refused(&o).contains("package lacks"), "no styles part");
}

#[test]
fn a_flat_drawing_refuses_pages_added_taken_away_or_moved_in_words_and_keeps_the_pages_it_has() {
    let flat = crate::flat_write::three().bytes();
    let refusal = |o: &Open| match crate::flat_write::save(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        other => panic!("a flat drawing is refused in words: {other:?}"),
    };
    let mut added = open_bytes(flat.clone());
    let page = new_page(&added.doc, 5, "Page-2", Vec::new());
    added.doc.pages.push(Arc::new(page));
    let words = refusal(&added);
    assert!(words.contains("flat drawing") && words.contains("page added"), "{words}");
    let mut renamed = open_bytes(flat.clone());
    page_mut(&mut renamed.doc, 0).name = Arc::from("Renamed");
    assert!(refusal(&renamed).contains("page's own fields"));
    // The pages put in another order: refused in the words of a page put in another place, as a page taken away is; and pages of other `ID`s, as many
    // as the drawing has, are the change of the pages' own fields they always were.
    let several = crate::flat::Flat::of(&pages_of(3)).bytes();
    let mut reordered = open_bytes(several.clone());
    assert_eq!(reordered.doc.pages.len(), 3);
    let last = reordered.doc.pages.remove(2).unwrap();
    reordered.doc.pages.insert(0, last).unwrap();
    let words = refusal(&reordered);
    assert!(words.contains("flat drawing") && words.contains("put in another place"), "{words}");
    let mut taken = open_bytes(several.clone());
    taken.doc.pages.remove(1);
    assert!(refusal(&taken).contains("taken from it"));
    let mut other_ids = open_bytes(several);
    for (at, id) in [(0, 7), (1, 8), (2, 9)] {
        page_mut(&mut other_ids.doc, at).id = id;
    }
    assert!(refusal(&other_ids).contains("page's own fields"));
    // The same drawing saved with another edit is saved, pages as they were.
    let mut moved = open_bytes(flat);
    crate::write_core::move_by(&mut moved.doc, 0, 1, 1.0, 0.0);
    assert!(crate::flat_write::save(&moved.doc, &moved.bytes, &moved.index).is_ok());
}

#[test]
fn background_pages_put_in_another_order_than_the_file_has_them_are_refused_and_the_pages_before_them_may_move() {
    // Two pages, each on a master page that holds shapes: two background pages, read after the two pages in the order of their master pages.
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(
            r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{}</style:master-page><style:master-page style:name="Second" style:page-layout-name="PM1">{}</style:master-page>"#,
            custom("bgd", "1cm"),
            custom("bgs", "1cm")
        ),
    );
    d.pages = format!(
        r#"<draw:page draw:name="a" draw:master-page-name="Default">{}</draw:page><draw:page draw:name="b" draw:master-page-name="Second">{}</draw:page>"#,
        custom("sa", "1cm"),
        custom("sb", "1cm")
    );
    let o = open(&d);
    assert_eq!(o.doc.pages.len(), 4);
    assert!(o.doc.pages.get(2).unwrap().background && o.doc.pages.get(3).unwrap().background);
    // The pages exchanged and the background pages with them: the background pages' order is the file's, so only the pages may move.
    let mut swapped = open(&d);
    let b = swapped.doc.pages.remove(1).unwrap();
    swapped.doc.pages.insert(0, b).unwrap();
    let saved = save(&swapped);
    assert_eq!(names(&reopen(&saved).doc)[..2], ["b", "a"]);
    assert_second_save_is_no_edit(&swapped, &saved);
    // The background pages exchanged: refused.
    let mut backgrounds = open(&d);
    let last = backgrounds.doc.pages.remove(3).unwrap();
    backgrounds.doc.pages.insert(2, last).unwrap();
    assert!(refused(&backgrounds).contains("another order than the file has them"), "background pages in another order");
}

#[test]
fn a_drawing_read_after_a_page_was_taken_away_is_edited_by_the_ids_the_document_file_gave_its_pages() {
    // The pages that are left are not numbered from 0: the document file says which `ID` each has, and the index the read gives is by those `ID`s, so a rename of one page is
    // the rename of that page and no page is added or taken away.
    let mut o = open(&pages_of(3));
    o.doc.pages.remove(0);
    let first = save(&o);
    let mut again = reopen(&first);
    assert_eq!(ids(&again.doc), [1, 2]);
    assert_eq!(
        (again.index.page(0).unwrap().page_id(), again.index.page(1).unwrap().page_id()),
        (Some(1), Some(2)),
        "the index is by the IDs the model has"
    );
    page_mut(&mut again.doc, 1).name = Arc::from("Renamed");
    let second = save(&again);
    let (before, after) = (part(&again.bytes, "content.xml"), part(&second.bytes, "content.xml"));
    assert_eq!(after, before.replace("draw:name=\"page3\"", "draw:name=\"Renamed\""), "the name of that page is the only change");
    let back = reopen(&second);
    assert_eq!((names(&back.doc), ids(&back.doc)), (vec!["page2".to_owned(), "Renamed".to_owned()], vec![1, 2]));
}

/// A package of one page called `Page-1` on a drawing-page style, as a new drawing's package holds it: the file a drawing of another format is saved into.
fn blank_package() -> Drawing {
    let mut d = Drawing::a4();
    d.auto = r#"<style:style style:name="dp1" style:family="drawing-page"/>"#.to_owned();
    d.pages = r#"<draw:page draw:name="Page-1" draw:style-name="dp1" draw:master-page-name="Default"></draw:page>"#.to_owned();
    d
}

#[test]
fn a_drawing_of_several_pages_under_ids_and_names_of_its_own_is_saved_onto_a_package_of_one_page() {
    // The shape of a drawing read from a VSDX file: its pages' `ID`s are not the places the package numbers its pages by, and none is called `Page-1`. Nothing of the
    // package's page is the model's page, so the package's is taken away and the model's pages are written, on the master page of the package's, each with its shapes.
    let mut o = open(&blank_package());
    assert_eq!(names(&o.doc), ["Page-1"]);
    let mut pages: Vec<Arc<Page>> = Vec::new();
    for (id, name) in [(1, "Intro"), (2, "Page-2"), (7, "Notes & <drafts>")] {
        pages.push(Arc::new(new_page(&o.doc, id, name, vec![rectangle(1, &format!("Box{id}"), (3.0, 3.0, 4.0, 2.0))])));
    }
    o.doc.pages = diacraft_doc::SharedVec::from_vec(pages);
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(!after.contains("draw:name=\"Page-1\""), "the package's page is gone: {after}");
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["Intro", "Page-2", "Notes & <drafts>"]);
    assert_eq!(ids(&back.doc), [1, 2, 7], "under the IDs the drawing gave them");
    for page in back.doc.pages.iter() {
        assert_eq!(page.shapes.len(), 1);
    }
    assert_eq!(part(&o.bytes, "styles.xml"), part(&saved.bytes, "styles.xml"), "on the master page the package has: the size is the package's");
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn the_pages_page1_and_page2_of_a_drawing_made_elsewhere_are_saved_onto_a_package_whose_page_is_called_page_1() {
    // A drawing whose pages are `page1` and `page2` (as LibreOffice names them), saved into a new drawing's package: the first page takes the package's page by its `ID` and
    // is renamed, the second is added.
    let mut o = open(&blank_package());
    page_mut(&mut o.doc, 0).name = Arc::from("page1");
    let page = new_page(&o.doc, 1, "page2", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains("<draw:page draw:name=\"page1\" draw:style-name=\"dp1\" draw:master-page-name=\"Default\">"),
        "the package's page is renamed and keeps the rest: {after}"
    );
    assert_eq!(names(&reopen(&saved).doc), ["page1", "page2"]);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_beyond_the_packages_that_carries_the_first_page_s_markup_is_refused_and_one_that_carries_none_is_written() {
    // What the save of a drawing of another format gives its pages beyond the package's own today: the first page's attributes (the markup of a page element). A page
    // element is written with the master page this save chooses and nothing else, so a page that holds markup is refused, in words that say why; the way across is to
    // give such a page none.
    let mut o = open(&blank_package());
    let first = o.doc.pages.get(0).unwrap().clone();
    let mut page = new_page(&o.doc, 1, "Page-2", Vec::new());
    page.ext = first.ext.clone();
    assert!(!page.ext.is_empty(), "the package's page holds the markup of its element");
    o.doc.pages.push(Arc::new(page));
    let words = refused(&o);
    assert!(words.contains("new page") && words.contains("markup"), "{words}");
    // The same page with none.
    let mut o = open(&blank_package());
    let page = new_page(&o.doc, 1, "Page-2", Vec::new());
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    assert_eq!(names(&reopen(&saved).doc), ["Page-1", "Page-2"]);
}

#[test]
fn pages_added_with_shapes_of_their_own_number_are_each_read_back_at_their_own_place() {
    // Three pages added in one save, holding one, three and none of the shapes, each with a user cell on its first shape (tier 2): the bytes made are checked against the
    // place each page has in the part, so a page that is taken for another fails the check of the shapes written on it.
    let mut o = open(&pages_of(2));
    let with_cell = |id: u32, name: &str| {
        let mut shape = rectangle(id, name, (3.0, 3.0, 4.0, 2.0));
        Arc::make_mut(&mut shape.sheet)
            .cells
            .insert(diacraft_doc::CellName::new("Comment"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text(name)));
        shape
    };
    let one = new_page(&o.doc, 2, "One", vec![with_cell(1, "a")]);
    let three =
        new_page(&o.doc, 3, "Three", vec![with_cell(1, "b1"), rectangle(2, "b2", (9.0, 3.0, 2.0, 2.0)), rectangle(3, "b3", (9.0, 9.0, 2.0, 2.0))]);
    let none = new_page(&o.doc, 4, "None", Vec::new());
    for page in [one, three, none] {
        o.doc.pages.push(Arc::new(page));
    }
    let saved = save(&o);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page2", "One", "Three", "None"]);
    let counts: Vec<usize> = back.doc.pages.iter().map(|p| p.shapes.len()).collect();
    assert_eq!(counts, [2, 2, 1, 3, 0]);
    let comment = |page: usize, shape: usize| {
        back.doc.pages.get(page).unwrap().shapes.get(shape).unwrap().sheet.cell("Comment").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    };
    assert_eq!((comment(2, 0).as_deref(), comment(3, 0).as_deref()), (Some("a"), Some("b1")));
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_added_in_the_size_of_a_page_that_shared_its_master_page_and_was_resized_names_the_copy_made_for_that_page() {
    // `page1` and `page2` share `Default`; `page1` is resized and so names a copy (`Default_2`). A page added in `page1`'s new size names that copy, the one master page
    // of that size, and no second copy is made.
    let mut o = open(&pages_of(2));
    set_page_cell(&mut o.doc, 0, "PageWidth", crate::drawing::cm(29.7));
    set_page_cell(&mut o.doc, 0, "PageHeight", crate::drawing::cm(42.0));
    let mut page = new_page(&o.doc, 2, "Same as the first", Vec::new());
    page.sheet = Arc::clone(&o.doc.pages.get(0).unwrap().sheet);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    let styles = part(&saved.bytes, "styles.xml");
    assert_eq!(styles.matches("<style:master-page ").count(), 2, "one copy: {styles}");
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("<draw:page draw:name=\"Same as the first\" draw:master-page-name=\"Default_2\"/>"), "{content}");
    assert!(page_element(&content, "page2").contains("draw:master-page-name=\"Default\""), "the page that was not resized keeps its master page");
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_added_in_the_size_a_layout_had_before_a_page_on_another_master_page_edited_it_names_a_copy_of_it() {
    // Two master pages, `Default` and `Other`, name one layout. `page1` (on `Default`) is resized: the layout is shared, so it names a copy and `Default` is left with no
    // page. `page2` (on `Other`) is resized in the same save, and the layout is then its alone, edited where it is. A page added after `page1` in the size the layout had
    // does not name `Default`, whose layout is no longer that size: it names a copy made from the layout as the file has it.
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/><style:master-page style:name="Other" style:page-layout-name="PM1"/>"#,
    );
    d.pages = format!(
        r#"<draw:page draw:name="page1" draw:master-page-name="Default">{}</draw:page><draw:page draw:name="page2" draw:master-page-name="Other">{}</draw:page>"#,
        custom("a", "1cm"),
        custom("b", "1cm")
    );
    let mut o = open(&d);
    set_page_cell(&mut o.doc, 0, "PageWidth", crate::drawing::cm(29.7));
    set_page_cell(&mut o.doc, 0, "PageHeight", crate::drawing::cm(42.0));
    set_page_cell(&mut o.doc, 1, "PageWidth", crate::drawing::cm(14.8));
    set_page_cell(&mut o.doc, 1, "PageHeight", crate::drawing::cm(21.0));
    let page = new_page(&o.doc, 2, "Back to A4", Vec::new());
    let mut page = page;
    Arc::make_mut(&mut page.sheet).cells = o.doc.pages.get(0).unwrap().sheet.cells.clone();
    // The first page's cells are the A3 it was resized to: the added page is A4, the size the layout had.
    let a4 = open(&d);
    page.sheet = Arc::clone(&a4.doc.pages.get(0).unwrap().sheet);
    o.doc.pages.insert(1, Arc::new(page)).unwrap();
    let saved = save(&o);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "Back to A4", "page2"]);
    assert!((number_of(&back.doc, 0, "PageHeight") - crate::drawing::cm(42.0)).abs() < 1e-9);
    assert!((number_of(&back.doc, 1, "PageHeight") - crate::drawing::cm(29.7)).abs() < 1e-9, "the page added is the size the model gave it");
    assert!((number_of(&back.doc, 2, "PageHeight") - crate::drawing::cm(21.0)).abs() < 1e-9);
    assert_second_save_is_no_edit(&o, &saved);
}

#[test]
fn a_page_added_is_laid_out_as_the_first_page_is_and_not_after_more_white_space_than_a_layout_has() {
    // The white space before the first page (a line feed and an indent) goes before a page added; a run of more than 64 bytes is no layout, and a page added is put after
    // the page before it as it stands.
    let page = |name: &str| format!(r#"<draw:page draw:name="{name}" draw:master-page-name="Default"></draw:page>"#);
    let mut d = Drawing::a4();
    d.pages = format!("\n  {}\n  {}\n", page("a"), page("b"));
    let mut o = open(&d);
    let new = new_page(&o.doc, 2, "New", Vec::new());
    o.doc.pages.push(Arc::new(new));
    let after = part(&save(&o).bytes, "content.xml");
    assert!(after.contains("</draw:page>\n  <draw:page draw:name=\"New\" draw:master-page-name=\"Default\"/>"), "{after}");
    let mut d = Drawing::a4();
    d.pages = format!("\n{}{}{}\n", " ".repeat(100), page("a"), page("b"));
    let mut o = open(&d);
    let new = new_page(&o.doc, 2, "New", Vec::new());
    o.doc.pages.push(Arc::new(new));
    let after = part(&save(&o).bytes, "content.xml");
    assert!(after.contains("</draw:page><draw:page draw:name=\"New\" draw:master-page-name=\"Default\"/>"), "{after}");
}

#[test]
fn a_page_added_is_written_under_the_prefix_the_drawing_element_binds_and_not_the_one_a_part_further_up_lacks() {
    // `xmlns:draw` is declared on `office:drawing` and not on the root: a page put in `office:drawing` is under `draw`, and no prefix of its own is declared for it.
    let d = Drawing::a4().page(&custom("a", "1cm"));
    let content = d.content_xml().replacen(r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#, "", 1).replacen(
        "<office:drawing>",
        r#"<office:drawing xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0">"#,
        1,
    );
    assert!(content.contains("<office:drawing xmlns:draw="), "{content}");
    let mut files = d.files();
    for (name, bytes) in files.iter_mut() {
        if name == "content.xml" {
            *bytes = content.clone().into_bytes();
        }
    }
    let mut o = open_bytes(crate::build::package(crate::build::DRAWING, files));
    assert_eq!(names(&o.doc), ["page1"]);
    let new = new_page(&o.doc, 1, "Added", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(new));
    let saved = save(&o);
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains("<draw:page draw:name=\"Added\" draw:master-page-name=\"Default\">"), "{after}");
    // (A style the shape needs goes in `office:automatic-styles`, outside the element that binds `draw`, under a prefix of its own: that is not the page.)
    let page = page_element(&after, "Added");
    assert!(!page.contains("xmlns:") && !page.contains("draw1:"), "no prefix of its own is declared for the page or what is in it: {page}");
    assert_eq!(names(&reopen(&saved).doc), ["page1", "Added"]);
}

#[test]
fn a_page_added_goes_in_the_first_drawing_element_under_what_that_element_binds() {
    // A part with a second `office:drawing` (the reader reads the pages of both) whose start tag rebinds `draw`: a page put in the first is under the prefix the first sees.
    let mut d = Drawing::a4().page(&custom("a", "1cm"));
    d.pages.push_str(r#"</office:drawing><office:drawing xmlns:draw="urn:another"><draw:page draw:name="foreign"/>"#);
    let mut o = open(&d);
    assert_eq!(names(&o.doc), ["page1"], "the second element's page is not a page of this drawing's namespace");
    let new = new_page(&o.doc, 1, "Added", Vec::new());
    o.doc.pages.push(Arc::new(new));
    let after = part(&save(&o).bytes, "content.xml");
    assert!(after.contains("</draw:page><draw:page draw:name=\"Added\" draw:master-page-name=\"Default\"/><"), "{after}");
}

#[test]
fn a_page_added_with_no_shape_is_an_empty_element_that_the_next_save_opens_to_put_a_shape_in() {
    let mut o = open(&pages_of(1));
    let page = new_page(&o.doc, 1, "Blank", Vec::new());
    o.doc.pages.push(Arc::new(page));
    let first = save(&o);
    assert!(part(&first.bytes, "content.xml").contains("<draw:page draw:name=\"Blank\" draw:master-page-name=\"Default\"/>"));
    // The next save puts a shape in it, and moves it before the first page in the same save.
    Arc::make_mut(o.doc.pages.get_mut(1).unwrap()).shapes.push(Arc::new(rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))));
    let last = o.doc.pages.remove(1).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    let second = real(&o.doc, &first.bytes, &first.index).unwrap_or_else(|e| panic!("refused: {} ({e})", e.class()));
    let after = part(&second.bytes, "content.xml");
    assert!(page_element(&after, "Blank").contains("draw:name=\"Box\""), "{after}");
    assert!(after.find("draw:name=\"Blank\"").unwrap() < after.find("draw:name=\"page1\"").unwrap(), "{after}");
    assert_eq!(names(&reopen(&second).doc), ["Blank", "page1"]);
    assert_second_save_is_no_edit(&o, &second);
}

#[test]
fn a_new_page_in_a_file_where_no_page_names_a_master_page_is_refused_in_words() {
    // A page that names no master page has none to take a size from, and a new page needs one to name: refused in words, and nothing is written.
    let mut d = Drawing::a4();
    d.pages = format!(r#"<draw:page draw:name="a">{}</draw:page>"#, custom("s", "1cm"));
    let mut o = open(&d);
    let page = new_page(&o.doc, 1, "B", Vec::new());
    o.doc.pages.push(Arc::new(page));
    let words = refused(&o);
    assert!(words.contains("master page"), "{words}");
}

// ------------------------------------------------------------------------------------------------------------------------------: the review's findings

/// A save of the model from the file and the index the save before it gave.
#[track_caller]
fn save_from(o: &Open, from: &Written) -> Written {
    real(&o.doc, &from.bytes, &from.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()))
}

/// A shape put on the page at `at`, under an `ID` the page does not hold.
fn put_shape(doc: &mut Document, at: usize) {
    let page = Arc::make_mut(doc.pages.get_mut(at).unwrap());
    let id = page.shapes.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    page.shapes.push(Arc::new(rectangle(id, "Put", (3.0, 3.0, 4.0, 2.0))));
}

#[test]
fn a_page_renamed_in_a_save_after_one_that_changed_it_is_renamed_back_by_the_third_and_the_second_saved_again_is_no_edit() {
    // The index of a save holds the page as the model had it when that save changed it, and the next save compared the name with that page's even
    // when a save since had renamed it, so the rename back was lost and the save said it had been written.
    let mut o = open(&pages_of(3));
    put_shape(&mut o.doc, 1);
    let first = save(&o);
    assert_eq!(names(&reopen(&first).doc), ["page1", "page2", "page3"]);
    page_mut(&mut o.doc, 1).name = Arc::from("R");
    let second = save_from(&o, &first);
    assert!(second.edited);
    assert_eq!(names(&reopen(&second).doc), ["page1", "R", "page3"]);
    assert_second_save_is_no_edit(&o, &second);
    // Back under the name the page had when the first save changed it.
    page_mut(&mut o.doc, 1).name = Arc::from("page2");
    let third = save_from(&o, &second);
    assert!(third.edited, "the rename back is a change of the file");
    assert_eq!(names(&reopen(&third).doc), ["page1", "page2", "page3"], "and the file says so");
    assert_eq!(part(&third.bytes, "content.xml"), part(&first.bytes, "content.xml"), "the content is what the first save made");
    assert_second_save_is_no_edit(&o, &third);
    // And to a name that no save of the page had.
    page_mut(&mut o.doc, 1).name = Arc::from("S");
    let fourth = save_from(&o, &third);
    assert_eq!(names(&reopen(&fourth).doc), ["page1", "S", "page3"]);
    assert_second_save_is_no_edit(&o, &fourth);
}

#[test]
fn a_page_added_in_one_save_and_renamed_in_the_next_is_renamed_back_by_the_third() {
    let mut o = open(&pages_of(2));
    let page = new_page(&o.doc, 2, "New", vec![rectangle(1, "Box", (3.0, 3.0, 4.0, 2.0))]);
    o.doc.pages.push(Arc::new(page));
    let first = save(&o);
    page_mut(&mut o.doc, 2).name = Arc::from("R");
    let second = save_from(&o, &first);
    assert!(second.edited);
    assert_eq!(names(&reopen(&second).doc), ["page1", "page2", "R"]);
    assert_second_save_is_no_edit(&o, &second);
    page_mut(&mut o.doc, 2).name = Arc::from("New");
    let third = save_from(&o, &second);
    assert!(third.edited, "the rename back is a change of the file");
    assert_eq!(names(&reopen(&third).doc), ["page1", "page2", "New"]);
    assert_second_save_is_no_edit(&o, &third);
}

#[test]
fn a_page_moved_and_renamed_in_saves_one_after_the_other_is_put_back_by_the_next() {
    // A rename alone and a move alone (the entry of the page the save did not change), and the page then put back as it was.
    let mut o = open(&pages_of(3));
    put_shape(&mut o.doc, 2);
    let first = save(&o);
    page_mut(&mut o.doc, 2).name = Arc::from("Z");
    let second = save_from(&o, &first);
    let last = o.doc.pages.remove(2).unwrap();
    o.doc.pages.insert(0, last).unwrap();
    let third = save_from(&o, &second);
    assert_eq!(names(&reopen(&third).doc), ["Z", "page1", "page2"]);
    page_mut(&mut o.doc, 0).name = Arc::from("page3");
    let last = o.doc.pages.remove(0).unwrap();
    o.doc.pages.push(last);
    let fourth = save_from(&o, &third);
    assert_eq!(names(&reopen(&fourth).doc), ["page1", "page2", "page3"]);
    assert_eq!(part(&fourth.bytes, "content.xml"), part(&first.bytes, "content.xml"));
    assert_second_save_is_no_edit(&o, &fourth);
}

#[test]
fn a_page_whose_id_is_the_largest_number_is_saved_as_it_is_and_moved_and_renamed_like_any_other() {
    // `u32::MAX` stood for "no ID" in the index and is an ID the document file may give a page, and a file whose first page had it could not be
    // saved again (an unedited save, a move and a rename each failed with a mismatch).
    let mut o = open(&pages_of(2));
    // The first page as a page of the largest number (a page the file lacks: written new, and the file's taken away).
    let mut first_page = new_page(&o.doc, u32::MAX, "page1", Vec::new());
    first_page.shapes = o.doc.pages.get(0).unwrap().shapes.clone();
    o.doc.pages.remove(0);
    o.doc.pages.insert(0, Arc::new(first_page)).unwrap();
    let first = save(&o);
    let mut again = reopen(&first);
    assert_eq!(ids(&again.doc), [u32::MAX, 1], "the file says the page is the largest number");
    assert_eq!(again.index.page(0).unwrap().page_id(), Some(u32::MAX), "and the index knows the page by it");
    // Unedited: no edit, the same bytes.
    let same = real(&again.doc, &again.bytes, &again.index).unwrap_or_else(|e| panic!("refused: {} ({e})", e.class()));
    assert!(!same.edited && same.bytes == again.bytes);
    // Renamed, then moved, then both: each is written, and the second save of each is no edit.
    page_mut(&mut again.doc, 0).name = Arc::from("Last number");
    let renamed = save(&again);
    assert!(renamed.edited);
    assert_eq!(names(&reopen(&renamed).doc), ["Last number", "page2"]);
    assert_second_save_is_no_edit(&again, &renamed);
    let page = again.doc.pages.remove(0).unwrap();
    again.doc.pages.push(page);
    let moved = save_from(&again, &renamed);
    let back = reopen(&moved);
    assert_eq!((names(&back.doc), ids(&back.doc)), (vec!["page2".to_owned(), "Last number".to_owned()], vec![1, u32::MAX]));
    assert_second_save_is_no_edit(&again, &moved);
    // A page taken away and another added beside it.
    let page = new_page(&again.doc, 7, "Seven", Vec::new());
    again.doc.pages.push(Arc::new(page));
    again.doc.pages.remove(0);
    let last = save_from(&again, &moved);
    assert_eq!(names(&reopen(&last).doc), ["Last number", "Seven"]);
    assert_second_save_is_no_edit(&again, &last);
}

/// A drawing with a master page that holds no shape (`Default`, which `page1` names) and one that does (`Bg`, which `page2` names): the model has `page1`, `page2` and the
/// background page `Bg`, in that order.
fn drawing_with_a_background_beside_an_empty_master_page() -> Drawing {
    let mut d = Drawing::a4();
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(
            r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/><style:master-page style:name="Bg" style:page-layout-name="PM1">{}</style:master-page>"#,
            custom("bg", "1cm")
        ),
    );
    d.pages = format!(
        r#"<draw:page draw:name="page1" draw:master-page-name="Default">{}</draw:page><draw:page draw:name="page2" draw:master-page-name="Bg">{}</draw:page>"#,
        custom("a", "1cm"),
        custom("b", "1cm")
    );
    d
}

#[test]
fn a_page_added_after_a_background_page_is_refused_in_the_words_of_the_order_a_file_reads_its_pages_in() {
    // This file has a master page without shapes to take a new page's size from, so the refusal the other test sees (no such master page) is not what
    // refuses here: a page put after the background page would be written before it, and the next save would find the pages in another order than the model's.
    let mut o = open(&drawing_with_a_background_beside_an_empty_master_page());
    assert_eq!(names(&o.doc), ["page1", "page2", "Bg"]);
    assert!(o.doc.pages.get(2).unwrap().background && !o.doc.pages.get(1).unwrap().background);
    let page = new_page(&o.doc, 3, "Page-4", Vec::new());
    o.doc.pages.push(Arc::new(page));
    let words = refused(&o);
    assert!(words.contains("a file reads its pages first"), "the words of the order: {words}");
    // The same page put before the background page is written, on the master page that holds no shape, and read back where it was put.
    let mut before = open(&drawing_with_a_background_beside_an_empty_master_page());
    let page = new_page(&before.doc, 3, "Page-4", Vec::new());
    before.doc.pages.insert(2, Arc::new(page)).unwrap();
    let saved = save(&before);
    let back = reopen(&saved);
    assert_eq!(names(&back.doc), ["page1", "page2", "Page-4", "Bg"]);
    assert!(part(&saved.bytes, "content.xml").contains("<draw:page draw:name=\"Page-4\" draw:master-page-name=\"Default\"/>"));
    assert_second_save_is_no_edit(&before, &saved);
}

/// A drawing whose first page has a layer set of its own (two layers, `Notes` and `Guides`) and the second the one of the document (`Shared`): the first page's `Layer` rows are
/// not the rows a page with no layer set of its own reads.
fn drawing_with_a_layer_set_on_a_page(document_set: bool, second_own: bool) -> Drawing {
    let mut d = Drawing::a4();
    if document_set {
        d.masters = d.masters.replace("</draw:layer-set>", r#"<draw:layer draw:name="Shared"/></draw:layer-set>"#);
    }
    let second = if second_own { r#"<draw:layer-set><draw:layer draw:name="Mine"/></draw:layer-set>"# } else { "" };
    d.pages = format!(
        r#"<draw:page draw:name="page1" draw:master-page-name="Default"><draw:layer-set><draw:layer draw:name="Notes"/><draw:layer draw:name="Guides"/></draw:layer-set>{}</draw:page><draw:page draw:name="page2" draw:master-page-name="Default">{second}{}</draw:page>"#,
        custom("a", "1cm"),
        custom("b", "1cm")
    );
    d
}

/// The names of the rows of a page's `Layer` section.
fn layer_names(doc: &Document, at: usize) -> Vec<String> {
    let Some(section) = doc.pages.get(at).and_then(|p| p.sheet.section(&diacraft_doc::SectionKey::new("Layer", None)).cloned()) else {
        return Vec::new();
    };
    section.rows.iter().filter_map(|row| row.cells.get("Name").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())).collect()
}

#[test]
fn a_page_added_with_the_layer_rows_of_a_page_that_has_a_layer_set_of_its_own_keeps_them() {
    // The element a new page is written as has no layer set of its own, so it reads the document's; the document file said the new page read as the
    // first page does, and the rows went silently.
    for (document_set, second_own) in [(false, false), (true, false), (true, true), (false, true)] {
        let mut o = open(&drawing_with_a_layer_set_on_a_page(document_set, second_own));
        assert_eq!(layer_names(&o.doc, 0), ["Notes", "Guides"]);
        let second: Vec<String> = match (second_own, document_set) {
            (true, _) => vec!["Mine".to_owned()],
            (false, true) => vec!["Shared".to_owned()],
            (false, false) => Vec::new(),
        };
        assert_eq!(layer_names(&o.doc, 1), second, "a page with no set reads the document's, one with its own reads that");
        // The page the engine's `page.add` with `like` makes: the first page's cells, sections and all.
        let page = new_page(&o.doc, 2, "Copy", Vec::new());
        o.doc.pages.push(Arc::new(page));
        assert_eq!(layer_names(&o.doc, 2), ["Notes", "Guides"]);
        let saved = save(&o);
        let back = reopen(&saved);
        assert_eq!(names(&back.doc), ["page1", "page2", "Copy"]);
        assert_eq!(
            layer_names(&back.doc, 2),
            ["Notes", "Guides"],
            "the new page keeps its rows (document set: {document_set}, every page with its own: {second_own})"
        );
        assert_eq!(layer_names(&back.doc, 1), layer_names(&o.doc, 1), "and the page after the first is as it was");
        assert_eq!(back.doc.pages.get(2).unwrap().sheet, o.doc.pages.get(2).unwrap().sheet, "the new page's cells are the model's");
        assert_second_save_is_no_edit(&o, &saved);
    }
    // A page with no layer rows at all, added beside a document that has a layer set: it reads none (the document file says so).
    let mut o = open(&drawing_with_a_layer_set_on_a_page(true, false));
    let mut page = new_page(&o.doc, 2, "Bare", Vec::new());
    Arc::make_mut(&mut page.sheet).sections.retain(|s| s.name != diacraft_doc::SectionName::new("Layer"));
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o);
    assert_eq!(layer_names(&reopen(&saved).doc, 2), Vec::<String>::new());
    assert_second_save_is_no_edit(&o, &saved);
}
