//! The public specification's vectors (`docs/diacraft-format.md`, section 8), copied by hand: each package is built from the text the spec shows, read, and held to the model the
//! spec says it decodes to, to the content stamp the spec states (8.4), and written back to the bytes of the canonical form. The refusals of 8.3 are here too.
//!
//! The vectors' manifests say `format="1.0"` and the manifest this build writes says its own version (`1.1`, the spec's 2.4 and 5.1): the vectors compare the document, page and
//! state parts byte for byte, and the manifest by what it lists.
use std::sync::Arc;

use diacraft_doc::{Document, ShapeType};
use diacraft_native::{FORMAT, NativeError, ReadNote, WriteOptions, read, write};
use diacraft_sheetxml::model::{Budget, Digest, content_stamp, stamp_of_bytes};

use crate::util::*;
use crate::zipbuild::Item;

fn stamp(doc: &Document) -> String {
    content_stamp(doc, &mut Budget::new(64_000_000, 640 << 20, 384 << 20)).unwrap().text()
}

fn cell(sheet: &diacraft_doc::Sheet, name: &str) -> (Option<String>, Option<String>, Option<String>) {
    let cell = sheet.cell(name).unwrap_or_else(|| panic!("no cell {name}"));
    (cell.value.as_ref().map(|v| v.text().to_owned()), cell.formula.text().map(str::to_owned), cell.unit.as_ref().map(|u| u.to_string()))
}

fn text(v: Option<&str>, f: Option<&str>, u: Option<&str>) -> (Option<String>, Option<String>, Option<String>) {
    (v.map(str::to_owned), f.map(str::to_owned), u.map(str::to_owned))
}

/// What a write gives back of the parts that the spec says come back in the canonical bytes.
fn written_parts(doc: &Document, opened: &diacraft_native::Opened) -> Vec<u8> {
    write(doc, &opened.state, None, &WriteOptions::default()).unwrap().bytes
}

/// Vector 1's model, as the spec says it decodes to.
fn assert_vector_1_model(doc: &Document) {
    assert!(doc.parts.is_empty());
    assert_vector_1_model_but_parts(doc);
}

fn assert_vector_1_model_but_parts(doc: &Document) {
    assert_eq!(&*doc.title, "Untitled");
    assert!(doc.masters.is_empty() && doc.themes.is_empty() && doc.colors.is_empty() && doc.fonts.is_empty());
    assert!(doc.vsdx.is_none() && doc.ext.is_empty() && doc.props.custom.is_empty());
    assert_eq!(doc.styles.len(), 1);
    let style = doc.styles.get(0).unwrap();
    assert_eq!((style.id, style.name.as_deref(), style.name_u.as_deref()), (0, Some("No Style"), Some("No Style")));
    assert_eq!(style.sheet.cells.len(), 18);
    assert_eq!(cell(&style.sheet, "BottomMargin"), text(Some("0.05555556"), None, None));
    assert_eq!(style.sheet.sections.len(), 1);
    let section = &style.sheet.sections[0];
    assert_eq!(section.rows.len(), 1);
    assert_eq!(section.rows[0].ix, Some(0));
    assert_eq!(section.rows[0].cells.len(), 2);
    assert_eq!(doc.pages.len(), 1);
    let page = doc.pages.get(0).unwrap();
    assert_eq!((page.id, &*page.name, page.name_u.as_deref()), (1, "Page-1", Some("Page-1")));
    assert_eq!(page.sheet.cells.len(), 8);
    assert_eq!(cell(&page.sheet, "PageWidth"), text(Some("11"), None, Some("IN")));
    assert_eq!(cell(&page.sheet, "PageHeight"), text(Some("8.5"), None, Some("IN")));
    assert_eq!(cell(&page.sheet, "DrawingScale"), text(Some("1"), None, Some("IN")));
    assert!(page.connects.is_empty());
}

#[test]
fn vector_1_a_blank_drawing() {
    let bytes = package(&vector_1());
    let (doc, opened) = open_ok(&bytes);
    assert_vector_1_model(&doc);
    assert!(doc.pages.get(0).unwrap().shapes.is_empty());
    // The state has no content stamp and no ledger entry; the content stamp (3.17) of the model is the spec's.
    assert_eq!(opened.state, diacraft_native::State::default());
    assert_eq!(opened.kind, diacraft_native::Kind::Drawing);
    assert_eq!(opened.format, (1, 0));
    assert!(opened.notes.is_empty());
    assert_eq!(stamp(&doc), "00ebc460d282d097");
    // Written back, the document, page and state parts are the vector's bytes; the manifest lists the same entries under this build's version.
    let written = written_parts(&doc, &opened);
    for name in ["document.xml", "pages/page1.xml", "state.xml"] {
        assert_eq!(entry(&written, name), entry(&bytes, name), "{name}");
    }
    assert_eq!(names(&written), names(&bytes));
    let manifest = String::from_utf8(entry(&written, "manifest.xml")).unwrap();
    assert!(manifest.contains(&format!("format=\"{}.{}\" kind=\"drawing\"", FORMAT.0, FORMAT.1)), "{manifest}");
    for row in [
        "<diacraft:part path=\"document.xml\" role=\"document\" required=\"1\"/>",
        "<diacraft:part path=\"pages/page1.xml\" role=\"page\" required=\"1\"/>",
        "<diacraft:part path=\"state.xml\" role=\"state\"/>",
    ] {
        assert!(manifest.contains(row), "{row} in {manifest}");
    }
}

#[test]
fn vector_2_one_rectangle() {
    let bytes = package(&vector_2());
    let (doc, opened) = open_ok(&bytes);
    let page = doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 1);
    let shape = page.shapes.get(0).unwrap();
    assert_eq!((shape.id, shape.name.as_deref(), &shape.kind), (1, Some("Rectangle"), &ShapeType::Shape));
    assert_eq!(shape.sheet.cells.len(), 6);
    assert_eq!(cell(&shape.sheet, "Height"), text(Some("1"), None, Some("IN")));
    assert_eq!(cell(&shape.sheet, "LocPinX"), text(Some("1"), Some("Width*0.5"), Some("IN")));
    assert_eq!(cell(&shape.sheet, "LocPinY"), text(Some("0.5"), Some("Height*0.5"), Some("IN")));
    assert_eq!(cell(&shape.sheet, "PinX"), text(Some("3"), None, Some("IN")));
    assert_eq!(cell(&shape.sheet, "PinY"), text(Some("2"), None, Some("IN")));
    assert_eq!(cell(&shape.sheet, "Width"), text(Some("2"), None, Some("IN")));
    assert_eq!(shape.sheet.sections.len(), 1);
    let geometry = &shape.sheet.sections[0];
    assert_eq!(geometry.ix, Some(0));
    assert_eq!(geometry.rows.len(), 5);
    let types: Vec<(Option<u32>, Option<&str>)> = geometry.rows.iter().map(|r| (r.ix, r.row_type.as_deref())).collect();
    assert_eq!(
        types,
        [(Some(1), Some("MoveTo")), (Some(2), Some("LineTo")), (Some(3), Some("LineTo")), (Some(4), Some("LineTo")), (Some(5), Some("LineTo"))]
    );
    let with_formula = geometry.rows.iter().flat_map(|r| r.cells.values()).filter(|c| matches!(c.formula.text(), Some("Width" | "Height"))).count();
    assert_eq!(geometry.rows.iter().map(|r| r.cells.len()).sum::<usize>(), 10);
    assert_eq!(with_formula, 4, "four of its ten geometry cells have a formula");
    let hello: Vec<String> = shape.text.as_ref().unwrap().parts.iter().map(|p| format!("{p:?}")).collect();
    assert_eq!(hello.len(), 1);
    assert!(hello[0].contains("Hello"));
    // The ledger says page 1 has had shape ID 1.
    assert_eq!(opened.state.ledger.0.get(&1), Some(&1));
    assert_eq!(stamp(&doc), "3ec10b64c29d5038");
    let written = written_parts(&doc, &opened);
    for name in ["document.xml", "pages/page1.xml", "state.xml"] {
        assert_eq!(entry(&written, name), entry(&bytes, name), "{name}");
    }
}

#[test]
fn vector_3_a_kept_attribute_that_holds_a_tab() {
    let mut items = vector_2();
    replace(&mut items, "pages/page1.xml", page_1(&rectangle(RECT_START, "", "<diacraft:attr name=\"Tag\" value=\"a&#9;b\"/>\n")));
    let bytes = package(&items);
    let (doc, opened) = open_ok(&bytes);
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let kept: Vec<(&str, &str)> = shape.raw.iter().collect();
    assert_eq!(kept, [("Tag", "a\tb")]);
    assert_eq!(stamp(&doc), "716eac1e9ba6c7e2");
    // A writer writes the tab as `&#9;`: a raw tab would be read as a space.
    let written = written_parts(&doc, &opened);
    assert_eq!(entry(&written, "pages/page1.xml"), entry(&bytes, "pages/page1.xml"));
    assert!(String::from_utf8(entry(&written, "pages/page1.xml")).unwrap().contains("value=\"a&#9;b\""));
}

#[test]
fn vector_4_a_forbidden_character() {
    let mut items = vector_2();
    replace(&mut items, "pages/page1.xml", page_1(&rectangle("<diacraft:shape id=\"1\" name-b64=\"QQFC\" type=\"Shape\">", "", "")));
    let bytes = package(&items);
    let (doc, opened) = open_ok(&bytes);
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    assert_eq!(shape.name.as_deref(), Some("A\u{1}B"));
    assert_eq!(stamp(&doc), "ff1285cc49617032");
    let written = written_parts(&doc, &opened);
    assert_eq!(entry(&written, "pages/page1.xml"), entry(&bytes, "pages/page1.xml"));
    for (name, content) in entries(&written) {
        assert!(!content.contains(&1u8), "no raw U+0001 in {name}");
    }
    // A reader that read `name-b64="!!"` would refuse the part.
    let mut items = vector_2();
    replace(&mut items, "pages/page1.xml", page_1(&rectangle("<diacraft:shape id=\"1\" name-b64=\"!!\" type=\"Shape\">", "", "")));
    assert_eq!(refused(&package(&items)).class(), "model_malformed");
}

#[test]
fn vector_5_a_picture_shown_by_two_shapes_stored_once() {
    let bytes = package(&vector_5());
    let (doc, opened) = open_ok(&bytes);
    assert_vector_1_model_without_page_shapes(&doc);
    let page = doc.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 2);
    for (i, pin_x) in [(0usize, "2"), (1, "5")] {
        let shape = page.shapes.get(i).unwrap();
        assert_eq!((shape.id, shape.name.as_deref(), &shape.kind), (i as u32 + 1, Some(format!("Picture {}", i + 1).as_str()), &ShapeType::Foreign));
        assert_eq!(cell(&shape.sheet, "PinX"), text(Some(pin_x), None, Some("IN")));
        assert_eq!(cell(&shape.sheet, "Width"), text(Some("1"), None, Some("IN")));
        let foreign = shape.foreign.as_ref().unwrap();
        assert_eq!(foreign.attr("ForeignType"), Some("Bitmap"));
        let media = foreign.picture.as_ref().unwrap();
        assert_eq!((&*media.part, &*media.content_type), ("media/image1.png", "image/png"));
        assert_eq!(&*media.bytes, &PICTURE[..]);
    }
    // Both pictures hold the very same bytes, and the package has one `blobs/` entry.
    assert!(Arc::ptr_eq(&picture_of(&doc, 0, 0).unwrap(), &picture_of(&doc, 0, 1).unwrap()));
    assert_eq!(opened.state.ledger.0.get(&1), Some(&2));
    assert_eq!(stamp(&doc), "e10a91ed0d11dd5d");
    let written = written_parts(&doc, &opened);
    assert_eq!(names(&written).iter().filter(|n| n.starts_with("blobs/")).count(), 1);
    for name in ["document.xml", "pages/page1.xml", "state.xml", "blobs/0.png"] {
        assert_eq!(entry(&written, name), entry(&bytes, name), "{name}");
    }
    // It is stored, because it begins with a PNG signature.
    assert!(methods(&written).iter().any(|(n, m)| n == "blobs/0.png" && *m == Some(diacraft_container::zip::Method::Stored)));
    let manifest = String::from_utf8(entry(&written, "manifest.xml")).unwrap();
    assert!(manifest.contains("<diacraft:part path=\"blobs/0.png\" role=\"blob\" media-type=\"image/png\"/>"), "{manifest}");
}

fn assert_vector_1_model_without_page_shapes(doc: &Document) {
    assert_eq!(&*doc.title, "Untitled");
    assert_eq!(doc.styles.len(), 1);
    assert_eq!(doc.pages.len(), 1);
}

#[test]
fn vector_6_an_unknown_part_from_a_newer_minor_version() {
    let mut items = vector_1();
    replace(
        &mut items,
        "manifest.xml",
        manifest(
            "1.2",
            "drawing",
            &[
                ("document.xml", "document", ""),
                ("pages/page1.xml", "page", ""),
                ("state.xml", "state", ""),
                ("notes/review.txt", "annotation", " media-type=\"text/plain\""),
            ],
        ),
    );
    items.push(Item::stored("notes/review.txt", b"check the margins"));
    for stored_or_deflated in [true, false] {
        let mut items = items.clone();
        if !stored_or_deflated {
            let last = items.len() - 1;
            items[last] = Item::deflated("notes/review.txt", b"check the margins");
        }
        let bytes = package(&items);
        let (doc, opened) = open_ok(&bytes);
        assert_vector_1_model_but_parts(&doc);
        assert_eq!(opened.kept_parts, 1);
        let part = doc.parts.get(0).unwrap();
        assert_eq!(
            (&*part.name, &*part.media_type, part.manifest.get("role"), part.bytes.len()),
            ("notes/review.txt", "text/plain", Some("annotation"), 17)
        );
        assert_eq!(&*part.bytes, b"check the margins");
        // Written again: the version is this writer's own, the entry is at the same path with the same bytes and the same row, after `state.xml`, and nothing is written
        // for it in `document.xml`.
        let written = written_parts(&doc, &opened);
        assert_eq!(names(&written).last().map(String::as_str), Some("notes/review.txt"));
        assert_eq!(entry(&written, "notes/review.txt"), b"check the margins");
        assert_eq!(entry(&written, "document.xml"), entry(&bytes, "document.xml"));
    }
}

#[test]
fn vector_7_unknown_content_inside_a_model_part() {
    let start = format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"Page-1\" name-u=\"Page-1\" layer-hint=\"3\">");
    let glow = format!("<diacraft:glow xmlns:diacraft=\"{NS}\" radius=\"2\"/>\n");
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page_with_start(&start, &rectangle(RECT_START, &glow, "")));
    let bytes = package(&items);
    let (doc, opened) = open_ok(&bytes);
    assert!(opened.notes.is_empty(), "nothing is lost: {:?}", opened.notes);
    let page = doc.pages.get(0).unwrap();
    // The page holds one unknown attribute, and the rectangle one unknown element placed after one known child, its sheet: both in the slot of the object they stood in.
    let page_native = &page.ext.get().unwrap().native;
    assert_eq!(page_native.raw.iter().collect::<Vec<_>>(), [("layer-hint", "3")]);
    let shape_native = &page.shapes.get(0).unwrap().ext.get().unwrap().native;
    assert_eq!(shape_native.kept.iter().count(), 1);
    assert_eq!(stamp(&doc), "3828032e1f7dcf03");
    // Written again, the same attribute and element are in the same places, and the part's bytes are the ones above.
    let written = written_parts(&doc, &opened);
    assert_eq!(entry(&written, "pages/page1.xml"), entry(&bytes, "pages/page1.xml"));
}

// ----------------------------------------------------------------------------------------------------------------------------------- 8.3

fn vector_1_with_page(page: Vec<u8>) -> Vec<u8> {
    let mut items = vector_1();
    replace(&mut items, "pages/page1.xml", page);
    package(&items)
}

#[test]
fn the_refusals_of_8_3_about_the_package() {
    // Vector 1 with the manifest `format="2.0"`: refused, the words name the version. `format="1.7"`: opens.
    let format = |f: &str| {
        let mut items = vector_1();
        replace(
            &mut items,
            "manifest.xml",
            manifest(f, "drawing", &[("document.xml", "document", ""), ("pages/page1.xml", "page", ""), ("state.xml", "state", "")]),
        );
        package(&items)
    };
    assert_eq!(refused(&format("2.0")).to_string(), "made by a newer DiaCraft (format 2.0)");
    assert_vector_1_model(&open_ok(&format("1.7")).0);
    // `mimetype` deflated, or not the first entry: not a `.diacraft` file, whatever its extension.
    let mut items = vector_1();
    items[0] = Item::deflated("mimetype", MEDIA_DRAWING.as_bytes());
    assert_eq!(refused(&package(&items)), NativeError::NotNative);
    let mut items = vector_1();
    items.rotate_left(1);
    assert_eq!(refused(&package(&items)), NativeError::NotNative);
    // An entry the manifest does not list, a listed entry that is missing: damaged (the other rows of 8.3 are in `package.rs`).
    let mut items = vector_1();
    items.push(Item::stored("extra.txt", b"x"));
    assert_eq!(refused(&package(&items)).class(), "entry_unlisted");
}

#[test]
fn the_refusals_of_8_3_about_a_model_part() {
    let rect = |start: &str| vector_1_with_page(page_1(&rectangle(start, "", "")));
    for (what, bytes) in [
        ("a shape of type Hologram", rect("<diacraft:shape id=\"1\" name=\"Rectangle\" type=\"Hologram\">")),
        ("an id that is not a u32", rect("<diacraft:shape id=\"4294967296\" name=\"Rectangle\" type=\"Shape\">")),
        ("name-b64 that is not base64", rect("<diacraft:shape id=\"1\" name-b64=\"!!\" type=\"Shape\">")),
        ("del=2", rect("<diacraft:shape id=\"1\" name=\"Rectangle\" type=\"Shape\" del=\"2\">")),
    ] {
        let error = refused(&bytes);
        assert_eq!(error.class(), "model_malformed", "{what}: {error}");
        assert!(error.to_string().contains("damaged"), "{what}: in words: {error}");
    }
    // A flag is `1`, `0`, `true` or `false`.
    for flag in ["1", "0", "true", "false"] {
        assert!(read(&rect(&format!("<diacraft:shape id=\"1\" name=\"Rectangle\" type=\"Shape\" del=\"{flag}\">")), &limits()).is_ok(), "del={flag}");
    }
    // A cell that has no `N`.
    let no_name = String::from_utf8(entry(&package(&vector_2()), "pages/page1.xml"))
        .unwrap()
        .replace("<diacraft:Cell N=\"Height\" V=\"1\" U=\"IN\"/>", "<diacraft:Cell V=\"1\" U=\"IN\"/>");
    assert_eq!(refused(&vector_1_with_page(no_name.into_bytes())).class(), "model_malformed");
    // Two cells `Height` in the shape's sheet.
    let twice = String::from_utf8(entry(&package(&vector_2()), "pages/page1.xml")).unwrap().replace(
        "<diacraft:Cell N=\"Height\" V=\"1\" U=\"IN\"/>",
        "<diacraft:Cell N=\"Height\" V=\"1\" U=\"IN\"/><diacraft:Cell N=\"Height\" V=\"2\"/>",
    );
    assert_eq!(refused(&vector_1_with_page(twice.into_bytes())).class(), "model_malformed");
    // The root of the page part given `id="2"`: the list says 1.
    let two = String::from_utf8(entry(&package(&vector_2()), "pages/page1.xml")).unwrap().replacen("id=\"1\"", "id=\"2\"", 1);
    assert_eq!(refused(&vector_1_with_page(two.into_bytes())).class(), "model_malformed");
}

/// A page of `n` shapes, each in the one before.
fn nested(n: usize) -> Vec<u8> {
    let mut inner = String::new();
    for i in 1..=n {
        inner.push_str(&format!("<diacraft:shape id=\"{i}\">"));
    }
    for _ in 0..n {
        inner.push_str("</diacraft:shape>");
    }
    page_1(&inner)
}

#[test]
fn the_refusals_of_8_3_about_depth() {
    // 200 shapes nested one in another: opens (the page part is 201 elements deep, which the depth of 512 allows). 201: refused in words.
    let (doc, _) = open_ok(&vector_1_with_page(nested(200)));
    let mut depth = 0;
    let mut level = doc.pages.get(0).unwrap().shapes.get(0).cloned();
    while let Some(shape) = level {
        depth += 1;
        level = shape.children.get(0).cloned();
    }
    assert_eq!(depth, 200);
    let error = refused(&vector_1_with_page(nested(201)));
    assert_eq!(error.class(), "model_too_complex");
    assert!(error.to_string().contains("200") && error.to_string().contains("nested"), "more than 200 levels of nested shapes: {error}");
}

/// Vector 1's drawing with a page of two shapes and `first`, `second` for their sheets.
fn two_shapes(first: &str, second: &str) -> Vec<u8> {
    vector_1_with_page(page_1(&format!("<diacraft:shape id=\"1\">{first}</diacraft:shape><diacraft:shape id=\"2\">{second}</diacraft:shape>")))
}

fn width_of(doc: &Document, shape: usize) -> Option<String> {
    let shape = doc.pages.get(0)?.shapes.get(shape)?;
    shape.sheet.cell("Width").and_then(|c| c.value.as_ref().map(|v| v.text().to_owned()))
}

fn unknown(opened: &diacraft_native::Opened) -> u64 {
    opened.notes.iter().map(|n| if let ReadNote::Unknown { count } = n { *count } else { 0 }).sum()
}

#[test]
fn the_refusals_of_8_3_about_shared_sheets() {
    let named = "<diacraft:sheet def=\"1\"><diacraft:Cell N=\"Width\" V=\"1\"/></diacraft:sheet>";
    let plain = "<diacraft:sheet><diacraft:Cell N=\"Width\" V=\"1\"/></diacraft:sheet>";
    // Two shapes with the same sheet: opens, and the drawing has the content and the stamp of the one whose second shape holds a copy of the first's sheet in full.
    let (shared, opened) = open_ok(&two_shapes(named, "<diacraft:same of=\"1\"/>"));
    assert_eq!((width_of(&shared, 0).as_deref(), width_of(&shared, 1).as_deref()), (Some("1"), Some("1")));
    assert_eq!(unknown(&opened), 0);
    let (full, _) = open_ok(&two_shapes(plain, plain));
    assert_eq!(stamp(&shared), stamp(&full));
    // No unit of the name: not read and counted, and the second shape has no sheet.
    let (doc, opened) = open_ok(&two_shapes(named, "<diacraft:same of=\"2\"/>"));
    assert_eq!((width_of(&doc, 0).as_deref(), width_of(&doc, 1)), (Some("1"), None));
    assert_eq!(unknown(&opened), 1);
    // The `same` in the first shape and the `def` in the second: a unit is named by an element before it.
    let (doc, opened) = open_ok(&two_shapes("<diacraft:same of=\"1\"/>", named));
    assert_eq!((width_of(&doc, 0), width_of(&doc, 1).as_deref()), (None, Some("1")));
    assert_eq!(unknown(&opened), 1);
    // `def="1"` on both: the second gives a name again and is read as a sheet of no name and counted; it keeps its sheet.
    let (doc, opened) = open_ok(&two_shapes(named, named));
    assert_eq!((width_of(&doc, 0).as_deref(), width_of(&doc, 1).as_deref()), (Some("1"), Some("1")));
    assert_eq!(unknown(&opened), 1);
    // `def="1"` on a Section in the first shape's sheet, and `same of="1"` where the second shape's sheet stands: the name is a section's and a sheet stands there.
    let section = "<diacraft:sheet><diacraft:Section N=\"User\" def=\"1\"><diacraft:Row N=\"a\"/></diacraft:Section></diacraft:sheet>";
    let (doc, opened) = open_ok(&two_shapes(section, "<diacraft:same of=\"1\"/>"));
    assert!(
        doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.cells.is_empty()
            && doc.pages.get(0).unwrap().shapes.get(1).unwrap().sheet.sections.is_empty()
    );
    assert_eq!(unknown(&opened), 1);
    // A `same` that has no `of`: refused as damaged.
    assert_eq!(refused(&two_shapes(named, "<diacraft:same/>")).class(), "model_malformed");
    // The fingerprint of a unit (3.19: the domain `unit`, then the fields `<`, `sheet`, `<`, `Cell`, `N`, `Width`, `V`, `1`, `/`, `/`): the two shapes have the sheet of width 1.
    let fingerprint = Digest::new("unit")
        .text("<")
        .text("sheet")
        .text("<")
        .text("Cell")
        .text("N")
        .text("Width")
        .text("V")
        .text("1")
        .text("/")
        .text("/")
        .finish()
        .text();
    assert_eq!(fingerprint, "5e36db3382a321fa");
    let fp = |v: &str| format!("<diacraft:sheet def=\"1\" fp=\"5e36db3382a321fa\"><diacraft:Cell N=\"Width\" V=\"{v}\"/></diacraft:sheet>");
    let (doc, opened) = open_ok(&two_shapes(&fp("1"), "<diacraft:same of=\"1\"/>"));
    assert_eq!((width_of(&doc, 0).as_deref(), width_of(&doc, 1).as_deref()), (Some("1"), Some("1")));
    assert_eq!(unknown(&opened), 0);
    // The same page with `V="2"` and `fp` as it is (a program of 1.0 edited the unit): the first shape has the sheet it says, its name is not given (counted), so the `same`
    // that names it is not read and is counted too, and the second shape has no sheet.
    let (doc, opened) = open_ok(&two_shapes(&fp("2"), "<diacraft:same of=\"1\"/>"));
    assert_eq!((width_of(&doc, 0).as_deref(), width_of(&doc, 1)), (Some("2"), None));
    assert_eq!(unknown(&opened), 2);
}

#[test]
fn the_stamps_of_8_4() {
    assert_eq!(Digest::new("cells").text("LineWeight").finish().text(), "7bce65b857cfdb09");
    assert_eq!(stamp_of_bytes(&PICTURE).text(), "71df7ba063b3b4da");
    assert_eq!(stamp_of_bytes(&[]).text(), "a09bd6c00ede502b");
    // The content stamps of the vectors' models are asserted with each vector above (the stamp of a model read from a package is the stamp of its content).
    let (doc, _) = open_ok(&package(&vector_1()));
    assert_eq!(stamp(&doc), "00ebc460d282d097");
}
