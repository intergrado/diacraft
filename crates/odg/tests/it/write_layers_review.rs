//! The fixes of the full review. Each case runs with the real writer, and the echo control at the end fails every one.
//!
//! * A new shape's layer is predicted from the layer set of the **markup** (what the reader resolves `draw:layer` against, before DiaCraft's document file puts
//!   its own rows over it), and a name is written only when that row of the markup has the name the model's row has.
//! * A save never writes a file DiaCraft then refuses to open (the styles a retext copies are charged to what the reader may read).
//! * A group's pin place edited as a constant is carried.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, SectionKey, TextPart, Value};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, read, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, Saver, echo, echo_says_edited, open, open_bytes, part, real};
use crate::write_kept_dropped::{T1, cell_number, edit_cell, layer_attr, layered_drawing, row_of, shape_in, style_of};
use crate::write_new::{push_shape, rectangle};
use crate::write_page_ops::{new_page, page_mut};
use crate::write_runs::{chars, set_parts, set_row};

/// The text a cell holds.
fn text_of(cell: Option<&Cell>) -> Option<String> {
    cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The `LayerMember` of the shape named `name` on page `page`.
fn layer_member(doc: &Document, page: usize, name: &str) -> Option<String> {
    let shape = doc.pages.get(page)?.shapes.iter().find(|s| s.name.as_deref() == Some(name))?;
    text_of(shape.sheet.cell("LayerMember"))
}

/// The names of the rows of the `Layer` section of page `page`.
fn layer_names(doc: &Document, page: usize) -> Vec<String> {
    let Some(section) = doc.pages.get(page).and_then(|p| p.sheet.section(&SectionKey::new("Layer", None)).cloned()) else { return Vec::new() };
    section.rows.iter().map(|r| text_of(r.cells.get("Name")).unwrap_or_default()).collect()
}

/// A `Layer` section of the rows `names`, as the reader makes the rows.
fn layer_section(names: &[&str]) -> Arc<Section> {
    let mut section = Section::new("Layer", None);
    for (ix, name) in names.iter().enumerate() {
        let mut row = Row { ix: Some(u32::try_from(ix).unwrap_or(u32::MAX)), ..Row::default() };
        for (cell, value) in [("Name", *name), ("NameUniv", *name)] {
            row.cells.insert(CellName::new(cell), Cell::constant(Value::from_text(value)));
        }
        for cell in ["Visible", "Print"] {
            row.cells.insert(CellName::new(cell), Cell::constant(Value::from_number(1.0).unwrap_or_else(|| Value::from_text("1"))));
        }
        row.cells.insert(CellName::new("Lock"), Cell::constant(Value::from_number(0.0).unwrap_or_else(|| Value::from_text("0"))));
        section.rows.push(Arc::new(row));
    }
    Arc::new(section)
}

/// Puts the rows `names` as the `Layer` section of page `page`.
fn set_layers(doc: &mut Document, page: usize, names: &[&str]) {
    let sheet = Arc::make_mut(&mut page_mut(doc, page).sheet);
    sheet.sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
    sheet.sections.push(layer_section(names));
}

/// A rectangle named `name` on the layer `member` (`LayerMember`) of its page.
fn rectangle_on(id: u32, name: &str, member: &str) -> diacraft_doc::Shape {
    let mut shape = rectangle(id, name, (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(member)));
    shape
}

/// The review's recipe: the layers are `Notes` and `Other`; row 0 is renamed `Comments` and saved (the document file carries it, `styles.xml` still says
/// `Notes`); a rectangle is added on row 0 and saved. The reader resolves `draw:layer` against the markup's set, so a name written from the model's rows (`Comments`)
/// names no layer of the markup: the shape lost its layer on reopening, with nothing in tier 2 and no read-back to say so. The layer is now written as the markup's row
/// has it only when its name is the model's, and `layout` with the cell in tier 2 otherwise: reopened, the shape is on row 0.
fn a_new_shape_on_a_layer_renamed_before_keeps_it_on_reopen(save: &Saver) {
    let mut o = open(&layered_drawing());
    set_layers(&mut o.doc, 0, &["Comments", "Other"]);
    let first = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the rename: {e}"));
    assert!(first.edited, "the rename is an edit");
    assert!(part(&first.bytes, "styles.xml").contains(r#"draw:name="Notes""#), "the markup still says Notes");
    let mut reopened = open_bytes(first.bytes.clone());
    assert_eq!(layer_names(&reopened.doc, 0), ["Comments", "Other"], "the rows the document file gives the page");
    // The rectangle on row 0.
    let id = reopened.index.next_id(0).unwrap_or(2);
    push_shape(&mut reopened.doc, rectangle_on(id, "R", "0"));
    let second = save(&reopened.doc, &reopened.bytes, &reopened.index).unwrap_or_else(|e| panic!("the rectangle: {e}"));
    let content = part(&second.bytes, "content.xml");
    assert_eq!(layer_attr(&content, "R").as_deref(), Some("layout"), "a name the markup's row 0 does not have is not written: {content}");
    let back = open_bytes(second.bytes.clone());
    assert_eq!(layer_member(&back.doc, 0, "R").as_deref(), Some("0"), "the shape is on row 0 when the file is opened again");
    assert_eq!(layer_names(&back.doc, 0), ["Comments", "Other"]);
    // Saved again, nothing is written.
    let again = save(&reopened.doc, &second.bytes, &second.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == second.bytes, "a second save is no edit");
    // A shape on row 1 (`Other`, which the markup says too) is written under its name and reads back as 1, with nothing carried for it.
    let mut other = open_bytes(first.bytes);
    let id = other.index.next_id(0).unwrap_or(2);
    push_shape(&mut other.doc, rectangle_on(id, "S", "1"));
    let third = save(&other.doc, &other.bytes, &other.index).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(layer_attr(&part(&third.bytes, "content.xml"), "S").as_deref(), Some("Other"));
    assert_eq!(layer_member(&open_bytes(third.bytes).doc, 0, "S").as_deref(), Some("1"));
}

/// The rename and the rectangle in **one** save. The model's rows are the model's page (`Comments`), the markup's rows are the file's (`Notes`): the names
/// differ, so `layout` is written and the cell is carried. (The baseline's page, which has the file's rows, names `Notes` for the same shape: the model is what the shape's
/// `LayerMember` refers to, not the baseline.)
fn a_rename_and_a_new_shape_in_one_save_write_layout_and_carry_the_layer(save: &Saver) {
    let mut o = open(&layered_drawing());
    set_layers(&mut o.doc, 0, &["Comments", "Other"]);
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "R", "0"));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(layer_attr(&content, "R").as_deref(), Some("layout"), "{content}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(layer_member(&back.doc, 0, "R").as_deref(), Some("0"));
    assert_eq!(layer_names(&back.doc, 0), ["Comments", "Other"]);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// The two new-page cases: a page the save adds has no layer set of its own, so the reader gives its shapes the **document's** set (`Notes`, `Other`), and the
/// page's own rows are the document file's. A page whose rows are `Fresh` loses a shape on its row 0 (the name `Fresh` names nothing of the document's set), and one
/// whose rows are `Other` turned row 0 into row 1 (`Other` is the document's row 1).
fn a_new_shape_on_a_new_page_with_rows_of_its_own_keeps_its_row(save: &Saver) {
    for (rows, member, written) in [
        (&["Fresh"][..], "0", "layout"),
        (&["Other"][..], "0", "layout"),
        (&["Other", "Fresh"][..], "1", "layout"),
        // The rows the document's set has: the name is written, and it reads back as the row it is.
        (&["Notes", "Other"][..], "1", "Other"),
        (&["Notes", "Other"][..], "0", "Notes"),
    ] {
        let mut o = open(&layered_drawing());
        let id = u32::try_from(o.doc.pages.len()).unwrap_or(1);
        let mut page = new_page(&o.doc, id, "Second", Vec::new());
        page.shapes.push(Arc::new(rectangle_on(1, "R", member)));
        Arc::make_mut(&mut page.sheet).sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
        Arc::make_mut(&mut page.sheet).sections.push(layer_section(rows));
        o.doc.pages.push(Arc::new(page));
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{rows:?}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        assert_eq!(layer_attr(&content, "R").as_deref(), Some(written), "{rows:?} {member}: {content}");
        let back = open_bytes(saved.bytes.clone());
        assert_eq!(layer_member(&back.doc, 1, "R").as_deref(), Some(member), "{rows:?}: the shape is on the row it was");
        assert_eq!(layer_names(&back.doc, 1), rows, "the page's rows");
        let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{rows:?}: {e}"));
        assert!(!again.edited && again.bytes == saved.bytes, "{rows:?}: a second save is no edit");
    }
}

/// A page of the file that has a layer set of its own: the reader resolves `draw:layer` against it and not the document's, so a new shape on its row 0 is written under
/// that set's name.
fn a_new_shape_on_a_page_with_a_layer_set_of_its_own_is_written_by_that_set(save: &Saver) {
    let mut d = layered_drawing();
    d.pages = d.pages.replacen(
        r#"master-page-name="Default">"#,
        r#"master-page-name="Default"><draw:layer-set><draw:layer draw:name="Mine"/><draw:layer draw:name="Theirs"/></draw:layer-set>"#,
        1,
    );
    let mut o = open(&d);
    assert_eq!(layer_names(&o.doc, 0), ["Mine", "Theirs"], "the page has its own layer set");
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "R", "1"));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(layer_attr(&content, "R").as_deref(), Some("Theirs"), "{content}");
    assert!(!content.contains("diacraft:version"), "nothing is carried: {content}");
    assert_eq!(layer_member(&open_bytes(saved.bytes).doc, 0, "R").as_deref(), Some("1"));
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A save never writes a file DiaCraft then refuses to open.

/// A drawing read with `limits`.
pub(crate) fn open_with(d: &Drawing, limits: &Limits) -> Open {
    let bytes = d.bytes();
    let (doc, report) = read(&bytes, limits).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()));
    Open { bytes, doc, index: report.index }
}

/// A file read with `limits`.
fn open_bytes_with(bytes: Vec<u8>, limits: &Limits) -> Open {
    let (doc, report) = read(&bytes, limits).unwrap_or_else(|e| panic!("the file did not read: {} ({e})", e.class()));
    Open { bytes, doc, index: report.index }
}

/// A save with `limits`, and what it cost.
pub(crate) fn save_with(o: &Open, limits: &Limits) -> (Result<diacraft_odg::Written, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let options = WriteOptions { limits: limits.clone(), ..WriteOptions::default() };
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    (saved, work)
}

/// A text style of `attrs` attributes no cell maps (a namespace of no meaning to the writer), `fo:font-weight="bold"` beside them.
fn heavy_style(attrs: usize) -> String {
    let attributes: String = (0..attrs).map(|k| format!(r#" xx:a{k}="{k:0>10}""#)).collect();
    format!(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"{attributes}/></style:style>"#)
}

/// A paragraph of `spans` one-character spans that all name `T1`, a style of `attrs` unmapped attributes, and the rows of the runs a retext gives each of them a size of its
/// own: the shape's text is then `spans` runs, each of a row of its own.
pub(crate) fn many_runs(spans: usize, attrs: usize, limits: &Limits) -> Open {
    many_runs_in(spans, attrs, limits, false)
}

/// [`many_runs`], in a flat drawing when `flat` says so.
fn many_runs_in(spans: usize, attrs: usize, limits: &Limits, flat: bool) -> Open {
    let body: String = (0..spans).map(|_| r#"<text:span text:style-name="T1">x</text:span>"#).collect();
    let mut d = Drawing::a4().auto(&heavy_style(attrs)).page(&shape_in(&format!("<text:p>{body}</text:p>")));
    d.ns.push_str(r#" xmlns:xx="urn:example:foreign""#);
    let mut o = if flat { open_bytes_with(crate::flat::Flat::of(&d).bytes(), limits) } else { open_with(&d, limits) };
    let mut parts = Vec::new();
    for k in 1..=spans {
        let ix = u32::try_from(k).unwrap_or(u32::MAX);
        set_row(&mut o, "Character", ix, &[("Style", 1.0), ("Size", (10.0 + k as f64 / 50.0) / 72.0)]);
        parts.push(TextPart::Cp(ix));
        parts.push(chars("x"));
    }
    set_parts(&mut o, parts);
    o
}

/// The review's recipe, scaled to a tree limit of 60,000 items: a style of 100 unmapped attributes named by many spans, and each run given a size of its own. Each
/// distinct row of a run is a copy of the whole style (the other spans keep the old one; what no cell maps stays), so the part grows by the style's items for each
/// run. A save that makes copies charges the items before it makes them against what the reader reads of one part, and refuses in words past it: **every file a save
/// gives reads again with the limits it was saved with**, and a retext that would make one that does not is refused with nothing written.
#[test]
fn a_retext_that_gives_many_runs_a_style_each_saves_a_file_that_reads_again_or_is_refused_in_words() {
    let limits = Limits { max_read_xml_nodes: 60_000, ..Limits::default() };
    for (spans, saves) in [(30usize, true), (400, true), (600, false), (1_000, false)] {
        let o = many_runs(spans, 100, &limits);
        assert_eq!(cell_number(&o.doc, "Character", 1, "Style"), Some(1.0), "{}", row_of(&o.doc, "Character", 1));
        let (saved, work) = save_with(&o, &limits);
        match saved {
            Ok(saved) => {
                assert!(saves, "{spans} spans: the save made a file with {} items charged and was expected to be refused", work.copied_items);
                let back =
                    read(&saved.bytes, &limits).unwrap_or_else(|e| panic!("{spans} spans: the file a save made does not read: {} ({e})", e.class()));
                assert!(back.0.pages.get(0).is_some_and(|p| !p.shapes.is_empty()), "{spans} spans: the shape is there");
                // The copies are the charge, and each holds the style's unmapped attributes as the file has them.
                assert!(work.copied_items >= spans as u64 * 100, "{spans} spans: {} items charged for the copies", work.copied_items);
                let content = part(&saved.bytes, "content.xml");
                assert_eq!(
                    content.matches(r#"xx:a99="0000000099""#).count(),
                    spans,
                    "{spans} spans: a copy for each run, and the style they were made from is gone"
                );
            }
            Err(error) => {
                assert!(!saves, "{spans} spans: the save was refused and was expected to be made: {error}");
                assert_eq!(error.class(), "write_limit", "{error}");
                assert!(error.to_string().contains("items in a part"), "in words: {error}");
            }
        }
    }
}

/// The review's recipe at its own size: a text style of 400 unmapped attributes, 10,000 one-character spans that name it, each run given a size of its own. The writer
/// before this round made a copy of the style for each run and saved a content part of 200 MB that the reader refuses (`xml_too_complex`); it is now refused in words,
/// when the copies it would make no longer fit the reader's limit on one part (about 200 MB are held while it makes them). Run by hand:
/// `cargo nextest run -p diacraft-odg --run-ignored only -E 'test(the_reviews_recipe)' --no-capture`.
#[test]
#[ignore = "the review's recipe at its own size (400 attributes, 10,000 spans): about 200 MB are held while it is refused; run by hand"]
fn the_reviews_recipe_at_its_own_size_is_refused_in_words_and_not_saved_as_a_file_that_cannot_be_opened() {
    let limits = Limits::default();
    let o = many_runs(10_000, 400, &limits);
    let (saved, work) = save_with(&o, &limits);
    let error = saved.err().unwrap_or_else(|| panic!("the save was made, with {} items charged for its copies", work.copied_items));
    assert_eq!(error.class(), "write_limit", "{error}");
    assert!(error.to_string().contains("items in a part"), "in words: {error}");
    eprintln!("REVIEW-RECIPE refused with {} items and {} bytes charged for copies: {error}", work.copied_items, work.style_copy_bytes);
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The other half: a style is edited where it stands only when nothing else wants what it said, and never so that another paragraph or run of the same
// save, which takes the style or its old row as the file has it, comes to read what it did not say.

/// Sets the text of the shape `id` of the first page.
pub(crate) fn set_text_of(o: &mut Open, id: u32, parts: Vec<TextPart>) {
    crate::write_core::shape_mut(&mut o.doc, 0, id).text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
}

/// Puts the row `ix` of `section` of the shape `id`, with `cells` (numbers), replacing the row of that `IX`.
pub(crate) fn set_row_of(o: &mut Open, id: u32, section: &str, ix: u32, cells: &[(&str, f64)]) {
    let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
    let at = match sheet.sections.iter().position(|s| s.name.as_str() == section) {
        Some(at) => at,
        None => {
            sheet.sections.push(Arc::new(Section::new(section, None)));
            sheet.sections.len() - 1
        }
    };
    let section = Arc::make_mut(&mut sheet.sections[at]);
    let mut row = Row { ix: Some(ix), ..Row::default() };
    for (name, v) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(*v).unwrap_or_else(|| Value::from_text("0"))));
    }
    match section.rows.iter().position(|r| r.ix == Some(ix)) {
        Some(k) => section.rows[k] = Arc::new(row),
        None => section.rows.push(Arc::new(row)),
    }
}

/// Puts the row `to` of `section` of the shape `id` as a copy of its row `from` with the numbers of `cells` set.
fn copy_row(o: &mut Open, id: u32, section: &str, (from, to): (u32, u32), cells: &[(&str, f64)]) {
    let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
    let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == section).unwrap_or_else(|| panic!("no section")));
    let mut row = section.rows.iter().find(|r| r.ix == Some(from)).map(|r| (**r).clone()).unwrap_or_else(|| panic!("no row {from}"));
    row.ix = Some(to);
    for (name, v) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(*v).unwrap_or_else(|| Value::from_text("0"))));
    }
    match section.rows.iter().position(|r| r.ix == Some(to)) {
        Some(k) => section.rows[k] = Arc::new(row),
        None => section.rows.push(Arc::new(row)),
    }
}

/// The `HorzAlign` each paragraph of the shape named `name` on the first page has, by the `pp` markers (a paragraph is opened by its marker).
fn aligns(doc: &Document, name: &str) -> Vec<Option<f64>> {
    let Some(shape) = doc.pages.get(0).and_then(|p| p.shapes.iter().find(|s| s.name.as_deref() == Some(name))) else { return Vec::new() };
    let mut row: Option<u32> = None;
    let (mut out, mut at_start) = (Vec::new(), true);
    for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match part {
            TextPart::Pp(ix) => row = Some(*ix),
            TextPart::Chars(c) => {
                for ch in c.chars() {
                    if at_start {
                        out.push(row.and_then(|ix| crate::write_runs::row_cell(shape, "Paragraph", ix, "HorzAlign")));
                    }
                    at_start = ch == '\n';
                }
            }
            _ => {}
        }
    }
    out
}

/// Three shapes `w`, `x` and `y`; `x`'s one paragraph names the style `P1`, which says `fo:text-align="center"` and nothing else, so that it is the style the file has
/// for exactly a centred paragraph.
fn three_shapes() -> Drawing {
    let style = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
    let body = format!(
        "{}{}{}",
        crate::write_runs::shape("w", "1cm", "<text:p>w</text:p>"),
        crate::write_runs::shape("x", "6cm", r#"<text:p text:style-name="P1">ab</text:p>"#),
        crate::write_runs::shape("y", "11cm", "<text:p>zz</text:p>")
    );
    Drawing::a4().auto(style).page(&body)
}

/// A style that a paragraph alone names is edited where it stands, and **a paragraph put in later that wants a style with the attributes it had** is not given it:
/// the index of the part's styles by their attributes leaves out a style the save edited, whether it was made before the edit (`w`'s new paragraph, which asks for a
/// justified one) or after it (`y`'s, which asks for a centred one: `P1` said that, and now says right).
fn a_paragraph_style_edited_where_it_stands_is_not_found_again_by_the_attributes_it_had(save: &Saver) {
    let mut o = open(&three_shapes());
    let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    assert_eq!(aligns(&o.doc, "x"), [Some(1.0)], "x is centred by P1");
    // `w`: a justified paragraph put in (the index of styles is made for it); `x`: its alignment to the right; `y`: a centred paragraph put in.
    set_row_of(&mut o, ids[0], "Paragraph", 1, &[("HorzAlign", 3.0)]);
    set_text_of(&mut o, ids[0], vec![chars("w\n"), TextPart::Pp(1), chars("new")]);
    set_row_of(&mut o, ids[1], "Paragraph", 0, &[("HorzAlign", 2.0)]);
    set_row_of(&mut o, ids[2], "Paragraph", 1, &[("HorzAlign", 1.0)]);
    set_text_of(&mut o, ids[2], vec![chars("zz\n"), TextPart::Pp(1), chars("ctr")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    // `P1` stands, edited: it says right, and nothing names it but `x`.
    assert!(style_of(&after, "P1").is_some_and(|s| s.contains(r#"fo:text-align="right""#) && !s.contains("center")), "{after}");
    assert_eq!(after.matches(r#"text:style-name="P1""#).count(), 1, "only x names P1: {after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(aligns(&back.doc, "x"), [Some(2.0)]);
    assert_eq!(aligns(&back.doc, "y"), [None, Some(1.0)], "y's new paragraph is centred, as the model has it");
    assert_eq!(aligns(&back.doc, "w"), [None, Some(3.0)]);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// A paragraph put in beside a paragraph takes that paragraph's style as the file has it (the one its row matches), so the neighbour is not edited where its style
/// stands: it gets a copy, and the new paragraph names the style as it was.
fn the_style_of_a_paragraph_a_new_one_is_beside_is_copied_and_not_edited_where_it_stands(save: &Saver) {
    let mut o = open(&three_shapes());
    let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    // `x`: its paragraph to the right, and a centred one put in after it (which matches what the paragraph was).
    set_row_of(&mut o, ids[1], "Paragraph", 0, &[("HorzAlign", 2.0)]);
    set_row_of(&mut o, ids[1], "Paragraph", 1, &[("HorzAlign", 1.0)]);
    set_text_of(&mut o, ids[1], vec![TextPart::Pp(0), chars("ab\n"), TextPart::Pp(1), chars("cd")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "P1").is_some_and(|s| s.contains("center")), "P1 still says centre, for the paragraph put in: {after}");
    assert!(
        style_of(&after, "P2").is_some_and(|s| s.contains(r#"fo:text-align="right""#)),
        "and the edited paragraph has a copy that says right: {after}"
    );
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(aligns(&back.doc, "x"), [Some(2.0), Some(1.0)]);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// A run's style is edited where it stands only when one new run takes it and no other run of the paragraph needs what it said: a run split in two, the first half
/// given a new size and the second half left as it was, keeps the style for the second half and gives the first a copy; a run split in two with a new size for each
/// gives each a copy, and the style nothing names is taken out.
fn a_run_split_in_two_keeps_the_style_for_the_half_that_is_as_it_was(save: &Saver) {
    // The first half sized, the second as it was: `T1` is needed as it is.
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    let id = crate::write_runs::id_of(&o.doc);
    let size = |points: f64| [("Size", points / 72.0)];
    copy_row(&mut o, id, "Character", (1, 2), &size(20.0));
    set_text_of(&mut o, id, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(2), chars("c"), TextPart::Cp(1), chars("d")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "T1").is_some_and(|s| s.contains(r#"fo:font-size="12pt""#)), "T1 stands as it was, for `d`: {after}");
    assert!(after.contains(r#"<text:span text:style-name="T2">c</text:span><text:span text:style-name="T1">d</text:span>"#), "{after}");
    assert!(style_of(&after, "T2").is_some_and(|s| s.contains(r#"fo:font-size="20pt""#) && s.contains("wave")), "the copy: {after}");
    let back = open_bytes(saved.bytes.clone());
    // (The reader numbers the rows by first use: the sizes are told by value.)
    let sizes: Vec<i64> = (0..4).filter_map(|ix| cell_number(&back.doc, "Character", ix, "Size")).map(|v| (v * 72.0).round() as i64).collect();
    assert!(sizes.contains(&20) && sizes.contains(&12), "the runs read back at 20 pt and 12 pt: {sizes:?}");
    // A new size for each half: two copies, and `T1` goes.
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    let id = crate::write_runs::id_of(&o.doc);
    copy_row(&mut o, id, "Character", (1, 2), &size(20.0));
    copy_row(&mut o, id, "Character", (1, 3), &size(30.0));
    set_text_of(&mut o, id, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(2), chars("c"), TextPart::Cp(3), chars("d")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "T1").is_none(), "nothing names T1 now: {after}");
    assert!(after.contains(r#"<text:span text:style-name="T2">c</text:span><text:span text:style-name="T3">d</text:span>"#), "{after}");
    assert!(style_of(&after, "T2").is_some_and(|s| s.contains("20pt") && s.contains("wave")), "{after}");
    assert!(style_of(&after, "T3").is_some_and(|s| s.contains("30pt") && s.contains("wave")), "{after}");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// Item 7 of for groups.

/// A group of the file (`draw:g`, two shapes), whose frame cells are what the reader rebuilds from the box of its members: `LocPinX` and `LocPinY` the formulas
/// `Width*0.5` and `Height*0.5`. The place of its pin set as a number with the pin compensating (the frame as it was), and a formula set on a frame cell, are carried by
/// no entry of the document file (a group's entry never carries its frame), and the save said it had nothing to write and the pin was the box's centre on reopening:
/// they are refused in words. A cell of the group that is no frame cell is the document file's, and a group that is not edited is not.
fn a_groups_pin_changed_as_a_number_or_a_formula_is_refused_in_words(save: &Saver) {
    let body = format!(r#"<draw:g draw:name="G">{}{}</draw:g>"#, crate::write_core::custom("A", "1cm"), crate::write_core::custom("B", "6cm"));
    let o = open(&Drawing::a4().page(&body));
    let group = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).cloned().unwrap_or_else(|| panic!("no group"));
    assert_eq!(group.kind, diacraft_doc::ShapeType::Group);
    let number = |name: &str| group.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(f64::NAN);
    assert!(group.sheet.cell("LocPinX").is_some_and(|c| c.formula.text() == Some("Width*0.5")), "the reader's group has the formula");
    let edited = |change: &dyn Fn(&mut diacraft_doc::Sheet)| {
        let mut doc = o.doc.clone();
        change(Arc::make_mut(&mut crate::write_core::shape_mut(&mut doc, 0, group.id).sheet));
        save(&doc, &o.bytes, &o.index)
    };
    let constant = |v: f64| Cell::read(None, Some(&v.to_string()), Some("IN"), None, &mut 1_000);
    // The place of the pin as a number, the pin compensating.
    for (place, pin) in [("LocPinX", "PinX"), ("LocPinY", "PinY")] {
        let (place_now, pin_now) = (number(place), number(pin));
        let saved = edited(&|sheet| {
            sheet.cells.insert(CellName::new(place), constant(place_now - 0.25));
            sheet.cells.insert(CellName::new(pin), constant(pin_now - 0.25));
        });
        match saved {
            Err(WriteError::Unsupported(words)) => assert!(words.contains("group's pin"), "{place}: {words}"),
            other => panic!("{place}: expected a refusal in words, got {:?}", other.map(|w| w.edited)),
        }
    }
    // A formula set on a frame cell, the value the same.
    let saved = edited(&|sheet| {
        let now = number("PinX");
        sheet.cells.insert(CellName::new("PinX"), Cell::read(Some("Width*0.5+1 in"), Some(&now.to_string()), Some("IN"), None, &mut 1_000));
    });
    assert!(matches!(saved, Err(WriteError::Unsupported(words)) if words.contains("group's pin")), "a formula on a frame cell");
    // A cell that is no frame cell is the document file's: the group is saved, and no refusal says otherwise.
    let saved = edited(&|sheet| {
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("hello")));
    });
    assert!(saved.as_ref().is_ok_and(|w| w.edited), "a cell of no frame is carried: {:?}", saved.map(|w| w.edited));
    // Nothing changed: nothing is written.
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!saved.edited, "a group that is not edited is not written");
    // The refusal is not made for a value the formula recalculates: a pin place that stays the formula's, whatever its cached value.
    let saved = edited(&|sheet| {
        if let Some(cell) = sheet.cells.get_mut("LocPinX") {
            cell.value = Value::from_number(number("LocPinX") + 0.0001);
        }
    });
    assert!(saved.is_ok(), "a recalculated place is no edit of the pin: {:?}", saved.map(|w| w.edited));
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A new automatic style never has the name of a common style.

/// The copy of a style two spans share is named by R27 (the prefix the file's automatic styles use and one more than the largest number), **and never a name a common
/// style of `styles.xml` has**: a reader that looks a name up among the common styles too would find two. `styles.xml` holds a common text style `T2` here, so the copy is
/// `T3`.
fn a_copy_is_never_given_the_name_of_a_common_style(save: &Saver) {
    let two = r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#;
    let d = Drawing::a4().common(r#"<style:style style:name="T2" style:family="text"/>"#).auto(T1).page(&shape_in(two));
    let mut o = open(&d);
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "T3").is_some_and(|s| s.contains("14pt")), "the copy is T3: {after}");
    assert!(style_of(&after, "T2").is_none(), "no automatic style has the name of the common one: {after}");
    assert_eq!(after.matches(r#"text:style-name="T3""#).count(), 2, "{after}");
    assert!(part(&saved.bytes, "styles.xml").contains(r#"style:name="T2""#), "the common style is as it was");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The counts and the guards the full review's hand deletions found no test for. Each pins a statement of the writer by what the save counted or wrote, and
// each was shown to fail with that statement taken out (the report names the deletions).

/// A save, counted, with the limits given (`Limits::default()` for none).
fn counted(o: &Open, limits: &Limits) -> (Result<diacraft_odg::Written, WriteError>, WriteWork) {
    save_with(o, limits)
}

/// M01: the skew a route takes out is an attribute written like any other: a save that reroutes the connector of the file writes one attribute more than the same save
/// where the connector has no skew, and the cap on attributes written counts it.
#[test]
fn the_skew_a_route_takes_out_is_charged_as_an_attribute_written() {
    use crate::drawing::cm;
    use crate::write_core::{move_by, num_of, set_num, shape_mut};
    use crate::write_more::{connector_drawing, set_route_end};
    let rerouted = |d: &Drawing| {
        let mut o = open(d);
        move_by(&mut o.doc, 0, 1, 1.0, 0.0);
        let c = shape_mut(&mut o.doc, 0, 2);
        let x = num_of(c, "BeginX");
        set_num(c, "BeginX", x + cm(1.0));
        set_route_end(c, cm(3.0));
        let (saved, work) = counted(&o, &Limits::default());
        (saved.unwrap_or_else(|e| panic!("{e}")), work)
    };
    let (with, with_work) = rerouted(&crate::write_kept_dropped::skewed_drawing());
    let (without, without_work) = rerouted(&connector_drawing());
    assert!(!part(&with.bytes, "content.xml").contains("line-skew"), "the skew went with the route");
    assert!(!part(&without.bytes, "content.xml").contains("line-skew"));
    assert_eq!(with_work.attributes_written, without_work.attributes_written + 1, "the skew taken out is one attribute written more");
    // The cap counts it: one attribute fewer than the save writes is refused.
    let mut o = open(&crate::write_kept_dropped::skewed_drawing());
    move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    {
        let c = shape_mut(&mut o.doc, 0, 2);
        let x = num_of(c, "BeginX");
        set_num(c, "BeginX", x + cm(1.0));
        set_route_end(c, cm(3.0));
    }
    let at = with_work.attributes_written as usize;
    assert!(counted(&o, &Limits { max_write_attributes: at, ..Limits::default() }).0.is_ok(), "the save is made at the count it has");
    assert!(
        matches!(counted(&o, &Limits { max_write_attributes: at - 1, ..Limits::default() }).0, Err(WriteError::Limit { .. })),
        "and refused one below it"
    );
}

/// M02, M03: **every byte of a style copied, and of the keys kept for the copies, is charged** to the bytes a save may copy. The save of a style two spans share makes one
/// copy: the bytes charged for it are the bytes put in (the style as the file has it, with the size), and the keys of the styles made are charged too.
#[test]
fn a_save_that_copies_a_style_charges_the_bytes_of_the_copy_and_of_its_key() {
    let two = r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#;
    let mut o = open(&Drawing::a4().auto(T1).page(&shape_in(two)));
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let (saved, work) = counted(&o, &Limits::default());
    let saved = saved.unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    let copy = style_of(&after, "T2").unwrap_or_else(|| panic!("no copy: {after}"));
    assert!(
        work.style_copy_bytes >= copy.len() as u64 && work.style_copy_bytes <= copy.len() as u64 + 16,
        "{} for a copy of {}",
        work.style_copy_bytes,
        copy.len()
    );
    // The key holds the style's name, the cells of the two rows and the new name: more than the two names.
    assert!(work.style_key_bytes > 2 * "T1".len() as u64 + 20, "the key: {}", work.style_key_bytes);
    // Both are charged to the cap on the bytes copied: a cap of the bytes copied less the bytes charged for the copy and the key does not hold.
    assert!(
        work.bytes_copied >= work.style_copy_bytes + work.style_key_bytes,
        "{} and {} of {}",
        work.style_copy_bytes,
        work.style_key_bytes,
        work.bytes_copied
    );
}

/// `n` spans alternately of the styles `T1` (bold, 12 pt, with what no cell maps) and `T2` (italic), as one paragraph, and the size of the row of `T1` edited: the
/// paragraph is written whole, every run is walked, the runs of `T1` are given the style made from their own with the new size, and those of `T2` are kept.
fn alternating(n: usize) -> Open {
    let italic = r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>"#;
    let spans: String = (0..n).map(|k| format!(r#"<text:span text:style-name="T{}">x</text:span>"#, 1 + k % 2)).collect();
    let mut o = open(&Drawing::a4().auto(T1).auto(italic).page(&shape_in(&format!("<text:p>{spans}</text:p>"))));
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    o
}

/// What a save of [`alternating`] held of the memory a save may hold, beside what it counted in steps.
fn held_and_steps(n: usize) -> (u64, u64) {
    let o = alternating(n);
    let limits = Limits::default();
    let (saved, work) = counted(&o, &limits);
    assert!(saved.is_ok(), "{n}: {saved:?}");
    let room = limits.max_write_memory.saturating_sub(work.estimated_memory);
    (room.saturating_sub(work.memory_room), work.order_steps)
}

/// M04, M05, M13: a paragraph written whole holds what its runs need and counts the steps of walking them. The style each run's span names costs the memory a save may
/// hold (`RUN_STYLE_BYTES` and the name's bytes, for each run of the file's paragraph), the spans kept cost theirs, and the steps are the runs, the tokens and the
/// segments walked, and one more for each run whose row changed. Between 100 and 300 runs, each added run adds at least what it is charged: 34 bytes beside the 160 of a
/// span kept (every other run is), and 13 steps for the pair of runs (the one that changed and the one kept: 4 for the paragraph's runs and tokens, 2 for the segments,
/// and the steps of the plan and the spans kept that the file's paragraph already had).
#[test]
fn the_runs_of_a_paragraph_written_whole_are_charged_to_the_memory_a_save_holds_and_to_its_steps() {
    let ((held_100, steps_100), (held_300, steps_300)) = (held_and_steps(100), held_and_steps(300));
    let (held, steps) = (held_300 - held_100, steps_300 - steps_100);
    // 200 more runs, each a name (`RUN_STYLE_BYTES`, 32 bytes, and the name's two): no span is kept, for the first run of the paragraph is the one that changed.
    // Exactly that, and no more: what the check for a style to edit where it stands holds while it looks at the runs is given back (`in_place_styles`).
    assert_eq!(held, 200 * 34, "the memory the runs hold");
    assert!(steps >= STEPS_FLOOR, "the steps the runs take: {steps}");
}

/// The steps 200 more runs add, as the writer counts them (ten for each run: its own, its tokens, its segments, the plan's and the check's; and one more for each of
/// the runs whose row changed, every other one). It is a count of the writer's, and moves with it.
const STEPS_FLOOR: u64 = 2_100;

/// M08, M09, and the view box of the `draw:path` row: an edit of a path regenerates `svg:viewBox` **as the file's own box, re-spelled with its origin** (four numbers
/// and spaces, `10 20 1000 1000` for `10,20,1000,1000`), and not as `0 0 W H`; a box the new text could not say (a number of more places than the text writes) is left as
/// the file has it; and the polygon's row says the opposite: its edit keeps the view box, as it is spelled.
#[test]
fn a_paths_view_box_is_the_files_own_re_spelled_with_its_origin_and_a_polygons_is_kept() {
    let frame = crate::write_kept_dropped::FRAME;
    let path = |view_box: &str| {
        Drawing::a4().page(&format!(
            r#"<draw:path draw:name="p" {frame} svg:viewBox="{view_box}" svg:d="M 10 20 L 1010 20 L 1010 1020 L 10 1020 Z"><text:p/></draw:path>"#
        ))
    };
    let edited = |d: &Drawing| {
        let mut o = open(d);
        let id = crate::write_kept_dropped::id_of(&o.doc, "p");
        crate::write_kept_dropped::with_a_fifth_point(&mut o.doc, id);
        let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
        let content = part(&saved.bytes, "content.xml");
        assert!(read(&saved.bytes, &Limits::default()).is_ok());
        content
    };
    // The origin is kept, and the commas and the spelling are the new text's.
    let content = edited(&path("10,20,1000,1000"));
    assert!(content.contains(r#"svg:viewBox="10 20 1000 1000""#) && !content.contains(r#"svg:viewBox="0 0"#), "{content}");
    assert!(content.contains("svg:d=\"M 10 20 L 1010 20 L 1010 1020 L 510 1270 L 10 1020 Z\""), "{content}");
    // A box the text cannot say: the file's own spelling stays (and the points are written in the box it says).
    let content = edited(&path("0 0 1000.1234567 1000"));
    assert!(content.contains(r#"svg:viewBox="0 0 1000.1234567 1000""#), "{content}");
    // A polygon's edit keeps the view box, as the file spells it.
    let body =
        format!(r#"<draw:polygon draw:name="g" {frame} svg:viewBox="0,0,1000,1000" draw:points="0,0 1000,0 500,1000"><text:p/></draw:polygon>"#);
    let mut o = open(&Drawing::a4().page(&body));
    let id = crate::write_kept_dropped::id_of(&o.doc, "g");
    {
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, id).sheet);
        let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Geometry").unwrap_or_else(|| panic!("no geometry")));
        let row = Arc::make_mut(&mut section.rows[2]);
        for (cell, v) in [("X", 0.25), ("Y", 0.25)] {
            row.cells.insert(CellName::new(cell), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
        }
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains(r#"draw:points="0,0 1000,0 250,750""#), "the point moved: {content}");
    assert!(content.contains(r#"svg:viewBox="0,0,1000,1000""#), "the polygon's view box is as the file spells it: {content}");
}

/// M16: a style with no properties of the kind that an edit only takes attributes out of (a cell gone that the style's parent gives) makes no empty element and no copy:
/// a style a single span names stays as it is, and so does one two spans share.
fn an_edit_that_only_takes_attributes_out_of_a_style_with_no_properties_changes_no_style(save: &Saver) {
    let strong = r#"<style:style style:name="Strong" style:family="text"><style:text-properties fo:font-weight="bold" fo:text-transform="uppercase"/></style:style>"#;
    let empty = r#"<style:style style:name="T1" style:family="text" style:parent-style-name="Strong"/>"#;
    for (spans, shared) in [
        (r#"<text:p>ab<text:span text:style-name="T1">cd</text:span></text:p>"#, false),
        (r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#, true),
    ] {
        let d = Drawing::a4().common(strong).auto(empty).page(&shape_in(spans));
        let mut o = open(&d);
        assert_eq!(cell_number(&o.doc, "Character", 1, "Case"), Some(1.0), "uppercase through the parent: {}", row_of(&o.doc, "Character", 1));
        crate::write_kept_dropped::drop_cell(&mut o, "Character", 1, "Case");
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("shared {shared}: {e}"));
        assert!(
            saved.edited && saved.bytes != o.bytes,
            "shared {shared}: the save is an edit: the paragraph is written again, and the file's other parts say so"
        );
        let after = part(&saved.bytes, "content.xml");
        assert!(after.contains(r#"<style:style style:name="T1" style:family="text" style:parent-style-name="Strong"/>"#), "shared {shared}: {after}");
        assert!(style_of(&after, "T2").is_none() && !after.contains("text-properties"), "shared {shared}: no copy and no empty element: {after}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The tests the second round of hand deletions asked for: the style of a run beside a paragraph put in, the index built after an edit in place, what a clone of a shared
// graphic style adds, and the cost of the layers of a page.

/// A paragraph put in beside a paragraph takes the styles its spans name for the runs of the same row **as the file has them** (the neighbour's spans are the ones it
/// looks at): the neighbour's style is therefore not edited where it stands, though nothing else names it. `ab` and a bold-underlined `cd` of 12 pt; `cd` is given 20 pt
/// and a new paragraph `ef` is put in after it in the old row (12 pt): `cd` has a copy and `ef` has the style as it was.
fn the_style_of_a_run_a_new_paragraph_is_beside_is_copied_and_not_edited_where_it_stands(save: &Saver) {
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    let id = crate::write_runs::id_of(&o.doc);
    copy_row(&mut o, id, "Character", (1, 2), &[("Size", 20.0 / 72.0)]);
    set_text_of(&mut o, id, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(2), chars("cd\n"), TextPart::Cp(1), chars("ef")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "T1").is_some_and(|s| s.contains(r#"fo:font-size="12pt""#) && s.contains("wave")), "T1 is as it was, for `ef`: {after}");
    assert!(
        style_of(&after, "T2").is_some_and(|s| s.contains(r#"fo:font-size="20pt""#) && s.contains("wave")),
        "and `cd` has a copy with its size: {after}"
    );
    let back = open_bytes(saved.bytes.clone());
    let sizes: Vec<i64> = (0..4).filter_map(|ix| cell_number(&back.doc, "Character", ix, "Size")).map(|v| (v * 72.0).round() as i64).collect();
    assert!(sizes.contains(&20) && sizes.contains(&12), "the runs read back at 20 pt and 12 pt: {sizes:?}");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// The index of the part's styles by their attributes is made the first time a style is asked for, and it leaves out a style the save edited where it stands: here the
/// edit comes **first** (`x`, before `y`), so that the index is made after it, and `y`'s new centred paragraph is not given `P1`, which said centre and now says right.
fn the_index_made_after_an_edit_in_place_leaves_out_the_style_edited(save: &Saver) {
    let style = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
    let body = format!(
        "{}{}",
        crate::write_runs::shape("x", "6cm", r#"<text:p text:style-name="P1">ab</text:p>"#),
        crate::write_runs::shape("y", "11cm", "<text:p>zz</text:p>")
    );
    let mut o = open(&Drawing::a4().auto(style).page(&body));
    let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    set_row_of(&mut o, ids[0], "Paragraph", 0, &[("HorzAlign", 2.0)]);
    set_row_of(&mut o, ids[1], "Paragraph", 1, &[("HorzAlign", 1.0)]);
    set_text_of(&mut o, ids[1], vec![chars("zz\n"), TextPart::Pp(1), chars("ctr")]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "P1").is_some_and(|s| s.contains(r#"fo:text-align="right""#)), "{after}");
    assert_eq!(after.matches(r#"text:style-name="P1""#).count(), 1, "only x names P1: {after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(aligns(&back.doc, "x"), [Some(2.0)]);
    assert_eq!(aligns(&back.doc, "y"), [None, Some(1.0)], "y's new paragraph is centred, as the model has it");
}

/// A copy that changes nothing charges no items: two spans share a style that already says what the new row asks for, so the copy is not made and the items charged for
/// it are given back.
#[test]
fn a_copy_that_changes_nothing_charges_no_items() {
    let double = r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="bold" style:text-underline-style="solid" style:text-underline-type="double"/></style:style>"##;
    let two = r#"<text:p>a<text:span text:style-name="T3">b</text:span></text:p><text:p>c<text:span text:style-name="T3">d</text:span></text:p>"#;
    let mut o = open(&Drawing::a4().auto(double).page(&shape_in(two)));
    edit_cell(&mut o, "Character", 1, "Style", 5.0);
    let (saved, work) = save_with(&o, &Limits::default());
    let saved = saved.unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(saved.edited && style_of(&after, "T4").is_none() && style_of(&after, "T3").is_some(), "no copy was made: {after}");
    assert_eq!(work.copied_items, 0, "what was charged for the copy was given back");
    assert_eq!(work.style_copy_bytes, 0);
}

/// A style edited where it stands is charged what the edit adds at most (an attribute for each attribute written, and the element when it has none): a size edit of a
/// style a single span names is one attribute and the element's room.
#[test]
fn a_style_edited_where_it_stands_is_charged_what_the_edit_adds() {
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let (saved, work) = save_with(&o, &Limits::default());
    let saved = saved.unwrap_or_else(|e| panic!("{e}"));
    assert!(style_of(&part(&saved.bytes, "content.xml"), "T1").is_some_and(|s| s.contains("14pt")));
    assert_eq!(work.copied_items, 1 + 2, "one attribute written, and the room of an element");
    // An edit that changes nothing of the style is given back: a style that says a double underline already, asked for one.
    let double = r##"<style:style style:name="T3" style:family="text"><style:text-properties fo:font-weight="bold" style:text-underline-style="solid" style:text-underline-type="double"/></style:style>"##;
    let mut o = open(&Drawing::a4().auto(double).page(&shape_in(r#"<text:p>ab<text:span text:style-name="T3">cd</text:span></text:p>"#)));
    edit_cell(&mut o, "Character", 1, "Style", 5.0);
    let (saved, work) = save_with(&o, &Limits::default());
    assert!(saved.is_ok_and(|w| w.edited));
    assert_eq!(work.copied_items, 0, "nothing was added, and what was charged is given back");
}

/// The review's recipe for the clone of a graphic style: many shapes share a style of 100 attributes no cell maps, and each is given a fill colour of its own. A clone is
/// the whole style, so its items are charged against what the reader reads of one part: the save is made and reads again, or it is refused in words.
#[test]
fn the_clone_of_a_shared_graphic_style_is_charged_against_what_the_reader_reads() {
    let limits = Limits { max_read_xml_nodes: 60_000, ..Limits::default() };
    for (shapes, saves) in [(20usize, true), (800, false)] {
        let attributes: String = (0..100).map(|k| format!(r#" xx:a{k}="{k:0>10}""#)).collect();
        let style = format!(
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"{attributes}/></style:style>"##
        );
        let body: String = (0..shapes)
            .map(|k| {
                format!(
                    r#"<draw:rect draw:name="r{k}" draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"><text:p/></draw:rect>"#
                )
            })
            .collect();
        let mut d = Drawing::a4().auto(&style).page(&body);
        d.ns.push_str(r#" xmlns:xx="urn:example:foreign""#);
        let mut o = open_with(&d, &limits);
        let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
        for (k, id) in ids.iter().enumerate() {
            let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, 0, *id).sheet);
            sheet.cells.insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text(&format!("#{:06x}", 0x10_0000 + k))));
        }
        let (saved, work) = save_with(&o, &limits);
        match saved {
            Ok(saved) => {
                assert!(saves, "{shapes} shapes: the save made a file with {} items charged and was expected to be refused", work.copied_items);
                assert!(read(&saved.bytes, &limits).is_ok(), "{shapes} shapes: the file a save made reads again");
                assert!(work.copied_items >= (shapes as u64 - 1) * 100, "{shapes} shapes: {} items charged for the clones", work.copied_items);
            }
            Err(error) => {
                assert!(!saves, "{shapes} shapes: {error}");
                assert_eq!(error.class(), "write_limit", "{error}");
            }
        }
    }
}

/// The maps of a page's layers are built only for a page that gains a shape, and charged to the memory a save may hold.
///
/// A drawing of `layers` layers (`L0` to `L<n-1>`) on the page, whose first shape is moved; with `add` a rectangle is put on its row 0 as well. What the save counted
/// of steps, and what it held of the memory a save may hold.
fn layer_costs(layers: usize, add: bool) -> (u64, u64) {
    let mut d = Drawing::a4().page(&crate::write_core::custom("s1", "1cm"));
    let rows: String = (0..layers).map(|k| format!(r#"<draw:layer draw:name="L{k}"/>"#)).collect();
    d.masters = d.masters.replace(r#"<draw:layer draw:name="measurelines"/>"#, &format!(r#"<draw:layer draw:name="measurelines"/>{rows}"#));
    let limits = Limits::default();
    let mut o = open_with(&d, &limits);
    crate::write_core::move_by(&mut o.doc, 0, 1, 1.0, 0.0);
    if add {
        let id = o.index.next_id(0).unwrap_or(2);
        push_shape(&mut o.doc, rectangle_on(id, "R", "0"));
    }
    let (saved, work) = save_with(&o, &limits);
    assert!(saved.is_ok(), "{layers} layers, add {add}: {saved:?}");
    let room = limits.max_write_memory.saturating_sub(work.estimated_memory);
    (work.order_steps, room.saturating_sub(work.memory_room))
}

/// A save that adds no shape to a page does not look at its layers: 40 and 2,000 layers cost it the same (the rows of the layers are no step of it), and a save that adds a
/// shape walks the rows of the markup's set and of the model's (a step for each), holding what each row needs (`ROW_BYTES` and its name's bytes twice).
#[test]
fn the_layers_of_a_page_are_walked_and_held_only_when_it_gains_a_shape() {
    let (small, large) = (layer_costs(40, false), layer_costs(2_000, false));
    assert!(large.0 < small.0 + 100, "no step for a layer where no shape is added: {} against {}", large.0, small.0);
    assert!(large.1 < small.1 + 10_000, "and no memory held for them: {} against {}", large.1, small.1);
    let (with_small, with_large) = (layer_costs(40, true), layer_costs(2_000, true));
    // 1,960 more layers, each a row of the markup and a row of the model: two steps each, and the room of a row (96 bytes and the name twice) for each map.
    assert!(with_large.0 >= with_small.0 + 2 * 1_960, "a step for each row of each map: {} against {}", with_large.0, with_small.0);
    assert!(with_large.1 >= with_small.1 + 2 * 1_960 * 96, "the room each row holds: {} against {}", with_large.1, with_small.1);
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// The second round of hand deletions: the layers the reader resolves against, a flat drawing, a paragraph put in before one, a style two things name, a group's frame
// cell taken out.

/// A page of the file whose own layer set holds **no layer** (the element is there, empty): the reader resolves `draw:layer` against that set, not the
/// document's, so a name the document's set has resolves to nothing here. The model's rows (those of the document's set, put on the page by the model) are what the
/// shape's `LayerMember` refers to: `layout` is written and the cell is carried, and the shape is on row 0 when the file is opened again.
fn a_new_shape_on_a_page_whose_own_layer_set_is_empty_is_not_written_by_the_documents(save: &Saver) {
    let mut d = layered_drawing();
    d.pages = d.pages.replacen(r#"master-page-name="Default">"#, r#"master-page-name="Default"><draw:layer-set/>"#, 1);
    assert!(d.pages.contains("<draw:layer-set/>"), "the page has a layer set of its own");
    let mut o = open(&d);
    assert!(layer_names(&o.doc, 0).is_empty(), "the page's own set holds no layer: {:?}", layer_names(&o.doc, 0));
    set_layers(&mut o.doc, 0, &["Notes", "Other"]);
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "R", "0"));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(layer_attr(&content, "R").as_deref(), Some("layout"), "the page's own set has no `Notes`: {content}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!(layer_member(&back.doc, 0, "R").as_deref(), Some("0"), "the shape is on row 0 when the file is opened again");
    assert_eq!(layer_names(&back.doc, 0), ["Notes", "Other"]);
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// The same in a flat drawing (`.fodg`), which has its own path through the writer and its own reader: a new shape on a row the markup has is written under that name and
/// reads back as that row (the reader's rows of the pages are what the save predicts from; with none, as was the case in this round's first build, `layout` was written and
/// the cell carried, and a shape on `Other` was on `layout` to every other program). A layer renamed in a flat drawing is refused as a change of the page's own layers.
fn a_new_shape_in_a_flat_drawing_is_written_on_the_layer_the_markup_has_for_it(save: &Saver) {
    let flat = || crate::flat::Flat::of(&layered_drawing()).bytes();
    // Row 1 (`Other`) is the markup's too.
    let mut o = open_bytes(flat());
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "S", "1"));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let text = crate::flat_write::text_of(&saved.bytes);
    assert_eq!(layer_attr(&text, "S").as_deref(), Some("Other"), "{text}");
    assert_eq!(layer_member(&open_bytes(saved.bytes.clone()).doc, 0, "S").as_deref(), Some("1"));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
    // A layer renamed: the page's own layers have no place in a flat drawing but the markup, so it is refused in words.
    let mut o = open_bytes(flat());
    set_layers(&mut o.doc, 0, &["Comments", "Other"]);
    assert!(
        matches!(save(&o.doc, &o.bytes, &o.index), Err(WriteError::Unsupported(words)) if words.contains("layers")),
        "a layer renamed in a flat drawing is refused in words"
    );
}

/// In a flat drawing: its one container holds the common styles too, and a name that some attribute of the part holds is never given to a new style (the scan of the
/// part's own bytes, `styles::count_refs`; a package's other part is read for its names). A common text style `T2` and two spans that share `T1`: the copy is `T3`.
fn a_copy_in_a_flat_drawing_is_never_given_the_name_of_a_common_style(save: &Saver) {
    let two = r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#;
    let d = Drawing::a4().common(r#"<style:style style:name="T2" style:family="text"/>"#).auto(T1).page(&shape_in(two));
    let mut o = open_bytes(crate::flat::Flat::of(&d).bytes());
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let text = crate::flat_write::text_of(&saved.bytes);
    assert!(style_of(&text, "T3").is_some_and(|s| s.contains("14pt")), "the copy is T3: {text}");
    assert_eq!(text.matches(r#"style:name="T2""#).count(), 1, "one style is named T2, the common one: {text}");
    assert_eq!(text.matches(r#"text:style-name="T3""#).count(), 2, "{text}");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// Where the common styles are **in the part the save edits**: a shape of a master page is in `styles.xml`, whose `office:styles` holds the common styles and whose
/// automatic styles are the ones it copies. A common text style `T2` there and two spans of the shape that share `T1` (an automatic style of `styles.xml`): the copy is `T3`
/// (the part's own names are avoided by the scan of its bytes, so nothing of the other part's names is needed for it).
fn a_copy_in_a_master_pages_shape_is_never_given_the_name_of_a_common_style(save: &Saver) {
    let two = r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p><text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#;
    let mut d = Drawing::a4().common(r#"<style:style style:name="T2" style:family="text"/>"#).page(&crate::write_core::custom("p", "1cm"));
    d.styles_auto.push_str(T1);
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        &format!(r#"<style:master-page style:name="Default" style:page-layout-name="PM1">{}</style:master-page>"#, shape_in(two)),
    );
    let mut o = open(&d);
    let (at, id) = o
        .doc
        .pages
        .iter()
        .enumerate()
        .find_map(|(at, p)| p.shapes.iter().find(|s| s.name.as_deref() == Some("x")).map(|s| (at, s.id)))
        .unwrap_or_else(|| panic!("the master page's shape is in the model"));
    let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut o.doc, at, id).sheet);
    let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Character").unwrap_or_else(|| panic!("no Character section")));
    let row = Arc::make_mut(section.rows.iter_mut().find(|r| r.ix == Some(1)).unwrap_or_else(|| panic!("no row 1")));
    row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_number(14.0 / 72.0).unwrap_or_else(|| Value::from_text("0"))));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let styles = part(&saved.bytes, "styles.xml");
    assert!(style_of(&styles, "T3").is_some_and(|s| s.contains("14pt")), "the copy is T3: {styles}");
    assert_eq!(styles.matches(r#"style:name="T2""#).count(), 1, "one style is named T2, the common one: {styles}");
    assert_eq!(part(&saved.bytes, "content.xml"), part(&o.bytes, "content.xml"), "the pages' part is the file's own");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// A style is cloned when another paragraph names it. Two paragraphs (of two shapes) name `P1`; the alignment of one is edited: it has a copy that says right,
/// and the other still names `P1`, which still says centre.
fn a_paragraph_style_that_two_paragraphs_name_is_copied_for_the_one_edited(save: &Saver) {
    let style = r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#;
    let body = format!(
        "{}{}",
        crate::write_runs::shape("w", "1cm", r#"<text:p text:style-name="P1">w</text:p>"#),
        crate::write_runs::shape("x", "6cm", r#"<text:p text:style-name="P1">ab</text:p>"#)
    );
    let mut o = open(&Drawing::a4().auto(style).page(&body));
    let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    set_row_of(&mut o, ids[1], "Paragraph", 0, &[("HorzAlign", 2.0)]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "P1").is_some_and(|s| s.contains("center")), "P1 still says centre, for `w`: {after}");
    assert!(style_of(&after, "P2").is_some_and(|s| s.contains(r#"fo:text-align="right""#)), "and `x` has a copy that says right: {after}");
    let back = open_bytes(saved.bytes.clone());
    assert_eq!((aligns(&back.doc, "w"), aligns(&back.doc, "x")), (vec![Some(1.0)], vec![Some(2.0)]));
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// R22 for a run's style: two shapes have a span that names `T1`; the size of one run is edited: it has a copy with the size and the other span still names `T1`, at
/// 12 pt.
fn a_text_style_that_two_runs_name_is_copied_for_the_run_edited(save: &Saver) {
    let body = format!(
        "{}{}",
        crate::write_runs::shape("p", "1cm", r#"<text:p>a<text:span text:style-name="T1">b</text:span></text:p>"#),
        crate::write_runs::shape("q", "6cm", r#"<text:p>c<text:span text:style-name="T1">d</text:span></text:p>"#)
    );
    let mut o = open(&Drawing::a4().auto(T1).page(&body));
    let ids: Vec<u32> = o.doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect();
    copy_row(&mut o, ids[0], "Character", (1, 1), &[("Size", 20.0 / 72.0)]);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&saved.bytes, "content.xml");
    assert!(style_of(&after, "T1").is_some_and(|s| s.contains(r#"fo:font-size="12pt""#)), "T1 is as it was, for `q`: {after}");
    assert!(
        style_of(&after, "T2").is_some_and(|s| s.contains(r#"fo:font-size="20pt""#) && s.contains("wave")),
        "`p` has a copy with the size: {after}"
    );
    assert_eq!(after.matches(r#"text:style-name="T1""#).count(), 1, "one span names T1: {after}");
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
}

/// A frame cell taken out of a group, **beside another change of the group** (a comment cell put on it): the box of its members is the only place the file has for the
/// cell, so the change is refused as the other changes of the pin are, and never dropped with the save saying it wrote nothing. (A frame cell taken out and nothing else
/// is the frame's, as a move: a cell one sheet has and the other lacks is the frame's, `own::pose_extras_same`.)
fn a_group_with_a_frame_cell_taken_out_beside_another_change_is_refused_in_words(save: &Saver) {
    let body = format!(r#"<draw:g draw:name="G">{}{}</draw:g>"#, crate::write_core::custom("A", "1cm"), crate::write_core::custom("B", "6cm"));
    let o = open(&Drawing::a4().page(&body));
    let id = o.doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|g| g.id).unwrap_or_else(|| panic!("no group"));
    for cell in ["LocPinX", "LocPinY"] {
        let mut doc = o.doc.clone();
        let sheet = Arc::make_mut(&mut crate::write_core::shape_mut(&mut doc, 0, id).sheet);
        assert!(sheet.cells.remove(cell).is_some(), "the group has {cell}");
        sheet.cells.insert(CellName::new("Comment"), Cell::constant(Value::from_text("hello")));
        match save(&doc, &o.bytes, &o.index) {
            Err(WriteError::Unsupported(words)) => assert!(words.contains("group's pin"), "{cell}: {words}"),
            other => panic!("{cell}: expected a refusal in words, got {:?}", other.map(|w| w.edited)),
        }
    }
    // The comment alone is the document file's: saved.
    let mut doc = o.doc.clone();
    Arc::make_mut(&mut crate::write_core::shape_mut(&mut doc, 0, id).sheet)
        .cells
        .insert(CellName::new("Comment"), Cell::constant(Value::from_text("hello")));
    assert!(save(&doc, &o.bytes, &o.index).is_ok_and(|w| w.edited), "a comment alone is saved");
}

/// A flat drawing is one tree, and the reader's cap on items is on the whole of it: a retext that gives each of many runs a style of its own is saved as a file that
/// reads again, or refused in words, as in a package.
#[test]
fn a_retext_in_a_flat_drawing_that_gives_many_runs_a_style_each_saves_a_file_that_reads_again_or_is_refused_in_words() {
    let limits = Limits { max_read_xml_nodes: 60_000, ..Limits::default() };
    for (spans, saves) in [(30usize, true), (1_000, false)] {
        let o = many_runs_in(spans, 100, &limits, true);
        assert!(crate::flat_write::text_of(&o.bytes).starts_with("<?xml"), "a flat drawing");
        let (saved, work) = save_with(&o, &limits);
        match saved {
            Ok(saved) => {
                assert!(saves, "{spans} spans: the save made a file with {} items charged and was expected to be refused", work.copied_items);
                read(&saved.bytes, &limits).unwrap_or_else(|e| panic!("{spans} spans: the file a save made does not read: {} ({e})", e.class()));
                assert!(work.copied_items >= spans as u64 * 100, "{spans} spans: {} items charged for the copies", work.copied_items);
            }
            Err(error) => {
                assert!(!saves, "{spans} spans: the save was refused and was expected to be made: {error}");
                assert_eq!(error.class(), "write_limit", "{error}");
                assert!(error.to_string().contains("items in a part"), "in words: {error}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------
// A black-box check against LibreOffice (not part of the suite: both tests are ignored, and run by hand with `ODG_R8FIX_DIR` set to an empty folder).
//
// 1. `lo_write_1` writes `in/*.odg`: the saves of the layer cases of a layer renamed and a shape added; a new page with rows of its own, a file whose shape
//    has a `draw:layer` that names no layer of the drawing (what a save wrote before the fix, made by hand), a flat drawing with a new shape on a layer, and the saves of the style cases of (a style edited
//    where it stands, one copied for a run split in two).
// 2. LibreOffice converts them as a black box: `soffice -env:UserInstallation=file://<folder>/profile --headless --convert-to odg --outdir out in/*.odg`, and
//    `--convert-to pdf`; only a missing output file is a failure.
// 3. `lo_read_1` reads `out/*.odg` with the reader and says what came back: the layer of each shape, the sizes of the runs.

fn lo_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ODG_R8FIX_DIR").expect("set ODG_R8FIX_DIR to an empty folder"))
}

#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_R8FIX_DIR set"]
fn lo_write_1() {
    let dir = lo_dir().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    // Row 0 renamed, then a rectangle on row 0.
    let mut o = open(&layered_drawing());
    set_layers(&mut o.doc, 0, &["Comments", "Other"]);
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let mut reopened = open_bytes(first.bytes.clone());
    let id = reopened.index.next_id(0).unwrap_or(2);
    push_shape(&mut reopened.doc, rectangle_on(id, "R", "0"));
    put("renamed_then_added.odg", &real(&reopened.doc, &reopened.bytes, &reopened.index).unwrap().bytes);
    // A new page whose own rows are `Fresh`, with a rectangle on its row 0.
    let mut o = open(&layered_drawing());
    let id = u32::try_from(o.doc.pages.len()).unwrap_or(1);
    let mut page = new_page(&o.doc, id, "Second", Vec::new());
    page.shapes.push(Arc::new(rectangle_on(1, "R", "0")));
    Arc::make_mut(&mut page.sheet).sections.retain(|s| !(s.name.as_str() == "Layer" && s.ix.is_none()));
    Arc::make_mut(&mut page.sheet).sections.push(layer_section(&["Fresh"]));
    o.doc.pages.push(Arc::new(page));
    put("new_page_own_rows.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // What the writer wrote before the fix, made by hand: a shape on a layer the drawing does not have.
    let d = layered_drawing();
    let stray = d.pages.replacen("<draw:custom-shape draw:name=\"s1\"", "<draw:custom-shape draw:layer=\"NoSuchLayer\" draw:name=\"s1\"", 1);
    assert_ne!(stray, d.pages);
    let mut d = d;
    d.pages = stray;
    put("stray_layer.odg", &d.bytes());
    // A style edited where it stands (a size), and a run split in two, the first half sized.
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    edit_cell(&mut o, "Character", 1, "Size", 14.0 / 72.0);
    put("style_in_place.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    let mut o = open(&crate::write_kept_dropped::run_drawing());
    let id = crate::write_runs::id_of(&o.doc);
    copy_row(&mut o, id, "Character", (1, 2), &[("Size", 20.0 / 72.0)]);
    set_text_of(&mut o, id, vec![TextPart::Pp(0), chars("ab"), TextPart::Cp(2), chars("c"), TextPart::Cp(1), chars("d")]);
    put("style_copied.odg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // In a flat drawing: a new shape on the row `Other`, which the flat reader's rows (the markup's) say.
    let mut o = open_bytes(crate::flat::Flat::of(&layered_drawing()).bytes());
    let id = o.index.next_id(0).unwrap_or(2);
    push_shape(&mut o.doc, rectangle_on(id, "S", "1"));
    put("flat_layer.fodg", &real(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    println!("wrote 6 files in {}", dir.display());
}

#[test]
#[ignore = "reads the files LibreOffice converted, run by hand with ODG_R8FIX_DIR set"]
fn lo_read_1() {
    let out = lo_dir().join("out");
    let read_one = |name: &str| {
        let bytes = std::fs::read(out.join(name)).unwrap_or_else(|e| panic!("out/{name}: run LibreOffice first ({e})"));
        read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("out/{name} did not read: {e}")).0
    };
    let layer_of = |doc: &Document, page: usize, name: &str| -> Option<Option<String>> {
        let shape = doc.pages.get(page)?.shapes.iter().find(|s| s.name.as_deref() == Some(name))?;
        Some(text_of(shape.sheet.cell("LayerMember")))
    };
    let doc = read_one("renamed_then_added.odg");
    println!("renamed_then_added: R on {:?}, rows {:?}", layer_of(&doc, 0, "R"), layer_names(&doc, 0));
    let doc = read_one("new_page_own_rows.odg");
    println!("new_page_own_rows: R on {:?}, rows {:?} and {:?}", layer_of(&doc, 1, "R"), layer_names(&doc, 0), layer_names(&doc, 1));
    let doc = read_one("stray_layer.odg");
    println!("stray_layer: s1 on {:?}, rows {:?}", layer_of(&doc, 0, "s1"), layer_names(&doc, 0));
    let doc = read_one("flat_layer.odg");
    println!("flat_layer: S on {:?}, rows {:?}", layer_of(&doc, 0, "S"), layer_names(&doc, 0));
    for name in ["style_in_place.odg", "style_copied.odg"] {
        let doc = read_one(name);
        let sizes: Vec<Option<i64>> = (0..4).map(|ix| cell_number(&doc, "Character", ix, "Size").map(|v| (v * 72.0).round() as i64)).collect();
        println!("{name}: sizes {sizes:?}");
    }
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 19] = [
    ("a_new_shape_on_a_layer_renamed_before_keeps_it_on_reopen", a_new_shape_on_a_layer_renamed_before_keeps_it_on_reopen),
    ("a_rename_and_a_new_shape_in_one_save_write_layout_and_carry_the_layer", a_rename_and_a_new_shape_in_one_save_write_layout_and_carry_the_layer),
    ("a_new_shape_on_a_new_page_with_rows_of_its_own_keeps_its_row", a_new_shape_on_a_new_page_with_rows_of_its_own_keeps_its_row),
    (
        "a_new_shape_on_a_page_with_a_layer_set_of_its_own_is_written_by_that_set",
        a_new_shape_on_a_page_with_a_layer_set_of_its_own_is_written_by_that_set,
    ),
    (
        "a_paragraph_style_edited_where_it_stands_is_not_found_again_by_the_attributes_it_had",
        a_paragraph_style_edited_where_it_stands_is_not_found_again_by_the_attributes_it_had,
    ),
    (
        "the_style_of_a_paragraph_a_new_one_is_beside_is_copied_and_not_edited_where_it_stands",
        the_style_of_a_paragraph_a_new_one_is_beside_is_copied_and_not_edited_where_it_stands,
    ),
    ("a_run_split_in_two_keeps_the_style_for_the_half_that_is_as_it_was", a_run_split_in_two_keeps_the_style_for_the_half_that_is_as_it_was),
    ("a_groups_pin_changed_as_a_number_or_a_formula_is_refused_in_words", a_groups_pin_changed_as_a_number_or_a_formula_is_refused_in_words),
    ("a_copy_is_never_given_the_name_of_a_common_style", a_copy_is_never_given_the_name_of_a_common_style),
    (
        "an_edit_that_only_takes_attributes_out_of_a_style_with_no_properties_changes_no_style",
        an_edit_that_only_takes_attributes_out_of_a_style_with_no_properties_changes_no_style,
    ),
    (
        "the_style_of_a_run_a_new_paragraph_is_beside_is_copied_and_not_edited_where_it_stands",
        the_style_of_a_run_a_new_paragraph_is_beside_is_copied_and_not_edited_where_it_stands,
    ),
    ("the_index_made_after_an_edit_in_place_leaves_out_the_style_edited", the_index_made_after_an_edit_in_place_leaves_out_the_style_edited),
    (
        "a_new_shape_on_a_page_whose_own_layer_set_is_empty_is_not_written_by_the_documents",
        a_new_shape_on_a_page_whose_own_layer_set_is_empty_is_not_written_by_the_documents,
    ),
    (
        "a_new_shape_in_a_flat_drawing_is_written_on_the_layer_the_markup_has_for_it",
        a_new_shape_in_a_flat_drawing_is_written_on_the_layer_the_markup_has_for_it,
    ),
    ("a_copy_in_a_flat_drawing_is_never_given_the_name_of_a_common_style", a_copy_in_a_flat_drawing_is_never_given_the_name_of_a_common_style),
    (
        "a_paragraph_style_that_two_paragraphs_name_is_copied_for_the_one_edited",
        a_paragraph_style_that_two_paragraphs_name_is_copied_for_the_one_edited,
    ),
    ("a_text_style_that_two_runs_name_is_copied_for_the_run_edited", a_text_style_that_two_runs_name_is_copied_for_the_run_edited),
    (
        "a_group_with_a_frame_cell_taken_out_beside_another_change_is_refused_in_words",
        a_group_with_a_frame_cell_taken_out_beside_another_change_is_refused_in_words,
    ),
    (
        "a_copy_in_a_master_pages_shape_is_never_given_the_name_of_a_common_style",
        a_copy_in_a_master_pages_shape_is_never_given_the_name_of_a_common_style,
    ),
];

#[test]
fn a_new_shape_on_a_layer_renamed_before_keeps_it_on_reopen_test() {
    a_new_shape_on_a_layer_renamed_before_keeps_it_on_reopen(&real);
}

#[test]
fn a_rename_and_a_new_shape_in_one_save_write_layout_and_carry_the_layer_test() {
    a_rename_and_a_new_shape_in_one_save_write_layout_and_carry_the_layer(&real);
}

#[test]
fn a_new_shape_on_a_new_page_with_rows_of_its_own_keeps_its_row_test() {
    a_new_shape_on_a_new_page_with_rows_of_its_own_keeps_its_row(&real);
}

#[test]
fn a_new_shape_on_a_page_with_a_layer_set_of_its_own_is_written_by_that_set_test() {
    a_new_shape_on_a_page_with_a_layer_set_of_its_own_is_written_by_that_set(&real);
}

#[test]
fn a_paragraph_style_edited_where_it_stands_is_not_found_again_by_the_attributes_it_had_test() {
    a_paragraph_style_edited_where_it_stands_is_not_found_again_by_the_attributes_it_had(&real);
}

#[test]
fn the_style_of_a_paragraph_a_new_one_is_beside_is_copied_and_not_edited_where_it_stands_test() {
    the_style_of_a_paragraph_a_new_one_is_beside_is_copied_and_not_edited_where_it_stands(&real);
}

#[test]
fn a_run_split_in_two_keeps_the_style_for_the_half_that_is_as_it_was_test() {
    a_run_split_in_two_keeps_the_style_for_the_half_that_is_as_it_was(&real);
}

#[test]
fn a_groups_pin_changed_as_a_number_or_a_formula_is_refused_in_words_test() {
    a_groups_pin_changed_as_a_number_or_a_formula_is_refused_in_words(&real);
}

#[test]
fn a_copy_is_never_given_the_name_of_a_common_style_test() {
    a_copy_is_never_given_the_name_of_a_common_style(&real);
}

#[test]
fn an_edit_that_only_takes_attributes_out_of_a_style_with_no_properties_changes_no_style_test() {
    an_edit_that_only_takes_attributes_out_of_a_style_with_no_properties_changes_no_style(&real);
}

#[test]
fn the_style_of_a_run_a_new_paragraph_is_beside_is_copied_and_not_edited_where_it_stands_test() {
    the_style_of_a_run_a_new_paragraph_is_beside_is_copied_and_not_edited_where_it_stands(&real);
}

#[test]
fn the_index_made_after_an_edit_in_place_leaves_out_the_style_edited_test() {
    the_index_made_after_an_edit_in_place_leaves_out_the_style_edited(&real);
}

#[test]
fn a_new_shape_on_a_page_whose_own_layer_set_is_empty_is_not_written_by_the_documents_test() {
    a_new_shape_on_a_page_whose_own_layer_set_is_empty_is_not_written_by_the_documents(&real);
}

#[test]
fn a_new_shape_in_a_flat_drawing_is_written_on_the_layer_the_markup_has_for_it_test() {
    a_new_shape_in_a_flat_drawing_is_written_on_the_layer_the_markup_has_for_it(&real);
}

#[test]
fn a_copy_in_a_flat_drawing_is_never_given_the_name_of_a_common_style_test() {
    a_copy_in_a_flat_drawing_is_never_given_the_name_of_a_common_style(&real);
}

#[test]
fn a_paragraph_style_that_two_paragraphs_name_is_copied_for_the_one_edited_test() {
    a_paragraph_style_that_two_paragraphs_name_is_copied_for_the_one_edited(&real);
}

#[test]
fn a_text_style_that_two_runs_name_is_copied_for_the_run_edited_test() {
    a_text_style_that_two_runs_name_is_copied_for_the_run_edited(&real);
}

#[test]
fn a_group_with_a_frame_cell_taken_out_beside_another_change_is_refused_in_words_test() {
    a_group_with_a_frame_cell_taken_out_beside_another_change_is_refused_in_words(&real);
}

#[test]
fn a_copy_in_a_master_pages_shape_is_never_given_the_name_of_a_common_style_test() {
    a_copy_in_a_master_pages_shape_is_never_given_the_name_of_a_common_style(&real);
}

/// The echo control: a writer that gives the original back fails every case, and the real writer passes each.
#[test]
fn echo_control_fails_every_case() {
    for (name, case) in CASES {
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo))).is_err(), "the echo writer passed the case {name}: the case checks nothing");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&real))).is_ok(), "the real writer failed the case {name}");
    }
}
