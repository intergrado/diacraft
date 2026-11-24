//! A drawing whose style sheets are numbered with gaps (a VSDX file's `ID`s are 0, 1, 2, 6, 7, 30, ...) is written as an `.odg` by the model writer and read back
//! with the `ID`s it had, with every link that names one, and a save into the file written does not refuse for the numbering.
//!
//! An OpenDocument file numbers its common styles by their places: the reader gives the style at place *p* the `ID` *p*. The writer writes the style sheets in the model's order, and the
//! document file carries the true `ID` of each style whose `ID` is not its place (the `id` of its `style` entry); the reader gives the `ID`s back and puts every link it made from the markup
//! (a shape's parent, a style's parent) through the same map. Every case here saves a model, reads the bytes back with the real reader and compares what came back with what went in.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Cell, CellName, Document, Links, RawAttrs, Sheet, Style, Value};
use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::model_write::blank;
use crate::write_core::{open_bytes, real};

/// A style sheet of the `ID` `id` with two cells, as a VSDX file's holds them, whose three links are `links`.
fn style(id: u32, name: &str, links: (u32, u32, u32), weight: &str) -> Style {
    let mut sheet = Sheet { links: Links { line: Some(links.0), fill: Some(links.1), text: Some(links.2) }, ..Sheet::default() };
    sheet.cells.insert(CellName::new("LineWeight"), Cell::constant(Value::from_text(weight)).with_unit("IN"));
    sheet.cells.insert(CellName::new("EnableFillProps"), Cell::constant(Value::from_text("1")));
    Style { id, name: Some(Arc::from(name)), name_u: Some(Arc::from(name)), sheet: Arc::new(sheet), ..Style::default() }
}

/// The model of a VSDX file's drawing as far as its style sheets go: a root with cells of its own, and `more` style sheets after it, with `shapes` rectangles on its page.
fn drawing(shapes: u32, more: Vec<Style>) -> Document {
    let mut doc = blank(shapes);
    let mut root = style(0, "No Style", (0, 0, 0), "0.01");
    root.sheet = {
        let mut sheet = Sheet::clone(&root.sheet);
        sheet.links = Links::default();
        Arc::new(sheet)
    };
    root.raw = RawAttrs::from_pairs([("IsCustomNameU", "1")]);
    *doc.styles.get_mut(0).unwrap() = Arc::new(root);
    for s in more {
        doc.styles.push(Arc::new(s));
    }
    doc
}

/// The style sheets the numbering of the task names: IDs 0, 1, 2, 6, 7 and 30, whose links name each other (one style whose three links differ).
fn gapped(shapes: u32) -> Document {
    drawing(
        shapes,
        vec![
            style(1, "Normal", (0, 0, 0), "0.02"),
            style(2, "Heading", (1, 1, 1), "0.03"),
            style(6, "Box", (2, 1, 2), "0.04"),
            style(7, "Accent", (6, 6, 6), "0.05"),
            style(30, "Last", (7, 7, 7), "0.06"),
        ],
    )
}

/// Puts the shape `id` of the page on the style sheet `link` (all three links).
fn link_shape(doc: &mut Document, id: u32, link: u32) {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == id).unwrap();
    let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
    Arc::make_mut(&mut shape.sheet).links = Links { line: Some(link), fill: Some(link), text: Some(link) };
}

/// What a style sheet shows: its `ID`, names, links and the values of its cells.
type Shown = (u32, Option<String>, Option<String>, Links, Vec<(String, String)>);

fn shown(doc: &Document) -> Vec<Shown> {
    doc.styles
        .iter()
        .map(|s| {
            let cells = s
                .sheet
                .cells
                .iter()
                .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default()))
                .collect();
            (s.id, s.name.as_deref().map(str::to_owned), s.name_u.as_deref().map(str::to_owned), s.sheet.links, cells)
        })
        .collect()
}

/// The links of the shapes of the first page, by `ID`.
fn shape_links(doc: &Document) -> Vec<(u32, Links)> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| (s.id, s.sheet.links)).collect()
}

/// The package `bytes` with the part `name` replaced by `text`, the others as they are (the `mimetype` first and stored).
fn replace_part(bytes: &[u8], name: &str, text: &str) -> Vec<u8> {
    use crate::zipbuild::{Item, build};
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    let items: Vec<Item> = package
        .parts()
        .map(|part| {
            let content: Vec<u8> = if part == name { text.as_bytes().to_vec() } else { package.part(part).unwrap_or_default().to_vec() };
            if part == "mimetype" { Item::stored(part, &content) } else { Item::deflated(part, &content) }
        })
        .collect();
    build(&items)
}

fn saved(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("not saved: {e}")).bytes
}

fn written_and_read(doc: &Document) -> Document {
    read(&saved(doc), &Limits::default()).unwrap().0
}

/// **The gaps.** A drawing whose style sheets are numbered 0, 1, 2, 6, 7 and 30 is written, and read back with the same `ID`s, names, links (the three that differ among them) and cells,
/// and the shapes that name a style sheet read back naming the same one. It was refused: "a style sheet whose ID is not its place among the style sheets".
#[test]
fn style_sheets_numbered_with_gaps_are_written_and_read_back_with_their_ids_links_and_cells() {
    let mut doc = gapped(3);
    link_shape(&mut doc, 1, 6);
    link_shape(&mut doc, 2, 30);
    link_shape(&mut doc, 3, 0);
    let back = written_and_read(&doc);
    assert_eq!(shown(&back), shown(&doc), "the style sheets: IDs, names, links and cells");
    assert_eq!(shape_links(&back), shape_links(&doc), "the shapes' links");
}

/// The way back: the model read from the file is written again (as a file made from the model, as a `.diacraft` or a VSDX file would be), and the second file reads the same.
#[test]
fn the_model_read_back_is_written_again_with_the_same_numbering() {
    let mut doc = gapped(3);
    link_shape(&mut doc, 1, 6);
    link_shape(&mut doc, 2, 30);
    let once = written_and_read(&doc);
    let twice = written_and_read(&once);
    assert_eq!(shown(&twice), shown(&doc));
    assert_eq!(shape_links(&twice), shape_links(&once));
    assert_eq!(saved(&once), saved(&twice), "a model read from the file saves as the file it was read from");
}

/// The `style` entries of the document file in `bytes`, each as the text of its start tag.
fn style_tags(bytes: &[u8]) -> Vec<String> {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    let file = String::from_utf8_lossy(package.part("DiaCraft/document.xml").unwrap_or_default()).into_owned();
    file.split("<diacraft:style ").skip(1).map(|rest| rest.split('>').next().unwrap_or_default().to_owned()).collect()
}

/// A numbering that has no gap writes no `id` on any entry for a style sheet (the document file of a drawing of gap-free numbering is as it was), and one with a gap writes the `ID` of the
/// style sheets that are not at their places, and of those alone.
#[test]
fn a_numbering_with_no_gap_writes_no_id() {
    let doc = drawing(1, vec![style(1, "Normal", (0, 0, 0), "0.02"), style(2, "Heading", (1, 1, 1), "0.03")]);
    let tags = style_tags(&saved(&doc));
    assert!(!tags.is_empty() && tags.iter().all(|t| !t.contains(" id=\"")), "{tags:?}");
    let tags = style_tags(&saved(&gapped(1)));
    for (name, id) in [("Box", "6"), ("Accent", "7"), ("Last", "30")] {
        assert!(tags.iter().any(|t| t.contains(&format!("name=\"{name}\"")) && t.contains(&format!(" id=\"{id}\""))), "{name}: {tags:?}");
    }
    assert_eq!(tags.iter().filter(|t| t.contains(" id=\"")).count(), 3, "{tags:?}");
}

/// Style sheets whose numbers are far apart cost no more than ones that are not: an `ID` of 4,000,000 is a number, never a place to count to.
#[test]
fn ids_far_apart_are_numbers_and_not_places() {
    let mut doc = drawing(
        2,
        vec![
            style(1, "Normal", (0, 0, 0), "0.02"),
            style(4_000_000, "Far", (1, 1, 1), "0.03"),
            style(u32::MAX - 1, "Last", (4_000_000, 4_000_000, 4_000_000), "0.04"),
        ],
    );
    link_shape(&mut doc, 1, 4_000_000);
    link_shape(&mut doc, 2, u32::MAX - 1);
    let back = written_and_read(&doc);
    assert_eq!(shown(&back), shown(&doc));
    assert_eq!(shape_links(&back), shape_links(&doc));
}

/// Two style sheets of one `ID` name no one: refused in words, and nothing is written (the `ID` is what a shape's link names).
#[test]
fn two_style_sheets_of_one_id_are_refused_in_words() {
    let doc = drawing(1, vec![style(1, "Normal", (0, 0, 0), "0.02"), style(6, "Box", (0, 0, 0), "0.03"), style(6, "Again", (0, 0, 0), "0.04")]);
    let Err(WriteError::Unsupported(words)) = write_with(&doc, None, &WriteOptions::default()) else { panic!("a repeated ID is refused") };
    assert!(words.contains("same ID"), "{words}");
    // The root's `ID` is a style sheet's too.
    let doc = drawing(1, vec![style(0, "Second root", (0, 0, 0), "0.03")]);
    let Err(WriteError::Unsupported(words)) = write_with(&doc, None, &WriteOptions::default()) else {
        panic!("a second style sheet of the root's ID is refused")
    };
    assert!(words.contains("same ID"), "{words}");
}

/// A style sheet with no `NameU` is refused in words, as it was: the file says a style by its name and the document file keys its entry by that name.
#[test]
fn a_style_sheet_with_no_name_is_refused_in_words() {
    let mut nameless = style(6, "x", (0, 0, 0), "0.03");
    nameless.name_u = None;
    let doc = drawing(1, vec![style(1, "Normal", (0, 0, 0), "0.02"), nameless]);
    let Err(WriteError::Unsupported(words)) = write_with(&doc, None, &WriteOptions::default()) else {
        panic!("a style sheet with no name is refused")
    };
    assert!(words.contains("needs a name"), "{words}");
}

/// Style sheets of one name (the file holds each style by a name of its own: `Box`, `Box_2`) and one whose name is empty (`Style_<place>`), with a shape on the second of the
/// two that are named alike: the shape's parent names the style the model's link names, and it reads back with the link it had.
#[test]
fn a_shape_names_its_style_sheet_by_the_name_the_file_holds_it_under() {
    let mut doc = drawing(
        3,
        vec![
            style(1, "Box", (0, 0, 0), "0.02"),
            style(6, "Box", (0, 0, 0), "0.03"),
            style(7, "", (0, 0, 0), "0.04"),
            style(9, "Other", (6, 6, 6), "0.05"),
        ],
    );
    link_shape(&mut doc, 1, 1);
    link_shape(&mut doc, 2, 6);
    link_shape(&mut doc, 3, 7);
    let back = written_and_read(&doc);
    assert_eq!(shape_links(&back), shape_links(&doc), "each shape on the style sheet it was on");
    assert_eq!(shown(&back), shown(&doc));
}

/// **A save into the file written** (the splice writer): the model read from the file has the numbering it had, the save of it is no edit, an edit of a shape is written, and a style
/// sheet added with the next `ID` (the greatest and one, as the engine numbers one) is written and read back where it stands. It would have been refused: the baseline's `ID`s are not its places.
#[test]
fn a_file_written_from_a_gapped_drawing_is_saved_into_edit_after_edit() {
    let mut doc = gapped(2);
    link_shape(&mut doc, 1, 6);
    link_shape(&mut doc, 2, 30);
    let o = open_bytes(saved(&doc));
    assert_eq!(shown(&o.doc), shown(&doc));
    // Unchanged: no edit, and the same bytes.
    let same = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{e}"));
    assert!(!same.edited && same.bytes == o.bytes, "a save of the model as it was read is no edit");
    // A shape edited (its fill) is written; the numbering is kept.
    let mut edited = o.doc.clone();
    {
        let page = Arc::make_mut(edited.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("FillForegnd"), Cell::constant(Value::from_text("#ff0000")));
    }
    let once = real(&edited, &o.bytes, &o.index).unwrap_or_else(|e| panic!("an edit of a shape of a gapped drawing is refused: {e}"));
    assert!(once.edited);
    let back = open_bytes(once.bytes.clone());
    assert_eq!(shown(&back.doc), shown(&doc));
    assert_eq!(shape_links(&back.doc), shape_links(&doc));
    // A style sheet added with the next ID.
    let mut grown = edited.clone();
    grown.styles.push(Arc::new(style(31, "Added", (30, 30, 30), "0.07")));
    let twice = real(&grown, &o.bytes, &o.index).unwrap_or_else(|e| panic!("a new style sheet of a gapped drawing is refused: {e}"));
    let back = open_bytes(twice.bytes.clone());
    assert_eq!(shown(&back.doc), shown(&grown), "the style sheet added is read where it was put, with the ID it has");
    let third = real(&back.doc, &back.bytes, &back.index).unwrap();
    assert!(!third.edited, "and the file as it is read is no edit");
}

/// The file's own numbering is the place for a file that has no gap: a new style sheet whose `ID` is not the next place is the splice writer's old refusal.
#[test]
fn a_file_with_no_gap_keeps_the_splice_writers_rule_for_a_new_style_sheet() {
    let doc = drawing(1, vec![style(1, "Normal", (0, 0, 0), "0.02")]);
    let o = open_bytes(saved(&doc));
    let mut grown = o.doc.clone();
    grown.styles.push(Arc::new(style(9, "Added", (0, 0, 0), "0.07")));
    let Err(WriteError::Unsupported(words)) = real(&grown, &o.bytes, &o.index) else {
        panic!("a new style sheet whose ID is not its place is refused for a file with none")
    };
    assert!(words.contains("a new style sheet that is not an empty common style"), "{words}");
    let mut next = o.doc.clone();
    next.styles.push(Arc::new(style(u32::try_from(next.styles.len()).unwrap(), "Added", (0, 0, 0), "0.07")));
    assert!(real(&next, &o.bytes, &o.index).is_ok());
}

/// What a read does with an entry of the document file that gives a style sheet an `ID`, when the file was edited by another program or is hostile: an `ID` another style sheet has is
/// ignored with a warning (the first claim stands), the root keeps its own, one that does not number is ignored, and a style that claims none keeps its place's number unless another
/// holds it, when it is given a number past all the others.
#[test]
fn what_the_reader_does_with_ids_in_the_document_file_that_collide_or_do_not_number() {
    let mut doc = gapped(1);
    link_shape(&mut doc, 1, 30);
    let bytes = saved(&doc);
    let package = diacraft_odg::package::Package::read(&bytes, &Limits::default()).unwrap();
    let file = String::from_utf8_lossy(package.part("DiaCraft/document.xml").unwrap_or_default()).into_owned();
    let edit = |from: &str, to: &str| -> Vec<u8> {
        assert!(file.contains(from), "{from} in {file}");
        replace_part(&bytes, "DiaCraft/document.xml", &file.replacen(from, to, 1))
    };
    let ids = |bytes: &[u8]| -> Vec<u32> { read(bytes, &Limits::default()).unwrap().0.styles.iter().map(|s| s.id).collect() };
    // The numbering as written.
    assert_eq!(ids(&bytes), [0, 1, 2, 6, 7, 30]);
    // `Accent` (7) claims 6, which `Box` holds: the later claim is ignored, and `Accent` takes the number past all the others (31), the place it stands at (4) being free.
    let collide = edit("id=\"7\"", "id=\"6\"");
    let (back, report) = read(&collide, &Limits::default()).unwrap();
    let got: Vec<u32> = back.styles.iter().map(|s| s.id).collect();
    let mut sorted = got.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), got.len(), "every ID is held by one style: {got:?}");
    assert!(report.warnings.iter().any(|w| w.contains("ID")), "{:?}", report.warnings);
    // Not a number: ignored.
    let word = edit("id=\"7\"", "id=\"seven\"");
    let got = ids(&word);
    let mut sorted = got.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), got.len(), "{got:?}");
}

/// The work a save of `doc` counts (the steps of every loop charged to the step cap, the cells compared, the pages and the shapes), and the bytes it made.
fn counted(doc: &Document) -> Result<(u64, usize), WriteError> {
    write_with(doc, None, &WriteOptions::default()).map(|w| (w.work.steps_used(), w.bytes.len()))
}

/// A 60,000-style drawing (the most a drawing holds is 65,536) numbered with gaps costs within a small constant of one with none, **in the work the save counts**, and the file made is as long.
#[test]
fn sixty_thousand_style_sheets_numbered_with_gaps_cost_as_many_with_none() {
    const N: u32 = 60_000;
    let build = |step: u32| {
        let more: Vec<Style> = (1..=N).map(|k| style(k * step, &format!("S{k}"), (0, 0, 0), "0.02")).collect();
        drawing(1, more)
    };
    let (plain, gap) = (counted(&build(1)), counted(&build(7)));
    assert_eq!(plain.is_ok(), gap.is_ok(), "the same fate: {:?} against {:?}", plain.as_ref().err(), gap.as_ref().err());
    let ((yard, plain_bytes), (took, gap_bytes)) = (plain.unwrap(), gap.unwrap());
    println!("60000 styles: {yard} steps and {plain_bytes} bytes with none, {took} steps and {gap_bytes} bytes with gaps");
    assert!(took <= 2 * yard + 1_000, "{took} steps with gaps against {yard} with none");
    // The file holds one entry more for each style sheet whose number is not its place, and the numbers are longer: no more than a few times the file of the plain numbering.
    assert!(gap_bytes <= 2 * plain_bytes, "{gap_bytes} bytes with gaps against {plain_bytes} with none");
}

/// A hostile drawing of 100,000 style sheets numbered far apart is refused as the same drawing numbered plainly is, in the same words and **before anything is made of it**: the most a drawing holds is
/// 65,536 style sheets, said by the plan of the package and not by the read of a file that was made first (it took 2.4 s to be refused when the file was made and read).
#[test]
fn a_hundred_thousand_style_sheets_are_refused_before_anything_is_made_whatever_their_numbers() {
    const N: u32 = 100_000;
    let build = |step: u32| {
        let more: Vec<Style> = (1..=N).map(|k| style(k.wrapping_mul(step), &format!("S{k}"), (0, 0, 0), "0.02")).collect();
        drawing(1, more)
    };
    for step in [1, 40_000] {
        match counted(&build(step)) {
            Err(WriteError::Limit { what, max }) => {
                assert_eq!((what, max), ("style sheets in the drawing", 65_536), "step {step}");
            }
            other => panic!("step {step}: a drawing of more style sheets than a drawing holds is refused as a limit: {other:?}"),
        }
    }
}

/// The clock on the two drawings of many style sheets (by hand, `--ignored --nocapture`): the same drawing numbered plainly and with gaps, written.
#[test]
#[ignore = "a clock, by hand"]
fn the_clock_on_sixty_thousand_style_sheets_with_and_without_gaps() {
    const N: u32 = 60_000;
    for step in [1u32, 7] {
        let more: Vec<Style> = (1..=N).map(|k| style(k * step, &format!("S{k}"), (0, 0, 0), "0.02")).collect();
        let doc = drawing(1, more);
        let start = Instant::now();
        let written = write_with(&doc, None, &WriteOptions::default());
        println!("60000 styles, step {step}: {:?} in {} ms", written.as_ref().err().map(ToString::to_string), start.elapsed().as_millis());
    }
}

/// The text of the part `name` of the package `bytes`.
fn part_text(bytes: &[u8], name: &str) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8_lossy(package.part(name).unwrap_or_default()).into_owned()
}

/// The package `bytes` as a program that does not know DiaCraft's document file writes it: the file, and the row of the manifest that lists it, are gone.
fn without_document_file(bytes: &[u8]) -> Vec<u8> {
    use crate::zipbuild::{Item, build};
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    let items: Vec<Item> = package
        .parts()
        .filter(|part| *part != "DiaCraft/document.xml")
        .map(|part| {
            let mut content = package.part(part).unwrap_or_default().to_vec();
            if part == "META-INF/manifest.xml" {
                let text = String::from_utf8_lossy(&content).into_owned();
                let from = text.find("<manifest:file-entry manifest:full-path=\"DiaCraft/document.xml\"").expect("a row for the file");
                let to = from + text[from..].find("/>").expect("the row ends") + 2;
                content = format!("{}{}", &text[..from], &text[to..]).into_bytes();
            }
            if part == "mimetype" { Item::stored(part, &content) } else { Item::deflated(part, &content) }
        })
        .collect();
    build(&items)
}

/// **Another program's edits.** A program that changes the properties of the common styles and keeps DiaCraft's document file (a text editor, a script) leaves the numbering as it was, and the cells it
/// gave a style are read; a program that does not know the file (LibreOffice) drops it, and the drawing reads numbered by the places of the styles, every link naming the style it named (the ID is the
/// place, and the file's own markup says nothing more: the same as for any `.odg`).
#[test]
fn a_program_that_edits_the_styles_keeps_the_numbering_and_one_that_drops_the_document_file_numbers_by_place() {
    let mut doc = gapped(3);
    link_shape(&mut doc, 1, 6);
    link_shape(&mut doc, 2, 30);
    link_shape(&mut doc, 3, 7);
    let bytes = saved(&doc);
    // The properties of `Box` are edited in `styles.xml`.
    let styles = part_text(&bytes, "styles.xml");
    let empty = r#"<style:style style:name="Box" style:family="graphic" style:parent-style-name="Heading"/>"#;
    assert!(styles.contains(empty), "{styles}");
    let edited = styles.replacen(
        empty,
        r##"<style:style style:name="Box" style:family="graphic" style:parent-style-name="Heading"><style:graphic-properties draw:fill="solid" draw:fill-color="#c4a000"/></style:style>"##,
        1,
    );
    let (back, _) = read(&replace_part(&bytes, "styles.xml", &edited), &Limits::default()).unwrap();
    assert_eq!(back.styles.iter().map(|s| s.id).collect::<Vec<_>>(), [0, 1, 2, 6, 7, 30], "the numbering is kept");
    assert_eq!(shape_links(&back), shape_links(&doc), "and so are the shapes' links");
    assert!(back.styles.get(3).unwrap().sheet.cell("FillForegnd").is_some(), "the cell the edit gave `Box` is read");
    // The document file is dropped: every number is a place.
    let (plain, report) = read(&without_document_file(&bytes), &Limits::default()).unwrap();
    assert_eq!(plain.styles.iter().map(|s| s.id).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
    assert_eq!(
        shape_links(&plain).iter().map(|(id, l)| (*id, l.line)).collect::<Vec<_>>(),
        [(1, Some(3)), (2, Some(5)), (3, Some(4))],
        "each shape is on the style it named, by its place now"
    );
    assert!(report.warnings.iter().all(|w| !w.contains("ID")), "{:?}", report.warnings);
    // A style added after the entries were written (the file's own, by another program) takes its place's number, unless a style holds it, and is read with no entry.
    let added = styles.replacen(
        "</office:styles>",
        r#"<style:style style:name="Extra" style:family="graphic"/><style:style style:name="Extra2" style:family="graphic"/></office:styles>"#,
        1,
    );
    let (more, _) = read(&replace_part(&bytes, "styles.xml", &added), &Limits::default()).unwrap();
    assert_eq!(
        more.styles.iter().map(|s| s.id).collect::<Vec<_>>(),
        [0, 1, 2, 6, 7, 30, 31, 32],
        "the numbers past the greatest: their places (6 and 7) are held by `Box` and `Accent`"
    );
}

/// What a read of `bytes` charges: its items.
fn items_of(bytes: &[u8]) -> u64 {
    read(bytes, &Limits::default()).map(|(_, report)| report.work.items).unwrap_or_else(|e| panic!("{e}"))
}

/// **The walk that maps the links is one visit for each shape, charged** (the reader's cost of a gap): a drawing of 4,000 shapes on style sheets numbered with gaps costs the reader its items for the
/// same drawing numbered by its places and a visit for each of its shapes, its pages and its styles, and no more.
#[test]
fn the_walk_that_maps_the_links_costs_a_visit_for_each_shape_and_no_more() {
    const SHAPES: u32 = 4_000;
    let build = |step: u32| {
        let more: Vec<Style> = (1..=8).map(|k| style(k * step, &format!("S{k}"), (0, 0, 0), "0.02")).collect();
        let mut doc = drawing(SHAPES, more);
        for id in 1..=SHAPES {
            link_shape(&mut doc, id, (1 + id % 8) * step);
        }
        doc
    };
    let (plain, gap) = (saved(&build(1)), saved(&build(5)));
    let (a, b) = (items_of(&plain), items_of(&gap));
    let extra = b.saturating_sub(a);
    assert!(extra >= u64::from(SHAPES), "a visit for each shape is charged: {a} items with none, {b} with gaps");
    assert!(extra <= u64::from(SHAPES) + 200, "and no more than that and the entries: {a} items with none, {b} with gaps");
    // The model the gapped file reads as has every shape on the style sheet it was on.
    let back = read(&gap, &Limits::default()).unwrap().0;
    let on: Vec<u32> = back.pages.get(0).unwrap().shapes.iter().map(|s| s.sheet.links.line.unwrap_or(u32::MAX)).collect();
    assert!(on.iter().enumerate().all(|(k, link)| *link == (1 + (k as u32 + 1) % 8) * 5), "{:?}", &on[..8]);
}

/// A document file of a hundred thousand entries that each give an `ID` (a hostile file, or one another program made of a drawing of many styles) costs the reader in proportion to the entries, in the
/// items it counts, and is read by the same rules as a small one: an entry for one style is the first claim's, the others are ignored.
#[test]
fn a_hundred_thousand_entries_that_claim_ids_cost_the_reader_in_proportion_to_their_number() {
    let doc = gapped(1);
    let bytes = saved(&doc);
    let file = part_text(&bytes, "DiaCraft/document.xml");
    let read_with = |entries: usize| {
        let many: String = (0..entries).map(|k| format!("<diacraft:style name=\"Accent\" id=\"{}\"/>", 100 + k)).collect();
        let hostile =
            replace_part(&bytes, "DiaCraft/document.xml", &file.replacen("</diacraft:document>", &format!("{many}</diacraft:document>"), 1));
        let (got, report) = diacraft_odg::read_partial(&hostile, &Limits::default());
        (got.map(|d| d.styles.iter().map(|s| s.id).collect::<Vec<_>>()), report.work.items)
    };
    let (small, few) = read_with(1_000);
    let (none, zero) = read_with(0);
    let (big, many) = read_with(100_000);
    assert!(small.is_ok() && none.is_ok(), "{small:?} {none:?}");
    // Each entry costs the items of its own and a few more: a hundred times the entries is about a hundred times the items, and never the square of them.
    let (per_small, per_big) = ((few - zero) / 1_000, many.saturating_sub(zero) / 100_000);
    println!("entries: {zero} items with none, {few} with 1,000 ({per_small} each), {many} with 100,000 ({per_big} each)");
    assert!(per_big <= 2 * per_small + 2, "{per_big} items for each of 100,000 entries against {per_small} for each of 1,000");
    match &big {
        Ok(ids) => assert_eq!(ids, small.as_ref().unwrap(), "the first claim stands, however many follow"),
        Err(e) => assert_eq!(e.class(), "limit", "a file past the items the reader holds is refused as a limit: {e}"),
    }
}

/// The check of the document file a save makes charges what the reader charges for the walk, so that a file the reader would refuse for its items is never written: the least number of items the
/// reader reads the file at is the least the writer writes it at (the check is made with the reader's own functions: `ReadCheck`).
#[test]
fn a_save_is_refused_for_items_exactly_where_the_reader_would_refuse_the_file_it_made() {
    let mut doc = gapped(40);
    for id in 1..=40 {
        link_shape(&mut doc, id, [1, 2, 6, 7, 30][id as usize % 5]);
    }
    let reads_at = |items: usize| read(&saved(&doc), &Limits { max_items: items, ..Limits::default() }).is_ok();
    let saves_at = |items: usize| {
        write_with(&doc, None, &WriteOptions { limits: Limits { max_items: items, ..Limits::default() }, ..WriteOptions::default() }).is_ok()
    };
    // The least number of items that reads the file the default limits made.
    let (mut low, mut high) = (0usize, usize::try_from(items_of(&saved(&doc))).unwrap() + 64);
    assert!(reads_at(high));
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        if reads_at(mid) { high = mid } else { low = mid }
    }
    assert!(saves_at(high), "a save at the items the reader needs");
    assert!(!saves_at(high - 1), "a save at one item fewer is refused, as the reader would refuse the file it made");
}

/// The numberings the synthetic patterns of the task stand for: a VSDX file's style sheets are numbered by the `ID`s it was made with, so that taking sheets out of a drawing, or adding them in another
/// order, leaves gaps of any width and any order. The root is first and 0.
fn patterns() -> Vec<(&'static str, Vec<u32>)> {
    let mut lcg = 0x9E37_79B9_7F4A_7C15u64;
    let mut random: Vec<u32> = vec![0];
    for _ in 0..300 {
        lcg = lcg.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let id = u32::try_from(lcg >> 40).unwrap_or(1).max(1);
        if !random.contains(&id) {
            random.push(id);
        }
    }
    vec![
        ("no gap", vec![0, 1, 2]),
        ("no gap, ten", (0..10).collect()),
        ("the first one missing", vec![0, 2]),
        ("the task's", vec![0, 1, 2, 6, 7, 30]),
        ("a gap after the root", vec![0, 3, 4, 5, 6, 7, 8]),
        ("a gap after the first", vec![0, 1, 3, 4, 5, 6]),
        ("primes", vec![0, 2, 3, 5, 7, 11, 13, 17, 19, 23]),
        ("hundreds", vec![0, 100, 200, 300]),
        ("every third", (0..34).map(|k| k * 3).collect()),
        ("every other", (0..60).map(|k| k * 2).collect()),
        ("far apart", vec![0, 4_000_000]),
        ("the greatest", vec![0, 1, u32::MAX]),
        ("descending", vec![0, 30, 7, 6, 2, 1]),
        ("unordered", vec![0, 5, 3, 9, 4, 8]),
        ("one hundred and fifty at every seventh", (0..150).map(|k| k * 7).collect()),
        ("three hundred at random", random),
    ]
}

/// A drawing numbered as `ids` says (the first is the root): each style sheet follows the one before it, but for every fourth, which has three links that differ and one that is the root's, with a shape
/// on every style sheet.
fn numbered(ids: &[u32]) -> Document {
    let more: Vec<Style> = ids
        .iter()
        .enumerate()
        .skip(1)
        .map(|(at, id)| {
            let before = ids.get(at - 1).copied().unwrap_or(0);
            let links = if at % 4 == 0 { (before, ids.get(at / 2).copied().unwrap_or(0), 0) } else { (before, before, before) };
            style(*id, &format!("Style {at}"), links, "0.02")
        })
        .collect();
    let mut doc = drawing(u32::try_from(ids.len()).unwrap_or(0), more);
    for (at, id) in ids.iter().enumerate() {
        link_shape(&mut doc, u32::try_from(at + 1).unwrap_or(0), *id);
    }
    doc
}

/// **The patterns cross.** Each synthetic numbering is written by the model writer and read back with every style sheet and every shape's link as it was: the count of those that cross is printed on a
/// `PATTERNS` line (before this task only the ones with no gap did).
#[test]
fn every_numbering_pattern_is_written_and_read_back_whole() {
    let mut crossed = 0usize;
    let all = patterns();
    for (what, ids) in &all {
        let doc = numbered(ids);
        let ok = match write_with(&doc, None, &WriteOptions::default()) {
            Ok(written) => match read(&written.bytes, &Limits::default()) {
                Ok((back, _)) => shown(&back) == shown(&doc) && shape_links(&back) == shape_links(&doc),
                Err(_) => false,
            },
            Err(_) => false,
        };
        println!("PATTERN {what}: {} style sheets, {}", ids.len(), if ok { "crossed" } else { "refused or not read back whole" });
        crossed += usize::from(ok);
    }
    println!("PATTERNS crossed {crossed} of {}", all.len());
    assert_eq!(crossed, all.len());
}

/// Writes the files a LibreOffice run and the byte comparison of the task are given into the folder `STYLEIDS_LO_DIR` names (by hand, with `--ignored`): the model writer's files for numberings with no
/// gap (which must be the same bytes before and after this task) and with gaps (shapes on style sheets, with no look of their own, so that a program that draws a shape by its style shows the style's).
#[test]
#[ignore = "writes files for a LibreOffice run, by hand"]
fn lo_files() {
    let Some(folder) = std::env::var_os("STYLEIDS_LO_DIR") else { return };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let put = |name: &str, doc: &Document| match write_with(doc, None, &WriteOptions::default()) {
        Ok(written) => std::fs::write(folder.join(name), written.bytes).unwrap(),
        Err(e) => println!("NOT WRITTEN {name}: {e}"),
    };
    for (what, ids) in patterns().iter().filter(|(what, _)| what.starts_with("no gap")) {
        put(&format!("nogap-{}.odg", what.replace([' ', ','], "")), &numbered(ids));
    }
    put("nogap-blank.odg", &blank(3));
    // Shapes on style sheets 6, 30 and 7 that hold no look of their own, and the style sheets that give it.
    let mut doc = gapped(3);
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
            let sheet = Arc::make_mut(&mut shape.sheet);
            for name in ["FillPattern", "FillForegnd", "LinePattern", "LineColor", "LineWeight"] {
                sheet.cells.remove(name);
            }
        }
    }
    link_shape(&mut doc, 1, 6);
    link_shape(&mut doc, 2, 30);
    link_shape(&mut doc, 3, 7);
    put("gap-linked.odg", &doc);
}
