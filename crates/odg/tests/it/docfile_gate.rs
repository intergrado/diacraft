//! The memory a save holds for the trees of the document file, and the crafted files the reader of the codec's masters is held to.
//!
//! * **The gate.** A save holds the memory of the trees it is going to make (counted from the models) before it makes the first of them, and gives it back when the file's
//!   bytes are made. A limit of exactly what it needs saves; one byte less is refused as a limit, in words, with no tree made.
//! * **Crafted files**: damaged masters, and a chain of nested shapes that all show one picture. Each is counted in the units the reader's loop walks, and one is timed
//!   beside its yardstick (`--ignored`).

use std::sync::Arc;

use diacraft_doc::{ForeignData, Master, Media, Shape, ShapeList, ShapeType, SnapWords};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, Written, read, write_counted};

use crate::docfile::base;
use crate::docfile_codec::{part_text, through_the_file};
use crate::docfile_cost::{file_of, package_with};
use crate::write_core::Open;

/// A master of `shapes` shapes, each with a name and `cells` cells: the kind of master the trees are counted for.
fn rich_master(id: u32, shapes: u32, cells: u32) -> Master {
    let members: Vec<Arc<Shape>> = (1..=shapes)
        .map(|k| {
            let mut s = Shape::new(k);
            s.name = Some(Arc::from(format!("Shape {k}")));
            let sheet = Arc::make_mut(&mut s.sheet);
            for c in 0..cells {
                sheet.cells.insert(
                    diacraft_doc::CellName::new(&format!("Cell{c}")),
                    diacraft_doc::Cell::read(Some("Width*2"), Some("2"), Some("IN"), None, &mut 1_000),
                );
            }
            Arc::new(s)
        })
        .collect();
    Master { id, shapes: ShapeList::from_vec(members), ..Master::default() }
}

/// A save of `doc` over `o`'s file at `limits`, with the work it counted.
fn save_counted(doc: &diacraft_doc::Document, o: &Open, limits: Limits) -> (Result<Written, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let options = WriteOptions { limits, ..WriteOptions::default() };
    let saved = write_counted(doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    (saved, work)
}

/// A save holds the memory of the trees it is going to make before it makes the first of them (counted from the models), and what it holds is never below the tree it made:
/// a limit of exactly the memory the save needs saves, one byte less is refused as a limit in words with no tree made, and nothing is held after the file is written.
#[test]
fn the_memory_of_the_trees_is_held_before_any_is_made_and_is_never_below_the_tree() {
    let mut o = base();
    for id in 1..=3 {
        o.doc.masters.push(Arc::new(rich_master(id, 40, 12)));
    }
    let limits = Limits::default();
    let (saved, work) = save_counted(&o.doc, &o, limits.clone());
    saved.unwrap_or_else(|e| panic!("not saved: {e}"));
    assert_eq!(work.docfile_trees, 3, "one tree for each master");
    assert!(
        work.docfile_held >= work.docfile_items * diacraft_sheetxml::model::cost::TREE_ITEM_BYTES,
        "{} bytes held for a tree of {} items",
        work.docfile_held,
        work.docfile_items
    );
    // What the save keeps taken for good, which is none of the file's trees: they are given back once the file's bytes are made.
    let kept = limits.max_write_memory - work.estimated_memory - work.memory_room;
    let exact = work.estimated_memory + kept + work.docfile_held;
    let (saved, _) = save_counted(&o.doc, &o, Limits { max_write_memory: exact, ..limits.clone() });
    assert!(saved.is_ok(), "exactly the memory it needs: {:?}", saved.err());
    let (refused, work) = save_counted(&o.doc, &o, Limits { max_write_memory: exact - 1, ..limits });
    match refused {
        Err(WriteError::Limit { what, max }) => {
            assert!(what.contains("bytes of memory") && what.contains("document file"), "{what}");
            assert_eq!(max, exact - 1);
        }
        other => panic!("one byte less than it needs was not refused as a limit: {:?}", other.err()),
    }
    assert_eq!(work.docfile_trees, 0, "refused before the first tree was made");
}

/// The memory held for the trees of masters is counted from the models, so a master that holds more is held more: the estimate grows in proportion to the shapes and the cells
/// of the masters, and a save that has no master makes no tree of one and holds nothing for one.
#[test]
fn the_memory_held_for_the_trees_grows_with_what_the_masters_hold_and_is_none_for_none() {
    let held = |shapes: u32, cells: u32| {
        let mut o = base();
        o.doc.masters.push(Arc::new(rich_master(1, shapes, cells)));
        let (saved, work) = save_counted(&o.doc, &o, Limits::default());
        saved.unwrap_or_else(|e| panic!("not saved: {e}"));
        work.docfile_held
    };
    let (small, more_shapes, more_cells) = (held(10, 5), held(20, 5), held(10, 10));
    assert!(more_shapes > small && more_cells > small, "{small} {more_shapes} {more_cells}");
    assert!(more_shapes < 3 * small && more_cells < 3 * small, "and in proportion to it: {small} {more_shapes} {more_cells}");
    let mut o = base();
    o.doc.set_snap_words(&SnapWords { snap: Some(Arc::from("1")), ..SnapWords::default() });
    let (saved, work) = save_counted(&o.doc, &o, Limits::default());
    saved.unwrap();
    assert_eq!((work.docfile_trees, work.docfile_held), (0, 0), "no master, no tree and nothing held for one");
}

/// The items a document file costs the reader, less the file's own size (charged once, at a sixty-fourth of a byte for an item).
fn items_of(file: &str) -> u64 {
    let report = read(&package_with(file), &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}")).1;
    report.work.items - u64::try_from(file.len().div_ceil(64)).unwrap_or(0)
}

/// A hand-made master of `shapes` shapes of a size the codec reads, to which `bad` adds a shape of a kind the codec does not list (the master is damaged at the end of it).
fn master_text(shapes: usize, bad: bool) -> String {
    let good = r#"<diacraft:shape id="1" name="n"><diacraft:sheet><diacraft:Cell N="Width" V="1"/></diacraft:sheet></diacraft:shape>"#.repeat(shapes);
    let broken = if bad { r#"<diacraft:shape type="Bogus"/>"# } else { "" };
    format!(r#"<diacraft:master id="1">{good}{broken}</diacraft:master>"#)
}

/// **A damaged master is charged what it cost to find it damaged**: a file of masters that the codec reads for fifty shapes and finds damaged at the end costs the reader as
/// much as the same file with the damage taken out, so a hostile file of damaged masters gets no work for nothing, and the cost is linear in their number.
#[test]
fn damaged_masters_are_charged_the_work_that_found_them_damaged_and_the_cost_is_linear() {
    let file = |n: usize, bad: bool| file_of(&master_text(50, bad).repeat(n));
    let (honest, hostile) = (items_of(&file(400, false)), items_of(&file(400, true)));
    assert!(
        hostile >= honest * 9 / 10,
        "{hostile} items for damaged masters, {honest} for the same masters whole: the work they cost is not charged"
    );
    let (a, b) = (items_of(&file(200, true)), items_of(&file(400, true)));
    assert!(b.abs_diff(2 * a) <= a / 50, "{a} items for 200 and {b} for 400: not linear");
    // And a damaged master is a word, counted, and no error: the masters after it are read.
    let (doc, report) = read(&package_with(&file(3, true)), &Limits::default()).unwrap();
    assert_eq!(doc.masters.len(), 0);
    assert_eq!(report.warning_counts.get("diacraft_file_unreadable"), Some(&3));
}

/// The chain D2 asks for: shapes nested to the model's depth, each showing one picture, are one blob in the file, charged once to the pictures' budget, and one picture in
/// the model that all of them hold.
#[test]
fn a_chain_of_nested_shapes_that_all_show_one_picture_holds_it_once_in_the_file_and_in_the_model() {
    let media = Arc::new(Media { part: Arc::from("media/p.png"), content_type: Arc::from("image/png"), bytes: Arc::from(vec![7u8; 4_998]) });
    let picture = |id: u32| {
        let mut s = Shape::new(id);
        s.kind = ShapeType::Foreign;
        s.foreign = Some(Arc::new(ForeignData { picture: Some(Arc::clone(&media)), ..ForeignData::default() }));
        s
    };
    let most = u32::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap();
    let mut s = picture(most);
    for id in (1..most).rev() {
        let mut parent = picture(id);
        parent.children = ShapeList::from_vec(vec![Arc::new(s)]);
        s = parent;
    }
    let master = Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(s)]), ..Master::default() };
    let (bytes, got) = through_the_file(master);
    assert_eq!(part_text(&bytes).matches("<diacraft:blob").count(), 1, "one blob for two hundred pictures");
    // The budget holds the entries the model keeps beside the picture, the thumbnail of the save for one.
    let beside: u64 = read(&bytes, &Limits::default()).unwrap().0.parts.iter().map(|p| p.bytes.len() as u64).sum();
    let exact = Limits { max_media_bytes: 4_998 + beside, ..Limits::default() };
    let back = read(&bytes, &exact).unwrap().0;
    assert_eq!(back.masters.len(), 1, "4,998 bytes of the budget of pictures are enough: they are charged once");
    // Every shape of the chain holds the same picture.
    let mut shape = got.shapes.get(0).cloned();
    let mut seen: Vec<Arc<Media>> = Vec::new();
    while let Some(s) = shape {
        seen.extend(s.foreign.as_ref().and_then(|f| f.picture.clone()));
        shape = s.children.get(0).cloned();
    }
    assert_eq!(seen.len(), 200);
    assert!(seen.iter().all(|m| Arc::ptr_eq(m, &seen[0])), "the model holds the picture once");
}

/// **Against a yardstick**: the damaged masters are read in the time the same masters whole are, back to back in one window. Slower than twice the yardstick is a hole.
/// Timed, so run on request (`--ignored`).
#[test]
#[ignore = "times the crafted files back to back with their yardsticks; run on request"]
fn the_crafted_files_are_read_in_the_time_of_their_yardsticks() {
    use std::time::Instant;
    let time = |file: &str| {
        let bytes = package_with(file);
        let start = Instant::now();
        for _ in 0..5 {
            read(&bytes, &Limits::default()).unwrap();
        }
        start.elapsed()
    };
    let honest = file_of(&master_text(50, false).repeat(2_000));
    let hostile = file_of(&master_text(50, true).repeat(2_000));
    let (yard, case) = (time(&honest), time(&hostile));
    eprintln!("damaged masters: {case:?} against {yard:?}");
    assert!(case <= 2 * yard, "damaged masters took {case:?} where whole ones took {yard:?}");
}
