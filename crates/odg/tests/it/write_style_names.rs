//! The review (`task-ODG.NEWSAVE-review.md`): the names a style sheet of the file is held by, a style sheet the model no longer has,
//! the cells of a style sheet the file's markup says nothing of, and the characters an `NCName` forbids. Every case saves a model, reads the bytes back with the
//! reader and compares what came back with what went in, and saves again, which must be no edit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Links, Value};
use diacraft_odg::{Limits, WriteError, Written, read};

use crate::drawing::Drawing;
use crate::write_core::{Open, custom, open, open_bytes, part, real};
use crate::write_styles_new::style_sheet;

const PATH: &str = "DiaCraft/document.xml";

fn all(id: u32) -> Links {
    Links { line: Some(id), fill: Some(id), text: Some(id) }
}

fn two_shapes() -> Open {
    open(&Drawing::a4().page(&[custom("s1", "1cm"), custom("s2", "8cm")].concat()))
}

/// The model of `o` read again from what `saved` made: a session of its own on the file.
fn reopened(saved: &Written) -> Open {
    open_bytes(saved.bytes.clone())
}

fn link(o: &mut Open, shape: usize, to: Links) {
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let s = Arc::make_mut(page.shapes.get_mut(shape).unwrap());
    Arc::make_mut(&mut s.sheet).links = to;
}

fn links_of(doc: &Document, shape: usize) -> (Option<u32>, Option<u32>, Option<u32>) {
    let l = doc.pages.get(0).unwrap().shapes.get(shape).unwrap().sheet.links;
    (l.line, l.fill, l.text)
}

fn refusal(o: &Open) -> String {
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped what the model holds: {} bytes", saved.bytes.len()),
    }
}

#[track_caller]
fn saves_again_as_no_edit(o: &Open, saved: &Written) {
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
    let reread = reopened(saved);
    let third = real(&reread.doc, &reread.bytes, &reread.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes, "nor is a save of the model the file reads as");
}

// ----: the name the file holds a style by ------------------------------------------------------------------------------------------

/// A style sheet `My Theme` the file holds as `My_20_Theme` is read with the `NameU` `My Theme` (the document file gives it back). **A shape relinked to it has the style the
/// file holds as its parent**, `My_20_Theme`, and not `My Theme`, a name no style has (the review: the file was saved, read back with the root's links and a warning, and the save
/// said nothing). A child style of it names it by the file's name too. The links read back from the file are the model's.
#[test]
fn a_parent_is_named_by_the_name_the_file_holds_the_style_by_and_not_by_the_nameu_the_document_file_restored() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "My Theme", 0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(part(&first.bytes, "styles.xml").contains(r#"style:name="My_20_Theme""#), "the file holds it by the name");
    let mut o = reopened(&first);
    assert_eq!(
        o.doc.styles.get(n as usize).unwrap().name_u.as_deref(),
        Some("My Theme"),
        "the document file gives the model the NameU back: the premise of the case"
    );
    // A shape relinked to it.
    link(&mut o, 0, all(n));
    let second = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let content = part(&second.bytes, "content.xml");
    assert_eq!(content.matches("style:parent-style-name=\"My_20_Theme\"").count(), 1, "the file's own name is the parent: {content}");
    assert!(!content.contains("style:parent-style-name=\"My Theme\""), "{content}");
    let (back, report) = read(&second.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(links_of(&back, 0), (Some(n), Some(n), Some(n)), "the shape reads back on the style");
    assert_eq!(links_of(&back, 1), (Some(0), Some(0), Some(0)));
    saves_again_as_no_edit(&o, &second);
    // A child of it.
    let mut o = reopened(&second);
    o.doc.styles.push(Arc::new(style_sheet(n + 1, "Child", n)));
    let third = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let styles = part(&third.bytes, "styles.xml");
    assert!(styles.contains(r#"<style:style style:name="Child" style:family="graphic" style:parent-style-name="My_20_Theme"/>"#), "{styles}");
    let (back, report) = read(&third.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(back.styles.get(n as usize + 1).unwrap().sheet.links, all(n), "the child reads back on its parent");
    saves_again_as_no_edit(&o, &third);
}

/// A second style sheet called `My Theme` is the clash it is: the file holds the first by `My_20_Theme`, which is the name R11 gives the second. It was saved (the names the model
/// had, `My Theme` and `My_20_Theme`, did not meet) and the file held two styles of one name.
#[test]
fn a_second_style_that_would_be_written_under_the_name_the_file_holds_one_by_is_refused() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "My Theme", 0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let mut o = reopened(&first);
    o.doc.styles.push(Arc::new(style_sheet(n + 1, "My Theme", 0)));
    assert!(refusal(&o).contains("a new style sheet that is not an empty common style"));
}

/// What the save wrote the parents of is read back: a relink alone makes the file be read again and its links compared (the review's mutant M32 deleted the call that does it
/// for the new style sheets, and nothing failed): the bytes read again are counted, and a save that writes no parent and no style sheet reads nothing again.
#[test]
fn a_save_that_writes_a_parent_or_a_style_sheet_reads_the_file_back_and_a_save_that_writes_none_does_not() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    let added = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(added.work.read_back_bytes, added.bytes.len() as u64, "a style sheet added is read back");
    assert!(added.work.cells_compared >= 1);
    let mut o = reopened(&added);
    link(&mut o, 0, all(n));
    let relinked = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(relinked.work.read_back_bytes, relinked.bytes.len() as u64, "a parent written is read back, with no style sheet added");
    // A move is neither.
    let mut o = reopened(&relinked);
    let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
    crate::write_core::move_by(&mut o.doc, 0, id, 0.5, 0.0);
    let moved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(moved.edited);
    assert_eq!(moved.work.read_back_bytes, 0, "a save that writes no parent and no style sheet reads nothing again");
}

// ----: the characters of a name ----------------------------------------------------------------------------------------------------

/// `ª`, `µ`, `²` and `½` are letters and digits to a program's `is_alphanumeric` and are no `NCName` characters (XML 1.0 gives a name neither the letters of the Latin-1
/// block before U+00C0 nor the digit-like numbers): each is written `_`, its code point in upper-case hexadecimal and `_`, and the name is the one the reader keeps. Letters of
/// other scripts stay.
#[test]
fn the_characters_an_ncname_forbids_that_a_program_calls_letters_and_digits_are_escaped() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    for (at, name) in ["a\u{AA}", "\u{B5}b", "x\u{B2}", "y\u{BD}z", "\u{E9}t\u{E9}", "\u{3b1}\u{3b2}"].into_iter().enumerate() {
        o.doc.styles.push(Arc::new(style_sheet(n + at as u32, name, 0)));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let styles = part(&saved.bytes, "styles.xml");
    for written in ["a_AA_", "_B5_b", "x_B2_", "y_BD_z", "\u{E9}t\u{E9}", "\u{3b1}\u{3b2}"] {
        assert!(styles.contains(&format!("style:name=\"{written}\"")), "{written}: {styles}");
    }
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(back.styles.len(), o.doc.styles.len());
    // The names the model has are the ones it had (the document file gives each back), so a second save is no edit.
    for (at, name) in ["a\u{AA}", "\u{B5}b", "x\u{B2}", "y\u{BD}z"].into_iter().enumerate() {
        assert_eq!(back.styles.get(n as usize + at).unwrap().name_u.as_deref(), Some(name));
    }
    saves_again_as_no_edit(&o, &saved);
}

// ----: a style sheet the model no longer has -----------------------------------------------------------------------------------------

/// A drawing saved with a style sheet added and a shape on it, and the model as an undo leaves it: the style sheet gone and the shape on the root. The save takes the empty
/// common style out of `office:styles` and the shape's parent out of its style; the document file holds no entry for it; the file reads with the style sheets the model has;
/// a second save is no edit.
#[test]
fn a_style_sheet_the_model_no_longer_has_is_taken_out_of_the_file_with_its_entry_and_the_shapes_parent() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    link(&mut o, 0, all(n));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(
        part(&saved.bytes, "styles.xml").contains("style:name=\"Theme\"") && part(&saved.bytes, PATH).contains("<diacraft:style name=\"Theme\">")
    );
    let before = reopened(&saved);
    // The undo: the style sheet and the link are as they were before the add.
    let mut undone = reopened(&saved);
    undone.doc.styles = diacraft_doc::SharedVec::from_vec(undone.doc.styles.iter().take(n as usize).cloned().collect());
    link(&mut undone, 0, all(0));
    let second = real(&undone.doc, &undone.bytes, &undone.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(second.edited);
    let styles = part(&second.bytes, "styles.xml");
    assert!(!styles.contains("Theme"), "the empty common style is out of office:styles: {styles}");
    assert!(!part(&second.bytes, "content.xml").contains("Theme"), "and no shape names it");
    // The document file held nothing but that entry: it goes with its manifest row (the model carries nothing else).
    assert!(
        !crate::write_core::has_part(&second.bytes, PATH) || !part(&second.bytes, PATH).contains("Theme"),
        "and the document file holds no entry for it"
    );
    let (back, report) = read(&second.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(back.styles.len(), n as usize);
    assert_eq!(links_of(&back, 0), (Some(0), Some(0), Some(0)));
    assert_eq!(before.doc.styles.len(), n as usize + 1);
    assert_eq!(second.work.read_back_bytes, second.bytes.len() as u64, "a style taken out is read back");
    saves_again_as_no_edit(&undone, &second);
    // The redo: the style sheet and the link are back; the save puts them in again, as the first did.
    let third = real(&before.doc, &second.bytes, &second.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(third.edited && part(&third.bytes, "styles.xml").contains("style:name=\"Theme\""));
    saves_again_as_no_edit(&before, &third);
}

/// What cannot be taken out is refused in words that say an undo took it back: a style sheet whose markup holds a property (a style the file made by another program, which a save does not
/// write), one that is not the last of the file's, and the root. Nothing is written.
#[test]
fn a_style_sheet_that_cannot_be_taken_out_of_the_file_is_refused_in_words_that_name_the_undo() {
    let drawing = Drawing::a4()
        .common(
            r##"<style:style style:name="Kept" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>
    <style:style style:name="Empty" style:family="graphic" style:display-name="Empty one">
      <style:graphic-properties/>
      <style:text-properties/>
    </style:style>
  "##,
        )
        .page(&custom("s1", "1cm"));
    let o = open(&drawing);
    let ids: Vec<(u32, Option<String>)> = o.doc.styles.iter().map(|s| (s.id, s.name_u.as_ref().map(ToString::to_string))).collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    let without = |kept: &[u32]| {
        let mut m = Open { doc: o.doc.clone(), bytes: o.bytes.clone(), index: o.index.clone() };
        m.doc.styles = diacraft_doc::SharedVec::from_vec(m.doc.styles.iter().filter(|s| kept.contains(&s.id)).cloned().collect());
        m
    };
    // The last, empty: taken out.
    let ok = real(&without(&[0, 1]).doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let styles = part(&ok.bytes, "styles.xml");
    assert!(!styles.contains("style:name=\"Empty\"") && styles.contains("style:name=\"Kept\""), "{styles}");
    // The whole element goes, with its children and its closing tag, and what is left is a file that reads with two style sheets and no word.
    assert_eq!(styles.matches("</style:style>").count(), 1, "no closing tag of the style taken out is left: {styles}");
    assert!(!styles.contains("<style:text-properties/>"), "{styles}");
    let (back, report) = read(&ok.bytes, &Limits::default()).unwrap();
    assert_eq!((back.styles.len(), report.warnings), (2, Vec::<String>::new()));
    // The one with a property: refused, with the words that say to redo.
    let words = refusal(&without(&[0]));
    assert!(words.contains("taken back by an undo"), "{words}");
    // One that is not the last of the file's is a change of a style sheet of the file, which is not saved.
    let words = refusal(&without(&[0, 2]));
    assert!(words.contains("a change of a style sheet the file holds"), "{words}");
    // The root is never taken.
    let mut none = Open { doc: o.doc.clone(), bytes: o.bytes.clone(), index: o.index.clone() };
    none.doc.styles = diacraft_doc::SharedVec::from_vec(Vec::new());
    assert!(refusal(&none).contains("taken back by an undo"));
}

// ----: the cells of a style sheet the file's markup says nothing of -------------------------------------------------------------

/// A LibreOffice save keeps an empty common style and drops the document file: the `Theme` style is in the file with no cell. **Its sheet may be filled in again**: the document
/// file carries the cells as it does for a style the save adds, the file reads back with them, and a second save is no edit. A style whose markup holds a property still may not
/// be changed, and neither may the names or the links of an empty one.
#[test]
fn the_cells_of_an_empty_common_style_the_file_holds_are_carried_by_the_document_file_and_no_other_change_of_a_style_is_saved() {
    let drawing = Drawing::a4()
        .common(
            r##"<style:style style:name="Theme" style:family="graphic"/><style:style style:name="Heavy" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##,
        )
        .page(&custom("s1", "1cm"));
    let mut o = open(&drawing);
    let (theme, heavy) = (1usize, 2usize);
    assert_eq!(o.doc.styles.get(theme).unwrap().name_u.as_deref(), Some("Theme"));
    assert!(o.doc.styles.get(theme).unwrap().sheet.cells.is_empty());
    let filled = style_sheet(1, "Theme", 0);
    let set = |o: &mut Open, at: usize, style: diacraft_doc::Style| {
        o.doc.styles = diacraft_doc::SharedVec::from_vec(
            o.doc.styles.iter().enumerate().map(|(i, s)| if i == at { Arc::new(style.clone()) } else { Arc::clone(s) }).collect(),
        );
    };
    set(&mut o, theme, filled.clone());
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let file = part(&saved.bytes, PATH);
    assert!(file.contains("<diacraft:style name=\"Theme\">") && file.contains("THEMEVAL()"), "{file}");
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(r#"<style:style style:name="Theme" style:family="graphic"/>"#), "the markup is as it was: {styles}");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    // The style sheet as read holds the `.odg` data of its slot, which the model's does not: the rest is the same.
    let bare = |s: &diacraft_doc::Style| diacraft_doc::Style { ext: Default::default(), ..s.clone() };
    assert_eq!(bare(back.styles.get(theme).unwrap()), bare(&filled));
    assert_eq!(saved.work.read_back_bytes, saved.bytes.len() as u64, "a sheet carried is read back");
    saves_again_as_no_edit(&o, &saved);
    // Refused: a cell of a style whose markup holds a property; the links of an empty one; its name.
    let mut changed = open(&drawing);
    let mut with_cell = (**changed.doc.styles.get(heavy).unwrap()).clone();
    Arc::make_mut(&mut with_cell.sheet).cells.insert(CellName::new("LineWeight"), Cell::constant(Value::from_text("0.5")));
    set(&mut changed, heavy, with_cell);
    assert!(refusal(&changed).contains("a change of a style sheet the file holds"));
    let mut relinked = open(&drawing);
    let mut moved = filled.clone();
    Arc::make_mut(&mut moved.sheet).links = all(2);
    set(&mut relinked, theme, moved);
    assert!(refusal(&relinked).contains("a change of a style sheet the file holds"));
    let mut renamed = open(&drawing);
    let mut other = filled.clone();
    other.name_u = Some(Arc::from("Other"));
    set(&mut renamed, theme, other);
    assert!(refusal(&renamed).contains("a change of a style sheet the file holds"));
    // And the root's cells are never carried this way: the document file holds no entry for the root.
    let mut root = open(&drawing);
    let mut changed_root = (**root.doc.styles.get(0).unwrap()).clone();
    Arc::make_mut(&mut changed_root.sheet).cells.insert(CellName::new("LineWeight"), Cell::constant(Value::from_text("0.5")));
    set(&mut root, 0, changed_root);
    assert!(refusal(&root).contains("a change of"));
}

// ----: what the review's mutants left alive ------------------------------------------------------------------------------------------

/// M13 (`style_entries`' `entry ||`): a new style sheet whose `NameU` is not the name R11 writes and which holds no cell has an entry of the document file for its `NameU`
/// alone. The entry is written, the `NameU` is read back, and the second save is no edit. (With the condition gone the entry was left out, and the only thing that stopped
/// the save was the check of the bytes made.)
#[test]
fn a_style_sheet_with_no_cell_whose_nameu_is_not_its_file_name_has_an_entry_for_the_nameu_alone() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    let bare = diacraft_doc::Style {
        id: n,
        name: Some(Arc::from("A B")),
        name_u: Some(Arc::from("A B")),
        sheet: Arc::new(diacraft_doc::Sheet { links: all(0), ..diacraft_doc::Sheet::default() }),
        ..diacraft_doc::Style::default()
    };
    o.doc.styles.push(Arc::new(bare));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let file = part(&saved.bytes, PATH);
    assert!(file.contains("<diacraft:style name=\"A_20_B\" name-u=\"A B\"/>"), "the entry holds the NameU alone: {file}");
    let back = reopened(&saved);
    assert_eq!(back.doc.styles.get(n as usize).unwrap().name_u.as_deref(), Some("A B"));
    saves_again_as_no_edit(&o, &saved);
}

/// M17 (`plan`'s `!style.passthrough.is_empty()`), closed: a new style sheet that keeps a node of the model's has a place to say it, DiaCraft's document file, and is saved
/// with the node, which a read gives back.
#[test]
fn a_new_style_sheet_that_keeps_a_node_is_saved_and_the_node_is_read_back() {
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    let mut style = style_sheet(n, "Theme", 0);
    style.passthrough.push(0, diacraft_doc::RawNode::Comment(Arc::from("kept")));
    style.raw.push("IsCustomName", "1");
    o.doc.styles.push(Arc::new(style));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let theme = back.styles.get(usize::try_from(n).unwrap()).unwrap();
    assert_eq!(theme.passthrough.len(), 1, "the node rides");
    assert_eq!(theme.raw.get("IsCustomName"), Some("1"), "and so does the attribute");
}

/// M33 (`names_by_id`'s charge): the names a parent is written with are made for each style sheet, and the save counts them: a drawing of 200 more style sheets costs 200 more
/// steps when a shape's parent changes than when a shape moves (on top of the two looks at the style sheets that every save of a model with styles makes).
#[test]
fn the_names_of_the_style_sheets_a_parent_is_written_with_are_counted_by_the_sheet() {
    let styles: String = (0..200).map(|k| format!(r#"<style:style style:name="S{k}" style:family="graphic"/>"#)).collect();
    let drawing = Drawing::a4().common(&styles).page(&[custom("s1", "1cm"), custom("s2", "8cm")].concat());
    let with_styles = open(&drawing);
    let relinked = |mut o: Open| {
        link(&mut o, 0, all(1));
        real(&o.doc, &o.bytes, &o.index).unwrap().work.cells_compared
    };
    let moved = |mut o: Open| {
        let id = o.doc.pages.get(0).unwrap().shapes.get(0).unwrap().id;
        crate::write_core::move_by(&mut o.doc, 0, id, 0.5, 0.0);
        real(&o.doc, &o.bytes, &o.index).unwrap().work.cells_compared
    };
    let with_names = relinked(with_styles) - moved(open(&drawing));
    // The same two saves of a drawing with one style sheet less than the 201 cost the names of one: the difference is the 200.
    let few = {
        let few_drawing = Drawing::a4()
            .common(r#"<style:style style:name="S0" style:family="graphic"/>"#)
            .page(&[custom("s1", "1cm"), custom("s2", "8cm")].concat());
        relinked(open(&few_drawing)) - moved(open(&few_drawing))
    };
    assert!(with_names >= few + 199, "200 more style sheets cost {with_names} against {few}: the names are counted for each");
}

/// M34 (`put_common_styles`' charges): each attribute a new style sheet's element is written with is charged: the name and the family, the display name and the parent, so
/// that a save of a hundred of them is held to the most attributes a save writes.
#[test]
fn the_attributes_of_the_elements_of_new_style_sheets_are_charged_each() {
    let attributes = |count: u32, display: bool, parent: bool| {
        let mut o = two_shapes();
        let n = u32::try_from(o.doc.styles.len()).unwrap();
        for k in 0..count {
            let name = if display { format!("My Theme {k}") } else { format!("T{k}") };
            let mut style = style_sheet(n + k, &name, if parent && k > 0 { n } else { 0 });
            if display {
                style.name = Some(Arc::from(format!("Shown {k}").as_str()));
            }
            o.doc.styles.push(Arc::new(style));
        }
        real(&o.doc, &o.bytes, &o.index).unwrap().work.attributes_written
    };
    // 10 more styles: a name and a family each (2), and the display name when the name differs (+1), and the parent when it has one (+1).
    assert_eq!(attributes(11, false, false) - attributes(1, false, false), 10 * 2, "the name and the family");
    assert_eq!(attributes(11, true, false) - attributes(1, true, false), 10 * 3, "and the display name");
    assert_eq!(attributes(11, false, true) - attributes(1, false, true), 10 * 3, "and the parent (the first has none: 10 of the 11 do)");
}

/// A new shape on a style sheet the file already holds is read back (its parent is written, and read again with the links the model has) and a new shape on the root is not:
/// the save of a drawing that is themed reads the file once when it puts a shape on the theme, and not when it puts one on none.
#[test]
fn a_new_shape_on_a_style_sheet_the_file_holds_is_read_back_and_one_on_the_root_is_not() {
    use crate::write_new::{push_shape, rectangle};
    let mut o = two_shapes();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    o.doc.styles.push(Arc::new(style_sheet(n, "Theme", 0)));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    // On the style: read back.
    let mut on_style = reopened(&first);
    let id = on_style.index.next_id(0).unwrap();
    let mut shape = rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0));
    Arc::make_mut(&mut shape.sheet).links = all(n);
    push_shape(&mut on_style.doc, shape);
    let saved = real(&on_style.doc, &on_style.bytes, &on_style.index).unwrap();
    assert_eq!(saved.work.read_back_bytes, saved.bytes.len() as u64);
    // On the root: not.
    let mut on_root = reopened(&first);
    let id = on_root.index.next_id(0).unwrap();
    push_shape(&mut on_root.doc, rectangle(id, "Rectangle.1", (2.0, 3.0, 4.0, 2.0)));
    let saved = real(&on_root.doc, &on_root.bytes, &on_root.index).unwrap();
    assert!(saved.edited);
    assert_eq!(saved.work.read_back_bytes, 0, "a shape on the root has no parent to read back");
}
