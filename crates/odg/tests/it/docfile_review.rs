//! On the review: the findings that sat in the document file's reader and writer, each pinned.
//!
//! * **Reading never fails the drawing for what the document file holds** (the design's 7.3): a master past a cap of the model is left out with a word.
//! * **A save over a file of another version writes the version it writes**, and says that it replaced a file of an older one.
//! * **The writer stops counting the file's bytes at the cap**, and counts every unit the review found uncounted (blobs, entry sheets, steps, the pictures in the estimate).
//! * **The order and the costs of the reader's loops** that a mutation of the old tests left alone.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, ForeignData, Master, Media, Shape, ShapeList, ShapeType, Value};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, Written, read, write_counted};

use crate::docfile::base;
use crate::docfile_codec::part_text;
use crate::docfile_cost::{file_of, package_with};
use crate::write_core::{Open, open_bytes, real};

/// A save of `doc` over `o`'s file, with the work it counted.
fn save(doc: &diacraft_doc::Document, o: &Open, work: &mut WriteWork) -> Result<Written, WriteError> {
    write_counted(doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), work)
}

/// A master of one shape named `name`, whose sheet holds `cells` cells.
fn master_of(id: u32, cells: u32) -> Master {
    let mut s = Shape::new(1);
    let sheet = Arc::make_mut(&mut s.sheet);
    for c in 0..cells {
        sheet.cells.insert(CellName::new(&format!("C{c}")), Cell::constant(Value::from_text("1")));
    }
    Master { id, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }
}

// ------------------------------------------------------------------------------------------------------------------ reading never fails the drawing

/// A master whose shapes nest past the model's 200 levels, and one whose kept element is deeper than the codec keeps one, are each damaged and left out with a word: the
/// master beside them is read, and the drawing is.
#[test]
fn a_master_past_a_cap_of_the_model_is_left_out_with_a_word_and_the_drawing_is_read() {
    let kept = format!("{}{}", "<diacraft:el name=\"k\">".repeat(250), "</diacraft:el>".repeat(250));
    let deep_kept = format!("<diacraft:master id=\"1\"><diacraft:shape id=\"1\">{kept}</diacraft:shape></diacraft:master>");
    let shapes = format!("{}{}", "<diacraft:shape id=\"1\">".repeat(201), "</diacraft:shape>".repeat(201));
    let deep_shapes = format!("<diacraft:master id=\"1\">{shapes}</diacraft:master>");
    for (what, bad) in [("a kept element 250 deep", deep_kept), ("shapes nested 201 deep", deep_shapes)] {
        let file = file_of(&format!("{bad}<diacraft:master id=\"2\"/>"));
        let (doc, report) =
            read(&package_with(&file), &Limits::default()).unwrap_or_else(|e| panic!("{what}: the drawing was refused: {} ({e})", e.class()));
        assert_eq!(doc.masters.iter().map(|m| m.id).collect::<Vec<_>>(), [2], "{what}: the master beside it is read, and it is left out");
        assert_eq!(report.warned("diacraft_file_unreadable"), 1, "{what}: said once");
        assert!(report.warnings.iter().any(|w| w.contains("past a limit of the model")), "{what}: in words: {:?}", report.warnings);
    }
    // The depth the model holds is read: 200 levels of shapes.
    let shapes = format!("{}{}", "<diacraft:shape id=\"1\">".repeat(200), "</diacraft:shape>".repeat(200));
    let file = file_of(&format!("<diacraft:master id=\"1\">{shapes}</diacraft:master>"));
    let (doc, report) = read(&package_with(&file), &Limits::default()).unwrap();
    assert_eq!((doc.masters.len(), report.warned("diacraft_file_unreadable")), (1, 0));
}

/// The reader's items are still a refusal of the drawing: a master that spends them is not a damaged one.
#[test]
fn the_readers_items_still_refuse_the_drawing() {
    let shapes = r#"<diacraft:shape id="1" name="n"/>"#.repeat(2_000);
    let file = file_of(&format!("<diacraft:master id=\"1\">{shapes}</diacraft:master>"));
    let limits = Limits { max_items: 12_000, ..Limits::default() };
    let err = read(&package_with(&file), &limits).err().map(|e| e.class());
    assert_eq!(err, Some("too_many_items"));
}

// ------------------------------------------------------------------------------------------------------------------ the version a save writes

const URN: &str = "urn:app.diacraft:extension";

fn old_file(version: &str) -> String {
    format!(
        r##"<diacraft:document xmlns:diacraft="{URN}" version="{version}"><diacraft:colors><diacraft:color ix="3" rgb="#010203"/></diacraft:colors><diacraft:future/></diacraft:document>"##
    )
}

/// A save over a file of a newer version writes the version of what it writes: the masters are re-encoded in this build's vocabulary, and the root no longer says 9. The
/// children this build does not know stay as the file had them.
#[test]
fn a_save_over_a_file_of_a_newer_version_says_the_version_it_wrote() {
    let mut o = open_bytes(package_with(&old_file("9")));
    o.doc.masters.push(Arc::new(master_of(7, 1)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let text = part_text(&saved.bytes);
    assert!(text.contains(r#"version="2""#) && !text.contains(r#"version="9""#), "{}", text.get(0..300).unwrap_or(&text));
    assert!(text.contains("future"), "what this build does not know is kept");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(back.masters.len(), 1);
    assert_eq!(report.warned("diacraft_version"), 0, "it is a file of this build's version now");
}

/// A save over a file of an older version replaces it, and says so; a save that changes nothing about the file leaves it, and says nothing.
#[test]
fn a_save_that_replaces_a_file_of_an_older_version_says_so() {
    let mut o = open_bytes(package_with(&old_file("1")));
    let untouched = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(untouched.notes.iter().all(|n| n.class() != "document_file_replaced"), "{:?}", untouched.notes);
    o.doc.masters.push(Arc::new(master_of(7, 1)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let note = saved.notes.iter().find(|n| n.class() == "document_file_replaced").unwrap_or_else(|| panic!("no note: {:?}", saved.notes));
    assert!(note.to_string().contains("older version"), "{note}");
    let text = part_text(&saved.bytes);
    assert!(text.contains(r#"version="2""#) && !text.contains("future"), "the old file is replaced, not written over");
    // A file of this build's version is not replaced, and says nothing.
    let mut o = open_bytes(package_with(&old_file("2")));
    o.doc.masters.push(Arc::new(master_of(7, 1)));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(saved.notes.iter().all(|n| n.class() != "document_file_replaced"), "{:?}", saved.notes);
}

// ------------------------------------------------------------------------------------------------------------------ the writer's loops

/// The loop that makes the masters' trees stops when what it has made is past the file's cap (64 MiB), and the file is refused in words: the trees of the masters after the
/// one that went past are never made.
#[test]
fn the_masters_trees_stop_being_made_when_the_file_is_past_its_cap() {
    let mut o = base();
    let text: Arc<str> = Arc::from("m".repeat(1 << 20));
    for id in 1..=80 {
        let mut s = Shape::new(1);
        s.text = Some(Arc::new(diacraft_doc::Text { parts: vec![diacraft_doc::TextPart::Chars(Arc::clone(&text))], ..Default::default() }));
        o.doc.masters.push(Arc::new(Master { id, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }));
    }
    let mut work = WriteWork::default();
    let refused = save(&o.doc, &o, &mut work).err().unwrap_or_else(|| panic!("80 MiB of masters were saved into a file of at most 64"));
    assert!(refused.to_string().contains("more than 64 MiB"), "{refused}");
    assert!(work.docfile_trees <= 65, "{} trees were made for a file the 65th went past", work.docfile_trees);
    assert!(work.docfile_trees >= 60, "and the ones before it were: {}", work.docfile_trees);
}

/// Every unit of a save of masters is counted: the trees (one for each master), the tree's items (the file's), the steps of the walk (a cap on them refuses the save), and the
/// pictures in the estimate (three times their size at the least, a tree, a file and the buffer it grows in).
#[test]
fn the_save_counts_its_trees_items_steps_and_pictures() {
    let mut o = base();
    for id in 1..=3 {
        o.doc.masters.push(Arc::new(master_of(id, 4)));
    }
    let mut work = WriteWork::default();
    let saved = save(&o.doc, &o, &mut work).unwrap();
    assert_eq!(work.docfile_trees, 3);
    // The items of the file are the elements, attributes and text nodes of the tree that was written, which the part gives again.
    let part = part_text(&saved.bytes);
    let tree = diacraft_container::xml::parse(part.as_bytes()).unwrap();
    let mut items = 0u64;
    let mut stack = vec![&tree];
    while let Some(e) = stack.pop() {
        items += 1 + e.attrs.len() as u64;
        for n in &e.children {
            match n {
                diacraft_container::xml::Node::El(c) => stack.push(c),
                diacraft_container::xml::Node::Text(_) | diacraft_container::xml::Node::CData(_) => items += 1,
                _ => {}
            }
        }
    }
    assert_eq!(work.docfile_items, items, "the items the save recorded are the file's");
    // The walk's steps: a cap of one step refuses the save in words.
    let options = WriteOptions { limits: Limits { max_write_steps: 1, ..Limits::default() }, ..WriteOptions::default() };
    let refused = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut WriteWork::default())
        .err()
        .unwrap_or_else(|| panic!("a save of three masters took one step"));
    assert!(refused.to_string().contains("steps"), "{refused}");
    // A picture is in the estimate: a master that shows 1 MiB of picture holds three times that, at the least.
    let mut doc = o.doc.clone();
    let picture = Arc::new(Media { part: Arc::from("p.png"), content_type: Arc::from("image/png"), bytes: Arc::from(vec![7u8; 1 << 20]) });
    let mut s = Shape::new(1);
    s.kind = ShapeType::Foreign;
    s.foreign = Some(Arc::new(ForeignData { picture: Some(picture), ..ForeignData::default() }));
    doc.masters =
        diacraft_doc::SharedVec::from_vec(vec![Arc::new(Master { id: 9, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() })]);
    let mut work = WriteWork::default();
    save(&doc, &o, &mut work).unwrap();
    assert!(work.docfile_held >= 3 << 20, "{} bytes held for a picture of 1 MiB", work.docfile_held);
}

/// The trees of an entry's sheet are held before they are made as the masters' are: a save that makes a document-sheet entry needs the memory its tree takes, and with a byte
/// less is refused as a limit.
#[test]
fn the_tree_of_an_entry_sheet_is_held_before_it_is_made() {
    let mut o = base();
    let sheet = Arc::make_mut(&mut o.doc.sheet);
    for c in 0..200 {
        sheet.cells.insert(CellName::new(&format!("User{c}")), Cell::constant(Value::from_text("1")));
    }
    let limits = Limits::default();
    let mut work = WriteWork::default();
    save(&o.doc, &o, &mut work).unwrap();
    assert!(work.docfile_held > 0, "a document-sheet entry is a tree, and its memory is held");
    let kept = limits.max_write_memory - work.estimated_memory - work.memory_room;
    let exact = work.estimated_memory + kept + work.docfile_held;
    let options = |max_write_memory: u64| WriteOptions { limits: Limits { max_write_memory, ..Limits::default() }, ..WriteOptions::default() };
    let at = |max: u64| write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options(max), &mut WriteWork::default());
    assert!(at(exact).is_ok(), "exactly the memory it needs");
    assert!(matches!(at(exact - 1), Err(WriteError::Limit { .. })), "a byte less is refused as a limit");
}

/// The pictures of the masters are the file's list of `blob` elements, and the list comes before the masters that name them (the design's 7.3 gives the order).
#[test]
fn the_blobs_are_written_before_the_masters() {
    let mut o = base();
    let picture = Arc::new(Media { part: Arc::from("p.png"), content_type: Arc::from("image/png"), bytes: Arc::from(vec![7u8; 64]) });
    let mut s = Shape::new(1);
    s.kind = ShapeType::Foreign;
    s.foreign = Some(Arc::new(ForeignData { picture: Some(picture), ..ForeignData::default() }));
    o.doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() }));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let text = part_text(&saved.bytes);
    let (blob, master) = (text.find(":blob").unwrap(), text.find(":master").unwrap());
    assert!(blob < master, "the blob list comes first: blob at {blob}, master at {master}");
}

// ------------------------------------------------------------------------------------------------------------------ the reader's costs

fn items_and_elements(file: &str) -> (u64, u64) {
    let report = read(&package_with(file), &Limits::default()).unwrap_or_else(|e| panic!("does not read: {e}")).1;
    (report.work.items, report.work.elements)
}

/// A blob is read, and charged, once, with the list of the pictures: its element and its attributes as items, the bytes it stands for against the pictures, its text by its
/// bytes, and the element counted; the loop that reads the other children skips it. A file with a blob costs the same as the file without it by exactly that.
#[test]
fn a_blob_is_charged_once_by_its_element_its_attribute_and_its_text() {
    let without = file_of("");
    let text = "A".repeat(4096);
    let with = file_of(&format!(r#"<diacraft:blob id="1">{text}</diacraft:blob>"#));
    let ((items_a, elements_a), (items_b, elements_b)) = (items_and_elements(&without), items_and_elements(&with));
    assert_eq!(elements_b - elements_a, 1, "the element is counted once, by the list of the pictures, and the other loop skips it");
    let backstop = |file: &str| file.len().div_ceil(64) as u64;
    // The element and its attribute, and the 4,096 bytes of the text at 64 to an item, and what the file's own bigger size adds to the backstop.
    let expected = (1 + 1) + 4096_u64.div_ceil(64) + backstop(&with) - backstop(&without);
    assert_eq!(items_b - items_a, expected, "a blob costs its element, its attribute and its text, once");
    // Its text is charged by its bytes: twice the text is 64 more items.
    let more = file_of(&format!(r#"<diacraft:blob id="1">{}</diacraft:blob>"#, "A".repeat(8192)));
    assert_eq!(items_and_elements(&more).0 - items_b, 64 + backstop(&more) - backstop(&with));
}

/// A master that names a picture the file does not hold is read without it, and says so; a file that holds names this build does not know says so once, in its own class.
#[test]
fn a_lost_picture_and_unknown_names_are_each_said_in_their_own_class() {
    let master = r#"<diacraft:master id="1"><diacraft:shape id="1" type="Foreign"><diacraft:foreign><diacraft:picture part="p.png" type="image/png" blob="7"/></diacraft:foreign></diacraft:shape></diacraft:master>"#;
    let (_, report) = read(&package_with(&file_of(master)), &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_picture_lost"), 1, "{:?}", report.warning_counts);
    assert!(report.warnings.iter().any(|w| w.contains("picture of a master")), "{:?}", report.warnings);
    // A second `sheet` in a shape is not read, and no slot keeps it: the codec counts it as a name it does not know.
    let unknown = r#"<diacraft:master id="1"><diacraft:shape id="1"><diacraft:sheet/><diacraft:sheet/></diacraft:shape><diacraft:shape id="2"><diacraft:sheet/><diacraft:sheet/></diacraft:shape></diacraft:master>"#;
    let (_, report) = read(&package_with(&file_of(unknown)), &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_version"), 1, "once, however many names: {:?}", report.warning_counts);
}

/// The cells of an entry's sheet are counted against the document's cells, as the cells of a master are.
#[test]
fn the_cells_of_an_entry_sheet_are_counted() {
    let cells = |n: usize| {
        let inner: String = (0..n).map(|k| format!(r#"<diacraft:Cell N="User{k}" V="1"/>"#)).collect();
        let file = file_of(&format!("<diacraft:docsheet><diacraft:sheet>{inner}</diacraft:sheet></diacraft:docsheet>"));
        read(&package_with(&file), &Limits::default()).unwrap().1.stats.cells
    };
    assert_eq!(cells(5) - cells(0), 5);
    // And a document that may hold fewer cells than the sheet has refuses it.
    let inner: String = (0..50).map(|k| format!(r#"<diacraft:Cell N="User{k}" V="1"/>"#)).collect();
    let file = file_of(&format!("<diacraft:docsheet><diacraft:sheet>{inner}</diacraft:sheet></diacraft:docsheet>"));
    let (_, plain) = read(&package_with(&file), &Limits::default()).unwrap();
    let limits = Limits { max_cells: plain.stats.cells - 1, ..Limits::default() };
    assert_eq!(read(&package_with(&file), &limits).err().map(|e| e.class()), Some("too_many_cells"));
}

/// What a decode spent is charged to the reader whether it ended or was refused as damaged, and a charge that the room cannot pay refuses the read: the words for a damaged
/// master do not hide it. The smallest room in which a file with a damaged master reads is found, and one item less refuses it as the reader's items.
#[test]
fn what_a_damaged_master_spent_is_charged_and_a_room_it_cannot_pay_refuses_the_read() {
    let good = r#"<diacraft:shape id="1" name="n"><diacraft:sheet><diacraft:Cell N="Width" V="1"/></diacraft:sheet></diacraft:shape>"#.repeat(300);
    let damaged = file_of(&format!(r#"<diacraft:master id="1">{good}<diacraft:shape type="Bogus"/></diacraft:master>"#));
    let package = package_with(&damaged);
    let reads_in = |max: usize| read(&package, &Limits { max_items: max, ..Limits::default() });
    let (_, report) = reads_in(Limits::default().max_items).unwrap();
    assert_eq!(report.warned("diacraft_file_unreadable"), 1, "the master is damaged and left out");
    // The least room that reads it, by bisection (a bigger room always reads it).
    let (mut no, mut yes) = (1usize, usize::try_from(report.work.items).unwrap_or(usize::MAX));
    assert!(reads_in(yes).is_ok());
    while yes - no > 1 {
        let mid = no + (yes - no) / 2;
        if reads_in(mid).is_ok() {
            yes = mid;
        } else {
            no = mid;
        }
    }
    assert_eq!(reads_in(no).err().map(|e| e.class()), Some("too_many_items"), "one item less than the file costs refuses it");
    let ok = reads_in(yes).unwrap().1;
    assert_eq!(ok.warned("diacraft_file_unreadable"), 1);
}
