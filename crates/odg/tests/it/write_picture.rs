//! A new picture in a package: a `draw:frame` holding a `draw:image` whose
//! `xlink:href` names a new entry under `Pictures/`, with a manifest row for it. The writer puts the entry in and leaves every other entry as it is.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, ForeignData, Media, RawAttrs, Section, Shape, ShapeType, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, OriginalPackage, SaveNote, WriteError, WriteOptions, Written, read, write_with};

use crate::drawing::cm;
use crate::flat_write::small_png;
use crate::write_core::{Open, compare, drawing_of, full_meta, has_part, open_bytes, part, part_bytes};
use crate::write_new::{push_shape, rectangle};

/// The writer, as the engine calls it: no thumbnail, the default limits.
pub fn save(doc: &Document, bytes: &[u8], index: &diacraft_odg::ShapeIndex) -> Result<Written, WriteError> {
    save_with(doc, bytes, index, Limits::default())
}

pub fn save_with(doc: &Document, bytes: &[u8], index: &diacraft_odg::ShapeIndex, limits: Limits) -> Result<Written, WriteError> {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions { limits, ..WriteOptions::default() })
}

/// A picture of `n` bytes that starts as `head` says and goes on with bytes that do not deflate far (a run of one byte deflates past the ratio the container reads).
pub fn noise(head: &[u8], n: usize, seed: u64) -> Vec<u8> {
    let mut out = head.to_vec();
    let mut x = 0x2545_F491_4F6C_DD1Du64 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    while out.len() < n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push((x >> 24) as u8);
    }
    out
}

pub const PNG_HEAD: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A new picture shape as the engine hands one to the writer: DiaCraft's rectangle, as a foreign shape holding `bytes`.
pub fn picture(id: u32, name: &str, frame: (f64, f64, f64, f64), bytes: &[u8]) -> Shape {
    let mut shape = rectangle(id, name, frame);
    shape.kind = ShapeType::Foreign;
    shape.foreign = Some(Arc::new(ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
        picture: Some(Arc::new(Media { part: Arc::from("pic.png"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })),
        ..ForeignData::default()
    }));
    // A picture has no outline of its own: a frame the reader gives no `Geometry`.
    Arc::make_mut(&mut shape.sheet).sections.retain(|s| s.name.as_str() != "Geometry");
    shape
}

/// A picture shape shaped as the VSDX reader gives one: a foreign shape with its relationship id and the attributes of its `ForeignData`, the cells of a picture's
/// frame and of its image (`ImgOffsetX` and the rest), no `Connection` section and no outline, and a part named as a VSDX package names it.
pub fn vsdx_picture(id: u32, name: &str, frame: (f64, f64, f64, f64), bytes: &[u8]) -> Shape {
    let mut shape = picture(id, name, frame, bytes);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.sections.clear();
    for fill in ["FillPattern", "FillForegnd", "LinePattern"] {
        sheet.cells.remove(fill);
    }
    let (w, h) = (cm(frame.2), cm(frame.3));
    let mut put = |n: &str, formula: Option<&str>, v: f64| {
        sheet.cells.insert(CellName::new(n), Cell::read(formula, Some(&v.to_string()), Some("IN"), None, &mut 1_000));
    };
    put("ImgOffsetX", None, 0.0);
    put("ImgOffsetY", None, 0.0);
    put("ImgWidth", Some("Width*1"), w);
    put("ImgHeight", Some("Height*1"), h);
    shape.foreign = Some(Arc::new(ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap"), ("CompressionType", "PNG")]),
        rel_id: Some(Arc::from("rId2")),
        picture: Some(Arc::new(Media { part: Arc::from("/visio/media/image1.png"), content_type: Arc::from("image/png"), bytes: Arc::from(bytes) })),
        ..ForeignData::default()
    }));
    shape
}

/// The package of three shapes, a generator and a thumbnail, opened.
pub fn three() -> Open {
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    open_bytes(d.bytes())
}

/// The model's first page's shape with the name `name`.
pub fn named<'a>(doc: &'a Document, name: &str) -> &'a Shape {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).as_ref()
}

/// The picture a shape shows, and its `ForeignType`.
pub fn shown(shape: &Shape) -> Option<(Vec<u8>, Option<String>)> {
    let f = shape.foreign.as_ref()?;
    Some((f.picture.as_ref()?.bytes.to_vec(), f.attr("ForeignType").map(str::to_owned)))
}

/// The names of the entries of a package, in the order they lie in it.
pub fn entry_names(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &Limits::default()).unwrap().entries().map(|e| e.name.clone()).collect()
}

/// The `Pictures/` entries of a package.
pub fn pictures_of(bytes: &[u8]) -> Vec<String> {
    entry_names(bytes).into_iter().filter(|n| n.starts_with("Pictures/") && !n.ends_with('/')).collect()
}

/// The text of the `draw:frame` the shape named `name` is, from the part's text.
pub fn frame_of<'a>(content: &'a str, name: &str) -> &'a str {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no frame {name}: {content}"));
    let start = content[..at].rfind("<draw:frame").unwrap_or_else(|| panic!("{name} is no frame: {content}"));
    let end = content[start..].find("</draw:frame>").map(|e| start + e + "</draw:frame>".len()).unwrap();
    &content[start..end]
}

/// The tags `<manifest:file-entry .../>` of the manifest, as written.
pub fn manifest_rows(bytes: &[u8]) -> Vec<String> {
    let text = part(bytes, "META-INF/manifest.xml");
    text.split("<manifest:file-entry").skip(1).map(|r| format!("<manifest:file-entry{}", r.split("/>").next().unwrap())).collect()
}

/// A foreign shape with a PNG picture, 3 cm by 2 cm, is a `draw:frame` holding a `draw:image` whose `xlink:href` names a new `Pictures/` entry with the
/// PNG's bytes, and a manifest row `image/png` for it. Reading it back gives a foreign shape with the same bytes.
#[test]
fn odg_shapes_a_new_picture_is_a_frame_naming_a_new_entry_with_its_bytes_and_a_manifest_row() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a new picture is written in a package");
    assert!(saved.edited);
    // The entry: one, under `Pictures/`, with the PNG's bytes.
    let pictures = pictures_of(&saved.bytes);
    assert_eq!(pictures, ["Pictures/image1.png"], "one new entry under Pictures/");
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), png);
    // The manifest lists it, after the rows it had, and says what it is.
    let rows = manifest_rows(&saved.bytes);
    let before = manifest_rows(&o.bytes);
    assert_eq!(rows.len(), before.len() + 1, "one row more: {rows:?}");
    assert_eq!(&rows[..before.len()], &before[..], "the rows it had keep their bytes and their order");
    assert!(
        rows.last().unwrap().contains("manifest:full-path=\"Pictures/image1.png\"")
            && rows.last().unwrap().contains("manifest:media-type=\"image/png\""),
        "{rows:?}"
    );
    // The frame: its frame and its image, which names the entry and holds no data.
    let content = part(&saved.bytes, "content.xml");
    let frame = frame_of(&content, "Picture.1");
    for want in ["svg:width=\"3cm\"", "svg:height=\"2cm\"", "svg:x=\"2cm\"", "draw:layer=\"layout\""] {
        assert!(frame.contains(want), "{want}: {frame}");
    }
    assert!(
        frame.contains(
            "<draw:image xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"Pictures/image1.png\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"
        ),
        "{frame}"
    );
    assert!(!frame.contains("binary-data") && !frame.contains("mime-type"), "a package names its picture and holds none inline: {frame}");
    // It reads back as the picture, as a Bitmap, and the file raises no warning about it.
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(shown(named(&back, "Picture.1")), Some((png.clone(), Some("Bitmap".to_owned()))));
    assert_eq!(report.warned("picture_missing"), 0);
    // The model keeps what it held: the picture is still the model's own, and the save says it left no note of the picture.
    assert!(!saved.notes.iter().any(|n| matches!(n, SaveNote::OutlineApproximated { .. })), "{:?}", saved.notes);
}

/// Every other entry is copied as it is (its compressed bytes, method and headers), the entry made is appended before the manifest, and `mimetype` stays
/// first; a second save of the same model is no edit and gives the same bytes.
#[test]
fn odg_package_the_other_entries_are_copied_and_the_new_one_goes_before_the_manifest_and_a_second_save_is_no_edit() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let diff = compare(&o.bytes, &saved.bytes);
    assert_eq!(diff.added, ["Pictures/image1.png"], "{diff:?}");
    assert!(diff.removed.is_empty() && diff.methods_changed.is_empty() && diff.headers_changed.is_empty(), "{diff:?}");
    let names = entry_names(&saved.bytes);
    assert_eq!(names.first().map(String::as_str), Some("mimetype"));
    assert_eq!(names.last().map(String::as_str), Some("META-INF/manifest.xml"), "{names:?}");
    assert_eq!(names.iter().position(|n| n == "Pictures/image1.png"), names.len().checked_sub(2), "just before the manifest: {names:?}");
    // The parts that were not touched are the file's own, byte for byte.
    for name in ["styles.xml", "settings.xml"] {
        if has_part(&o.bytes, name) {
            assert_eq!(part_bytes(&o.bytes, name), part_bytes(&saved.bytes, name), "{name}");
        }
    }
    // A second save through the index the first gave, of the same model, is no edit.
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited, "no edit");
    assert_eq!(again.bytes, saved.bytes);
}

/// The full loop: save, open the file as a fresh read, edit another shape, save again. The picture reads back byte for byte, its entry is the first save's own
/// (copied, not written again and not twice), and the manifest's rows keep their bytes.
#[test]
fn a_picture_saved_and_opened_again_stays_one_entry_through_an_edit_of_another_shape() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let first = save(&o.doc, &o.bytes, &o.index).unwrap();
    // A fresh read of the file, an edit of another shape.
    let mut o2 = open_bytes(first.bytes.clone());
    let s1 = o2.doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name_u.as_deref() == Some("s1")).unwrap().id;
    crate::write_core::move_by(&mut o2.doc, 0, s1, 1.0, 0.0);
    let second = save(&o2.doc, &o2.bytes, &o2.index).unwrap();
    assert!(second.edited);
    let diff = compare(&first.bytes, &second.bytes);
    assert!(diff.added.is_empty() && diff.removed.is_empty(), "no entry made or taken: {diff:?}");
    assert!(diff.headers_changed.is_empty() && diff.methods_changed.is_empty() && !diff.order_changed, "{diff:?}");
    assert_eq!(pictures_of(&second.bytes), ["Pictures/image1.png"]);
    assert_eq!(part_bytes(&second.bytes, "Pictures/image1.png"), png);
    assert_eq!(manifest_rows(&first.bytes), manifest_rows(&second.bytes), "the manifest's rows are the first save's, byte for byte");
    let (back, _) = read(&second.bytes, &Limits::default()).unwrap();
    assert_eq!(shown(named(&back, "Picture.1")), Some((png, Some("Bitmap".to_owned()))));
    // The picture frame itself was not touched by the edit of another shape.
    let (before, after) = (part(&first.bytes, "content.xml"), part(&second.bytes, "content.xml"));
    assert_eq!(frame_of(&before, "Picture.1"), frame_of(&after, "Picture.1"));
}

/// A frame read from a file the writer made is a picture a save edits like any other frame: moved, it changes its own `svg:x` and `svg:y` and nothing else.
#[test]
fn a_new_picture_moved_in_the_same_session_is_one_frame_written_where_it_now_is() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let first = save(&o.doc, &o.bytes, &o.index).unwrap();
    crate::write_core::move_by(&mut o.doc, 0, id, 1.0, 0.0);
    let second = save(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(second.edited);
    let content = part(&second.bytes, "content.xml");
    let frame = frame_of(&content, "Picture.1");
    assert!(frame.contains("svg:x=\"3cm\""), "{frame}");
    assert_eq!(pictures_of(&second.bytes), ["Pictures/image1.png"]);
    let diff = compare(&first.bytes, &second.bytes);
    assert!(diff.added.is_empty() && diff.removed.is_empty(), "{diff:?}");
}

/// What the page holds besides: a drawing of an old package that has a `Pictures/` folder entry already takes the entry as any other.
#[test]
fn a_picture_is_put_beside_a_folder_entry_of_pictures_the_package_has() {
    let mut d = drawing_of(2);
    d.files.push(("Pictures/".to_owned(), Vec::new()));
    let mut o = open_bytes(d.bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
    assert!(entry_names(&saved.bytes).contains(&"Pictures/".to_owned()), "the folder entry stays");
}

/// A section the tests add: a user cell.
pub fn user_cell(shape: &mut Shape, name: &str, v: f64) {
    let mut s = Section::new("User", None);
    let mut row = diacraft_doc::Row { name: Some(Arc::from(name)), ..diacraft_doc::Row::default() };
    row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_number(v).unwrap()));
    s.rows.push(Arc::new(row));
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(s));
}
