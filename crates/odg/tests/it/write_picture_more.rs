//! A new picture in a package, the rest of it: the type told by the bytes, the
//! same bytes as one entry, names no one else holds, a picture shaped as the VSDX reader gives one, what is tier 2, the connection points of a frame, and the caps of a
//! package that DiaCraft reads again.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape};
use diacraft_odg::{Limits, WriteError, WriteOptions, read, write_with};

use crate::build;
use crate::drawing::Drawing;
use crate::flat_write::small_png;
use crate::write_core::{Open, PNG, compare, drawing_of, full_meta, open_bytes, part, part_bytes};
use crate::write_new::{connector, glue, push_shape};
use crate::write_picture::{
    PNG_HEAD, entry_names, frame_of, manifest_rows, named, noise, picture, pictures_of, save, save_with, shown, three, user_cell, vsdx_picture,
};

/// A picture of the type `head` starts, padded to `n` bytes.
fn of_type(head: &[u8], at: usize, n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; at];
    bytes.extend_from_slice(head);
    bytes.resize(n.max(at + head.len()), 7);
    bytes
}

/// The text of the error a save is refused with, and its class.
fn refusal(r: Result<diacraft_odg::Written, WriteError>) -> (&'static str, String) {
    let e = r.expect_err("the save is refused");
    (e.class(), e.to_string())
}

/// The entry's extension and the manifest row's media type are what the bytes say, whatever the model says the picture is, and each reads back as its kind.
#[test]
fn a_picture_is_typed_by_its_own_bytes_and_not_by_the_name_or_type_the_model_gives_it() {
    let cases: [(&str, Vec<u8>, &str, &str); 5] = [
        ("image/png", of_type(&PNG_HEAD, 0, 40), "png", "Bitmap"),
        ("image/jpeg", of_type(&[0xFF, 0xD8, 0xFF], 0, 40), "jpg", "Bitmap"),
        ("image/gif", of_type(b"GIF89a", 0, 40), "gif", "Bitmap"),
        ("image/bmp", of_type(b"BM", 0, 40), "bmp", "Bitmap"),
        ("image/emf", of_type(b" EMF", 40, 80), "emf", "EnhMetaFile"),
    ];
    for (media, bytes, ext, kind) in &cases {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        // The model says it is a TIFF named by a path of another format: the writer goes by the bytes.
        let mut shape = picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), bytes);
        let mut foreign = (**shape.foreign.as_ref().unwrap()).clone();
        let m = foreign.picture.as_ref().unwrap();
        foreign.picture = Some(Arc::new(diacraft_doc::Media {
            part: Arc::from("/visio/media/x.tiff"),
            content_type: Arc::from("image/tiff"),
            bytes: Arc::clone(&m.bytes),
        }));
        shape.foreign = Some(Arc::new(foreign));
        push_shape(&mut o.doc, shape);
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{media}: {e}"));
        let name = format!("Pictures/image1.{ext}");
        assert_eq!(pictures_of(&saved.bytes), std::slice::from_ref(&name), "{media}");
        assert_eq!(part_bytes(&saved.bytes, &name), *bytes, "{media}");
        let row = manifest_rows(&saved.bytes).into_iter().find(|r| r.contains(&name)).unwrap_or_else(|| panic!("{media}: no row"));
        assert!(row.contains(&format!("manifest:media-type=\"{media}\"")), "{row}");
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        assert_eq!(shown(named(&back, "Picture.1")), Some((bytes.clone(), Some((*kind).to_owned()))), "{media}");
    }
}

/// A picture whose bytes are in none of the formats the reader draws is refused in words, and nothing is written (the model keeps it). Near misses of each
/// signature, a Windows metafile, an SVG and nothing at all are such; one of them among drawable pictures refuses the whole save.
#[test]
fn a_picture_whose_type_cannot_be_told_is_refused_in_words_and_nothing_is_written() {
    let wmf = [0xD7, 0xCD, 0xC6, 0x9A, 0, 0, 0, 0, 0, 0, 0, 0];
    let near: Vec<(&str, Vec<u8>)> = vec![
        ("nothing", Vec::new()),
        ("GIF without its 8", b"GIF".to_vec()),
        ("a B", b"B".to_vec()),
        ("a JPEG that is not", vec![0xFF, 0xD7, 1, 2]),
        ("an EMF signature a byte early", of_type(b" EMF", 39, 60)),
        ("an EMF signature a byte late", of_type(b" EMF", 41, 60)),
        ("an EMF signature misspelt", of_type(b" EME", 40, 60)),
        ("a Windows metafile", wmf.to_vec()),
        ("an SVG", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()),
        ("a TIFF", vec![b'I', b'I', 42, 0, 8, 0, 0, 0]),
    ];
    for (what, bytes) in &near {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), bytes));
        let (class, words) = refusal(save(&o.doc, &o.bytes, &o.index));
        assert_eq!(class, "write_unsupported", "{what}");
        assert!(words.contains("format the reader does not draw"), "{what}: {words}");
    }
    // One of them beside a good one: the good one is not written alone.
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Good", (2.0, 8.0, 3.0, 2.0), &small_png()));
    push_shape(&mut o.doc, picture(id + 1, "Bad", (6.0, 8.0, 3.0, 2.0), &wmf));
    assert_eq!(refusal(save(&o.doc, &o.bytes, &o.index)).0, "write_unsupported");
    // Nothing of the model changed by the refusal: the same model saved without the bad one is written.
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let kept: Vec<_> = page.shapes.iter().filter(|s| s.name_u.as_deref() != Some("Bad")).cloned().collect();
    page.shapes = diacraft_doc::SharedVec::from_vec(kept);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
}

/// The same bytes are one entry however many frames show them and however the model holds them (once, shared, or twice, in two `Arc`s); other bytes are another entry, and
/// the names are the least free from the cursor on, in the order the pictures are first written.
#[test]
fn the_same_bytes_twice_are_one_entry_named_by_both_frames_and_other_bytes_are_two() {
    let png = small_png();
    let other = noise(&PNG_HEAD, 64, 1);
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    let shared = picture(id, "A", (1.0, 8.0, 3.0, 2.0), &png);
    // B holds the same media `Arc`; C holds the same bytes in an `Arc` of its own; D holds other bytes of the same length.
    let mut b = shared.clone();
    b.id = id + 1;
    b.name = Some(Arc::from("B"));
    b.name_u = Some(Arc::from("B"));
    let c = picture(id + 2, "C", (9.0, 8.0, 3.0, 2.0), &png);
    let d = picture(id + 3, "D", (13.0, 8.0, 3.0, 2.0), &other);
    for shape in [shared, b, c, d] {
        push_shape(&mut o.doc, shape);
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png", "Pictures/image2.png"]);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), png);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image2.png"), other);
    let content = part(&saved.bytes, "content.xml");
    for (name, entry) in [("A", 1), ("B", 1), ("C", 1), ("D", 2)] {
        let frame = frame_of(&content, name);
        assert!(frame.contains(&format!("xlink:href=\"Pictures/image{entry}.png\"")), "{name}: {frame}");
    }
    // One manifest row for each entry, however many frames name it.
    let rows = manifest_rows(&saved.bytes);
    for entry in ["Pictures/image1.png", "Pictures/image2.png"] {
        assert_eq!(rows.iter().filter(|r| r.contains(&format!("\"{entry}\""))).count(), 1, "{rows:?}");
    }
    // Every frame reads back as its picture; the file is read with no picture left out.
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    for (name, want) in [("A", &png), ("B", &png), ("C", &png), ("D", &other)] {
        assert_eq!(shown(named(&back, name)).map(|s| s.0), Some(want.clone()), "{name}");
    }
    assert_eq!(report.warned("picture_missing"), 0);
}

/// A name that something already holds is not given: an entry of the package, a row of its manifest with no entry, an entry that differs only by its case, and an `href` of the
/// part that no entry answers (a picture added at that name would show in that frame).
#[test]
fn a_name_the_package_its_manifest_or_the_part_already_holds_is_never_given_whatever_its_case() {
    let png = small_png();
    let dangling = "\n    <draw:custom-shape draw:name=\"Gone\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"><text:p><text:a xlink:href=\"./Pictures/image4.png\" xlink:type=\"simple\">a hyperlink at the name</text:a></text:p><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/></draw:custom-shape>";
    let d = Drawing::a4().page(&format!("{}{dangling}", crate::write_core::custom("s1", "1cm")));
    let mut files = d.files();
    files.push(("Pictures/image1.png".to_owned(), png.clone()));
    files.push(("pictures/IMAGE3.png".to_owned(), png.clone()));
    let mut rows: Vec<(String, &str)> =
        files.iter().map(|(n, _)| (n.clone(), if n.to_lowercase().ends_with(".png") { "image/png" } else { "text/xml" })).collect();
    rows.push(("Pictures/image2.png".to_owned(), "image/png"));
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    files.push(("META-INF/manifest.xml".to_owned(), build::manifest(build::DRAWING, &rows).into_bytes()));
    let mut o = open_bytes(build::write(Some(build::DRAWING), &files));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &noise(&PNG_HEAD, 50, 3)));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(entry_names(&saved.bytes).contains(&"Pictures/image5.png".to_owned()), "{:?}", entry_names(&saved.bytes));
    let rows = manifest_rows(&saved.bytes);
    assert_eq!(rows.iter().filter(|r| r.contains("\"Pictures/image5.png\"")).count(), 1, "{rows:?}");
    // The picture the package held first is untouched, and the row that had no entry is still a row with none.
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), png);
}

/// A picture shaped as the VSDX reader gives one (a relationship id, the attributes of its `ForeignData`, the `Img` cells, no outline and no connection rows) is written with
/// what a frame cannot say as tier 2, and reads back with the same cells. Nothing the file holds names the VSDX package it came from, and the save says what of the foreign
/// data it did not write.
#[test]
fn a_picture_shaped_as_the_vsdx_reader_gives_one_is_written_with_tier_2_and_reads_back_with_its_cells() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, vsdx_picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a VSDX picture is written");
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), png);
    // The foreign data beyond the picture rides on the frame's own style: nothing is left out, and a read gives it back.
    assert!(!saved.notes.iter().any(|n| matches!(n, diacraft_odg::SaveNote::NewShapeUnwritten { foreign: true, .. })), "{:?}", saved.notes);
    let (carried, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let foreign = named(&carried, "Picture.1").foreign.as_ref().unwrap();
    assert_eq!((foreign.rel_id.as_deref(), foreign.attr("CompressionType")), (Some("rId2"), Some("PNG")));
    // Read back: the picture and its cells (the `Img` cells are tier 2, and the four connection rows a frame is given are taken away as the model had none).
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let shape = named(&back, "Picture.1");
    assert_eq!(shown(shape), Some((png.clone(), Some("Bitmap".to_owned()))));
    let model = named(&o.doc, "Picture.1");
    for cell in ["ImgOffsetX", "ImgOffsetY", "ImgWidth", "ImgHeight"] {
        let (a, b) = (model.sheet.cell(cell).unwrap(), shape.sheet.cell(cell).unwrap_or_else(|| panic!("{cell} is not read back")));
        assert_eq!(a.formula.text(), b.formula.text(), "{cell}");
        let (x, y) =
            (a.value.as_ref().and_then(diacraft_doc::Value::number).unwrap(), b.value.as_ref().and_then(diacraft_doc::Value::number).unwrap());
        assert!((x - y).abs() < 1e-9, "{cell}: {x} {y}");
    }
    assert!(shape.sheet.sections.iter().all(|s| s.name.as_str() != "Connection" && s.name.as_str() != "Geometry"), "no rows the model did not have");
    // Nothing names the VSDX package: no part, no entry, no href.
    for name in entry_names(&saved.bytes) {
        assert!(!name.to_lowercase().contains("visio"), "{name}");
        if name.ends_with(".xml") {
            assert!(!part(&saved.bytes, &name).to_lowercase().contains("visio"), "{name}");
        }
    }
    // A second save of the same model is no edit; the file opened again and a shape moved keeps the picture's cells.
    let again = save(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited);
    let mut o2 = open_bytes(saved.bytes.clone());
    let s1 = named(&o2.doc, "s1").id;
    crate::write_core::move_by(&mut o2.doc, 0, s1, 1.0, 0.0);
    let third = save(&o2.doc, &o2.bytes, &o2.index).unwrap();
    let (back, _) = read(&third.bytes, &Limits::default()).unwrap();
    assert_eq!(named(&back, "Picture.1").sheet.cell("ImgWidth").and_then(|c| c.formula.text()), Some("Width*1"));
}

/// The prediction of the rebuild is a picture frame's (no outline, the four standard connection rows, the frame cells): a plain picture, whose cells the frame says
/// itself, carries no tier 2 and reads back with its frame cells and its four rows, as the model had them.
#[test]
fn a_plain_picture_frame_carries_no_tier_2_and_reads_back_with_the_cells_and_rows_it_had() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    let before = part(&o.bytes, "content.xml").matches("diacraft:").count();
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("diacraft:").count(), before, "the frame's own markup says all of it: {content}");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let (model, shape) = (named(&o.doc, "Picture.1"), named(&back, "Picture.1"));
    for cell in ["Width", "Height", "LocPinX", "LocPinY", "PinX", "PinY", "FillPattern", "FillForegnd", "LinePattern"] {
        let (a, b) = (model.sheet.cell(cell).unwrap(), shape.sheet.cell(cell).unwrap_or_else(|| panic!("{cell} is not read back")));
        assert_eq!(a.formula.text(), b.formula.text(), "{cell}");
        match (a.value.as_ref().and_then(diacraft_doc::Value::number), b.value.as_ref().and_then(diacraft_doc::Value::number)) {
            (Some(x), Some(y)) => assert!((x - y).abs() < 1e-6, "{cell}: {x} {y}"),
            _ => assert_eq!(a.value.as_ref().map(diacraft_doc::Value::text), b.value.as_ref().map(diacraft_doc::Value::text), "{cell}"),
        }
    }
    let rows = |s: &Shape| s.sheet.sections.iter().find(|x| x.name.as_str() == "Connection").map(|x| x.rows.len());
    assert_eq!((rows(model), rows(shape)), (Some(4), Some(4)));
    assert!(shape.sheet.sections.iter().all(|s| s.name.as_str() != "Geometry"), "a frame has no outline");
}

/// What the frame cannot say is carried as tier 2, whatever it is: an outline (a frame is given none), a section of its own, a cell no style attribute says, a style cell
/// with a formula, a link to a master. Each reads back as the model had it.
#[test]
fn what_a_frame_cannot_say_is_carried_as_tier_2_and_reads_back() {
    let png = small_png();
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    // The outline of a rectangle (the model of a picture that was a shape), a user cell, a cell no style attribute says, a style cell with a formula, a link to a master.
    let mut shape = crate::write_new::rectangle(id, "Picture.1", (2.0, 8.0, 3.0, 2.0));
    shape.kind = diacraft_doc::ShapeType::Foreign;
    shape.foreign = picture(id, "x", (2.0, 8.0, 3.0, 2.0), &png).foreign;
    user_cell(&mut shape, "Mark", 5.0);
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert(CellName::new("Brightness"), Cell::read(None, Some("0.25"), None, None, &mut 1_000));
    sheet.cells.insert(CellName::new("FillForegnd"), Cell::read(Some("THEMEVAL(\"FillColor\")"), Some("#112233"), None, None, &mut 1_000));
    shape.master = Some(3);
    push_shape(&mut o.doc, shape);
    let saved = save(&o.doc, &o.bytes, &o.index).expect("tier 2 carries what the frame cannot say");
    assert!(
        part(&saved.bytes, "content.xml").matches("diacraft:").count() > part(&o.bytes, "content.xml").matches("diacraft:").count(),
        "tier 2 is written"
    );
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    let (model, got) = (named(&o.doc, "Picture.1"), named(&back, "Picture.1"));
    assert_eq!(shown(got).map(|s| s.0), Some(png));
    assert_eq!(got.master, Some(3));
    let cell = |s: &Shape, n: &str| s.sheet.cell(n).map(|c| (c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned())));
    assert_eq!(cell(got, "Brightness").map(|c| c.0), Some(None));
    assert_eq!(cell(got, "FillForegnd").map(|c| c.0), Some(Some("THEMEVAL(\"FillColor\")".to_owned())));
    let section = |s: &Shape, n: &str| s.sheet.sections.iter().find(|x| x.name.as_str() == n).map(|x| x.rows.len());
    assert_eq!(section(got, "Geometry"), section(model, "Geometry"), "the outline is carried");
    assert_eq!(section(got, "User"), Some(1));
    assert_eq!(section(got, "Connection"), Some(4));
}

/// A frame has the four connection points ODF gives every shape: a connector glued to a new picture's rows 0 to 3 is written with the glue id of that point (the
/// rows 0, 1, 2 and 3 are ids 0, 3, 2 and 1) and reads back glued to that row; one glued to a later row of a picture is refused in words.
#[test]
fn a_connector_is_glued_to_the_four_connection_points_of_a_new_picture_and_not_to_a_fifth() {
    let png = small_png();
    for (row, glue_id) in [(0u32, 0u32), (1, 3), (2, 2), (3, 1)] {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
        push_shape(&mut o.doc, connector(id + 1, (12.0, 3.0, 8.0, 12.0)));
        glue(&mut o.doc, id + 1, false, id, row);
        let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("row {row}: {e}"));
        let content = part(&saved.bytes, "content.xml");
        let tag = content.split("<draw:connector").nth(1).and_then(|t| t.split('>').next()).unwrap();
        assert!(tag.contains(&format!("draw:end-glue-point=\"{glue_id}\"")), "row {row}: {tag}");
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        let page = back.pages.get(0).unwrap();
        let target = named(&back, "Picture.1").id;
        assert!(page.connects.iter().any(|c| c.to_sheet == target && c.to_part == Some(100 + row)), "row {row}: {:?}", page.connects);
    }
    // A fifth point (row 4) is not a point a frame has: refused in words.
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &png));
    push_shape(&mut o.doc, connector(id + 1, (12.0, 3.0, 8.0, 12.0)));
    glue(&mut o.doc, id + 1, false, id, 4);
    let (class, words) = refusal(save(&o.doc, &o.bytes, &o.index));
    assert_eq!(class, "write_unsupported");
    assert!(words.contains("past the four a frame has in ODF"), "{words}");
}

/// The entries of the new pictures come before the thumbnail, and so do their manifest rows.
#[test]
fn the_pictures_come_before_the_thumbnail_in_the_package_and_in_the_manifest() {
    let mut o = open_bytes(drawing_of(2).bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let saved = write_with(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() },
    )
    .unwrap();
    let names = entry_names(&saved.bytes);
    let at = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n}: {names:?}"));
    assert!(at("Pictures/image1.png") < at("Thumbnails/thumbnail.png") && at("Thumbnails/thumbnail.png") < at("META-INF/manifest.xml"), "{names:?}");
    let rows = manifest_rows(&saved.bytes);
    let row = |n: &str| rows.iter().position(|r| r.contains(&format!("\"{n}\""))).unwrap_or_else(|| panic!("{n}: {rows:?}"));
    assert!(row("Pictures/image1.png") < row("Thumbnails/thumbnail.png"), "{rows:?}");
}

/// The caps of a package DiaCraft reads again, each by itself: a picture is written when it fits and refused in words, as a limit, when it does not. The picture at exactly the
/// cap for a part, and one byte more; the bytes over the package at exactly what is left; the entries and the rows of the manifest; and the bytes of the manifest.
#[test]
fn each_cap_of_the_package_a_picture_is_written_in_holds_at_its_edge_and_not_past_it() {
    let one = |bytes: &[u8], limits: Limits| {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), bytes));
        save_with(&o.doc, &o.bytes, &o.index, limits)
    };
    let limit_of = |r: Result<diacraft_odg::Written, WriteError>| match r {
        Err(WriteError::Limit { what, .. }) => what,
        Ok(_) => "saved",
        Err(e) => panic!("another error: {e}"),
    };
    let container = |f: &dyn Fn(&mut diacraft_container::Limits)| {
        let mut l = Limits::default();
        f(&mut l.container);
        l
    };
    // The cap for one part: a picture of exactly the cap is written (and the package it makes opens under the same limits), one byte more is refused.
    let at_cap = noise(&PNG_HEAD, 4_000, 5);
    let limits = container(&|c| c.max_part_bytes = 4_000);
    let saved = one(&at_cap, limits.clone()).expect("a picture of exactly the cap");
    assert!(read(&saved.bytes, &limits).is_ok(), "the package made is read under the limit it was written under");
    assert!(limit_of(one(&noise(&PNG_HEAD, 4_001, 5), limits)).contains("one picture"));
    // The bytes over the package: the package made holds the original's entries, the picture and the part grown by its frame; exactly that is written, one byte less is a limit.
    let picture_bytes = noise(&PNG_HEAD, 900, 6);
    let made: u64 = {
        let saved = one(&picture_bytes, Limits::default()).unwrap();
        diacraft_odg::package::Package::read(&saved.bytes, &Limits::default()).unwrap().entries().map(|e| e.size).sum::<u64>()
    };
    // `Package::entries` is the parts: `mimetype` and the manifest are entries of the archive too, so the edge is found by trying.
    let fits = |cap: u64| one(&picture_bytes, container(&|c| c.max_package_bytes = cap)).is_ok();
    let (mut lo, mut hi) = (0u64, made + 100_000);
    assert!(fits(hi) && !fits(lo), "a wide cap admits it and a cap of nothing does not");
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if fits(mid) { hi = mid } else { lo = mid }
    }
    assert!(!fits(hi - 1) && fits(hi), "the edge is one byte wide: {hi}");
    assert!(limit_of(one(&picture_bytes, container(&|c| c.max_package_bytes = hi - 1))).contains("in a package"));
    // The entries: the package holds E entries, so room for E + 1 takes the picture and room for E does not.
    let entries = entry_names(&three().bytes).len();
    assert!(one(&picture_bytes, container(&|c| c.max_parts = entries + 1)).is_ok());
    assert!(limit_of(one(&picture_bytes, container(&|c| c.max_parts = entries))).contains("entries"));
    // The rows of the manifest (the root row is one): one row more is taken, none more is a limit.
    let rows = manifest_rows(&three().bytes).len();
    assert!(one(&picture_bytes, Limits { max_manifest_entries: rows + 1, ..Limits::default() }).is_ok());
    assert!(limit_of(one(&picture_bytes, Limits { max_manifest_entries: rows, ..Limits::default() })).contains("rows of a manifest"));
    // The bytes of the manifest: exactly the size it comes to is written, one less is refused.
    let size = part(&one(&picture_bytes, Limits::default()).unwrap().bytes, "META-INF/manifest.xml").len() as u64;
    assert!(one(&picture_bytes, Limits { max_manifest_bytes: size, ..Limits::default() }).is_ok());
    assert!(limit_of(one(&picture_bytes, Limits { max_manifest_bytes: size - 1, ..Limits::default() })).contains("manifest"));
}

/// The memory a save may hold: a picture takes twice its bytes from the room (the deflated copy and the archive put together) before either is made, so a limit that leaves
/// exactly that room admits it and one byte less refuses it as a limit.
#[test]
fn the_memory_for_a_picture_is_taken_from_the_saves_room_before_it_is_copied() {
    let bytes = noise(&PNG_HEAD, 3_000, 7);
    let one = |limits: Limits| {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &bytes));
        save_with(&o.doc, &o.bytes, &o.index, limits)
    };
    // What the save estimates it holds before the picture, and the room that leaves: from a save with the gate wide open and one with the picture left out.
    let wide = one(Limits::default()).unwrap();
    let held = wide.work.estimated_memory;
    assert!(held > 0);
    let exact = Limits { max_write_memory: held + 2 * 3_000, ..Limits::default() };
    assert!(one(exact).is_ok(), "exactly the room");
    let tight = Limits { max_write_memory: held + 2 * 3_000 - 1, ..Limits::default() };
    match one(tight) {
        Err(WriteError::Limit { what, .. }) => assert!(what.contains("memory"), "{what}"),
        other => panic!("one byte short of the room is a limit: {:?}", other.map(|w| w.bytes.len())),
    }
}

/// A picture that deflates past the ratio the package layer reads would make a package that does not open: the save is refused, in words, and nothing is written. One that
/// deflates a long way and not that far is written.
#[test]
fn a_picture_that_deflates_past_the_ratio_the_reader_allows_is_refused_in_words() {
    let mut zeros = PNG_HEAD.to_vec();
    zeros.resize(3_000_000, 0);
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &zeros));
    let (class, words) = refusal(save(&o.doc, &o.bytes, &o.index));
    assert_eq!(class, "write_unsupported");
    assert!(words.contains("ratio"), "{words}");
    // A run of 20 KB deflates to some tens of bytes, which is a ratio the container reads.
    let mut some = PNG_HEAD.to_vec();
    some.resize(20_000, 0);
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &some));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a picture that deflates well is written");
    assert!(read(&saved.bytes, &Limits::default()).is_ok());
}

/// The doc's `ForeignData` of the model are the model's after a save: the picture is the same `Arc` and the shape is the shape the engine handed in.
#[test]
fn a_save_leaves_the_model_as_it_was() {
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let before: Document = o.doc.clone();
    let _ = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(before, o.doc);
}

/// A copy of a frame the file already holds (a paste in the engine: a new shape that shows the very `Arc` of the picture the first shows) is a new picture like any other: its
/// bytes are written under a name of their own. **The entry the file holds is never reused**: that it holds the same bytes would need it inflated, and a save reads no entry it
/// is not told to; the cost is a copy of the picture in the file, which is the rule's to change (the report's question).
#[test]
fn a_copy_of_a_frame_the_file_holds_is_written_as_a_picture_of_its_own_and_the_files_entry_is_left_as_it_is() {
    let png = small_png();
    let frame = "\n    <draw:frame draw:name=\"Existing\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"3cm\" svg:height=\"2cm\"><draw:image xlink:href=\"Pictures/a.png\"/></draw:frame>";
    let mut d = Drawing::a4().page(frame);
    d.files.push(("Pictures/a.png".to_owned(), png.clone()));
    let mut o = open_bytes(d.bytes());
    let existing = named(&o.doc, "Existing").clone();
    assert_eq!(shown(&existing).map(|s| s.0), Some(png.clone()), "the file's picture is read");
    let mut copy = existing.clone();
    copy.id = o.index.next_id(0).unwrap();
    copy.name = Some(Arc::from("Copy"));
    copy.name_u = Some(Arc::from("Copy"));
    crate::write_core::set_num(&mut copy, "PinX", 5.0);
    push_shape(&mut o.doc, copy);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/a.png", "Pictures/image1.png"]);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/a.png"), png);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), png);
    let content = part(&saved.bytes, "content.xml");
    assert!(frame_of(&content, "Existing").contains("xlink:href=\"Pictures/a.png\""), "the first frame is the file's own");
    assert!(frame_of(&content, "Copy").contains("xlink:href=\"Pictures/image1.png\""));
    let diff = compare(&o.bytes, &saved.bytes);
    assert!(diff.added.contains(&"Pictures/image1.png".to_owned()), "{diff:?}");
    assert!(
        !diff.added.contains(&"Pictures/a.png".to_owned()) && !diff.bytes_only.contains(&"Pictures/a.png".to_owned()),
        "the file's entry is copied: {diff:?}"
    );
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(shown(named(&back, "Copy")).map(|s| s.0), Some(png));
}

/// The manifest is edited by rows, never written again: a manifest in another hand (single quotes, tabs, another prefix) keeps every row's bytes, and the row for the
/// new picture follows them with the file's own prefix and indent.
#[test]
fn the_row_of_a_picture_is_added_in_the_hand_the_manifest_is_written_in_and_the_rows_it_had_keep_their_bytes() {
    let d = drawing_of(2);
    let mut files = d.files();
    let media = build::DRAWING;
    let mut manifest = format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n<m:manifest xmlns:m='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' m:version='1.3'>\n\t<m:file-entry m:full-path='/' m:media-type='{media}'/>\n"
    );
    for (name, _) in &files {
        manifest.push_str(&format!("\t<m:file-entry m:full-path='{name}' m:media-type='text/xml'/>\n"));
    }
    manifest.push_str("</m:manifest>\n");
    files.push(("META-INF/manifest.xml".to_owned(), manifest.clone().into_bytes()));
    let mut o = open_bytes(build::write(Some(media), &files));
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "META-INF/manifest.xml");
    let rows_before = manifest.trim_end().trim_end_matches("</m:manifest>");
    assert!(after.starts_with(rows_before.trim_end_matches('\n')), "every row the file had keeps its bytes:\n{after}");
    assert!(
        after.contains("\t<m:file-entry m:full-path=\"Pictures/image1.png\" m:media-type=\"image/png\"/>"),
        "the row has the manifest's own prefix and indent:\n{after}"
    );
    assert_eq!(after.matches("Pictures/image1.png").count(), 1);
}

/// A new picture is written into a template (`.otg`), and into a drawing saved as a template: the package states the kind it was asked for and holds the entry and its row.
#[test]
fn a_new_picture_is_written_into_a_package_saved_as_a_template() {
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let options = WriteOptions { variant: Some(diacraft_odg::Variant::Template), ..WriteOptions::default() };
    let saved = write_with(&o.doc, Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }), &options).unwrap();
    assert_eq!(diacraft_odg::sniff(&saved.bytes), Some(diacraft_odg::Variant::Template));
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
    assert_eq!(entry_names(&saved.bytes).first().map(String::as_str), Some("mimetype"));
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(shown(named(&back, "Picture.1")).is_some());
}

/// The new pictures of a save are named once over both parts: a picture put on a master page goes into `styles.xml` and one on a page into `content.xml`, and no two entries
/// share a name (`content.xml` is written first, so its picture is the first).
#[test]
fn pictures_on_a_page_and_on_a_master_page_are_named_once_over_the_two_parts() {
    let mut d = Drawing::a4().page(&crate::write_core::custom("s1", "1cm"));
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><draw:rect draw:name="bg" svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></style:master-page>"#,
    );
    let mut o = open_bytes(d.bytes());
    let background = o.doc.pages.iter().position(|p| p.background).expect("the master page is a background page of the model");
    let foreground = o.doc.pages.iter().position(|p| !p.background).unwrap();
    let (first, second) = (small_png(), small_bmp());
    let ids = (o.index.next_id(foreground).unwrap(), o.index.next_id(background).unwrap());
    for (at, id, name, bytes) in [(foreground, ids.0, "OnPage", &first), (background, ids.1, "OnMaster", &second)] {
        let page = Arc::make_mut(o.doc.pages.get_mut(at).unwrap());
        let mut shape = picture(id, name, (2.0, 8.0, 3.0, 2.0), bytes);
        shape.id = id;
        page.shapes.push(Arc::new(shape));
    }
    let saved = save(&o.doc, &o.bytes, &o.index).expect("a picture on a master page is written");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png", "Pictures/image2.bmp"]);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image1.png"), first);
    assert_eq!(part_bytes(&saved.bytes, "Pictures/image2.bmp"), second);
    assert!(frame_of(&part(&saved.bytes, "content.xml"), "OnPage").contains("xlink:href=\"Pictures/image1.png\""));
    assert!(frame_of(&part(&saved.bytes, "styles.xml"), "OnMaster").contains("xlink:href=\"Pictures/image2.bmp\""));
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("picture_missing"), 0);
    let shown_on = |background: bool, name: &str| {
        back.pages
            .iter()
            .filter(|p| p.background == background)
            .flat_map(|p| p.shapes.iter())
            .find(|s| s.name_u.as_deref() == Some(name))
            .and_then(|s| shown(s))
            .map(|s| s.0)
    };
    assert_eq!(shown_on(false, "OnPage"), Some(first));
    assert_eq!(shown_on(true, "OnMaster"), Some(second));
    assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "a second save is no edit");
}

/// The book counts the entries the pictures add; the save adds others beside them (a thumbnail, a `meta.xml`), and the package made is held to the cap the reader opens a package
/// under: past it the save is a limit, in words, and not a package that the next open refuses.
#[test]
fn an_entry_the_save_makes_beside_the_pictures_that_takes_the_package_past_its_cap_is_a_limit() {
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let entries = entry_names(&o.bytes).len();
    let mut limits = Limits::default();
    limits.container.max_parts = entries + 1;
    let with_thumbnail = |limits: Limits| {
        write_with(
            &o.doc,
            Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
            &WriteOptions { thumbnail: Some(PNG.to_vec()), limits, ..WriteOptions::default() },
        )
    };
    // The picture fits the room the book counts (one entry); the thumbnail is the second, and the package made has one more than the reader opens.
    match with_thumbnail(limits.clone()) {
        Err(WriteError::Limit { what, max }) => assert!(what.contains("entries") && max == (entries + 1) as u64, "{what} {max}"),
        other => panic!("a limit: {:?}", other.map(|w| w.bytes.len())),
    }
    limits.container.max_parts = entries + 2;
    let saved = with_thumbnail(limits.clone()).expect("room for both");
    assert!(read(&saved.bytes, &limits).is_ok());
}

/// A name no XML part can hold (a control character) is refused in words, whether it is the universal name the frame is written with or the local name tier 2 carries, and nothing
/// is written.
#[test]
fn a_picture_named_with_a_character_xml_cannot_hold_is_refused_in_words_by_either_of_its_names() {
    for (what, name, name_u) in [("the universal name", "ok", "a\u{1}b"), ("the local name", "a\u{1}b", "ok"), ("both", "a\u{1}b", "a\u{1}b")] {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        let mut shape = picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png());
        shape.name = Some(Arc::from(name));
        shape.name_u = Some(Arc::from(name_u));
        push_shape(&mut o.doc, shape);
        let (class, words) = refusal(save(&o.doc, &o.bytes, &o.index));
        assert_eq!(class, "write_unsupported", "{what}");
        assert!(words.contains("character"), "{what}: {words}");
    }
}

/// The save reads the file it made again for the tier 2 it wrote: a new picture that carries some has it read back (the bytes read are counted), and one that carries
/// none has no read made for it.
#[test]
fn the_tier_2_a_new_picture_carries_is_read_back_by_the_check_of_the_bytes_made() {
    let saved_with = |edit: &dyn Fn(&mut Shape)| {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        let mut shape = picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png());
        edit(&mut shape);
        push_shape(&mut o.doc, shape);
        save(&o.doc, &o.bytes, &o.index).unwrap()
    };
    let plain = saved_with(&|_| {});
    assert_eq!(plain.work.read_back_bytes, 0, "a plain picture carries nothing to read back");
    let carrying = saved_with(&|s| user_cell(s, "Mark", 5.0));
    assert!(carrying.work.read_back_bytes > 0, "the tier 2 a picture carries is read back: {}", carrying.work.read_back_bytes);
}

/// A package made as a template, of three shapes and a generator: the kind a new drawing is made from, saved as a drawing.
fn template_three() -> Open {
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    open_bytes(build::package(build::TEMPLATE, d.files()))
}

/// The save of `o` as a drawing (a kind asked for, which the package made states in `mimetype` and in the manifest's root row), with a thumbnail when `thumbnail` and `limits`.
fn saved_as_a_drawing(o: &Open, thumbnail: bool, limits: Limits) -> Result<diacraft_odg::Written, WriteError> {
    write_with(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &WriteOptions {
            variant: Some(diacraft_odg::Variant::Drawing),
            thumbnail: thumbnail.then(|| PNG.to_vec()),
            limits,
            ..WriteOptions::default()
        },
    )
}

/// Makes a new drawing from a template and saves it as a drawing, and a VSDX drawing saved as an `.odg` goes into that package: a new picture is written when the save
/// asks for the other kind, and the package says the kind it was asked for.
#[test]
fn a_new_picture_is_written_when_a_template_is_saved_as_a_drawing() {
    let mut o = template_three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let saved = saved_as_a_drawing(&o, true, Limits::default()).expect("a picture is written into a template saved as a drawing");
    assert_eq!(diacraft_odg::sniff(&saved.bytes), Some(diacraft_odg::Variant::Drawing));
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
    let names = entry_names(&saved.bytes);
    let at = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n}: {names:?}"));
    assert!(at("Pictures/image1.png") < at("Thumbnails/thumbnail.png"), "{names:?}");
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(shown(named(&back, "Picture.1")).is_some());
}

/// The words of a refusal are the picture's, whether or not the save asks for a kind: a package made past a cap, or a picture that deflates past the ratio, is a limit or a
/// refusal in words and not a fault of the kind's check.
#[test]
fn a_picture_that_the_package_made_cannot_hold_is_refused_in_its_own_words_when_a_template_is_saved_as_a_drawing() {
    let mut zeros = PNG_HEAD.to_vec();
    zeros.resize(3_000_000, 0);
    let mut o = template_three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &zeros));
    let (class, words) = refusal(saved_as_a_drawing(&o, false, Limits::default()));
    assert_eq!((class, words.contains("ratio")), ("write_unsupported", true), "{words}");
    // An entry the save makes beside the picture (the thumbnail) that takes the package past the cap the reader opens one under.
    let mut o = template_three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "Picture.1", (2.0, 8.0, 3.0, 2.0), &small_png()));
    let mut limits = Limits::default();
    limits.container.max_parts = entry_names(&o.bytes).len() + 1;
    match saved_as_a_drawing(&o, true, limits) {
        Err(WriteError::Limit { what, .. }) => assert!(what.contains("entries"), "{what}"),
        other => panic!("a limit: {:?}", other.map(|w| w.bytes.len())),
    }
}

/// A picture turned about its centre by `radians` (the model holds the turn in radians, shown in degrees).
fn turned(id: u32, name: &str, frame: (f64, f64, f64, f64), bytes: &[u8], radians: f64) -> Shape {
    let mut shape = picture(id, name, frame, bytes);
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Angle"), Cell::read(None, Some(&radians.to_string()), Some("DEG"), None, &mut 1_000));
    shape
}

fn cell_number(shape: &Shape, name: &str) -> f64 {
    shape.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0)
}

/// A new picture that is turned is written as a custom shape that is: `svg:width`, `svg:height` and `draw:transform="rotate (..)
/// translate (..)"` and no `svg:x` or `svg:y`, in a package and in a flat drawing. The reader gives the turn back as `Angle`, so the frame needs no tier 2; the file's frame
/// is where the model has it, and a second save is no edit.
#[test]
fn a_turned_new_picture_is_written_with_a_transform_and_reads_back_in_the_same_place_without_tier_2() {
    let png = small_png();
    for radians in [std::f64::consts::FRAC_PI_6, -0.9, 3.0, -3.0, std::f64::consts::FRAC_PI_2] {
        for flat in [false, true] {
            let mut o = if flat {
                let mut d = drawing_of(3);
                d.meta = Some(full_meta());
                open_bytes(crate::flat::Flat::of(&d).bytes())
            } else {
                three()
            };
            let id = o.index.next_id(0).unwrap();
            push_shape(&mut o.doc, turned(id, "Picture.1", (2.0, 8.0, 4.0, 2.0), &png, radians));
            let before = if flat { String::from_utf8(o.bytes.clone()).unwrap() } else { part(&o.bytes, "content.xml") };
            let saved = save(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{radians} (flat {flat}): {e}"));
            let content = if flat { String::from_utf8(saved.bytes.clone()).unwrap() } else { part(&saved.bytes, "content.xml") };
            let frame = frame_of(&content, "Picture.1");
            let tag = frame.split('>').next().unwrap();
            assert!(tag.contains("draw:transform=\"rotate ("), "{radians}: {tag}");
            assert!(!tag.contains("svg:x=") && !tag.contains("svg:y="), "a turned frame has no corner: {tag}");
            assert_eq!(
                content.matches("diacraft:").count(),
                before.matches("diacraft:").count(),
                "{radians}: the turn is the frame's own, no tier 2"
            );
            let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
            let (model, got) = (named(&o.doc, "Picture.1"), named(&back, "Picture.1"));
            assert_eq!(shown(got).map(|s| s.0), Some(png.clone()));
            for name in ["PinX", "PinY", "Width", "Height"] {
                assert!((cell_number(model, name) - cell_number(got, name)).abs() < 1e-6, "{radians} (flat {flat}): {name}");
            }
            assert!((cell_number(model, "Angle") - cell_number(got, "Angle")).abs() < 1e-6, "{radians} (flat {flat}): the turn reads back");
            assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "{radians} (flat {flat}): a second save is no edit");
        }
    }
}

/// A flip has no mirror on a frame that the reader knows (the mirrors are on a custom shape's geometry), so a flipped new picture is refused in words, alone or with a turn,
/// and nothing is written; the same picture unflipped is.
#[test]
fn a_flipped_new_picture_is_refused_in_words_and_a_turned_one_is_not() {
    let png = small_png();
    for (flips, turn) in [(("FlipX", "1"), 0.0), (("FlipY", "1"), 0.0), (("FlipX", "1"), 0.7)] {
        let mut o = three();
        let id = o.index.next_id(0).unwrap();
        let mut shape = turned(id, "Picture.1", (2.0, 8.0, 4.0, 2.0), &png, turn);
        Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(flips.0), Cell::read(None, Some(flips.1), None, None, &mut 1_000));
        push_shape(&mut o.doc, shape);
        let (class, words) = refusal(save(&o.doc, &o.bytes, &o.index));
        assert_eq!(class, "write_unsupported", "{flips:?} {turn}");
        assert!(words.contains("flipped"), "{words}");
    }
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, turned(id, "Picture.1", (2.0, 8.0, 4.0, 2.0), &png, 0.7));
    save(&o.doc, &o.bytes, &o.index).expect("the turned picture is written");
}

/// Adds a page to a drawing: the shapes of the new page go through the same writer as a page's own, so a picture on an added page names a new entry beside the one a picture
/// on a page of the file names (one book for the save: the second picture is `image2`, and the same bytes on both pages are one entry).
#[test]
fn pictures_on_an_added_page_and_on_a_page_of_the_file_share_the_names_and_the_entries_of_the_save() {
    let (png, bmp) = (small_png(), small_bmp());
    let mut o = open_bytes(crate::write_page_ops::pages_of(2).bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, picture(id, "OnFile", (2.0, 8.0, 3.0, 2.0), &png));
    let shapes = vec![picture(1, "OnNew", (2.0, 8.0, 3.0, 2.0), &bmp), picture(2, "OnNewAgain", (6.0, 8.0, 3.0, 2.0), &png)];
    let page = crate::write_page_ops::new_page(&o.doc, 7, "page3", shapes);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o.doc, &o.bytes, &o.index).expect("pictures on an added page are written");
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png", "Pictures/image2.bmp"], "the same bytes on both pages are one entry");
    let content = part(&saved.bytes, "content.xml");
    assert!(frame_of(&content, "OnFile").contains("Pictures/image1.png"));
    assert!(frame_of(&content, "OnNewAgain").contains("Pictures/image1.png"));
    assert!(frame_of(&content, "OnNew").contains("Pictures/image2.bmp"));
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("picture_missing"), 0);
    assert_eq!(back.pages.len(), 3);
    let on =
        |name: &str| back.pages.iter().flat_map(|p| p.shapes.iter()).find(|s| s.name_u.as_deref() == Some(name)).and_then(|s| shown(s)).map(|s| s.0);
    assert_eq!((on("OnFile"), on("OnNew"), on("OnNewAgain")), (Some(png.clone()), Some(bmp), Some(png)));
    assert!(!save(&o.doc, &saved.bytes, &saved.index).unwrap().edited, "a second save is no edit");
    // A page that holds only a picture, and no other change: the picture is the whole edit.
    let mut o = open_bytes(crate::write_page_ops::pages_of(1).bytes());
    let page = crate::write_page_ops::new_page(&o.doc, 9, "page2", vec![picture(1, "Only", (2.0, 8.0, 3.0, 2.0), &small_png())]);
    o.doc.pages.push(Arc::new(page));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert_eq!(pictures_of(&saved.bytes), ["Pictures/image1.png"]);
}

/// A 2 by 2 BMP (24 bits a pixel, each row padded to four bytes), written from the format's own header fields, for a picture LibreOffice decodes.
pub fn small_bmp() -> Vec<u8> {
    let mut v = b"BM".to_vec();
    v.extend(70u32.to_le_bytes());
    v.extend([0, 0, 0, 0]);
    v.extend(54u32.to_le_bytes());
    v.extend(40u32.to_le_bytes());
    v.extend(2i32.to_le_bytes());
    v.extend(2i32.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(24u16.to_le_bytes());
    v.extend([0u8; 24]);
    for row in 0..2u8 {
        for col in 0..2u8 {
            v.extend([200 * col, 60 + 100 * row, 255 - 200 * col]);
        }
        v.extend([0, 0]);
    }
    v
}

/// A 1 by 1 GIF (the format's smallest image: a two-colour table, one pixel).
pub fn small_gif() -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend([1, 0, 1, 0, 0x80, 0, 0, 0xFF, 0, 0, 0, 0xFF, 0]);
    v.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B]);
    v
}

/// `DIACRAFT_PICTURE_DIR`: where the files are written (run with `--run-ignored only`): packages with new pictures of three types, a shared picture, a VSDX-shaped one carrying
/// tier 2 with a connector glued to it, the same file opened and saved again after a move, and a flat drawing with a picture that carries tier 2. LibreOffice opens them as a
/// black box.
#[test]
#[ignore = "writes the files the LibreOffice check opens"]
fn write_picture_files_for_libreoffice() {
    let dir = std::path::PathBuf::from(std::env::var("DIACRAFT_PICTURE_DIR").expect("DIACRAFT_PICTURE_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    // Three types, side by side, and the PNG shown twice (one entry).
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    for (k, (name, bytes)) in [("Png", small_png()), ("Bmp", small_bmp()), ("Gif", small_gif()), ("PngAgain", small_png())].into_iter().enumerate() {
        push_shape(&mut o.doc, picture(id + k as u32, name, (1.0 + 4.0 * k as f64, 14.0, 3.0, 3.0), &bytes));
    }
    // A fifth, turned by 30 degrees (counterclockwise, as the model has it).
    push_shape(&mut o.doc, turned(id + 4, "Turned", (3.0, 20.0, 6.0, 3.0), &small_png(), std::f64::consts::FRAC_PI_6));
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    write("types.odg", &saved.bytes);
    // The same file opened again, a shape moved, saved again.
    let mut again = open_bytes(saved.bytes.clone());
    let s1 = named(&again.doc, "s1").id;
    crate::write_core::move_by(&mut again.doc, 0, s1, 1.0, 0.0);
    write("types_moved.odg", &save(&again.doc, &again.bytes, &again.index).unwrap().bytes);
    // A picture as the VSDX reader gives one, with a connector glued to its bottom point.
    let mut o = three();
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, vsdx_picture(id, "Picture.1", (3.0, 6.0, 5.0, 4.0), &small_png()));
    push_shape(&mut o.doc, connector(id + 1, (12.0, 3.0, 5.5, 20.0)));
    glue(&mut o.doc, id + 1, false, id, 2);
    write("vsdx_shaped.odg", &save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
    // A flat drawing with a picture that carries tier 2.
    let mut d = drawing_of(3);
    d.meta = Some(full_meta());
    let mut o = open_bytes(crate::flat::Flat::of(&d).bytes());
    let id = o.index.next_id(0).unwrap();
    push_shape(&mut o.doc, vsdx_picture(id, "Picture.1", (3.0, 6.0, 5.0, 4.0), &small_png()));
    write("flat.fodg", &save(&o.doc, &o.bytes, &o.index).unwrap().bytes);
}
