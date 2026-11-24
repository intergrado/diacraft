//! The `.odg` model writer at package and document level (`write_with(doc, None, ..)`; the native-format design's §3.4, §4.4, §4.5).
//! A drawing that has no file is written from the model alone: its package, `meta.xml`, `settings.xml` and manifest are written from what the
//! model holds, its `Basic/`, `Scripts/` and object entries as they are, and its pages and shapes by the splice writer's new-shape path.
//!
//! Every read-back goes through the real reader (`diacraft_odg::read`), never through the writer's own tokenizer.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, CustomProp, DocProps, Document, Format, Part, PartRole, Passthrough, RawAttrs, RawElement, RawNode, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, SaveNote, Variant, WriteError, WriteOptions, Written, compare_content, read, sniff, write_with};

use crate::write_core::PNG;
use crate::write_new::{push_shape, rectangle};

/// A4 portrait, in inches, as the engine's pages hold a size.
const A4: (f64, f64) = (21.0 / 2.54, 29.7 / 2.54);

/// A new drawing as the engine makes one (`Document::new_blank`), on A4 paper, with `shapes` rectangles on its page.
pub fn blank(shapes: u32) -> Document {
    let mut doc = Document::new_blank();
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut page.sheet);
    for (name, value) in [("PageWidth", A4.0), ("PageHeight", A4.1)] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(value).unwrap()).with_unit("IN"));
    }
    for k in 0..shapes {
        push_shape(&mut doc, rectangle(k + 1, &format!("Rectangle.{}", k + 1), (2.0 + f64::from(k) * 3.0, 3.0, 2.0, 2.0)));
    }
    doc
}

/// The model written from nothing, with `options`.
pub fn save(doc: &Document, options: &WriteOptions) -> Result<Written, WriteError> {
    write_with(doc, None, options)
}

fn text(name: &str, doc: &[u8]) -> String {
    Package::read(doc, &Limits::default()).unwrap().part(name).map_or_else(|| format!("<<no {name}>>"), |b| String::from_utf8_lossy(b).into_owned())
}

fn names(bytes: &[u8]) -> Vec<String> {
    Package::read(bytes, &Limits::default()).unwrap().entries().map(|e| e.name.clone()).collect()
}

/// A package entry of the `.odg` format.
fn odf_part(name: &str, media: &str, role: PartRole, attrs: &[(&str, &str)], bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Odf,
        name: Arc::from(name),
        media_type: Arc::from(media),
        role,
        manifest: RawAttrs::from_pairs(attrs.iter().copied()),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children })
}

fn t(text: &str) -> RawNode {
    RawNode::Text(Arc::from(text))
}

fn props() -> DocProps {
    let mut p = DocProps {
        subject: Some(Arc::from("A subject")),
        creator: Some(Arc::from("A. Author")),
        last_modified_by: Some(Arc::from("B. Editor")),
        keywords: Some(Arc::from("first")),
        description: Some(Arc::from("What it is")),
        created: Some(Arc::from("2026-01-02T03:04:05Z")),
        modified: Some(Arc::from("2026-02-03T04:05:06Z")),
        last_printed: Some(Arc::from("2026-03-04T05:06:07Z")),
        language: Some(Arc::from("en-US")),
        ..DocProps::default()
    };
    p.custom.push(CustomProp {
        format: Format::Odf,
        name: Arc::from("Project"),
        value_type: Arc::from("string"),
        value: Arc::from("Atlas"),
        raw: RawAttrs::from_pairs([("meta:extra", "yes")]),
    });
    p.custom.push(CustomProp {
        format: Format::Odf,
        name: Arc::from("Revision"),
        value_type: Arc::from("float"),
        value: Arc::from("7"),
        raw: RawAttrs::new(),
    });
    p
}

/// The settings tree of a LibreOffice-shaped drawing: one item set with a few items.
fn settings_tree() -> Arc<RawElement> {
    let RawNode::Element(root) = el(
        "office:document-settings",
        &[
            ("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
            ("xmlns:config", "urn:oasis:names:tc:opendocument:xmlns:config:1.0"),
            ("xmlns:ooo", "http://openoffice.org/2004/office"),
            ("office:version", "1.3"),
        ],
        vec![el(
            "office:settings",
            &[],
            vec![el(
                "config:config-item-set",
                &[("config:name", "ooo:configuration-settings")],
                vec![
                    el("config:config-item", &[("config:name", "ScaleNumerator"), ("config:type", "int")], vec![t("1")]),
                    el("config:config-item", &[("config:name", "ScaleDenominator"), ("config:type", "int")], vec![t("1")]),
                    el("config:config-item", &[("config:name", "IsSnapToGrid"), ("config:type", "boolean")], vec![t("true")]),
                ],
            )],
        )],
    ) else {
        panic!("an element")
    };
    Arc::new(root)
}

/// A model with every field of the package and the document's own data that the reader fills and this writer writes.
fn full() -> Document {
    let mut doc = blank(2);
    doc.title = Arc::from("The title");
    doc.props = Arc::new(props());
    let odf = &mut doc.ext.make_mut().odf;
    odf.mimetype = Some(Arc::from(Variant::Drawing.media_type()));
    odf.settings = Some(settings_tree());
    // The fields the reader keeps whole, each after the number of typed fields before it: the generator first, the cycles after the creator, the statistics last.
    let mut meta = Passthrough::new();
    meta.push(0, el("meta:generator", &[], vec![t("LibreOffice/7.3$Linux")]));
    meta.push(5, el("meta:editing-cycles", &[], vec![t("3")]));
    meta.push(5, el("meta:editing-duration", &[], vec![t("PT5M")]));
    meta.push(12, el("meta:document-statistic", &[("meta:object-count", "2")], vec![]));
    odf.meta = meta;
    let o = "urn:opendocument:test";
    odf.roots = vec![
        (
            Arc::from("meta.xml"),
            RawAttrs::from_pairs([
                ("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
                ("xmlns:meta", "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"),
                ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
                ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
                ("xmlns:ooo", o),
                ("office:version", "1.3"),
            ]),
        ),
        (
            Arc::from("META-INF/manifest.xml"),
            RawAttrs::from_pairs([("xmlns:manifest", "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"), ("manifest:version", "1.3")]),
        ),
    ];
    odf.manifest_rows = vec![
        (Arc::from("/"), RawAttrs::from_pairs([("manifest:version", "1.3"), ("manifest:media-type", Variant::Drawing.media_type())])),
        (Arc::from("settings.xml"), RawAttrs::from_pairs([("manifest:media-type", "text/xml"), ("manifest:size", "999")])),
    ];
    doc
}

fn read_back(bytes: &[u8]) -> (Document, diacraft_odg::ReadReport) {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file did not read back: {} ({e})", e.class()))
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- the writer from nothing

/// A drawing that was never saved is written: `write_with(doc, None, ..)` gives a package that reads back as the drawing, and the error that said "never saved" is gone from the
/// writer's classes.
#[test]
fn no_original_error_is_gone() {
    let doc = blank(2);
    let written = save(&doc, &WriteOptions::default()).expect("a drawing with no file is written");
    assert_eq!(sniff(&written.bytes), Some(Variant::Drawing));
    let (back, report) = read_back(&written.bytes);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.pages.len(), 1);
    let page = back.pages.get(0).unwrap();
    assert_eq!(page.shapes.len(), 2, "both rectangles are in the file");
    let number = |name: &str| page.sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).unwrap();
    assert!((number("PageWidth") - A4.0).abs() < 1e-9 && (number("PageHeight") - A4.1).abs() < 1e-9, "the page has the model's size");
    // No class of the writer says a drawing is never saved: every refusal it gives is for what the drawing holds.
    let refused =
        [WriteError::Unsupported("x"), WriteError::Mismatch("x"), WriteError::Limit { what: "x", max: 1 }, WriteError::SelfCheck(String::new())];
    assert!(refused.iter().all(|e| e.class() != "write_no_original"));
    // And the words of the one that was are not what any refusal says.
    let none = save(&Document::empty(), &WriteOptions::default()).unwrap_err();
    assert!(!none.to_string().contains("never saved"), "{none}");
}

/// D6 (a second save is no edit): the same model written again is the same bytes, and the file read again and written once more is the same content.
#[test]
fn a_second_model_save_is_no_edit() {
    let doc = blank(3);
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    let first = save(&doc, &options).unwrap();
    let again = save(&doc, &options).unwrap();
    assert_eq!(first.bytes, again.bytes, "in the same session: the same bytes");
    // After a reopen: the model the file reads as, written from nothing, is the same content.
    let (reopened, _) = read_back(&first.bytes);
    let after = save(&reopened, &options).unwrap();
    let diff = compare_content(&first.bytes, &after.bytes, &Limits::default()).unwrap();
    assert!(diff.same(), "{:?}", diff.shown);
}

/// The model with every field of the package and the document's own data, written, read again and written again, is the same content (design 4.3): the title, the properties, the fields
/// of `meta.xml`, `settings.xml`, the manifest's rows, the entries and the containers' markup all come back as they were written.
#[test]
fn a_full_model_written_read_again_and_written_again_is_the_same_content() {
    let mut doc = full();
    for part in [
        odf_part("Basic/", "", PartRole::MacroProject, &[], &[]),
        odf_part("Basic/Standard/Module1.xml", "text/xml", PartRole::MacroProject, &[], b"<script:module/>"),
        odf_part("Object 1/", "application/vnd.oasis.opendocument.chart", PartRole::ObjectFile, &[("manifest:version", "1.3")], &[]),
        odf_part("Object 1/content.xml", "text/xml", PartRole::ObjectFile, &[], b"<office:document-content/>"),
    ] {
        doc.parts.push(part);
    }
    let scripts =
        el("office:document-content", &[], vec![el("office:scripts", &[], vec![el("office:script", &[("script:language", "ooo:Basic")], vec![])])]);
    doc.ext.make_mut().odf.kept.kept = {
        let mut kept = Passthrough::new();
        kept.push(0, scripts);
        kept
    };
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    let first = save(&doc, &options).unwrap();
    let (reopened, _) = read_back(&first.bytes);
    let second = save(&reopened, &options).unwrap();
    let diff = compare_content(&first.bytes, &second.bytes, &Limits::default()).unwrap();
    assert!(diff.same(), "{:?}", diff.shown);
    // And once more, from the model the second file reads as.
    let (again, _) = read_back(&second.bytes);
    let third = save(&again, &options).unwrap();
    assert!(compare_content(&second.bytes, &third.bytes, &Limits::default()).unwrap().same());
}

/// The model writer has no file to compare with: what it writes of the bookkeeping is the edited save's unless the caller says the content is as it was read.
#[test]
fn the_flat_form_is_refused_in_words_for_a_drawing_with_no_file() {
    let refused = save(&blank(1), &WriteOptions { form: Some(diacraft_odg::Form::Flat), ..WriteOptions::default() }).unwrap_err();
    assert!(matches!(&refused, WriteError::Unsupported(w) if w.contains("flat")), "{refused}");
}

/// A drawing is a template or a drawing as asked, else as its own `mimetype` says, else a drawing.
#[test]
fn the_kind_is_the_one_asked_or_the_models_own() {
    let doc = blank(1);
    assert_eq!(sniff(&save(&doc, &WriteOptions::default()).unwrap().bytes), Some(Variant::Drawing));
    let asked = save(&doc, &WriteOptions { variant: Some(Variant::Template), ..WriteOptions::default() }).unwrap();
    assert_eq!(sniff(&asked.bytes), Some(Variant::Template));
    let mut template = blank(1);
    template.ext.make_mut().odf.mimetype = Some(Arc::from(Variant::Template.media_type()));
    let own = save(&template, &WriteOptions::default()).unwrap();
    assert_eq!(sniff(&own.bytes), Some(Variant::Template), "the model's own mimetype");
    let over = save(&template, &WriteOptions { variant: Some(Variant::Drawing), ..WriteOptions::default() }).unwrap();
    assert_eq!(sniff(&over.bytes), Some(Variant::Drawing), "what is asked wins");
    let package = Package::read(&own.bytes, &Limits::default()).unwrap();
    assert_eq!(package.media_type(), Some(Variant::Template.media_type()));
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- meta, settings, manifest

/// The title, the properties, the user-defined fields, the fields kept whole, `settings.xml`, the roots and the manifest's rows are written from the model and read back equal.
#[test]
fn the_manifest_meta_and_settings_are_written_from_the_model_and_read_back_equal() {
    let doc = full();
    let written = save(&doc, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).expect("the model is written");
    let (back, report) = read_back(&written.bytes);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // meta.xml: the title, the fields both formats say, and the user-defined fields.
    assert_eq!(back.title, doc.title);
    assert_eq!(*back.props, *doc.props);
    // What the reader kept whole: the generator as held (the content is as it was read), at its places.
    let (held, read) = (&doc.ext.get().unwrap().odf, &back.ext.get().unwrap().odf);
    assert_eq!(read.meta, held.meta, "the kept fields are where they were");
    // settings.xml: the whole tree, with the version the package says (every part of a package this writer makes says 1.4: a root row that says 1.3 over parts of 1.4 is a package
    // LibreOffice refuses to load).
    let mut tree = (**held.settings.as_ref().unwrap()).clone();
    tree.attrs = RawAttrs::from_pairs(tree.attrs.iter().map(|(k, v)| (k, if k == "office:version" { "1.4" } else { v })));
    assert_eq!(read.settings.as_deref(), Some(&tree));
    // The roots the model holds are the roots of the parts written, but for the version.
    for (part, key) in [("meta.xml", "office:version"), ("META-INF/manifest.xml", "manifest:version")] {
        let at_14 = |attrs: &RawAttrs| RawAttrs::from_pairs(attrs.iter().map(|(k, v)| (k, if k == key { "1.4" } else { v })));
        let want = held.roots.iter().find(|(n, _)| &**n == part).map(|(_, a)| at_14(a));
        let got = read.roots.iter().find(|(n, _)| &**n == part).map(|(_, a)| a.clone());
        assert_eq!(got, want, "the root of {part}");
    }
    assert_eq!(read.mimetype.as_deref(), Some(Variant::Drawing.media_type()));
    // The manifest's rows: the root's attributes (the version the model holds) and a part's, as held.
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    let rows = &package.manifest().entries;
    let root = rows.iter().find(|r| r.full_path == "/").expect("the root row");
    assert_eq!(root.media_type.as_deref(), Some(Variant::Drawing.media_type()));
    assert!(root.attrs.iter().any(|(k, v)| k == "manifest:version" && v == "1.4"), "the version the package says: {:?}", root.attrs);
    let settings = rows.iter().find(|r| r.full_path == "settings.xml").expect("settings.xml is listed");
    assert!(settings.attrs.iter().any(|(k, v)| k == "manifest:size" && v == "999"), "the row's other attributes are held: {:?}", settings.attrs);
    assert!(rows.iter().any(|r| r.full_path == "content.xml") && rows.iter().any(|r| r.full_path == "styles.xml"));
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
}

/// The generator (design 4.4): with the content as it was read, the generator the model holds is written as it was, and the model's other fields with it; otherwise it is
/// DiaCraft's, in the place of the model's, and never written twice. A model that holds none is given DiaCraft's.
#[test]
fn the_generator_is_the_models_while_the_content_stamp_holds_and_diacraft_s_otherwise() {
    let doc = full();
    let generator = |bytes: &[u8]| -> Vec<String> {
        let meta = text("meta.xml", bytes);
        meta.match_indices("<meta:generator>").map(|(at, _)| meta[at + 16..].split('<').next().unwrap().to_owned()).collect()
    };
    let kept = save(&doc, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    assert_eq!(generator(&kept.bytes), ["LibreOffice/7.3$Linux"]);
    assert!(!kept.edited, "the content is as it was read");
    let edited = save(&doc, &WriteOptions::default()).unwrap();
    let own = format!("DiaCraft/{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(generator(&edited.bytes), std::slice::from_ref(&own), "DiaCraft's, once, in the place of the model's");
    assert!(edited.edited);
    // The other fields are the same either way.
    let (a, _) = read_back(&kept.bytes);
    let (b, _) = read_back(&edited.bytes);
    assert_eq!((a.title, a.props), (b.title, b.props));
    // No generator held: DiaCraft's, whatever the stamp says.
    let bare = blank(1);
    for unchanged in [true, false] {
        let written = save(&bare, &WriteOptions { content_unchanged: unchanged, ..WriteOptions::default() }).unwrap();
        assert_eq!(generator(&written.bytes), std::slice::from_ref(&own), "content_unchanged {unchanged}");
    }
}

/// The thumbnail: the one passed is written and the model's own replaced, unless the content is as it was read, when the model's is written as it was.
#[test]
fn the_thumbnail_follows_the_content_stamp() {
    let mut doc = blank(1);
    let held = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 9, 9, 9];
    let thumbnail = odf_part("Thumbnails/thumbnail.png", "image/png", PartRole::Thumbnail, &[], &held);
    doc.parts.push(thumbnail);
    let png = |bytes: &[u8]| Package::read(bytes, &Limits::default()).unwrap().part("Thumbnails/thumbnail.png").map(<[u8]>::to_vec);
    let fresh = Some(PNG.to_vec());
    // Edited, with a picture: the picture passed.
    let edited = save(&doc, &WriteOptions { thumbnail: fresh.clone(), ..WriteOptions::default() }).unwrap();
    assert_eq!(png(&edited.bytes), fresh);
    // As it was read, with a picture passed: the model's own, as it was.
    let stamped = save(&doc, &WriteOptions { thumbnail: fresh.clone(), content_unchanged: true, ..WriteOptions::default() }).unwrap();
    assert_eq!(png(&stamped.bytes), Some(held.to_vec()));
    // Edited, with none passed: the model's own is kept, as an edited splice save keeps the package's.
    let kept = save(&doc, &WriteOptions::default()).unwrap();
    assert_eq!(png(&kept.bytes), Some(held.to_vec()));
    // None held, none passed: none. One row, once, when there is one.
    let none = save(&blank(1), &WriteOptions::default()).unwrap();
    assert_eq!(png(&none.bytes), None);
    let rows = |bytes: &[u8]| {
        Package::read(bytes, &Limits::default()).unwrap().manifest().entries.iter().filter(|e| e.full_path == "Thumbnails/thumbnail.png").count()
    };
    assert_eq!((rows(&edited.bytes), rows(&stamped.bytes), rows(&kept.bytes), rows(&none.bytes)), (1, 1, 1, 0));
    // A drawing that has none, saved for the first time with a picture, has it.
    assert_eq!(png(&save(&blank(1), &WriteOptions { thumbnail: fresh.clone(), ..WriteOptions::default() }).unwrap().bytes), fresh);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- Basic/, Scripts/, objects

/// The macro projects: `Basic/` and `Scripts/` and their folders are written as they are with their rows, nothing is run, and the declaration that names them (`office:scripts`
/// in `content.xml`, kept whole by the reader) is put back where it stood, so that the reader says the file holds macros.
#[test]
fn basic_and_scripts_are_written_natively() {
    let mut doc = blank(1);
    let library = b"<?xml version=\"1.0\"?><script:module/>".to_vec();
    let script = b"print('not run')\n".to_vec();
    for part in [
        odf_part("Basic/", "", PartRole::MacroProject, &[], &[]),
        odf_part("Basic/Standard/", "", PartRole::MacroProject, &[], &[]),
        odf_part("Basic/Standard/Module1.xml", "text/xml", PartRole::MacroProject, &[], &library),
        odf_part("Scripts/python/hello.py", "application/x-python", PartRole::MacroProject, &[("manifest:size", "17")], &script),
    ] {
        doc.parts.push(part);
    }
    // The declaration: `office:scripts` is kept whole as a child of the root, before the children the reader read.
    let scripts =
        el("office:document-content", &[], vec![el("office:scripts", &[], vec![el("office:script", &[("script:language", "ooo:Basic")], vec![])])]);
    let odf = &mut doc.ext.make_mut().odf;
    odf.kept.kept = {
        let mut kept = Passthrough::new();
        kept.push(0, scripts);
        kept
    };
    odf.roots.push((
        Arc::from("content.xml"),
        RawAttrs::from_pairs([
            ("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
            ("xmlns:script", "urn:oasis:names:tc:opendocument:xmlns:script:1.0"),
            ("office:version", "1.3"),
        ]),
    ));
    let written = save(&doc, &WriteOptions::default()).expect("a drawing with macros is written as an .odg");
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(package.part("Basic/Standard/Module1.xml"), Some(&library[..]));
    assert_eq!(package.part("Scripts/python/hello.py"), Some(&script[..]));
    assert!(package.folders().any(|f| f == "Basic/") && package.folders().any(|f| f == "Basic/Standard/"), "the folders are entries");
    let row = |path: &str| package.manifest().entries.iter().find(|r| r.full_path == path).cloned();
    assert_eq!(row("Basic/Standard/Module1.xml").and_then(|r| r.media_type), Some("text/xml".to_owned()));
    assert!(
        row("Scripts/python/hello.py").is_some_and(|r| r.attrs.iter().any(|(k, v)| k == "manifest:size" && v == "17")),
        "a row's other attributes"
    );
    assert!(package.warnings().is_empty(), "every entry is listed, so no program offers a repair: {:?}", package.warnings());
    // Read back: the parts are the model's, in order, and the file says it holds macros.
    let (back, report) = read_back(&written.bytes);
    assert!(report.macros, "the declaration is where it was");
    let held = |d: &Document| {
        d.parts.iter().map(|p| (p.name.to_string(), p.media_type.to_string(), p.role, p.manifest.clone(), p.bytes.to_vec())).collect::<Vec<_>>()
    };
    assert_eq!(held(&back), held(&doc));
    // `office:scripts` comes first, as the schema has it, and the root is the model's.
    let content = text("content.xml", &written.bytes);
    let (scripts_at, drawing_at) = (content.find("<office:scripts>").unwrap(), content.find("<office:body>").unwrap());
    assert!(scripts_at < drawing_at, "{content}");
    assert!(content.contains("xmlns:script=") && content.contains("office:version=\"1.4\""), "{content}");
    // A second write of the model is the same bytes.
    assert_eq!(save(&doc, &WriteOptions::default()).unwrap().bytes, written.bytes);
}

/// An object of the `.odg` format: its folder with the document it is, its replacement picture, and the rows of each (the folder's with the version of its own and its
/// media type) are written as they are, and read back as the model had them.
#[test]
fn an_odf_object_is_written_natively() {
    let mut doc = blank(1);
    let chart = "application/vnd.oasis.opendocument.chart";
    let picture = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 5, 5];
    for part in [
        odf_part("Object 1/", chart, PartRole::ObjectFile, &[("manifest:version", "1.3")], &[]),
        odf_part("Object 1/content.xml", "text/xml", PartRole::ObjectFile, &[], b"<office:document-content/>"),
        odf_part("Object 1/styles.xml", "text/xml", PartRole::ObjectFile, &[], b"<office:document-styles/>"),
        odf_part("ObjectReplacements/Object 1", "image/png", PartRole::ObjectReplacement, &[], &picture),
    ] {
        doc.parts.push(part);
    }
    let written = save(&doc, &WriteOptions::default()).expect("a drawing with an object is written");
    let names = names(&written.bytes);
    for entry in ["Object 1/", "Object 1/content.xml", "Object 1/styles.xml", "ObjectReplacements/Object 1"] {
        assert!(names.iter().any(|n| n == entry), "{entry} is an entry: {names:?}");
    }
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    let folder = package.manifest().entries.iter().find(|r| r.full_path == "Object 1/").cloned().expect("the folder's row");
    assert_eq!(folder.media_type.as_deref(), Some(chart));
    assert!(folder.attrs.iter().any(|(k, v)| k == "manifest:version" && v == "1.3"), "{:?}", folder.attrs);
    assert_eq!(package.part("ObjectReplacements/Object 1"), Some(&picture[..]));
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
    let (back, _) = read_back(&written.bytes);
    let held = |d: &Document| {
        d.parts.iter().map(|p| (p.name.to_string(), p.media_type.to_string(), p.role, p.manifest.clone(), p.bytes.to_vec())).collect::<Vec<_>>()
    };
    assert_eq!(held(&back), held(&doc), "the object's entries are the model's, in its order, with the roles the names give");
}

/// An entry the manifest never listed is listed by the file this writer makes (no program is offered a repair), by the media type its name says or none.
#[test]
fn an_entry_the_model_holds_with_no_row_gets_one() {
    let mut doc = blank(1);
    doc.parts.push(odf_part("Configurations2/accelerator/current.xml", "", PartRole::Other, &[], b"<x/>"));
    doc.parts.push(odf_part("manifest.rdf", "", PartRole::Other, &[], b"<rdf/>"));
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    assert!(package.warnings().is_empty(), "{:?}", package.warnings());
    let media = |p: &str| package.manifest().entries.iter().find(|r| r.full_path == p).and_then(|r| r.media_type.clone());
    assert_eq!(media("Configurations2/accelerator/current.xml").as_deref(), Some("text/xml"));
    assert_eq!(media("manifest.rdf").as_deref(), Some(""));
}

/// A signature is kept and no longer matches: the save says so, once.
#[test]
fn a_signature_is_kept_and_the_save_says_it_no_longer_matches() {
    let mut doc = blank(1);
    doc.parts.push(odf_part("META-INF/documentsignatures.xml", "", PartRole::Signature, &[], b"<dsig/>"));
    let written = save(&doc, &WriteOptions::default()).unwrap();
    assert_eq!(written.notes.iter().filter(|n| **n == SaveNote::SignatureStale).count(), 1, "{:?}", written.notes);
    assert!(Package::read(&written.bytes, &Limits::default()).unwrap().part("META-INF/documentsignatures.xml").is_some());
    assert!(save(&blank(1), &WriteOptions::default()).unwrap().notes.iter().all(|n| *n != SaveNote::SignatureStale));
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- refusals, in words

/// What an `.odg` cannot say rides in DiaCraft's own folder and document file: a part of a VSDX file or of a newer DiaCraft, a property ODF has no field for and the markup of a VSDX document
/// come back; the refusals that remain are in words, and nothing is left out.
#[test]
fn what_an_odg_cannot_say_rides_and_the_refusals_that_remain_are_in_words_and_never_left_out() {
    let words = |doc: &Document| match save(doc, &WriteOptions::default()) {
        Err(WriteError::Unsupported(w)) => w.to_owned(),
        other => panic!("expected a refusal in words, got {other:?}"),
    };
    // A VSDX part (a macro project), and a part of a newer DiaCraft, ride as bytes and come back.
    for format in [Format::Vsdx, Format::Native] {
        let mut doc = blank(1);
        doc.parts.push(Arc::new(Part {
            format,
            // A part of a VSDX package is named as that package names one, with its slash; a part of a `.diacraft` is a name of its ZIP, with none.
            name: Arc::from(if format == Format::Vsdx { "/vbaProject.bin" } else { "vbaProject.bin" }),
            media_type: Arc::from("application/vnd.ms-office.vbaProject"),
            role: PartRole::MacroProject,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(&b"vba"[..]),
        }));
        let saved = save(&doc, &WriteOptions::default()).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        assert!(back.parts.iter().any(|p| p.format == format && p.name.ends_with("vbaProject.bin") && &p.bytes[..] == b"vba"), "{format:?}");
    }
    // The thumbnail of a VSDX file is no loss: the `.odg` gets its own.
    let mut with_thumbnail = blank(1);
    with_thumbnail.parts.push(Arc::new(Part {
        format: Format::Vsdx,
        name: Arc::from("/docProps/thumbnail.png"),
        media_type: Arc::from("image/png"),
        role: PartRole::Thumbnail,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(&PNG[..]),
    }));
    assert!(save(&with_thumbnail, &WriteOptions::default()).is_ok());
    // A property ODF has no field for rides in the document file.
    for set in [
        |p: &mut DocProps| p.category = Some(Arc::from("c")),
        |p: &mut DocProps| p.company = Some(Arc::from("c")),
        |p: &mut DocProps| p.manager = Some(Arc::from("c")),
        |p: &mut DocProps| p.hyperlink_base = Some(Arc::from("c")),
    ] {
        let mut doc = blank(1);
        let mut p = DocProps::default();
        set(&mut p);
        doc.props = Arc::new(p.clone());
        let saved = save(&doc, &WriteOptions::default()).unwrap();
        let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
        assert_eq!(
            (&back.props.category, &back.props.company, &back.props.manager, &back.props.hyperlink_base),
            (&p.category, &p.company, &p.manager, &p.hyperlink_base)
        );
    }
    // The application version is the producer's, as the generator is: not a loss, not written.
    let mut doc = blank(1);
    doc.props = Arc::new(DocProps { app_version: Some(Arc::from("16.0000")), ..DocProps::default() });
    assert!(save(&doc, &WriteOptions::default()).is_ok());
    // Markup of a VSDX document that no field reads rides too.
    let mut doc = blank(1);
    let mut other = Passthrough::new();
    other.push(0, el("EventList", &[], vec![]));
    doc.passthrough = other;
    let saved = save(&doc, &WriteOptions::default()).unwrap();
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(back.passthrough, doc.passthrough);
    // Parts named as the writer's own entries, or twice.
    for name in ["content.xml", "mimetype", "META-INF/manifest.xml", "DiaCraft/document.xml"] {
        let mut doc = blank(1);
        doc.parts.push(odf_part(name, "text/xml", PartRole::Other, &[], b"<x/>"));
        assert!(words(&doc).contains("named as an entry the writer makes"), "{name}");
    }
    let mut twice = blank(1);
    twice.parts.push(odf_part("a.bin", "", PartRole::Other, &[], b"1"));
    twice.parts.push(odf_part("a.bin", "", PartRole::Other, &[], b"2"));
    assert!(words(&twice).contains("two parts of one name"));
}

/// A drawing with no page, or with a first page that has no size, is refused in words.
#[test]
fn a_drawing_with_no_page_or_no_size_is_refused_in_words() {
    let none = save(&Document::empty(), &WriteOptions::default()).unwrap_err();
    assert!(matches!(&none, WriteError::Unsupported(w) if w.contains("no page")), "{none}");
    let mut doc = blank(1);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(&mut page.sheet).cells.remove("PageHeight");
    let refused = save(&doc, &WriteOptions::default()).unwrap_err();
    assert!(matches!(&refused, WriteError::Unsupported(w) if w.contains("width and height")), "{refused}");
    let mut doc = blank(1);
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("PageWidth"), Cell::constant(Value::from_number(0.0).unwrap()).with_unit("IN"));
    assert!(matches!(save(&doc, &WriteOptions::default()), Err(WriteError::Unsupported(_))));
}

/// What the model holds of the document that this save does not write yet is said in a note (never left out silently), with its count by kind.
#[test]
fn odf_data_of_the_document_that_is_not_written_yet_is_said_in_a_note() {
    let mut doc = blank(1);
    let odf = &mut doc.ext.make_mut().odf;
    odf.layers.push(Arc::new(RawElement {
        name: Arc::from("draw:layer"),
        attrs: RawAttrs::from_pairs([("draw:name", "extra")]),
        children: Vec::new(),
    }));
    odf.styles.push(diacraft_doc::KeptStyle {
        family: Arc::from("common:text"),
        name: Arc::from("Strong"),
        element: Arc::new(RawElement {
            name: Arc::from("style:style"),
            attrs: RawAttrs::from_pairs([("style:name", "Strong"), ("style:family", "text")]),
            children: Vec::new(),
        }),
    });
    // A piece of a container that the part made has no place for: it stands in a master page the package is not made with.
    let stray = el(
        "office:document-styles",
        &[],
        vec![el("office:master-styles", &[], vec![el("style:master-page", &[("style:name", "Other")], vec![el("draw:layer-set", &[], vec![])])])],
    );
    odf.kept.kept = {
        let mut kept = Passthrough::new();
        kept.push(1, stray);
        kept
    };
    let written = save(&doc, &WriteOptions::default()).unwrap();
    let note = written.notes.iter().find(|n| n.class() == "document_data_unwritten").expect("a note says what was not written");
    // The layer and the common style are written: what is said is the piece of the containers the part made has no place for.
    assert_eq!(*note, SaveNote::DocumentDataUnwritten { definitions: 0, layers: 0, styles: 0, containers: 1 });
    let said = note.to_string();
    assert!(said.contains("1 piece of the containers' markup"), "{said}");
    let (back, _) = read_back(&save(&doc, &WriteOptions::default()).unwrap().bytes);
    let after = back.ext.get().map(|d| d.odf.clone()).unwrap_or_default();
    assert!(after.layers.iter().any(|l| l.attrs.get("draw:name") == Some("extra")), "the layer is in the file");
    assert!(
        after.styles.iter().any(|s| &*s.name == "Strong" && &*s.family == "common:text"),
        "the style is in the file: {:?} in {}",
        after.styles.iter().map(|s| (s.family.clone(), s.name.clone())).collect::<Vec<_>>(),
        crate::write_core::part(&save(&doc, &WriteOptions::default()).unwrap().bytes, "styles.xml")
    );
    // A model with none of it says nothing.
    assert!(save(&blank(1), &WriteOptions::default()).unwrap().notes.iter().all(|n| n.class() != "document_data_unwritten"));
}

/// The second page of a file holds the markup of its element and is written with its master page (closed the refusal), and a frame of an object is written with the
/// element of the object it kept.
#[test]
fn a_second_pages_markup_and_an_object_frames_elements_are_written() {
    use crate::build::{DRAWING, package};
    use crate::drawing::Drawing;
    let model = |drawing: &Drawing| read_back(&package(DRAWING, drawing.files())).0;
    let rect = r#"<draw:rect svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#;
    // One page of a file: the page the package is made for, so it is written.
    let one = model(&Drawing::a4().page(rect));
    assert!(save(&one, &WriteOptions::default()).is_ok());
    // Two: the second page is written with the master page it names.
    let two = model(&Drawing::a4().page(rect).page(rect));
    let written = save(&two, &WriteOptions::default()).expect("the second page of a file is written");
    let (back, _) = read_back(&written.bytes);
    assert_eq!(back.pages.len(), 2);
    for page in back.pages.iter() {
        assert_eq!(page.ext.get().and_then(|d| d.odf.master_page.clone()).as_deref(), Some("Default"));
    }
    // An object frame: written, and the note says what the new shape held that the file does not.
    let mut object = Drawing::a4().page(
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#,
    );
    object.files.push(("Object 1/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
    object.files.push(("ObjectReplacements/Object 1".to_owned(), PNG.to_vec()));
    let written = save(&model(&object), &WriteOptions::default()).expect("written");
    // The model path writes the frame's own markup back, the object's element among it, and says nothing of it.
    let content = crate::write_core::part(&written.bytes, "content.xml");
    assert!(content.contains("<draw:object ") && content.contains("xlink:href=\"./Object 1\""), "the object's element is written: {content}");
    assert!(written.notes.iter().all(|n| n.class() != "new_shape_unwritten"), "{:?}", written.notes);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- limits

/// The parts' bytes are counted from the lengths they state before anything is made: a model whose parts would pass the memory a save may hold is refused in the splice's own words.
#[test]
fn parts_past_the_memory_a_save_may_hold_are_refused_before_anything_is_made() {
    let mut doc = blank(1);
    let big: Arc<[u8]> = Arc::from(vec![7u8; 4096]);
    for k in 0..3 {
        doc.parts.push(Arc::new(Part {
            format: Format::Odf,
            name: Arc::from(format!("Fonts/f{k}.ttf")),
            media_type: Arc::from("application/x-font-ttf"),
            role: PartRole::Font,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::clone(&big),
        }));
    }
    // 3 parts of 4 KiB are 12 KiB: four times that and the largest is 52 KiB, so a gate of 50,000 bytes refuses and one of 60,000 does not.
    let gate = |max: u64| save(&doc, &WriteOptions { limits: Limits { max_write_memory: max, ..Limits::default() }, ..WriteOptions::default() });
    let refused = gate(50_000).unwrap_err();
    assert_eq!(refused.class(), "write_limit");
    assert!(refused.to_string().contains("bytes of memory a save holds"), "{refused}");
    assert!(gate(2_900_000_000).is_ok());
}

/// The thumbnail passed is checked as for any save.
#[test]
fn a_thumbnail_that_is_no_png_is_refused() {
    let refused = save(&blank(1), &WriteOptions { thumbnail: Some(vec![1, 2, 3]), ..WriteOptions::default() }).unwrap_err();
    assert_eq!(refused.class(), "write_thumbnail");
}

/// A file this writer makes holds no unlisted entry and no row for an entry it lacks, and its `mimetype` is first, stored and bare.
#[test]
fn the_package_is_what_the_package_rules_ask_of_a_new_drawing() {
    let written = save(&blank(2), &WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() }).unwrap();
    let bytes = &written.bytes;
    let media = Variant::Drawing.media_type();
    assert_eq!(&bytes[..2], b"PK");
    assert_eq!(&bytes[26..30], &[8, 0, 0, 0], "a name of 8 bytes and no extra field");
    assert_eq!(&bytes[8..10], &[0, 0], "stored");
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..38 + media.len()], media.as_bytes());
    let package = Package::read(bytes, &Limits::default()).unwrap();
    assert!(package.mimetype_regular() && package.warnings().is_empty(), "{:?}", package.warnings());
    let listed: Vec<String> = package.manifest().entries.iter().map(|e| e.full_path.clone()).collect();
    for entry in package.entries().filter(|e| e.name != "mimetype" && !e.name.starts_with("META-INF/")) {
        assert!(listed.contains(&entry.name), "{} is listed", entry.name);
    }
    // A folder's row lists a folder whose entries the package has (`DiaCraft/`, whenever an entry of DiaCraft's rides).
    let present = |p: &String| package.entries().any(|e| e.name == *p || (p.ends_with('/') && e.name.starts_with(p.as_str())));
    assert!(listed.iter().all(|p| p == "/" || present(p)), "no row for a missing entry: {listed:?}");
    assert_eq!(&listed[..5], ["/", "content.xml", "styles.xml", "meta.xml", "settings.xml"]);
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        assert!(text(part, bytes).contains("office:version=\"1.4\""), "{part}");
    }
    assert!(names(bytes).iter().all(|n| !n.starts_with("Configurations2/")), "the package has no Configurations2/");
}

/// A model read from a file of ODF 1.0 holds roots that bind `fo` and `svg` to the pre-standard W3C namespaces and say version 1.0: the package is written as an ODF 1.4 one, the two
/// prefixes bound to the standard's, so that the names the writer writes are in them, and the file reads back as the drawing it was.
#[test]
fn a_root_of_an_odf_1_0_file_is_written_as_an_odf_1_4_part() {
    let mut doc = blank(2);
    doc.ext.make_mut().odf.roots.push((
        Arc::from("content.xml"),
        RawAttrs::from_pairs([
            ("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
            ("xmlns:fo", "http://www.w3.org/1999/XSL/Format"),
            ("xmlns:svg", "http://www.w3.org/2000/svg"),
            ("office:version", "1.0"),
        ]),
    ));
    let written = save(&doc, &WriteOptions::default()).expect("written");
    let content = text("content.xml", &written.bytes);
    assert!(content.contains("xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\""), "{content}");
    assert!(content.contains("xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\""), "{content}");
    assert!(content.contains("office:version=\"1.4\"") && !content.contains("office:version=\"1.0\""), "{content}");
    let (back, report) = read_back(&written.bytes);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.pages.get(0).unwrap().shapes.len(), 2);
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------- a black box

/// A black-box check against LibreOffice, run by hand (ignored): writes the files into `$ODG_OUT_DIR/in`; then
/// `soffice -env:UserInstallation=file://$ODG_OUT_DIR/profile --headless --convert-to pdf --outdir $ODG_OUT_DIR/out $ODG_OUT_DIR/in/<file>` for each of them. Only a missing output file
/// is a failure: LibreOffice offers a repair for a package it finds damaged, and headless it then loads nothing. The files are a drawing with shapes and a thumbnail, a template, a
/// drawing with every kind of package entry this writer writes, and a drawing from the model of a file read back.
#[test]
#[ignore = "a black-box check against LibreOffice, run by hand with ODG_OUT_DIR set to an empty folder"]
fn lo_model_write() {
    let dir = std::path::PathBuf::from(std::env::var("ODG_OUT_DIR").expect("set ODG_OUT_DIR to an empty folder")).join("in");
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).unwrap();
    let options = WriteOptions { thumbnail: Some(PNG.to_vec()), ..WriteOptions::default() };
    put("shapes.odg", &save(&blank(5), &options).unwrap().bytes);
    put("template.otg", &save(&blank(2), &WriteOptions { variant: Some(Variant::Template), ..options.clone() }).unwrap().bytes);
    put("empty.odg", &save(&blank(0), &WriteOptions::default()).unwrap().bytes);
    // The model's own data in four drawings, so that a refusal of LibreOffice names the part that caused it.
    put("full_fields.odg", &save(&full(), &options).unwrap().bytes);
    let mut no_size = full();
    no_size.ext.make_mut().odf.manifest_rows.pop();
    put("full_nosize.odg", &save(&no_size, &options).unwrap().bytes);
    let mut no_settings = full();
    no_settings.ext.make_mut().odf.settings = None;
    put("full_nosettings.odg", &save(&no_settings, &options).unwrap().bytes);
    let mut no_meta = full();
    no_meta.ext.make_mut().odf.meta = Passthrough::new();
    put("full_nometa.odg", &save(&no_meta, &options).unwrap().bytes);
    let basic = odf_part(
        "Basic/Standard/Module1.xml",
        "text/xml",
        PartRole::MacroProject,
        &[],
        b"<script:module xmlns:script=\"urn:oasis:names:tc:opendocument:xmlns:script:1.0\"/>",
    );
    let object = [
        odf_part("Object 1/", "application/vnd.oasis.opendocument.chart", PartRole::ObjectFile, &[("manifest:version", "1.3")], &[]),
        odf_part(
            "Object 1/content.xml",
            "text/xml",
            PartRole::ObjectFile,
            &[],
            b"<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\"/>",
        ),
    ];
    let mut with_basic = blank(1);
    with_basic.parts.push(Arc::clone(&basic));
    put("full_basic.odg", &save(&with_basic, &options).unwrap().bytes);
    let mut with_object = blank(1);
    for part in &object {
        with_object.parts.push(Arc::clone(part));
    }
    put("full_object.odg", &save(&with_object, &options).unwrap().bytes);
    let mut other = blank(1);
    other.parts.push(odf_part(
        "manifest.rdf",
        "application/rdf+xml",
        PartRole::Other,
        &[],
        b"<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/>",
    ));
    other.parts.push(odf_part("Configurations2/", "application/vnd.sun.xml.ui.configuration", PartRole::Other, &[], &[]));
    put("full_other.odg", &save(&other, &options).unwrap().bytes);
    let first = save(&blank(3), &options).unwrap();
    let (again, _) = read_back(&first.bytes);
    put("reopened.odg", &save(&again, &options).unwrap().bytes);
}
