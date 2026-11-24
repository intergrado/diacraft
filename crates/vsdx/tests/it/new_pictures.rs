//! **A picture new to a page part the file has is written by the save into the file** (`write/page_rows.rs`, `Plan::dress_new_pictures`), as a new page's picture is: a
//! row of the page part's relationships to a part that holds the picture (one of the file's that holds its bytes, else one made from the model's), and its `ForeignData` names the row.
//! refused it ("a picture new to a page the file already has"), so a picture inserted into a drawing saved as a VSDX type could not be saved; **and a page part with no `Shapes` element (a blank
//! page) made its first shapes without asking, so a picture inserted on it was saved as a `ForeignData` with no `Rel`, lost with no word** (`write/par.rs`).
//!
//! **The next save is no edit**: the drawing holds the picture by its bytes and names no relationship, the file names the row the save made, and the two show one picture (`patch::shows_the_same`).
//!
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::{Document, Media, Shape};
use diacraft_testkit::package::{OD, PageSpec, Pkg, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{NOW, new_picture_shape, picture};
use crate::read_ok;

/// The bytes of a PNG as the reader tells one: its signature, and `n` after it.
fn png(n: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G'];
    bytes.extend_from_slice(&n.to_le_bytes());
    bytes
}

/// A save into `original`, whose model was `baseline`.
fn splice(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &opts).unwrap_or_else(|e| panic!("the save refused: {e}"))
}

/// The model with `shapes` added to the first page.
fn with_shapes(doc: &Document, shapes: Vec<Shape>) -> Document {
    let mut doc = doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    for shape in shapes {
        page.shapes.push(Arc::new(shape));
    }
    doc
}

/// A `Foreign` shape's `ID`, the relationship it names and the bytes of the picture it shows.
type Shown = (u32, Option<String>, Option<Vec<u8>>);

/// The bytes of the picture each `Foreign` shape of the first page shows, by `ID`, and the relationship it names.
fn pictures_of(doc: &Document) -> Vec<Shown> {
    doc.pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .filter_map(|s| s.foreign.as_ref().map(|f| (s.id, f.rel_id.as_deref().map(str::to_owned), f.picture.as_ref().map(|m| m.bytes.to_vec()))))
        .collect()
}

/// A page of one box and a picture of the file's (`rId1` to `image1.png`).
fn file_with_a_picture() -> Vec<u8> {
    let picture = r#"<Shape ID="7" Type="Foreign"><Cell N="PinX" V="6"/><ForeignData ForeignType="Bitmap" CompressionType="PNG"><Rel r:id="rId1"/></ForeignData></Shape>"#;
    VsdxBuilder::new()
        .page(PageSpec::new(1, "P").shapes(&format!("{}{picture}", shape(6, &cell("PinX", "1")))).rel(
            "rId1",
            &format!("{OD}image"),
            "../media/image1.png",
        ))
        .with(Pkg::new().part("/visio/media/image1.png", "image/png", png(1)))
        .build()
}

/// **A picture new to a page part the file has is written**: a new picture gets a row to a part made from its bytes, and one that shows the file's picture a row to the file's part (nothing
/// copied); the file read again shows both; and the save after it, given the drawing as it is (the pictures name no relationship), is no edit and the same bytes.
#[test]
fn a_picture_new_to_a_page_part_the_file_has_is_written_and_the_save_after_it_is_no_edit() {
    let original = file_with_a_picture();
    let (doc, _) = read_ok(&original);
    let fresh: Arc<Media> = picture("media/image9.png", &png(2));
    // (A picture of the bytes of one the drawing holds is that picture: `insert.picture` shares it, and so does this.)
    let same: Arc<Media> = Arc::clone(
        doc.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 7).and_then(|s| s.foreign.as_ref()).and_then(|f| f.picture.as_ref()).unwrap(),
    );
    let edited = with_shapes(&doc, vec![new_picture_shape(20, &fresh), new_picture_shape(21, &same)]);
    let written = splice(&edited, &original, &doc);
    let (back, _) = read_ok(&written.bytes);
    let shown = pictures_of(&back);
    let by_id = |id: u32| shown.iter().find(|(s, _, _)| *s == id).cloned().unwrap_or_else(|| panic!("shape {id} is not in the file: {shown:?}"));
    assert_eq!(by_id(20).2, Some(png(2)), "the new picture is in the file");
    assert_eq!(by_id(21).2, Some(png(1)), "and the one of the file's bytes");
    assert_eq!(by_id(7).2, Some(png(1)), "the file's picture is as it was");
    let rels: Vec<Option<String>> = [7, 20, 21].iter().map(|id| by_id(*id).1).collect();
    assert!(rels.iter().all(Option::is_some) && rels[0] != rels[1] && rels[1] != rels[2], "each names a row of its own: {rels:?}");
    let parts: Vec<&str> = written.added.iter().map(String::as_str).filter(|p| p.contains("/media/")).collect();
    assert_eq!(parts.len(), 1, "one part made, for the new bytes; the file's picture's part is named again: {:?}", written.added);
    let again = splice(&edited, &written.bytes, &written.document);
    assert!(again.bytes == written.bytes, "the save after it is no edit (rewritten {:?})", again.rewritten);
}

/// **A blank page part given its first shapes writes their pictures** (the part has no `Shapes` element; before, the shapes were made there without the refusal `patch_shapes` makes, and a picture
/// was written as a `ForeignData` with no `Rel`).
#[test]
fn a_page_part_with_no_shapes_given_a_picture_writes_it() {
    let original = VsdxBuilder::new().page(PageSpec::new(1, "Blank")).build();
    let (doc, _) = read_ok(&original);
    assert!(doc.pages.get(0).unwrap().shapes.is_empty() && doc.pages.get(0).unwrap().part_name.is_some(), "a blank page with a part");
    let media = picture("media/image1.png", &png(3));
    let edited = with_shapes(&doc, vec![new_picture_shape(1, &media)]);
    let written = splice(&edited, &original, &doc);
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(pictures_of(&back), vec![(1, Some("rId1".to_owned()), Some(png(3)))], "the picture is in the file, through a row of the page part");
    let again = splice(&edited, &written.bytes, &written.document);
    assert!(again.bytes == written.bytes, "the save after it is no edit");
}

/// The members of a group new to the page are new: a picture among them is written too.
#[test]
fn a_picture_inside_a_group_new_to_the_page_is_written() {
    let original = file_with_a_picture();
    let (doc, _) = read_ok(&original);
    let media = picture("media/image1.png", &png(4));
    let mut group = Shape { id: 30, kind: diacraft_doc::ShapeType::Group, ..Shape::default() };
    group.children.push(Arc::new(new_picture_shape(31, &media)));
    let edited = with_shapes(&doc, vec![group]);
    let written = splice(&edited, &original, &doc);
    let (back, _) = read_ok(&written.bytes);
    let group = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 30).cloned().expect("the group is in the file");
    let member = group.children.iter().find(|s| s.id == 31).expect("its member");
    let shown = member.foreign.as_ref().and_then(|f| f.picture.as_ref()).map(|m| m.bytes.to_vec());
    assert_eq!(shown, Some(png(4)), "the member's picture is in the file");
}

/// **An embedded object new to a page is still refused** in words: its relationship names the object, which this writer does not make for a page the file has (the model writer does).
#[test]
fn an_object_new_to_a_page_part_is_still_refused_in_words() {
    let original = file_with_a_picture();
    let (doc, _) = read_ok(&original);
    let media = picture("media/image1.png", &png(5));
    let mut object = new_picture_shape(40, &media);
    let mut data = diacraft_doc::ForeignData::clone(object.foreign.as_ref().unwrap());
    data.attrs = diacraft_doc::RawAttrs::from_pairs([("ForeignType", "Object"), ("ObjectType", "2")]);
    object.foreign = Some(Arc::new(data));
    let edited = with_shapes(&doc, vec![object]);
    let opts = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let Err(refused) = write_with(&edited, Some(&OriginalBytes { bytes: &original, baseline: Some(&doc) }), &opts) else {
        panic!("an object new to a page is refused")
    };
    assert!(refused.to_string().contains("a picture new to a page the file already has"), "{refused}");
}

/// **Many new pictures on one page take their ids in one walk of the page part's ids** (the security scan: an id asked for each picture walked the part's ids each time, the square
/// of their number). 1,500 pictures of their own bytes beside 60 rows the part has: the ids scanned are the rows there and a few walks of small parts, and not 1,500 walks of hundreds. Each picture
/// is in the file, through a row of its own.
#[test]
fn many_pictures_new_to_a_page_take_their_ids_in_one_walk_of_the_part() {
    const ROWS: u32 = 60;
    const NEW: u32 = 1_500;
    let mut page = PageSpec::new(1, "P").shapes(&shape(6, &cell("PinX", "1")));
    let mut pkg = Pkg::new();
    for k in 0..ROWS {
        page = page.rel(&format!("rId{}", k + 1), &format!("{OD}image"), &format!("../media/held{k}.png"));
        pkg = pkg.part(&format!("/visio/media/held{k}.png"), "image/png", png(10_000 + k));
    }
    let original = VsdxBuilder::new().page(page).with(pkg).build();
    let (doc, _) = read_ok(&original);
    let shapes: Vec<Shape> = (0..NEW).map(|k| new_picture_shape(100 + k, &picture("media/x.png", &png(k)))).collect();
    let edited = with_shapes(&doc, shapes);
    diacraft_vsdx::work::reset();
    let written = splice(&edited, &original, &doc);
    let scanned = diacraft_vsdx::work::id_scans();
    let walked_once = u64::from(ROWS + NEW) * 4;
    assert!(
        scanned <= walked_once,
        "{scanned} ids scanned for {NEW} new pictures beside {ROWS} rows: more than one walk of the part (at most {walked_once})"
    );
    let (back, _) = read_ok(&written.bytes);
    let shown = pictures_of(&back);
    let new: Vec<&Shown> = shown.iter().filter(|(id, _, _)| *id >= 100).collect();
    assert_eq!(new.len(), NEW as usize);
    assert!(new.iter().all(|(id, rel, bytes)| rel.is_some() && *bytes == Some(png(id - 100))), "each picture is in the file");
    let mut rels: Vec<&String> = new.iter().filter_map(|(_, rel, _)| rel.as_ref()).collect();
    rels.sort();
    rels.dedup();
    assert_eq!(rels.len(), NEW as usize, "each names a row of its own");
}
