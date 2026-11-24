//! DiaCraft's data in VSDX files, keyed, stamped and judged (the native format's design, 5.2 and 5.4; `crates/vsdx/src/carry.rs` says what the carrier is).
//!
//! A drawing that holds the data of the other format (what the `.odg` reader keeps in the `odf` side of the slots, the parts and the custom properties it tags as its own, and what a
//! newer `.diacraft` wrote that this build does not know) is written as a VSDX file with that data inside DiaCraft's solution part, and read back from it. **Every read-back here goes
//! through the real reader (`read`) and every file through the real writer (`write_with`)**; the files a hostile or another program's edit makes are the written ones, changed by hand at
//! the text of one part and packed again with right checksums, so that the reader meets what such an edit leaves.
//!
//! The crafted inputs are the point of half of these tests: a name with `..`, an absolute name, names that differ only in case, 49,990 and 50,000 parts, a part one byte over the cap, two records
//! naming one part, a macro part of text that looks like XML. Each default-run test counts the work in the units the loop walks (`diacraft_vsdx::work`), with a bound near the measured value; each
//! `#[ignore]` test times the case beside a yardstick of plain content of the same size, and fails when the case is slower.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::stamp::Stamp;
use diacraft_doc::{
    CustomProp, DefFamily, Definition, Document, Format, Held, HeldGroup, Kept, OdfPage, OdfRow, OdfShape, OdfStyle, OdfText, Part, PartRole,
    Passthrough, RawAttrs, RawElement, RawNode, Shape, Text, TextPart,
};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_sheetxml::model::{MemBlobs, encode};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, ZipItem, cell, shape, shape_with, zip};
use diacraft_vsdx::{Kind, Limits, OriginalBytes, ReadReport, Variant, WriteError, WriteOptions, Written, read, work, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const SOLUTION: &str = "/visio/solutions/solution1.xml";
const PAGE1: &str = "/visio/pages/page1.xml";

fn options(macros: bool) -> WriteOptions {
    WriteOptions {
        now: Some(NOW),
        content_unchanged: true,
        variant: macros.then_some(Variant { kind: Kind::Drawing, macro_enabled: true }),
        ..WriteOptions::default()
    }
}

fn write(doc: &Document, macros: bool) -> Written {
    write_with(doc, None, &options(macros)).unwrap_or_else(|e| panic!("the writer refused: {e} ({})", e.class()))
}

// ------------------------------------------------------------------------------------------------------------------------------ the model

/// A drawing of two pages, a master, a style and two faces, read from a package: what a VSDX file holds, with a group, a connection point and a text.
fn base_bytes() -> Vec<u8> {
    let second = format!(
        "{}<Section N=\"Connection\"><Row IX=\"0\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"2\"/></Row></Section><Text>abcd</Text>",
        cell("PinX", "2")
    );
    let group = format!("{}<Shapes>{}</Shapes>", cell("PinX", "3"), shape(4, &cell("PinX", "4")));
    VsdxBuilder::new()
        .face_names(r#"<FaceName NameU="Arial"/><FaceName NameU="Noto Sans"/>"#)
        .more_styles(r#"<StyleSheet ID="1" Name="Fancy" NameU="Fancy"><Cell N="LineWeight" V="0.01"/></StyleSheet>"#)
        .master(MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1"))))
        .page(PageSpec::new(1, "One").shapes(&format!(
            "{}{}{}",
            shape(1, &cell("PinX", "1")),
            shape(2, &second),
            shape_with(3, " Type=\"Group\"", &group)
        )))
        .page(PageSpec::new(2, "Two").shapes(&shape(7, &cell("PinX", "7"))))
        .build()
}

fn raw(name: &str, attrs: &[(&str, &str)]) -> RawElement {
    RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() }
}

fn kept(attrs: &[(&str, &str)]) -> Kept {
    Kept { raw: RawAttrs::from_pairs(attrs.iter().copied()), kept: Passthrough::new() }
}

fn part(format: Format, name: &str, media: &str, role: PartRole, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format,
        name: Arc::from(name),
        media_type: Arc::from(media),
        role,
        manifest: RawAttrs::from_pairs([("manifest:version", "1.3")]),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

fn shape_mut(doc: &mut Document, page: usize, id: u32) -> &mut Shape {
    let page = Arc::make_mut(doc.pages.get_mut(page).unwrap());
    let slot = (0..page.shapes.len()).find(|&i| page.shapes.get(i).is_some_and(|s| s.id == id)).unwrap();
    Arc::make_mut(page.shapes.get_mut(slot).unwrap())
}

/// The drawing of [`base_bytes`], read, with data of the other format in every place it rides: the document (its slot, a fill image's bytes, a layer, a custom property, a face), parts of the
/// other format (an object, a folder, a macro project, a picture), a page, a master, a style, three shapes (one in a group), a glue point's row, a text with a part that rides, and what a newer
/// build wrote on a shape.
fn odf_model() -> Document {
    let (mut doc, _) = read_ok(&base_bytes());
    {
        let slot = doc.ext.make_mut();
        slot.odf.mimetype = Some(Arc::from("application/vnd.oasis.opendocument.graphics"));
        slot.odf.roots = vec![(Arc::from("content.xml"), RawAttrs::from_pairs([("office:version", "1.3")]))];
        slot.odf.layers = vec![Arc::new(raw("draw:layer", &[("draw:name", "layout")]))];
        slot.odf.definitions = vec![Definition {
            family: DefFamily::FillImage,
            name: Arc::from("img"),
            element: Arc::new(raw("draw:fill-image", &[("draw:name", "img")])),
            bytes: Some(Arc::from(&b"fill image bytes"[..])),
        }];
        slot.native = kept(&[("later", "1")]);
    }
    doc.parts.push(part(Format::Odf, "Object 1/content.xml", "text/xml", PartRole::ObjectFile, b"<office:document-content/>"));
    doc.parts.push(part(Format::Odf, "Object 1/", "", PartRole::ObjectFile, b""));
    doc.parts.push(part(Format::Odf, "Basic/Standard/script-lb.xml", "text/xml", PartRole::MacroProject, b"<ooo:library/>"));
    doc.parts.push(part(Format::Odf, "Pictures/p.png", "image/png", PartRole::Other, &[0x89, b'P', b'N', b'G', 1, 2, 3]));
    Arc::make_mut(&mut doc.props).custom.push(CustomProp {
        format: Format::Odf,
        name: Arc::from("Field"),
        value_type: Arc::from("string"),
        value: Arc::from("v"),
        raw: RawAttrs::new(),
    });
    {
        let fonts = Arc::make_mut(&mut doc.fonts);
        fonts.get_mut(1).unwrap().ext.make_mut().odf = kept(&[("svg:font-family", "Noto Sans")]);
    }
    {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        page.ext.make_mut().odf = OdfPage { master_page: Some(Arc::from("Default")), layout: Some(Arc::from("PM1")), ..OdfPage::default() };
    }
    Arc::make_mut(doc.masters.get_mut(0).unwrap()).ext.make_mut().native = kept(&[("later-master", "1")]);
    Arc::make_mut(doc.styles.iter().position(|s| s.name.as_deref() == Some("Fancy")).and_then(|i| doc.styles.get_mut(i)).unwrap())
        .ext
        .make_mut()
        .odf = OdfStyle { name: Some(Arc::from("Fancy")), family: Some(Arc::from("common:graphic")), ..OdfStyle::default() };
    shape_mut(&mut doc, 0, 1).ext.make_mut().odf = OdfShape {
        style_name: Some(Arc::from("gr1")),
        held: vec![Held {
            group: HeldGroup::Fill,
            stamp: Some(Stamp(0x1234)),
            attrs: RawAttrs::from_pairs([("draw:fill", "hatch")]),
            kept: Passthrough::new(),
        }],
        units: RawAttrs::from_pairs([("svg:width", "2cm")]),
        ..OdfShape::default()
    };
    {
        let two = shape_mut(&mut doc, 0, 2);
        two.ext.make_mut().odf.style_name = Some(Arc::from("gr2"));
        two.ext.make_mut().native = kept(&[("later-shape", "x")]);
        let sheet = Arc::make_mut(&mut two.sheet);
        let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "Connection").unwrap());
        let row = Arc::make_mut(section.rows.get_mut(0).unwrap());
        row.ext.make_mut().odf = OdfRow {
            held: Some(Held {
                group: HeldGroup::Glue,
                stamp: None,
                attrs: RawAttrs::from_pairs([("draw:escape-direction", "horizontal")]),
                kept: Passthrough::new(),
            }),
            ..OdfRow::default()
        };
        two.text = Some(Arc::new(Text {
            parts: vec![
                TextPart::Chars(Arc::from("ab")),
                TextPart::OdfRaw(Box::new(RawNode::Element(raw("text:bookmark", &[("text:name", "b")])))),
                TextPart::Chars(Arc::from("cd")),
            ],
            ext: {
                let mut ext = diacraft_doc::Ext::default();
                ext.make_mut().odf = OdfText { kept: kept(&[("text:style-name", "P1")]), ..OdfText::default() };
                ext
            },
        }));
    }
    {
        let group = shape_mut(&mut doc, 0, 3);
        let child = Arc::make_mut(group.children.get_mut(0).unwrap());
        child.ext.make_mut().odf.style_name = Some(Arc::from("gr9"));
    }
    doc
}

/// What of the other format a model holds, in each place, for comparing two models by it: the `odf` and `native` sides of the slots and the parts, not the `vsdx` side.
fn data_of(doc: &Document) -> Vec<String> {
    fn sides<T: Default + Clone + PartialEq + std::fmt::Debug>(label: &str, slot: &diacraft_doc::Ext<T>, out: &mut Vec<String>) {
        if let Some(d) = slot.get()
            && (d.odf != T::default() || d.native != Kept::default())
        {
            out.push(format!("{label}: {:?} / {:?}", d.odf, d.native));
        }
    }
    let mut out = Vec::new();
    sides("doc", &doc.ext, &mut out);
    for (i, f) in doc.fonts.iter().enumerate() {
        sides(&format!("font {i}"), &f.ext, &mut out);
    }
    for p in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        out.push(format!("part {:?} {} {} {:?} {:?} {}", p.format, p.name, p.media_type, p.role, p.manifest, p.bytes.len()));
    }
    for c in doc.props.custom.iter().filter(|c| c.format != Format::Vsdx) {
        out.push(format!("custom {c:?}"));
    }
    for (i, p) in doc.pages.iter().enumerate() {
        sides(&format!("page {i}"), &p.ext, &mut out);
        fn walk(list: &diacraft_doc::ShapeList, label: &str, out: &mut Vec<String>) {
            for s in list.iter() {
                sides(&format!("{label} shape {}", s.id), &s.ext, out);
                if let Some(t) = &s.text {
                    sides(&format!("{label} shape {} text", s.id), &t.ext, out);
                    for (n, part) in t.parts.iter().enumerate() {
                        if matches!(part, TextPart::OdfRaw(_) | TextPart::NativeRaw(_)) {
                            out.push(format!("{label} shape {} text part {n}: {part:?}", s.id));
                        }
                    }
                }
                for sec in &s.sheet.sections {
                    for (r, row) in sec.rows.iter().enumerate() {
                        sides(&format!("{label} shape {} {} row {r}", s.id, sec.name.as_str()), &row.ext, out);
                    }
                }
                walk(&s.children, label, out);
            }
        }
        walk(&p.shapes, &format!("page {i}"), &mut out);
    }
    for m in doc.masters.iter() {
        sides(&format!("master {}", m.id), &m.ext, &mut out);
    }
    for s in doc.styles.iter() {
        sides(&format!("style {}", s.id), &s.ext, &mut out);
    }
    out
}

/// The model's parts encoded, for a comparison that says which part differs.
fn encoded(doc: &Document) -> Vec<(String, diacraft_opc::xml::El)> {
    let mut blobs = MemBlobs::new();
    let e = encode(doc, &mut blobs).unwrap_or_else(|e| panic!("the model does not encode: {e}"));
    let mut out = vec![("document".to_owned(), e.document)];
    out.extend(e.pages.into_iter().map(|(id, el)| (format!("page {id}"), el)));
    out.extend(e.masters.into_iter().map(|(id, el)| (format!("master {id}"), el)));
    out
}

// ------------------------------------------------------------------------------------------------------------------------------ packages

fn items_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let pkg = Package::read(bytes, &OpcLimits::default()).unwrap();
    let mut out = vec![("[Content_Types].xml".to_owned(), pkg.content_types_bytes().to_vec())];
    out.extend(pkg.parts().map(|p| (p.trim_start_matches('/').to_owned(), pkg.part(p).unwrap().to_vec())));
    out
}

fn pack(items: &[(String, Vec<u8>)]) -> Vec<u8> {
    zip(&items.iter().map(|(n, b)| ZipItem::deflated(n, b)).collect::<Vec<_>>())
}

/// The package with the part `name` changed by `f` (text in, text out), which must change it.
fn edit(bytes: &[u8], name: &str, f: impl FnOnce(&str) -> String) -> Vec<u8> {
    let mut items = items_of(bytes);
    let slot = items.iter_mut().find(|(n, _)| format!("/{n}") == name).unwrap_or_else(|| panic!("no part {name}"));
    let before = String::from_utf8(slot.1.clone()).unwrap();
    let after = f(&before);
    assert_ne!(before, after, "the edit of {name} changed nothing");
    slot.1 = after.into_bytes();
    pack(&items)
}

fn solution(bytes: &[u8]) -> String {
    String::from_utf8(Package::read(bytes, &OpcLimits::default()).unwrap().part(SOLUTION).unwrap().to_vec()).unwrap()
}

/// The names of the parts of DiaCraft's own.
fn blob_parts(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &OpcLimits::default()).unwrap().parts().filter(|p| p.starts_with("/diacraft/")).map(str::to_owned).collect()
}

/// The two lists of data are the same, or the entries only one of them has (cut), for a message.
fn assert_same_data(got: &[String], wanted: &[String]) {
    let cut = |s: &String| s.chars().take(300).collect::<String>();
    let missing: Vec<String> = wanted.iter().filter(|w| !got.contains(w)).map(cut).collect();
    let extra: Vec<String> = got.iter().filter(|g| !wanted.contains(g)).map(cut).collect();
    assert!(missing.is_empty() && extra.is_empty(), "missing:\n{missing:#?}\nextra:\n{extra:#?}");
    assert_eq!(got.len(), wanted.len());
}

fn read_back(bytes: &[u8]) -> (Document, ReadReport) {
    read_ok(bytes)
}

fn warnings_of_the_carrier(report: &ReadReport) -> u64 {
    report.warning_counts.iter().filter(|(class, _)| class.starts_with("diacraft_")).map(|(_, n)| *n).sum()
}

/// A package that carries data of the other format in every place it rides: the seed of the fuzz properties of `fuzz.rs`.
pub(crate) fn carried_seed() -> Vec<u8> {
    write(&odf_model(), true).bytes
}

/// [`carried_seed`] with one part mutated by the generator (the solution part, a relationships part, a part of DiaCraft's own, the types or any other), packed again with right checksums.
pub(crate) fn mutated_carried(seed: u64) -> Vec<u8> {
    let mut g = crate::fuzz::Gen(seed | 1);
    let mut items = items_of(&carried_seed());
    let at = g.below(items.len());
    // The parts that matter most are chosen twice as often: the solution part and the relationships and types that name the parts of DiaCraft's own.
    let at = if g.below(2) == 0 { items.iter().position(|(n, _)| n.contains("solution")).unwrap_or(at) } else { at };
    let mut inner = crate::fuzz::Gen(g.next() | 1);
    crate::fuzz::mutate(&mut inner, &mut items[at].1);
    pack(&items)
}

// ------------------------------------------------------------------------------------------------------------------------------ the eleven

#[test]
fn odf_data_rides_and_comes_back() {
    let model = odf_model();
    let before = data_of(&model);
    assert!(before.len() > 15, "the fixture holds data in many places: {before:#?}");
    let written = write(&model, true);
    // The data is in the file: the solution part, the parts of DiaCraft's own and the relationships to them.
    let parts = blob_parts(&written.bytes);
    assert_eq!(parts.len(), 5, "the object, the macro project, the picture, the fill image and the folder's empty bytes: {parts:?}");
    let (back, report) = read_back(&written.bytes);
    assert_eq!(data_of(&back), before, "the data of the other format comes back where it stood");
    assert_eq!(report.carry.stale + report.carry.gone + report.carry.left_out, 0, "{:?}", report.carry);
    assert_eq!(warnings_of_the_carrier(&report), 0, "{:?}", report.warnings);
    // The model that comes back is the model, whole. What may differ is the new held classes of the design's 5.4, the writer's own choices: the row of the document to the solutions
    // index that the file now has (a model that came from a package without a solution held none).
    let mut back = back;
    if let Some(v) = back.vsdx.as_mut() {
        let held = Arc::make_mut(v);
        held.rels.retain(|r| !r.kind.ends_with("/solutions"));
    }
    assert!(back.diacraft.is_none(), "the data of the file holds nothing but the items, which are the carrier's");
    let (a, b) = (encoded(&model), encoded(&back));
    assert_eq!(a.len(), b.len());
    for ((name, x), (_, y)) in a.iter().zip(&b) {
        assert_eq!(x, y, "the {name} part differs after the trip");
    }
    // The parts of DiaCraft's own are not parts of the package the model holds: they are the model's parts of the other format.
    assert!(back.parts.iter().all(|p| p.format != Format::Vsdx || !p.name.starts_with("/diacraft/")));
    assert!(back.vsdx.as_ref().is_none_or(|v| v.rels.iter().all(|r| !r.source.starts_with("/visio/solutions/"))));
    // A second save of the model read is the same data again, and no part is new.
    let again = write(&back, true);
    assert_eq!(blob_parts(&again.bytes), parts);
    assert_eq!(data_of(&read_back(&again.bytes).0), before);
}

#[test]
fn a_stale_item_is_dropped_when_its_native_output_changed() {
    let written = write(&odf_model(), true);
    // Another program changes the cell that shape 1 holds, and one of the page's own: the item of each sits beside what it changed.
    let edited = edit(&written.bytes, PAGE1, |text| text.replacen("N=\"PinX\" V=\"1\"", "N=\"PinX\" V=\"9\"", 1));
    let (back, report) = read_back(&edited);
    assert_eq!(report.carry.stale, 1, "{:?}", report.carry);
    let data = data_of(&back);
    assert!(data.iter().all(|d| !d.starts_with("page 0 shape 1:")), "the stale shape's data is dropped: {data:#?}");
    assert!(data.iter().any(|d| d.starts_with("page 0 shape 2:")), "the shapes that were not changed keep theirs: {data:#?}");
    assert!(data.iter().any(|d| d.starts_with("page 0:")), "and so does the page");
    assert_eq!(report.carry.gone, 0);
    // The page's own item: a page another program renamed.
    let renamed = edit(&written.bytes, "/visio/pages/pages.xml", |text| text.replacen("Name=\"One\"", "Name=\"Eins\"", 1));
    let (back, report) = read_back(&renamed);
    assert_eq!(report.carry.stale, 1, "{:?}", report.carry);
    assert!(data_of(&back).iter().all(|d| !d.starts_with("page 0:")));
}

#[test]
fn an_item_for_a_gone_shape_is_dropped_silently() {
    let written = write(&odf_model(), true);
    // Another program took shape 1 out of the page.
    let edited = edit(&written.bytes, PAGE1, |text| {
        let start = text.find("<Shape ID=\"1\"").unwrap();
        let end = start + text[start..].find("</Shape>").unwrap() + "</Shape>".len();
        format!("{}{}", &text[..start], &text[end..])
    });
    let (back, report) = read_back(&edited);
    assert_eq!(report.carry.gone, 1, "{:?}", report.carry);
    assert_eq!((report.carry.stale, report.carry.left_out), (0, 0));
    assert_eq!(warnings_of_the_carrier(&report), 0, "silently: {:?}", report.warnings);
    assert!(back.pages.get(0).unwrap().shapes.iter().all(|s| s.id != 1));
    assert!(data_of(&back).iter().any(|d| d.starts_with("page 0 shape 2:")));
}

#[test]
fn an_odf_macro_project_goes_only_into_a_macro_enabled_type() {
    let model = odf_model();
    // A type that promises no macros takes none, whatever the language, and says how to keep it.
    let refused = write_with(&model, None, &options(false)).unwrap_err();
    assert_eq!(refused, WriteError::MacroProject);
    assert!(refused.to_string().contains(".vsdm"), "{refused}");
    // The caller's choice to leave it out: the rest rides, and the words name what was left out.
    let dropped = write_with(&model, None, &WriteOptions { drop_macros: true, ..options(false) }).unwrap();
    assert_eq!(dropped.macros_dropped, vec!["Basic/Standard/script-lb.xml".to_owned()]);
    assert!(dropped.warnings.iter().any(|w| w.contains("macro project")), "{:?}", dropped.warnings);
    let (back, _) = read_back(&dropped.bytes);
    assert!(back.parts.iter().all(|p| p.role != PartRole::MacroProject));
    assert!(back.parts.iter().any(|p| &*p.name == "Object 1/content.xml"), "the rest of the data rides");
    // A macro-enabled type keeps it, as bytes.
    let kept = write(&model, true);
    let (back, _) = read_back(&kept.bytes);
    let project = back.parts.iter().find(|p| p.role == PartRole::MacroProject).expect("the macro project comes back");
    assert_eq!((project.format, &*project.name, &project.bytes[..]), (Format::Odf, "Basic/Standard/script-lb.xml", &b"<ooo:library/>"[..]));
}

#[test]
fn a_carried_part_one_byte_over_the_cap_is_refused_in_words() {
    let cap = diacraft_container::limits::MAX_PART_BYTES;
    let mut model = odf_model();
    model.parts.push(part(
        Format::Odf,
        "Object 2/big.bin",
        "application/octet-stream",
        PartRole::Embedding,
        &vec![0u8; usize::try_from(cap).unwrap() + 1],
    ));
    let refused = write_with(&model, None, &options(true)).unwrap_err();
    assert!(matches!(&refused, WriteError::Carried { part, .. } if part == "Object 2/big.bin"), "{refused:?}");
    let words = refused.to_string();
    assert!(words.contains("Object 2/big.bin") && words.contains("256 MiB"), "{words}");
    assert_eq!(refused.class(), "carried_part_refused");
}

#[test]
fn fifty_thousand_small_carried_parts_are_counted() {
    fn drawing(n: usize) -> Document {
        let (mut doc, _) = read_ok(&base_bytes());
        for i in 0..n {
            // Four distinct bytes each, so that each is a blob of its own.
            let bytes = u32::try_from(i).unwrap().to_le_bytes();
            doc.parts.push(part(Format::Odf, &format!("Object {i}/x.bin"), "application/octet-stream", PartRole::ObjectFile, &bytes));
        }
        doc
    }
    // The most one file names, 30,000, is written once and read once each.
    let n = 30_000usize;
    let doc = drawing(n);
    work::reset();
    let written = write(&doc, false);
    let walked = work::carry_objects_walked();
    assert!(walked >= n as u64 && walked <= 4 * n as u64, "the writer looked at {walked} objects for {n} parts");
    assert_eq!(blob_parts(&written.bytes).len(), n);
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(back.parts.iter().filter(|p| p.format == Format::Odf).count(), n);
    assert_eq!((work::carry_parts_read(), work::carry_bytes_copied()), (n as u64, 4 * n as u64), "each part once, each byte once");
    assert_eq!((report.carry.parts_seen, report.carry.parts_kept), (n as u64, n as u64));
    // One more is refused in words that say the number, before any part is written, and the work to find that out is as linear.
    let doc = drawing(n + 1);
    work::reset();
    let refused = write_with(&doc, None, &options(false)).unwrap_err();
    assert!(matches!(&refused, WriteError::Limit(words) if words.contains("30,000")), "{refused:?}");
    let walked = work::carry_objects_walked();
    assert!(walked <= 2 * (n as u64 + 1), "{walked}");
    assert_eq!(work::model_bytes_written(), 0, "nothing was written");
    // 50,000 parts are more than a package holds beside the drawing: refused in words at the door of the writer, before any look at them.
    let doc = drawing(50_000);
    work::reset();
    let refused = write_with(&doc, None, &options(false)).unwrap_err();
    assert_eq!(refused, WriteError::Limit("more parts than a package holds"));
    assert!(work::carry_objects_walked() <= 50_000, "{}", work::carry_objects_walked());
    assert_eq!(work::model_bytes_written(), 0, "nothing was written");
}

/// `n` elements that differ from each other, as text.
fn noise_xml(n: u32) -> String {
    let mut x = 12_345u32;
    (0..n)
        .map(|i| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            format!("<a{i} x=\"{x}\"><b y=\"{}\"/></a{i}>", x.rotate_left(7))
        })
        .collect()
}

/// `len` bytes of text that is not XML and does not compress well.
fn noise_text(len: usize) -> String {
    let mut x = 777u32;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            char::from(b'a' + u8::try_from((x >> 24) % 26).unwrap())
        })
        .collect()
}

/// The package with the part that holds `bytes` taken out (its entry and nothing else: the relationship that names it stays, and leads nowhere).
fn without_the_part_that_holds(bytes: &[u8], what: &[u8]) -> Vec<u8> {
    let mut items = items_of(bytes);
    let before = items.len();
    items.retain(|(n, b)| !(n.starts_with("diacraft/") && b == what));
    assert_eq!(items.len(), before - 1);
    // The content types keep their row, which names no part now.
    pack(&items)
}

#[test]
fn an_item_naming_a_missing_part_is_dropped_with_a_note() {
    let written = write(&odf_model(), true);
    let edited = without_the_part_that_holds(&written.bytes, b"<office:document-content/>");
    let (back, report) = read_back(&edited);
    assert_eq!(report.warning_count("diacraft_part_missing"), 1, "{:?}", report.warnings);
    let names: Vec<&str> = back.parts.iter().filter(|p| p.format == Format::Odf).map(|p| &*p.name).collect();
    assert!(!names.contains(&"Object 1/content.xml"), "{names:?}");
    assert!(names.contains(&"Basic/Standard/script-lb.xml") && names.contains(&"Pictures/p.png"), "the other parts stay: {names:?}");
    // What the missing part's item held besides is read: only the part is left out.
    assert!(data_of(&back).iter().any(|d| d.starts_with("page 0 shape 2:")));
    // The part was not held as a part of the package either: it is not anywhere.
    assert!(back.parts.iter().all(|p| !p.name.starts_with("/diacraft/")));
}

#[test]
fn two_items_naming_one_part_are_dropped_with_a_note() {
    let written = write(&odf_model(), true);
    // The records of the parts' blob list: the second names the relationship the first does.
    let edited = edit(&written.bytes, SOLUTION, |text| {
        let mut paths: Vec<(usize, String)> = Vec::new();
        let mut from = 0;
        while let Some(at) = text[from..].find("path=\"") {
            let start = from + at + "path=\"".len();
            let end = start + text[start..].find('"').unwrap();
            paths.push((start, text[start..end].to_owned()));
            from = end;
        }
        assert!(paths.len() >= 4, "{paths:?}");
        // The records of the item of the parts are the second to the fifth (the first is the document's fill image): the third names the relationship the second does.
        let (first, second) = (&paths[1], &paths[2]);
        format!("{}{}{}", &text[..second.0], first.1, &text[second.0 + second.1.len()..])
    });
    let (back, report) = read_back(&edited);
    assert_eq!(report.warning_count("diacraft_part_twice"), 1, "{:?}", report.warnings);
    // Neither part of the two comes back; the others do.
    let names: Vec<&str> = back.parts.iter().filter(|p| p.format == Format::Odf).map(|p| &*p.name).collect();
    assert!(names.len() < 4 && !names.is_empty(), "{names:?}");
    assert_eq!(report.carry.parts_seen, 5);
}

#[test]
fn a_crafted_odf_entry_name_never_reaches_outside_an_odg() {
    let written = write(&odf_model(), true);
    // The writer refuses a name that no package could hold, in words that name the part and the way out.
    for bad in [
        "../evil",
        "/absolute",
        "a/../../b",
        "C:\\x",
        "back\\slash",
        "Object/./x",
        "a//b",
        "mimetype",
        "META-INF/manifest.xml",
        "DiaCraft/document.xml",
    ] {
        let mut model = odf_model();
        model.parts.push(part(Format::Odf, bad, "text/xml", PartRole::Other, b"x"));
        let refused = write_with(&model, None, &options(true)).unwrap_err();
        assert!(matches!(&refused, WriteError::Carried { part, .. } if part == bad), "{bad}: {refused:?}");
    }
    // A file another program (or a hostile one) wrote, with such a name in the record of a part: the part is left out, with a note, and is never in the model.
    for bad in ["../evil", "/absolute", "a/../../b", "C:\\x", "a//b"] {
        let edited = edit(&written.bytes, SOLUTION, |text| text.replacen("name=\"Object 1/content.xml\"", &format!("name=\"{bad}\""), 1));
        let (back, report) = read_back(&edited);
        assert_eq!(report.warning_count("diacraft_part_name_refused"), 1, "{bad}: {:?}", report.warnings);
        assert!(back.parts.iter().all(|p| &*p.name != bad), "{bad}");
        assert!(back.parts.iter().any(|p| &*p.name == "Pictures/p.png"), "{bad}: the others come back");
    }
    // Names that differ only in case: the second is left out, with a note, by the writer and by the reader.
    let mut model = odf_model();
    model.parts.push(part(Format::Odf, "Object 1/CONTENT.xml", "text/xml", PartRole::Other, b"twin"));
    let twin = write(&model, true);
    assert!(twin.warnings.iter().any(|w| w.contains("differs from it at most in case")), "{:?}", twin.warnings);
    let edited = edit(&written.bytes, SOLUTION, |text| text.replacen("name=\"Pictures/p.png\"", "name=\"OBJECT 1/Content.XML\"", 1));
    let (back, report) = read_back(&edited);
    assert_eq!(report.warning_count("diacraft_part_name_twice"), 1, "{:?}", report.warnings);
    assert_eq!(back.parts.iter().filter(|p| p.name.eq_ignore_ascii_case("object 1/content.xml")).count(), 1);
}

#[test]
fn a_macro_part_is_never_parsed() {
    // A macro project of text that looks like the XML a parser would walk for a long time: a megabyte and more of elements, none of them the same as the last (so that it is no
    // bomb to the container: its ratio stays under the cap), and as many bytes of text that is not.
    let looks_like_xml = noise_xml(60_000);
    let plain = noise_text(looks_like_xml.len());
    let make = |bytes: &str| {
        let mut model = odf_model();
        model.parts = diacraft_doc::SharedVec::from_vec(model.parts.iter().filter(|p| p.role != PartRole::MacroProject).cloned().collect());
        model.parts.push(part(Format::Odf, "Basic/Standard/big.xml", "text/xml", PartRole::MacroProject, bytes.as_bytes()));
        write(&model, true).bytes
    };
    let (with_xml, with_text) = (make(&looks_like_xml), make(&plain));
    work::reset();
    let (a, ra) = read_back(&with_xml);
    let events_xml = work::xml_events_read();
    work::reset();
    let (b, rb) = read_back(&with_text);
    let events_text = work::xml_events_read();
    assert!(events_xml > 0, "the document parts were parsed");
    assert_eq!(events_xml, events_text, "no XML event is read over the bytes of the macro part");
    assert_eq!(ra.stats.xml_bytes, rb.stats.xml_bytes, "and no byte of it is counted as XML");
    // It is bytes: copied once into the model, as they are.
    let macro_of = |d: &Document| d.parts.iter().find(|p| p.role == PartRole::MacroProject).unwrap().bytes.len();
    assert_eq!((macro_of(&a), macro_of(&b)), (looks_like_xml.len(), plain.len()));
    // And the writer reads none of it either.
    work::reset();
    let _ = write(&a, true);
    assert_eq!(work::xml_events_read(), events_xml, "the check of the file made parses the same parts and not the macro part");
}

#[test]
fn the_file_draws_the_same_without_the_part() {
    let model = odf_model();
    // The same drawing with none of the data of the other format: the package it was read from, read.
    let (plain, _) = read_ok(&base_bytes());
    let (with, without) = (write(&model, true), write(&plain, true));
    let (a, b) = (items_of(&with.bytes), items_of(&without.bytes));
    // Every part that says what the drawing is, is the same bytes in the two: the cells are written natively and no cell is in the data that rides.
    for (name, bytes) in &b {
        if name == "[Content_Types].xml" || name.starts_with("_rels/") || name.contains("solution") || name.starts_with("visio/_rels/") {
            continue;
        }
        assert_eq!(a.iter().find(|(n, _)| n == name).map(|(_, x)| x), Some(bytes), "{name}");
    }
    assert!(b.iter().all(|(n, _)| !n.contains("solution") && !n.starts_with("diacraft/")), "no data, no part");
    // No cell, value or formula of the drawing is in the part: the cells that are in it say nothing of the file's.
    let part = solution(&with.bytes);
    for name in ["PinX", "LineWeight", "Width", "<diacraft:Cell"] {
        assert!(!part.contains(name), "{name} is in the part:\n{part}");
    }
    // A reader that does not know the part, as another program does not, reads the drawing as it reads the plain one: the document the carried data is taken out of is the same.
    let (mut read_with, _) = read_back(&with.bytes);
    let (read_without, _) = read_back(&without.bytes);
    assert!(!data_of(&read_with).is_empty());
    read_with.ext = diacraft_doc::Ext::default();
    assert_eq!(read_with.pages.len(), read_without.pages.len());
    for (p, q) in read_with.pages.iter().zip(read_without.pages.iter()) {
        assert_eq!((p.id, &p.name, p.shapes.len()), (q.id, &q.name, q.shapes.len()));
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the scan's findings: macros and the cost of a crafted list

/// The package with the type of its document part changed to one that allows no macros.
fn as_macro_free(bytes: &[u8]) -> Vec<u8> {
    let mut items = items_of(bytes);
    let types = &mut items.iter_mut().find(|(n, _)| n == "[Content_Types].xml").unwrap().1;
    let text = String::from_utf8(types.clone()).unwrap();
    assert!(text.contains("drawing.macroEnabled.main+xml"));
    *types = text.replace("drawing.macroEnabled.main+xml", "drawing.main+xml").into_bytes();
    pack(&items)
}

#[test]
fn a_part_is_what_its_name_says_and_not_what_its_item_claims() {
    let written = write(&odf_model(), true);
    // The item says the macro project is a part of no role, and a picture is a macro project: the roles are made again from the names.
    let edited = edit(&written.bytes, SOLUTION, |text| {
        let text = text.replacen("role=\"macro-project\"", "role=\"other\"", 1);
        let at = text.find("name=\"Pictures/p.png\"").unwrap();
        let end = at + text[at..].find("role=\"other\"").unwrap();
        format!("{}role=\"macro-project\"{}", &text[..end], &text[end + "role=\"other\"".len()..])
    });
    let (back, report) = read_back(&edited);
    let role_of = |name: &str| back.parts.iter().find(|p| &*p.name == name).map(|p| p.role).unwrap();
    assert_eq!(role_of("Basic/Standard/script-lb.xml"), PartRole::MacroProject, "the name says it");
    assert_eq!(role_of("Pictures/p.png"), PartRole::Other, "and this one is a picture");
    assert_eq!(report.warning_count("diacraft_part_role"), 1, "{:?}", report.warnings);
}

#[test]
fn a_macro_part_in_a_file_of_a_type_that_allows_none_is_not_read() {
    let written = write(&odf_model(), true);
    // The same file, typed as a drawing that allows no macros.
    let free = as_macro_free(&written.bytes);
    work::reset();
    let (back, report) = read_back(&free);
    assert_eq!(report.variant.map(|v| v.macro_enabled), Some(false));
    assert!(
        back.parts.iter().all(|p| !matches!(p.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData)),
        "no macro project is in the model"
    );
    assert!(back.parts.iter().all(|p| !p.name.starts_with("Basic/")));
    assert_eq!(report.warning_count("diacraft_macro_refused"), 1, "{:?}", report.warnings);
    assert!(
        back.parts.iter().any(|p| &*p.name == "Object 1/content.xml") && back.parts.iter().any(|p| &*p.name == "Pictures/p.png"),
        "the rest comes back"
    );
    // Its bytes were not copied, nor charged: the parts that were read are the others (26 + 0 + 7 bytes, and the fill image's 16).
    assert_eq!(work::carry_bytes_copied(), 26 + 7 + 16);
    // A claim of another role does not get it in: the entry is a macro project by its name.
    let claims_other = edit(&free, SOLUTION, |text| text.replacen("role=\"macro-project\"", "role=\"other\"", 1));
    work::reset();
    let (back, report) = read_back(&claims_other);
    assert!(back.parts.iter().all(|p| !p.name.starts_with("Basic/")), "{:?}", back.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
    assert_eq!(report.warning_count("diacraft_macro_refused"), 1);
    assert_eq!(work::carry_bytes_copied(), 26 + 7 + 16);
    // The macro-enabled type keeps it.
    let (back, _) = read_back(&written.bytes);
    assert!(back.parts.iter().any(|p| &*p.name == "Basic/Standard/script-lb.xml"));
}

#[test]
fn a_macro_project_is_known_by_its_name_when_a_file_is_written() {
    // The role of a part is a claim: the name is what the entry is, and a macro project goes only into a type that allows macros, whatever role a model states.
    for name in ["Scripts/python/x.py", "Basic/Standard/m.xml", "basic/lower.xml", "SCRIPTS/up.py", "META-INF/macrosignatures.xml"] {
        let mut model = odf_model();
        model.parts = diacraft_doc::SharedVec::from_vec(model.parts.iter().filter(|p| p.role != PartRole::MacroProject).cloned().collect());
        model.parts.push(part(Format::Odf, name, "text/xml", PartRole::Other, b"x"));
        assert_eq!(write_with(&model, None, &options(false)).unwrap_err(), WriteError::MacroProject, "{name}");
        let dropped = write_with(&model, None, &WriteOptions { drop_macros: true, ..options(false) }).unwrap();
        assert_eq!(dropped.macros_dropped, vec![name.to_owned()], "{name}");
        assert!(write_with(&model, None, &options(true)).is_ok(), "{name}");
    }
}

/// A drawing whose first page has `connects` connects (so that stamping it takes that many steps) and data of the other format on it.
fn big_page_model(connects: u32) -> Document {
    let (mut doc, _) = read_ok(&base_bytes());
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    page.connects = diacraft_doc::Connects::from_vec(
        (0..connects).map(|i| diacraft_doc::Connect { from_sheet: i, to_sheet: i + 1, ..Default::default() }).collect(),
    );
    page.ext.make_mut().odf = OdfPage { master_page: Some(Arc::from("Default")), ..OdfPage::default() };
    doc
}

/// The text of the item of page 1, and the text before and after it.
fn page_item_of(solution: &str) -> (&str, &str, &str) {
    let start = solution.find("<dc:item kind=\"page\" id=\"1\"").unwrap();
    let end = start + solution[start..].find("</dc:item>").unwrap() + "</dc:item>".len();
    (&solution[..start], &solution[start..end], &solution[end..])
}

#[test]
fn many_items_for_one_big_page_are_one_ambiguity_and_are_not_stamped() {
    let written = write(&big_page_model(30_000), false);
    work::reset();
    let (alone, one) = read_back(&written.bytes);
    let stamped_once = work::carry_items_stamped();
    assert!(stamped_once >= 30_000, "the page is stamped once: {stamped_once}");
    assert!(data_of(&alone).iter().any(|d| d.starts_with("page 0:")));
    // The same item named by two thousand items: none is judged, and the page is not stamped for each of them.
    let copies = 2_000usize;
    let crafted = edit(&written.bytes, SOLUTION, |text| {
        let (before, item, after) = page_item_of(text);
        format!("{before}{}{after}", item.repeat(copies))
    });
    work::reset();
    let (back, report) = read_back(&crafted);
    assert!(work::carry_items_stamped() <= stamped_once, "{} steps for {copies} items, {stamped_once} for one", work::carry_items_stamped());
    assert_eq!(report.carry.left_out, copies as u64, "{:?}", report.carry);
    assert_eq!(report.warning_count("diacraft_item_ambiguous"), 1, "one word for them all: {:?}", report.warnings);
    assert!(data_of(&back).iter().all(|d| !d.starts_with("page 0:")), "an item that names the same object as another names nothing");
    assert_eq!(one.carry.kept, 1);
    // Items for one shape, the same: the shape is not stamped or placed for each.
    let model = odf_model();
    let written = write(&model, true);
    let crafted = edit(&written.bytes, SOLUTION, |text| {
        let start = text.find("<dc:item kind=\"shape\" of=\"1\" id=\"1\"").unwrap();
        let end = start + text[start..].find("</dc:item>").unwrap() + "</dc:item>".len();
        format!("{}{}{}", &text[..start], text[start..end].repeat(500), &text[end..])
    });
    let (back, report) = read_back(&crafted);
    assert_eq!(report.carry.left_out, 500, "{:?}", report.carry);
    assert!(data_of(&back).iter().all(|d| !d.starts_with("page 0 shape 1:")));
    assert!(data_of(&back).iter().any(|d| d.starts_with("page 0 shape 2:")), "the other shapes keep theirs");
}

#[test]
fn shapes_that_share_a_big_sheet_stamp_it_once() {
    // 1,500 shapes that hold one sheet of 400 cells, each with data of the other format: the sheet is stamped once and held, and not again after each item is put.
    let (mut doc, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P")).build());
    let sheet = Arc::new({
        let mut sheet = diacraft_doc::Sheet::default();
        for i in 0..400 {
            sheet.cells.insert(diacraft_doc::CellName::new(&format!("C{i}")), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        }
        sheet
    });
    let n = 1_500u32;
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    for id in 1..=n {
        let mut s = Shape::new(id);
        s.sheet = Arc::clone(&sheet);
        s.ext.make_mut().odf.style_name = Some(Arc::from(format!("gr{id}")));
        page.shapes.push(Arc::new(s));
    }
    work::reset();
    let written = write(&doc, false);
    let on_write = work::carry_items_stamped();
    assert!(on_write <= 400 * 3 + 20 * u64::from(n), "the writer stamped the shared sheet once for {n} shapes: {on_write} steps");
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, u64::from(n), "{:?}", report.carry);
    let on_read = work::carry_items_stamped();
    assert!(on_read <= 400 * 3 + 20 * u64::from(n), "the reader stamped it once, and not after each of the {n} items: {on_read} steps");
    assert_eq!(data_of(&back).len(), n as usize);
}

#[test]
fn the_judge_stops_where_its_steps_are_spent_and_says_so() {
    let doc = page_of(2_000);
    let written = write(&doc, false);
    let limits = Limits { max_carry_steps: 3_000, ..Limits::default() };
    let (back, report) = crate::read_with(&written.bytes, &limits);
    assert!(report.carry.kept > 0 && report.carry.kept < 2_000, "{:?}", report.carry);
    assert_eq!(data_of(&back).len() as u64, report.carry.kept, "what was judged is in the model, and what was not is not");
    assert_eq!(report.warning_count("diacraft_items_not_read"), 1, "{:?}", report.warnings);
    // The default budget judges them all.
    let (_, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, 2_000);
}

// ------------------------------------------------------------------------------------------------------------------------------ a save into the file

fn splice(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options(true))
        .unwrap_or_else(|e| panic!("the splice save refused: {e}"))
}

#[test]
fn a_save_into_a_file_that_carries_data_keeps_it_and_an_unedited_save_changes_nothing() {
    let first = write(&odf_model(), true);
    let (doc, _) = read_back(&first.bytes);
    let before = data_of(&doc);
    // No edit: the same package.
    let same = splice(&doc, &first.bytes, &doc);
    assert_eq!(
        items_of(&same.bytes).iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        items_of(&first.bytes).iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    );
    assert_eq!(solution(&same.bytes), solution(&first.bytes));
    // An edit of a cell of shape 1: its item is made again, with the stamp of the cell it now sits beside, and the data is in the file as it was.
    let mut edited = doc.clone();
    {
        let shape = shape_mut(&mut edited, 0, 1);
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(diacraft_doc::CellName::new("PinX"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("5")));
    }
    let saved = splice(&edited, &first.bytes, &doc);
    assert_eq!(blob_parts(&saved.bytes).len(), 5, "the parts of DiaCraft's own are as many as they were");
    let (again, report) = read_back(&saved.bytes);
    assert_eq!(data_of(&again), before, "the data comes back, the edited shape's too: its item was made again");
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    assert_eq!(warnings_of_the_carrier(&report), 0);
    // A model that has none of it any more: the part, its parts and its row are gone.
    let (plain, _) = read_ok(&base_bytes());
    let stripped = splice(&plain, &first.bytes, &doc);
    let names: Vec<String> = items_of(&stripped.bytes).into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().all(|n| !n.contains("solution") && !n.starts_with("diacraft/")), "{names:?}");
    assert!(data_of(&read_back(&stripped.bytes).0).is_empty());
}

#[test]
fn a_save_into_a_file_with_no_solution_makes_the_part_the_parts_and_their_relationships() {
    let original = base_bytes();
    let (plain, _) = read_ok(&original);
    let model = odf_model();
    let saved = splice(&model, &original, &plain);
    assert_eq!(blob_parts(&saved.bytes).len(), 5);
    let (back, report) = read_back(&saved.bytes);
    assert_eq!(report.variant, Some(Variant { kind: Kind::Drawing, macro_enabled: true }), "{:?} {:?}", report.warnings, report.carry);
    assert!(report.warnings.is_empty(), "{:?} {:?}", report.warnings, report.carry);
    assert_same_data(&data_of(&back), &data_of(&model));
    assert_eq!(report.carry.kept, report.carry.items, "{:?}", report.carry);
    // The type allows macros here (a splice into a file of the type the caller names): a type that allows none refuses the macro project, as it does for VBA.
    let refused = write_with(&model, Some(&OriginalBytes { bytes: &original, baseline: Some(&plain) }), &options(false)).unwrap_err();
    assert_eq!(refused, WriteError::MacroProject);
    // A part that holds more than DiaCraft's data is patched as it was and gets no items, in words.
    let other = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"/><Note>another reader's</Note></SolutionXML>")
        .build();
    let (base, _) = read_ok(&other);
    let (mut with_data, _) = read_ok(&other);
    shape_mut(&mut with_data, 0, 1).ext.make_mut().odf.style_name = Some(Arc::from("gr1"));
    let saved = splice(&with_data, &other, &base);
    assert!(saved.warnings.iter().any(|w| w.contains("holds more than DiaCraft's data")), "{:?}", saved.warnings);
    assert!(solution(&saved.bytes).contains("another reader's") && !solution(&saved.bytes).contains("dc:item"));
}

#[test]
fn parts_of_the_package_that_are_held_and_parts_of_the_other_format_are_both_in_the_model() {
    // A package that holds a part the model has no field for, beside the data of the other format: the read moves the held part in, and the parts that rode stay.
    let with_held = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .with(diacraft_testkit::package::Pkg::new().part("/visio/custom/held.bin", "application/octet-stream", [9u8, 8, 7]))
        .build();
    let (mut doc, _) = read_ok(&with_held);
    assert_eq!(doc.parts.len(), 1);
    doc.parts.push(part(Format::Odf, "Object 1/content.xml", "text/xml", PartRole::ObjectFile, b"<x/>"));
    let written = write(&doc, false);
    let (back, report) = read_back(&written.bytes);
    let formats: Vec<(Format, &str)> = back.parts.iter().map(|p| (p.format, &*p.name)).collect();
    assert_eq!(formats, vec![(Format::Vsdx, "/visio/custom/held.bin"), (Format::Odf, "Object 1/content.xml")], "{:?}", report.warnings);
    assert_eq!(report.held_parts, 1);
}

#[test]
fn a_drawing_made_in_diacraft_that_holds_data_of_the_other_format_carries_it_too() {
    // No package was read: the drawing is new, with a shape that came from an `.odg` and the document's own data. It is written as a new drawing is, and the data rides.
    let mut doc = Document::new_blank();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let mut s = Shape::new(1);
    s.ext.make_mut().odf.style_name = Some(Arc::from("gr1"));
    page.shapes.push(Arc::new(s));
    doc.ext.make_mut().odf.mimetype = Some(Arc::from("application/vnd.oasis.opendocument.graphics"));
    doc.parts.push(part(Format::Odf, "Pictures/p.png", "image/png", PartRole::Other, &[1, 2, 3]));
    let written = write(&doc, false);
    let (back, report) = read_back(&written.bytes);
    assert_same_data(&data_of(&back), &data_of(&doc));
    assert_eq!((report.carry.items, report.carry.kept), (3, 3), "{:?}", report.carry);
    assert_eq!(warnings_of_the_carrier(&report), 0, "{:?}", report.warnings);
}

// ------------------------------------------------------------------------------------------------------------------------------ the scan's findings: relationship targets, and the copies

/// The package with the first `Target` of a relationships part changed to `target`.
fn retarget_first_row(bytes: &[u8], rels_part: &str, target: &str) -> Vec<u8> {
    edit(bytes, rels_part, |text| {
        let at = text.find("Target=\"").unwrap() + "Target=\"".len();
        let end = at + text[at..].find('"').unwrap();
        format!("{}{target}{}", &text[..at], &text[end..])
    })
}

const SOLUTION_RELS: &str = "/visio/solutions/_rels/solution1.xml.rels";

#[test]
fn a_relationship_of_the_type_of_diacraft_parts_that_leads_to_another_part_is_not_read_and_that_part_is_never_removed() {
    let written = write(&odf_model(), true);
    // A crafted file: the first relationship of the solution part is of the type of DiaCraft's parts and leads to the first page.
    let crafted = retarget_first_row(&written.bytes, SOLUTION_RELS, "../pages/page1.xml");
    work::reset();
    let (doc, report) = read_back(&crafted);
    assert_eq!(report.carry.items, 0, "the part is not DiaCraft's, and its items are not read: {:?}", report.carry);
    assert_eq!(work::carry_bytes_copied(), 0, "no part is read as bytes the data names");
    assert!(doc.parts.iter().all(|p| p.format == Format::Vsdx), "no part of the other format comes of it");
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 3, "the page is read as a page");
    // A save that changes DiaCraft's own data does not take the page out: it is the file's, whatever the relationship says.
    let mut edited = doc.clone();
    assert!(edited.set_diacraft_child("snap", Some(&[("alignment_box", "1")])));
    let saved = splice(&edited, &crafted, &doc);
    let names: Vec<String> = items_of(&saved.bytes).into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().any(|n| n == "visio/pages/page1.xml"), "the page is still in the file: {names:?}");
    let (again, _) = read_back(&saved.bytes);
    assert_eq!(again.pages.get(0).unwrap().shapes.len(), 3);
    // The same for a row that leads to the document part and to a part under another folder than `/diacraft/`.
    for target in ["../document.xml", "../../docProps/core.xml", "../../diacraftx/blob1.xml"] {
        let crafted = retarget_first_row(&written.bytes, SOLUTION_RELS, target);
        let (doc, _) = read_back(&crafted);
        let mut edited = doc.clone();
        assert!(edited.set_diacraft_child("snap", Some(&[("alignment_box", "1")])));
        let saved = splice(&edited, &crafted, &doc);
        let before: Vec<String> =
            items_of(&crafted).into_iter().map(|(n, _)| n).filter(|n| !n.starts_with("diacraft/") && !n.contains("solution")).collect();
        let after: Vec<String> = items_of(&saved.bytes).into_iter().map(|(n, _)| n).collect();
        for n in before {
            assert!(after.contains(&n), "{target}: {n} is gone from the saved file");
        }
    }
}

#[test]
fn a_part_of_diacraft_that_another_relationship_names_is_not_read_as_ours_and_is_not_removed() {
    let written = write(&odf_model(), true);
    let blobs = blob_parts(&written.bytes);
    assert_eq!(blobs.len(), 5);
    // A crafted file: the document also names one of the parts of DiaCraft's own, as a picture.
    let crafted = edit(&written.bytes, "/visio/_rels/document.xml.rels", |text| {
        text.replace("</Relationships>", &format!("<Relationship Id=\"rId99\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"..{}\"/></Relationships>", blobs[3]))
    });
    let (doc, report) = read_back(&crafted);
    assert_eq!(report.carry.items, 0, "a part that another part names is not DiaCraft's alone: {:?}", report.carry);
    assert_eq!(doc.parts.iter().filter(|p| p.name.starts_with("/diacraft/")).count(), 5, "the parts are held as the parts of the package they are");
    // A save that changes DiaCraft's data takes none of them out.
    let mut edited = doc.clone();
    assert!(edited.set_diacraft_child("snap", Some(&[("alignment_box", "1")])));
    let saved = splice(&edited, &crafted, &doc);
    assert_eq!(blob_parts(&saved.bytes).len(), 5, "{:?}", blob_parts(&saved.bytes));
}

#[test]
fn a_save_that_changes_diacraft_s_data_is_not_refused_for_relationships_of_the_part_that_do_not_read() {
    // The `vsdx_roundtrip` fuzz target found that a save which writes the solution part again was refused as `Mismatch("a relationships part has another root")` for a file that opened
    // (a damaged file, or another program's): the part's own relationships hold another root than `Relationships`, the reader holds such a part as the bytes it is, and the writer read it with `?`.
    // A part the writer cannot use is kept as it is, and the save says so (the part that is not XML, the index that does not read), and never fails.
    let written = write(&odf_model(), true);
    let crafted = edit(&written.bytes, SOLUTION_RELS, |_| "<Row/>".to_owned());
    let (doc, report) = read_back(&crafted);
    assert_eq!(report.carry.items, 0, "the part's relationships do not read, and its items are not read: {:?}", report.carry);
    let mut edited = doc.clone();
    assert!(edited.set_diacraft_child("snap", Some(&[("alignment_box", "1")])));
    let saved = splice(&edited, &crafted, &doc);
    let words = saved.warnings.join(" ");
    assert!(
        words.contains("the relationships of the file's DiaCraft part do not read"),
        "the save says why the settings were not written: {:?}",
        saved.warnings
    );
    // The part, its relationships and the parts they name are as the file had them.
    let part_of = |bytes: &[u8], name: &str| items_of(bytes).into_iter().find(|(n, _)| format!("/{n}") == name).map(|(_, b)| b);
    assert_eq!(part_of(&saved.bytes, SOLUTION), part_of(&crafted, SOLUTION), "the solution part is not changed");
    assert_eq!(part_of(&saved.bytes, SOLUTION_RELS), part_of(&crafted, SOLUTION_RELS), "nor are its relationships");
    assert_eq!(blob_parts(&saved.bytes), blob_parts(&crafted), "and no part of DiaCraft's own is taken out");
}

#[test]
fn a_save_that_changes_diacraft_s_data_is_not_refused_for_another_relationships_part_that_does_not_read() {
    // A relationships part of a page holds another root: it may name any part, one of DiaCraft's own among them (the reader takes it to, and reads no item), so the writer takes out no part and
    // patches the part as it is: DiaCraft's settings are written, and the items the reader left in the data stay where they are. It was refused as a `Mismatch` before.
    let written = write(&odf_model(), true);
    let mut items = items_of(&written.bytes);
    items.push(("visio/pages/_rels/page2.xml.rels".to_owned(), b"<Row/>".to_vec()));
    let crafted = pack(&items);
    let (doc, report) = read_back(&crafted);
    assert_eq!(report.carry.items, 0, "a relationships part that does not read is taken to name the parts of DiaCraft's: {:?}", report.carry);
    let mut edited = doc.clone();
    assert!(edited.set_diacraft_child("snap", Some(&[("alignment_box", "1")])));
    let saved = splice(&edited, &crafted, &doc);
    assert_eq!(blob_parts(&saved.bytes), blob_parts(&crafted), "no part of DiaCraft's own is taken out");
    let text = solution(&saved.bytes);
    assert!(text.contains("alignment_box"), "the settings are written: {text}");
    assert!(text.contains("kind=\"document\"") && text.contains("kind=\"shape\""), "the items the reader did not read are where they were: {text}");
}

#[test]
fn items_that_put_data_on_one_shared_sheet_are_charged_the_copies_they_make() {
    // 400 shapes that hold one sheet of 500 cells, each with data of the other format in a row of it: each item's put copies the sheet (the table of stamps holds it, and others share it).
    let (mut doc, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P")).build());
    let mut sheet = diacraft_doc::Sheet::default();
    for i in 0..500 {
        sheet.cells.insert(diacraft_doc::CellName::new(&format!("C{i}")), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
    }
    let mut section = diacraft_doc::Section::new("Connection", None);
    let mut row = diacraft_doc::Row { ix: Some(0), ..diacraft_doc::Row::default() };
    row.ext.make_mut().odf = OdfRow { style_name: Some(Arc::from("gr")), ..OdfRow::default() };
    section.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(section));
    let sheet = Arc::new(sheet);
    let n = 400u32;
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    for id in 1..=n {
        let mut s = Shape::new(id);
        s.sheet = Arc::clone(&sheet);
        page.shapes.push(Arc::new(s));
    }
    let written = write(&doc, false);
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, u64::from(n), "{:?}", report.carry);
    let copied = work::carry_nodes_copied();
    assert!(copied >= u64::from(n) * 500, "each of the {n} puts was charged the nodes of the sheet it copies: {copied}");
    assert!(copied <= u64::from(n) * 520, "and no more: {copied}");
    assert_eq!(data_of(&back).len(), n as usize);
    // A judge that can pay for a few copies and no more stops, in words: the items it did not judge are left out.
    let limits = Limits { max_carry_steps: 5 * 600, ..Limits::default() };
    let (back, report) = crate::read_with(&written.bytes, &limits);
    assert!(report.carry.kept >= 1 && report.carry.kept <= 6, "{:?}", report.carry);
    assert_eq!(report.warning_count("diacraft_items_not_read"), 1, "{:?}", report.warnings);
    assert_eq!(data_of(&back).len() as u64, report.carry.kept);
}

#[test]
#[cfg(target_os = "linux")]
fn items_that_copy_a_shared_sheet_are_refused_in_words_by_a_child_under_the_address_limit_and_do_not_abort() {
    // 40 pages that name one part of 40 shapes that hold one sheet of 2,400 cells (3.8 million cells, as many as the file may hold): 1,600 items, each a copy of the sheet (some 300 KB of the
    // model). The pool is what the read holds and room for 96 MB of copies, a few hundred of them: the read keeps the items it can pay for, and says that it left the others out, in a process
    // under the address limit.
    let text = under_the_limit("shared:40:40:2400");
    assert!(text.contains("OUTCOME read pages=40 items=1600"), "{text}");
    assert!(text.contains("diacraft_items_not_read"), "{text}");
    let kept: u64 = text.split("kept=").nth(1).and_then(|t| t.split(' ').next()).and_then(|v| v.parse().ok()).unwrap();
    assert!(kept > 0 && kept < 400, "{kept}");
}

#[test]
fn items_that_put_data_on_one_shared_text_are_charged_the_text_they_copy() {
    // 100 pages that name one part of a shape whose text has 5,000 parts: each page's item puts a part among them, which copies the text the others share.
    let (pages, parts) = (100u64, 5_000usize);
    let written = write(&shared_text_doc(usize::try_from(pages).unwrap(), parts), false);
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, pages, "{:?}", report.carry);
    let copied = work::carry_nodes_copied();
    assert!(copied >= pages * parts as u64, "each of the {pages} puts was charged the parts of the text it copies: {copied}");
    assert!(copied <= pages * (parts as u64 + 400), "and no more: {copied}");
    assert_eq!(data_of(&back).len(), pages as usize);
    // A judge that can pay for a few copies and no more stops, in words.
    let limits = Limits { max_carry_steps: 3 * parts as u64 + 2_000, ..Limits::default() };
    let (back, report) = crate::read_with(&written.bytes, &limits);
    assert!(report.carry.kept >= 1 && report.carry.kept <= 3, "{:?}", report.carry);
    assert_eq!(report.warning_count("diacraft_items_not_read"), 1, "{:?}", report.warnings);
    assert_eq!(data_of(&back).len() as u64, report.carry.kept);
}

#[test]
#[cfg(target_os = "linux")]
fn items_that_copy_a_shared_text_are_refused_in_words_by_a_child_under_the_address_limit_and_do_not_abort() {
    // 60 pages that name one part of a shape whose text has 20,000 parts: 60 items, each a copy of the text (some 2 MB of the model); the pool is what the read holds and room for 96 MB of
    // copies, a few dozen of them: the read keeps the items it can pay for, and says that it left the others out, in a process under the address limit.
    let text = under_the_limit("text:60:20000");
    assert!(text.contains("OUTCOME read pages=60 items=60"), "{text}");
    assert!(text.contains("diacraft_items_not_read"), "{text}");
    let kept: u64 = text.split("kept=").nth(1).and_then(|t| t.split(' ').next()).and_then(|v| v.parse().ok()).unwrap();
    assert!(kept > 0 && kept < 60, "{kept}");
}

// ------------------------------------------------------------------------------------------------------------------------------ crafted inputs, counted

/// A drawing of one page of `shapes` shapes that each hold data of the other format.
fn page_of(shapes: u32) -> Document {
    let (mut doc, _) = read_ok(
        &VsdxBuilder::new()
            .page(PageSpec::new(1, "P").shapes(&(1..=shapes).map(|i| shape(i, &cell("PinX", &i.to_string()))).collect::<String>()))
            .build(),
    );
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    for i in 0..page.shapes.len() {
        let s = Arc::make_mut(page.shapes.get_mut(i).unwrap());
        s.ext.make_mut().odf.style_name = Some(Arc::from(format!("gr{}", s.id)));
    }
    doc
}

#[test]
fn many_items_are_walked_stamped_and_judged_once_each() {
    let n = 20_000u32;
    let doc = page_of(n);
    work::reset();
    let written = write(&doc, false);
    let walked = work::carry_objects_walked();
    // Each shape is looked at to find what rides, and again to find a key two shapes share (the page has items): twice, and the sheets are shared by none here.
    assert!(walked <= 8 * u64::from(n), "{walked}");
    assert!(work::carry_items_stamped() <= 12 * u64::from(n), "{}", work::carry_items_stamped());
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, u64::from(n), "{:?}", report.carry);
    assert_eq!(work::carry_items_judged(), u64::from(n));
    assert!(work::carry_objects_walked() <= 2 * u64::from(n) + 10, "the page is walked once to place the data: {}", work::carry_objects_walked());
    assert_eq!(data_of(&back).len(), n as usize);
}

#[test]
fn a_chain_of_nested_groups_each_with_data_costs_a_walk_of_the_chain() {
    // Each shape holds the next, to the depth the model allows; each has an item, and the place of each is the path down to it.
    let depth = 90usize;
    let (mut doc, _) = read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).build());
    let mut leaf = Arc::new({
        let mut s = Shape::new(u32::try_from(depth).unwrap());
        s.ext.make_mut().odf.style_name = Some(Arc::from("leaf"));
        s
    });
    for id in (1..depth).rev() {
        let mut s = Shape::new(u32::try_from(id).unwrap());
        s.ext.make_mut().odf.style_name = Some(Arc::from(format!("g{id}")));
        s.children.push(leaf);
        leaf = Arc::new(s);
    }
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes = std::iter::once(leaf).collect();
    work::reset();
    let written = write(&doc, false);
    assert!(work::carry_objects_walked() <= 8 * depth as u64, "{}", work::carry_objects_walked());
    work::reset();
    let (back, report) = read_back(&written.bytes);
    assert_eq!(report.carry.kept, depth as u64, "{:?}", report.carry);
    assert_eq!(data_of(&back).len(), depth);
    assert!(work::carry_objects_walked() <= 2 * depth as u64 + 10, "{}", work::carry_objects_walked());
}

#[test]
fn one_byte_string_named_by_five_thousand_parts_is_one_part_copied_once() {
    let (mut doc, _) = read_ok(&base_bytes());
    for i in 0..5_000 {
        doc.parts.push(part(Format::Odf, &format!("Pictures/p{i}.png"), "image/png", PartRole::Other, &[7u8; 1000]));
    }
    let written = write(&doc, false);
    assert_eq!(blob_parts(&written.bytes).len(), 1, "equal bytes are one part");
    work::reset();
    let (back, _) = read_back(&written.bytes);
    assert_eq!(back.parts.iter().filter(|p| p.format == Format::Odf).count(), 5_000);
    assert_eq!(work::carry_bytes_copied(), 1000, "one copy, however many parts name it");
}

// ------------------------------------------------------------------------------------------------------------------------------ memory: a refusal is not an abort

/// `len` bytes that no compression makes smaller.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            u8::try_from(x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56).unwrap()
        })
        .collect()
}

fn status_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.strip_prefix(field).and_then(|v| v.trim_start_matches(':').trim().strip_suffix("kB").and_then(|n| n.trim().parse().ok()))
            })
        })
        .unwrap_or(0)
}

const SCENARIO: &str = "DIACRAFT_CARRY_SCENARIO";

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs a scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("the scenario");
    let mut fields = scenario.split(':');
    let (kind, n, bytes, cells): (&str, usize, usize, usize) = (
        fields.next().unwrap_or(""),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
    );
    if kind == "shared" {
        return shared_sheet_child(n, bytes, cells);
    }
    if kind == "text" {
        return shared_text_child(n, bytes);
    }
    let (mut doc, _) = read_ok(&base_bytes());
    for i in 0..n {
        let held: Arc<[u8]> = Arc::from(noise(bytes, i as u64 * 2 + 5));
        doc.parts.push(Arc::new(Part {
            format: Format::Odf,
            name: Arc::from(format!("Object {i}/big.bin")),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Embedding,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: held,
        }));
    }
    let outcome = write_with(&doc, None, &options(false));
    match &outcome {
        Err(e) => println!("OUTCOME refused:{} {e}", e.class()),
        Ok(written) if kind == "roundtrip" => {
            drop(doc);
            let (back, report) = read(&written.bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file does not read: {e}"));
            println!("OUTCOME written {} bytes, read {} parts, {} kept", written.bytes.len(), back.parts.len(), report.carry.parts_kept);
        }
        Ok(written) => println!("OUTCOME written {} bytes", written.bytes.len()),
    }
    println!("VMPEAK {}", status_kib("VmPeak"));
    println!("carry: no abort");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// `pages` pages that all name one part of `shapes` shapes, which all hold one sheet of `cells` cells and a row that holds data of the other format: a small file (the part is written once,
/// and the sheet is one allocation in the model), and an item for each shape of each page, each of which puts data on that sheet. **What the judge does not charge for the copies is the size
/// of a sheet for each item**, which is far past the address space: the read must stop where the pool is spent, in words, and not abort.
fn shared_sheet_child(pages: usize, shapes: usize, cells: usize) {
    let mut sheet = diacraft_doc::Sheet::default();
    for i in 0..cells {
        sheet.cells.insert(diacraft_doc::CellName::new(&format!("C{i}")), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
    }
    let mut section = diacraft_doc::Section::new("Connection", None);
    let mut row = diacraft_doc::Row { ix: Some(0), ..diacraft_doc::Row::default() };
    row.ext.make_mut().odf = OdfRow { style_name: Some(Arc::from("gr")), ..OdfRow::default() };
    section.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(section));
    let sheet = Arc::new(sheet);
    let list: diacraft_doc::ShapeList = (1..=u32::try_from(shapes).unwrap())
        .map(|id| {
            let mut s = Shape::new(id);
            s.sheet = Arc::clone(&sheet);
            Arc::new(s)
        })
        .collect();
    let mut doc = Document::new_blank();
    // A drawing read from a package (it holds its rows): its pages that name one part are written to one part while their content is the same.
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    doc.pages = diacraft_doc::SharedVec::new();
    for id in 1..=u32::try_from(pages).unwrap() {
        let mut page = diacraft_doc::Page::new(id, &format!("P{id}"));
        page.shapes = list.clone();
        page.part_name = Some(Arc::from("/visio/pages/page1.xml"));
        doc.pages.push(Arc::new(page));
    }
    let written = match write_with(&doc, None, &options(false)) {
        Ok(written) => written,
        Err(e) => {
            println!("OUTCOME refused-at-the-write:{} {e}", e.class());
            println!("carry: no abort");
            return;
        }
    };
    drop(doc);
    read_in_a_tight_pool(&written.bytes);
}

/// Reads a file in a pool that is what the read holds without any item (a judge with no steps judges none) and room for 96 MB of copies, and says what it kept: the copies are what the pool
/// cannot hold, and the read stops where it is spent, in words.
fn read_in_a_tight_pool(bytes: &[u8]) {
    // What the read holds without any item (a judge with no steps judges none), and a pool that is that and room for a few copies of the sheet: the copies are what the pool cannot hold.
    let (_, bare) = read(bytes, &Limits { max_carry_steps: 0, ..Limits::default() }).expect("the file reads with no item judged");
    let room = bare.stats.file_bytes + bare.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    let tight = Limits { max_memory: room + bare.stats.memory_peak + (96 << 20), ..Limits::default() };
    match read(bytes, &tight) {
        Ok((back, report)) => println!(
            "OUTCOME read pages={} items={} kept={} warnings={:?}",
            back.pages.len(),
            report.carry.items,
            report.carry.kept,
            report.warning_counts.iter().filter(|(c, _)| c.starts_with("diacraft_")).collect::<Vec<_>>()
        ),
        Err(e) => println!("OUTCOME refused:{} {e}", e.class()),
    }
    println!("VMPEAK {}", status_kib("VmPeak"));
    println!("carry: no abort");
}

/// A document of `pages` pages that name one part of one shape whose text has `parts` parts (and a part that rides among them), so that each page has an item for a shape and a text that all of
/// them share.
fn shared_text_doc(pages: usize, parts: usize) -> Document {
    let mut text = Text { parts: Vec::with_capacity(parts + 1), ext: diacraft_doc::Ext::default() };
    for i in 0..parts {
        text.parts.push(if i % 2 == 0 { TextPart::Chars(Arc::from("ab")) } else { TextPart::Cp(0) });
    }
    text.parts.push(TextPart::OdfRaw(Box::new(RawNode::Element(raw("text:bookmark", &[("text:name", "b")])))));
    let mut shape = Shape::new(1);
    shape.text = Some(Arc::new(text));
    let list: diacraft_doc::ShapeList = std::iter::once(Arc::new(shape)).collect();
    let mut doc = Document::new_blank();
    // A drawing read from a package (it holds its rows): its pages that name one part are written to one part while their content is the same.
    doc.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld::default()));
    doc.pages = diacraft_doc::SharedVec::new();
    for id in 1..=u32::try_from(pages).unwrap() {
        let mut page = diacraft_doc::Page::new(id, &format!("P{id}"));
        page.shapes = list.clone();
        page.part_name = Some(Arc::from("/visio/pages/page1.xml"));
        doc.pages.push(Arc::new(page));
    }
    doc
}

fn shared_text_child(pages: usize, parts: usize) {
    let doc = shared_text_doc(pages, parts);
    let written = match write_with(&doc, None, &options(false)) {
        Ok(written) => written,
        Err(e) => {
            println!("OUTCOME refused-at-the-write:{} {e}", e.class());
            println!("carry: no abort");
            return;
        }
    };
    drop(doc);
    read_in_a_tight_pool(&written.bytes);
}

/// Runs a scenario in a child under `ulimit -v 3000000`: it ends well (no abort), and says how.
#[cfg(target_os = "linux")]
fn under_the_limit(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact carry::child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("carry: no abort"), "{scenario}: the child did not end well: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("OUTCOME") || l.contains("VMPEAK")).collect::<Vec<_>>().join(" | "));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn carried_parts_too_large_to_write_are_refused_in_words_and_not_an_abort() {
    // Three parts of 256 MiB that no compression makes smaller: the file and the package the check inflates are each that large, beside the model.
    let text = under_the_limit("write:3:268435456");
    assert!(text.contains("OUTCOME refused:write_limit"), "{text}");
    assert!(text.contains("too large to be written and read back"), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn carried_parts_that_fit_are_written_and_read_back_under_the_address_limit() {
    let text = under_the_limit("roundtrip:4:33554432");
    assert!(text.contains("OUTCOME written") && text.contains("read 4 parts"), "{text}");
    let peak: u64 = text.lines().find_map(|l| l.strip_prefix("VMPEAK ")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    assert!(peak > 0 && peak < 3_000_000, "{peak}");
}

// ------------------------------------------------------------------------------------------------------------------------------ crafted inputs, timed

/// Each crafted input beside the same drawing with none of it, read: the case costs no more than a multiple of the plain read of a file of the same size, and says what it measured.
#[test]
#[ignore = "timing: run with --ignored, on a machine that is not busy"]
fn timed_beside_a_yardstick() {
    let time = |label: &str, work: &dyn Fn()| -> f64 {
        let start = Instant::now();
        work();
        let secs = start.elapsed().as_secs_f64();
        println!("{label}: {secs:.3} s");
        secs
    };
    // 30,000 parts against 30,000 parts of the same bytes that are held as parts of the package (a plain VSDX file of the same size).
    let n = 30_000usize;
    let carried = {
        let (mut doc, _) = read_ok(&base_bytes());
        for i in 0..n {
            doc.parts.push(part(
                Format::Odf,
                &format!("Object {i}/x.bin"),
                "application/octet-stream",
                PartRole::ObjectFile,
                &u32::try_from(i).unwrap().to_le_bytes(),
            ));
        }
        write(&doc, false).bytes
    };
    let plain = {
        let (mut doc, _) = read_ok(&base_bytes());
        for i in 0..n {
            doc.parts.push(part(
                Format::Vsdx,
                &format!("/custom/x{i}.bin"),
                "application/octet-stream",
                PartRole::Other,
                &u32::try_from(i).unwrap().to_le_bytes(),
            ));
        }
        write(&doc, false).bytes
    };
    let a = time("read 30,000 carried parts", &|| drop(read(&carried, &Limits::default()).unwrap()));
    let b = time("read 30,000 held parts (the yardstick)", &|| drop(read(&plain, &Limits::default()).unwrap()));
    assert!(a <= 4.0 * b + 0.5, "{a} against {b}");
    // 20,000 shapes with items against the same 20,000 shapes with none.
    let n = 20_000;
    let with = write(&page_of(n), false).bytes;
    let without = write(
        &read_ok(
            &VsdxBuilder::new()
                .page(PageSpec::new(1, "P").shapes(&(1..=n).map(|i| shape(i, &cell("PinX", &i.to_string()))).collect::<String>()))
                .build(),
        )
        .0,
        false,
    )
    .bytes;
    let a = time("read 20,000 shapes with items", &|| drop(read(&with, &Limits::default()).unwrap()));
    let b = time("read 20,000 shapes with none (the yardstick)", &|| drop(read(&without, &Limits::default()).unwrap()));
    assert!(a <= 4.0 * b + 0.5, "{a} against {b}");
    let a = time("write 20,000 shapes with items", &|| drop(write(&page_of(n), false)));
    let b = time("write 20,000 shapes with none (the yardstick)", &|| {
        drop(write(
            &read_ok(
                &VsdxBuilder::new()
                    .page(PageSpec::new(1, "P").shapes(&(1..=n).map(|i| shape(i, &cell("PinX", &i.to_string()))).collect::<String>()))
                    .build(),
            )
            .0,
            false,
        ))
    });
    assert!(a <= 4.0 * b + 0.5, "{a} against {b}");
    // Twenty thousand items for one big page against twenty thousand for a page that is not there (each is dropped at once): the same bytes in the part.
    let written = write(&big_page_model(30_000), false).bytes;
    let make = |id: &str| {
        edit(&written, SOLUTION, |text| {
            let (before, item, after) = page_item_of(text);
            format!("{before}{}{after}", item.replacen("id=\"1\"", &format!("id=\"{id}\""), 1).repeat(20_000))
        })
    };
    let (dup, gone) = (make("1"), make("99"));
    let a = time("read 20,000 items for one big page", &|| drop(read(&dup, &Limits::default()).unwrap()));
    let b = time("read 20,000 items for a page that is gone (the yardstick)", &|| drop(read(&gone, &Limits::default()).unwrap()));
    assert!(a <= 2.0 * b + 0.3, "{a} against {b}");
    // A macro part of two megabytes of text that looks like XML against one of the same size that does not.
    let xml = noise_xml(60_000);
    let text = noise_text(xml.len());
    let make = |bytes: &str| {
        let mut model = odf_model();
        model.parts.push(part(Format::Odf, "Basic/Standard/big.xml", "text/xml", PartRole::MacroProject, bytes.as_bytes()));
        write(&model, true).bytes
    };
    let (fx, ft) = (make(&xml), make(&text));
    let a = time("read a macro part that looks like XML", &|| drop(read(&fx, &Limits::default()).unwrap()));
    let b = time("read a macro part that is plain text (the yardstick)", &|| drop(read(&ft, &Limits::default()).unwrap()));
    assert!(a <= 2.0 * b + 0.2, "{a} against {b}");
}
