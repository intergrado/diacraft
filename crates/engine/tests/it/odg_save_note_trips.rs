//! The save note's false claims and the silent losses found in the trips across the formats. Each test is a trip a person can make in the engine,
//! one leg a session (`file.open`, then `file.save_as`), and every file is built in code from the standards: the VSDX files by the testkit's builder, the `.odg` files by hand from the OASIS ODF
//! standard.

use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, opened};

/// One leg of a trip: `from` opened in a session of its own and saved as `to` in `dir`, with what the save said, or its refusal in words.
fn leg(dir: &Dir, from: &str, to: &str) -> Result<Value, String> {
    let mut s = opened(&dir.path(from));
    s.execute("file.save_as", &json!({ "path": dir.path(to) })).map_err(|e| e.to_string())
}

/// The legs of a trip, in order, from `first` (already in `dir`): each leg's result, or the first refusal.
fn trip(dir: &Dir, names: &[&str]) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for pair in names.windows(2) {
        out.push(leg(dir, pair[0], pair[1]).map_err(|e| format!("{} to {}: {e}", pair[0], pair[1]))?);
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------ item 6: the VSDX face table

/// The `FaceNames` element of a VSDX file's document part, as written (`None` when the part has none).
fn face_table(bytes: &[u8]) -> Option<String> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let text = String::from_utf8(package.part(concat!("/vi", "sio/document.xml")).unwrap().to_vec()).unwrap();
    let at = text.find("<FaceNames")?;
    let end = match text[at..].find("</FaceNames>") {
        Some(end) => at + end + "</FaceNames>".len(),
        None => at + text[at..].find("/>").unwrap() + 2,
    };
    Some(text[at..end].to_owned())
}

/// The faces of a VSDX file's table, each as its attributes sorted by name (the order of the attributes is the writer's to choose; the order of the faces is the table's).
fn faces(bytes: &[u8]) -> Vec<Vec<(String, String)>> {
    let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap();
    doc.fonts
        .iter()
        .map(|f| {
            let mut attrs: Vec<(String, String)> = f.raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
            attrs.push(("NameU".to_owned(), f.name_u.to_string()));
            attrs.sort();
            attrs
        })
        .collect()
}

const PAGE_SHEET: &str = r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#;

/// A rectangle whose text is in the face of `ID` 2, and one with no text format.
fn shapes() -> String {
    concat!(
        r#"<Shape ID="1" Type="Shape"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#,
        r#"<Section N="Character"><Row IX="0"><Cell N="Font" V="2"/></Row></Section><Text>hi</Text></Shape>"#,
        r#"<Shape ID="2" Type="Shape"><Cell N="PinX" V="5"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/></Shape>"#
    )
    .to_owned()
}

/// The VSDX drawing of `faces`, the table, saved as an `.odg` and back as a VSDX file: the source and what came back.
fn faces_trip(table: &str) -> (Vec<u8>, Vec<u8>) {
    let source = VsdxBuilder::new().face_names(table).page(PageSpec::new(1, "Page-1").sheet(PAGE_SHEET).shapes(&shapes())).build();
    let dir = Dir::new();
    dir.write("a.vsdx", &source);
    trip(&dir, &["a.vsdx", "b.odg", "c.vsdx"]).unwrap_or_else(|e| panic!("{e}"));
    (source, dir.read("c.vsdx"))
}

/// **Every face the source had comes back, in its order** (the VSDX written after an `.odg` trip dropped a face from `FaceNames` in 10 of 25 files). The `.odg` carries the table in
/// DiaCraft's document file, and the reader put it back over the faces the `.odg` declares **by name**: two faces of one name were one (the second took the first's place, and the face of `ID` 1 was
/// gone), and the faces no run names came after the ones a run names, so the table came back in another order. Red on main: the face of `ID` 1 is lost, and the order is not the source's.
#[test]
fn every_face_of_the_table_comes_back_in_its_order_after_an_odg_trip() {
    let cases = [
        (
            "two faces of one name",
            r#"<FaceName ID="1" NameU="Liberation Sans" Name="Liberation Sans"/><FaceName ID="2" NameU="Liberation Sans" Name="Liberation Sans" Flags="325"/>"#,
        ),
        (
            "a face no run names before the one a run names",
            r#"<FaceName ID="1" NameU="Liberation Sans" Name="Liberation Sans"/><FaceName ID="2" NameU="DejaVu Serif" Name="DejaVu Serif"/>"#,
        ),
        (
            "three faces, the one a run names last",
            r#"<FaceName ID="3" NameU="Noto Sans"/><FaceName ID="1" NameU="Liberation Sans" CharSets="1073742335 -65536"/><FaceName ID="2" NameU="DejaVu Serif" Panos="2 11 6 4 2 2 2 2 2 4"/>"#,
        ),
    ];
    for (what, table) in cases {
        let (source, back) = faces_trip(table);
        assert_eq!(faces(&back), faces(&source), "{what}: the faces, their attributes and their order");
        let diff = diacraft_vsdx::compare_content(&source, &back, &diacraft_vsdx::Limits::default()).unwrap();
        let in_table: Vec<String> = diff.shown.iter().filter(|d| d.path.contains("FaceName")).map(|d| format!("{} {:?}", d.path, d.found)).collect();
        assert_eq!(in_table, Vec::<String>::new(), "{what}: the content check finds no difference in the table");
    }
}

/// **No save writes an empty `FaceNames`**, which the schema refuses (MS-VSDX wants one `FaceName` at least; the F1b, still open: 10 files gained one): a VSDX file whose
/// table is empty comes back with none, and an `.odg` that names no face, saved as a VSDX type, gets a table with a face. The content check counts an empty table as none. Red on main:
/// the empty table comes back as `<FaceNames/>`.
#[test]
fn no_save_writes_an_empty_face_table() {
    let (source, back) = faces_trip("");
    assert_eq!(face_table(&source).as_deref(), Some("<FaceNames></FaceNames>"), "the builder's source has an empty table");
    assert_eq!(face_table(&back), None, "an empty table is not written back");
    // An `.odg` with no text, so no face, saved as a VSDX type from the model: a new VSDX drawing, whose table has the default face (`write/skeleton.rs`), and never an empty one.
    let odg = crate::odg_build::Drawing::new().page(&crate::odg_build::rect("R", (2.0, 3.0, 4.0, 2.0), "")).bytes();
    let dir = Dir::new();
    dir.write("a.odg", &odg);
    trip(&dir, &["a.odg", "b.vsdx"]).unwrap_or_else(|e| panic!("{e}"));
    let table = face_table(&dir.read("b.vsdx"));
    assert!(table.as_deref().is_none_or(|t| t.contains("<FaceName ")), "a table holds a face: {table:?}");
    // The control: a drawing with a face gets its table.
    let (_, back) = faces_trip(r#"<FaceName ID="2" NameU="DejaVu Serif"/>"#);
    assert!(face_table(&back).is_some_and(|t| t.contains("DejaVu Serif")), "{:?}", face_table(&back));
}

// ------------------------------------------------------------------------------------------------------------------ items 2 and 3: the crossing refuses what the visit refuses; objects

/// The namespace of tables, beside the ones `odg_build::NS` declares.
const TABLE_NS: &str = r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#;

const LAYERS: &str = r#"<draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/>"#;

const LINKS: &str = r#"xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad""#;

/// A PNG of 2 by 2 pixels.
fn png() -> Vec<u8> {
    diacraft_render::export::encode_png(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255], None).unwrap()
}

/// Bytes that start as LibreOffice's own metafile does (`VCLMTF`): a picture in a format the `.odg` reader does not draw, as the replacement of a table or an object LibreOffice writes is.
const METAFILE: &[u8] = b"VCLMTF\x01\x00 not drawn by DiaCraft";

/// An `.odg` written by hand from the standard: the common styles, the automatic styles and the shapes of one page, and more entries, each with the media type of its manifest row (a name that
/// ends in `/` is a folder's row, with no entry).
struct Odg {
    common: String,
    auto: String,
    shapes: String,
    files: Vec<(String, String, Vec<u8>)>,
}

impl Odg {
    fn new() -> Odg {
        Odg { common: String::new(), auto: crate::odg_build::GR1.to_owned(), shapes: String::new(), files: Vec::new() }
    }

    fn bytes(&self) -> Vec<u8> {
        use diacraft_container::zip::{ArchiveWriter, Method};
        let ns = crate::odg_build::NS;
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {ns}{TABLE_NS} office:version=\"1.3\"><office:font-face-decls/><office:automatic-styles>{}</office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">{}</draw:page></office:drawing></office:body></office:document-content>",
            self.auto, self.shapes
        );
        let common = if self.common.is_empty() { "<office:styles/>".to_owned() } else { format!("<office:styles>{}</office:styles>", self.common) };
        let styles = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {ns}{TABLE_NS} office:version=\"1.3\"><office:font-face-decls/>{common}<office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:margin-top=\"1cm\" fo:margin-bottom=\"1cm\" fo:margin-left=\"1cm\" fo:margin-right=\"1cm\" fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set>{LAYERS}</draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
        );
        let mut files = vec![
            ("content.xml".to_owned(), "text/xml".to_owned(), content.into_bytes()),
            ("styles.xml".to_owned(), "text/xml".to_owned(), styles.into_bytes()),
        ];
        files.extend(self.files.iter().cloned());
        let media = crate::odg_build::DRAWING;
        let mut manifest = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\n",
        );
        manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{media}\"/>\n"));
        for (name, media_type, _) in &files {
            manifest.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media_type}\"/>\n"));
        }
        manifest.push_str("</manifest:manifest>\n");
        let mut w = ArchiveWriter::new();
        w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
        for (name, _, data) in &files {
            if !name.ends_with('/') {
                w.add(name, data, Method::Deflated).unwrap();
            }
        }
        w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).unwrap();
        w.finish().unwrap()
    }
}

/// A rectangle with a paragraph, beside what each drawing holds.
const RECT: &str = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hello</text:p></draw:rect>"#;

/// A drawing of a picture frame whose picture is `picture` (bytes, with its name and type).
fn picture_drawing(name: &str, media: &str, picture: &[u8]) -> Vec<u8> {
    let mut d = Odg::new();
    d.shapes = format!(
        r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="P" svg:width="4cm" svg:height="3cm" svg:x="2cm" svg:y="10cm"><draw:image xlink:href="{name}" {LINKS}><text:p/></draw:image></draw:frame>{RECT}"#
    );
    d.files.push((name.to_owned(), media.to_owned(), picture.to_vec()));
    d.bytes()
}

/// A drawing of a table of two cells (a frame holding `table:table`, then its replacement picture `replacement`, as LibreOffice writes a table).
fn table_drawing(replacement: &[u8]) -> Vec<u8> {
    let mut d = Odg::new();
    d.shapes = format!(
        concat!(
            r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="T" svg:width="6cm" svg:height="2cm" svg:x="2cm" svg:y="14cm"><table:table><table:table-column/><table:table-column/>"#,
            r#"<table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell><text:p>b</text:p></table:table-cell></table:table-row></table:table>"#,
            r#"<draw:image xlink:href="Pictures/t.bin" {links}/></draw:frame>{rect}"#
        ),
        links = LINKS,
        rect = RECT
    );
    d.files.push(("Pictures/t.bin".to_owned(), "image/x-unknown".to_owned(), replacement.to_vec()));
    d.bytes()
}

/// The files of an embedded formula (`draw:object`): a folder of its own files.
fn object_files() -> Vec<(String, String, Vec<u8>)> {
    vec![
        ("Object 1/".to_owned(), "application/vnd.oasis.opendocument.formula".to_owned(), Vec::new()),
        (
            "Object 1/content.xml".to_owned(),
            "text/xml".to_owned(),
            br#"<?xml version="1.0" encoding="UTF-8"?><math xmlns="http://www.w3.org/1998/Math/MathML"><mi>x</mi></math>"#.to_vec(),
        ),
        (
            "Object 1/settings.xml".to_owned(),
            "text/xml".to_owned(),
            br#"<?xml version="1.0" encoding="UTF-8"?><office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#
                .to_vec(),
        ),
    ]
}

/// A drawing of an embedded formula (a frame holding `draw:object`, then its replacement picture `replacement` under `ObjectReplacements/`).
fn object_drawing(replacement: &[u8]) -> Vec<u8> {
    let mut d = Odg::new();
    d.shapes = format!(
        r#"<draw:frame draw:style-name="gr1" draw:layer="layout" draw:name="O" svg:width="4cm" svg:height="3cm" svg:x="10cm" svg:y="10cm"><draw:object xlink:href="./Object 1" {LINKS}/><draw:image xlink:href="./ObjectReplacements/Object 1" {LINKS}/></draw:frame>{RECT}"#
    );
    d.files.extend(object_files());
    d.files.push(("ObjectReplacements/Object 1".to_owned(), "image/x-unknown".to_owned(), replacement.to_vec()));
    d.bytes()
}

/// **The crossing refuses what the plain visit refuses**, by the `.odg` writer's own rule (`diacraft_odg::model_refusal`), which both paths call. When a drawing holds a
/// picture in a format the reader does not draw, the plain visit (`.odg` to `.diacraft` to `.odg`) refuses in words, and the crossing (`.odg` to `.diacraft` to VSDX to `.diacraft` to `.odg`)
/// finished in all 65 such trips, with fewer frames, pictures, tables and objects in the `.odg` written, and paragraphs lost. Here the save across to VSDX is refused in the visit's words, whether
/// the drawing stands on its `.odg` or on a `.diacraft` file made of it, and nothing is written. Red on main: both crossings finish.
#[test]
fn the_crossing_refuses_what_the_plain_visit_refuses() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"/>"#;
    let cases: [(&str, Vec<u8>); 3] = [
        ("a picture of another format", picture_drawing("Pictures/a.svg", "image/svg+xml", svg)),
        ("a table whose replacement is a metafile of LibreOffice's", table_drawing(METAFILE)),
        ("an embedded object whose replacement is a metafile of LibreOffice's", object_drawing(METAFILE)),
    ];
    for (what, source) in cases {
        let dir = Dir::new();
        dir.write("a.odg", &source);
        let visit = trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).expect_err(what);
        assert!(visit.contains("a new picture in a format the reader does not draw"), "{what}: the visit is refused in words: {visit}");
        assert!(!dir.files().contains(&"c.odg".to_owned()), "{what}");
        for (from, name) in [("b.diacraft", "d.vsdx"), ("a.odg", "e.vsdx")] {
            let crossing = leg(&dir, from, name).expect_err(what);
            assert!(
                crossing.contains("a new picture in a format the reader does not draw"),
                "{what}, from {from}: the crossing is refused in the visit's words: {crossing}"
            );
            assert!(
                crossing.contains("does not write what a save of it as an .odg does not") && crossing.contains("Nothing was written"),
                "{what}: {crossing}"
            );
            assert!(!dir.files().contains(&name.to_owned()), "{what}: nothing is written");
        }
    }
}

/// The names and bytes of the entries of an `.odg` under `prefix`.
fn entries_under(bytes: &[u8], prefix: &str) -> Vec<(String, Vec<u8>)> {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut out: Vec<(String, Vec<u8>)> =
        package.parts().filter(|p| p.starts_with(prefix)).map(|p| (p.to_owned(), package.part(p).unwrap().to_vec())).collect();
    out.sort();
    out
}

/// **The controls of the rule, and item 3: objects ride as object entries.** The same drawings with pictures the reader draws (PNG) are not refused: the crossing finishes, and the `.odg` written
/// at its end holds the table (its `table:table` and the paragraphs of its cells) and the embedded object as the source had them, **its own entries as entries of the package** with their bytes
/// (`Object 1/...`, the frame's `draw:object` naming them) and not in DiaCraft's folder, and the VSDX leg says the object rides (`objects: true`). Found 155 object entries of 27
/// files that came back only as bytes in `DiaCraft/` entries: the objects whose replacement LibreOffice writes in its own metafile, which the crossing now refuses (the test above).
#[test]
fn pictures_tables_and_objects_the_reader_draws_cross_and_come_back() {
    for (what, source) in [
        ("a picture", picture_drawing("Pictures/a.png", "image/png", &png())),
        ("a table", table_drawing(&png())),
        ("an embedded object", object_drawing(&png())),
    ] {
        let dir = Dir::new();
        dir.write("a.odg", &source);
        let said = trip(&dir, &["a.odg", "b.diacraft", "c.vsdx", "d.diacraft", "e.odg"]).unwrap_or_else(|e| panic!("{what}: {e}"));
        let back = dir.read("e.odg");
        let content = crate::odg_build::content_of(&back);
        assert!(content.contains("<text:p>hello</text:p>"), "{what}: the rectangle's paragraph");
        match what {
            "a table" => {
                assert!(
                    content.contains("<table:table>") && content.contains("<text:p>a</text:p>") && content.contains("<text:p>b</text:p>"),
                    "{what}: {content}"
                );
            }
            "an embedded object" => {
                assert!(content.contains(r#"<draw:object xlink:href="./Object 1""#), "{what}: the frame names its object: {content}");
                let files: Vec<(String, Vec<u8>)> =
                    object_files().into_iter().filter(|(n, _, _)| !n.ends_with('/')).map(|(n, _, b)| (n, b)).collect();
                assert_eq!(entries_under(&back, "Object 1/"), files, "{what}: the object's own entries, byte for byte");
                let carried = entries_under(&back, "DiaCraft/");
                assert!(carried.iter().all(|(_, b)| !files.iter().any(|(_, f)| f == b)), "{what}: no file of the object rides in DiaCraft's folder");
                assert_eq!(said[1]["carried"]["objects"], json!(true), "{what}: the VSDX leg says the object rides: {}", said[1]["carried"]);
            }
            _ => assert!(content.contains("<draw:image "), "{what}: {content}"),
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ item 5: a bold edited in LibreOffice is read back

/// A rectangle of `ID` `id` with the text `text` and the `Character` row `row` (none when empty).
fn text_rect(id: u32, x: f64, text: &str, row: &str) -> String {
    let rows = if row.is_empty() { String::new() } else { format!(r#"<Section N="Character"><Row IX="0">{row}</Row></Section>"#) };
    format!(
        r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>{rows}<Text>{text}</Text></Shape>"#
    )
}

/// A VSDX drawing of three texts: **1** bold in the face of `ID` 2, **2** red and not bold, **3** with no row.
fn three_texts() -> Vec<u8> {
    let shapes = [
        text_rect(1, 2.0, "bold one", r#"<Cell N="Font" V="2"/><Cell N="Style" V="1"/>"#),
        text_rect(2, 5.0, "red one", r##"<Cell N="Color" V="#FF0000"/>"##),
        text_rect(3, 8.0, "plain one", ""),
    ]
    .concat();
    VsdxBuilder::new()
        .face_names(r#"<FaceName ID="1" NameU="Liberation Sans"/><FaceName ID="2" NameU="DejaVu Serif"/>"#)
        .page(PageSpec::new(1, "Page-1").sheet(PAGE_SHEET).shapes(&shapes))
        .build()
}

/// The package `bytes` with its `content.xml` made by `edit` (every other entry as it is, in its place, and the manifest as it was): what a program that edits the drawing and keeps the rest writes.
fn with_content(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    use diacraft_container::zip::{ArchiveWriter, Method};
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut w = ArchiveWriter::new();
    for name in package.parts() {
        let data = package.part(name).unwrap();
        if name == "mimetype" {
            w.add(name, data, Method::Stored).unwrap();
        } else if name == "content.xml" {
            let text = edit(std::str::from_utf8(data).unwrap());
            w.add(name, text.as_bytes(), Method::Deflated).unwrap();
        } else {
            w.add(name, data, Method::Deflated).unwrap();
        }
    }
    w.finish().unwrap()
}

/// Whether the text of the shape of `ID` `id` on the first page is bold in the model the `.odg` reader makes of `bytes` (bit 1 of `Style` in its first `Character` row), and the rows of its
/// `Character` section as text (what tells one shape's formatting from another's).
fn bold_of(bytes: &[u8], id: u32) -> (bool, String) {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let shape = doc.pages.iter().next().unwrap().shapes.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}")).clone();
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").cloned();
    let style = section
        .as_ref()
        .and_then(|s| s.rows.iter().next().cloned())
        .and_then(|row| row.cells.get("Style").and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
        .unwrap_or(0.0);
    let rows = section.map(|s| {
        format!(
            "{:?}",
            s.rows
                .iter()
                .map(|r| r.cells.iter().map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()))).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        )
    });
    ((style as u32) & 1 == 1, rows.unwrap_or_default())
}

/// The name of the automatic text style of the span whose text is `text`, in `content.xml`.
fn span_style(content: &str, text: &str) -> String {
    let end = content.find(&format!(">{text}</text:span>")).unwrap_or_else(|| panic!("no span of {text}: {content}"));
    let open = content[..end].rfind("<text:span text:style-name=\"").unwrap();
    let name = &content[open + "<text:span text:style-name=\"".len()..end];
    name.trim_end_matches('"').to_owned()
}

/// `content` with the text properties of the automatic style `style` changed by `edit`.
fn restyled(content: &str, style: &str, edit: impl Fn(&str) -> String) -> String {
    let head = format!(r#"<style:style style:name="{style}" style:family="text">"#);
    let at = content.find(&head).unwrap_or_else(|| panic!("no style {style}: {content}"));
    let end = at + content[at..].find("</style:style>").unwrap();
    format!("{}{}{}", &content[..at], edit(&content[at..end]), &content[end..])
}

/// **A bold edited in LibreOffice is read back, in the shape it was edited in and no other** (a carried text's stamp covers its formatting). A VSDX drawing saved as an `.odg` writes each
/// text in its characters, with its runs, and its `Character` rows ride in DiaCraft's data on the shape's style, since the markup cannot say them whole (the face's number, a row the file did not
/// read back). They rode with no stamp, so they were put over whatever the markup said: found a bold turned on in LibreOffice unseen in 8 of 18 files of the VSDX direction, and turned
/// off in 4 more. The rows are now stamped with the formatting the markup written says (`fp-format`), and a change of it in another program makes them stale. Red on main: the bold turned off
/// stays on, and the bold turned on stays off.
#[test]
fn a_bold_turned_on_or_off_in_another_program_is_read_back_in_that_shape_alone() {
    let dir = Dir::new();
    dir.write("a.vsdx", &three_texts());
    trip(&dir, &["a.vsdx", "b.odg"]).unwrap_or_else(|e| panic!("{e}"));
    let written = dir.read("b.odg");
    let content = crate::odg_build::content_of(&written);
    assert!(content.contains("fp-format"), "the rows that ride are stamped with the formatting the markup says: {content}");
    let before: Vec<(bool, String)> = [1, 2, 3].map(|id| bold_of(&written, id)).to_vec();
    assert_eq!(before.iter().map(|b| b.0).collect::<Vec<_>>(), [true, false, false], "as the VSDX file has them");
    // An edit as another program makes it: the bold run of shape 1 is written as not bold.
    let bold = span_style(&content, "bold one");
    let off = with_content(&written, |c| restyled(c, &bold, |s| s.replace(r#"fo:font-weight="bold""#, r#"fo:font-weight="normal""#)));
    assert!(!bold_of(&off, 1).0, "the bold turned off is read: {}", crate::odg_build::content_of(&off));
    assert_eq!([2, 3].map(|id| bold_of(&off, id)).to_vec(), before[1..].to_vec(), "no other shape changes");
    // And the red run of shape 2 written bold.
    let red = span_style(&content, "red one");
    let on = with_content(&written, |c| {
        restyled(c, &red, |s| s.replacen("<style:text-properties ", r#"<style:text-properties fo:font-weight="bold" "#, 1))
    });
    assert!(bold_of(&on, 2).0, "the bold turned on is read: {}", crate::odg_build::content_of(&on));
    assert_eq!([1, 3].map(|id| bold_of(&on, id)).to_vec(), vec![before[0].clone(), before[2].clone()], "no other shape changes");
    // The control: with no edit, the file reads as it was written, the face's number and all.
    let none = with_content(&written, str::to_owned);
    assert_eq!([1, 2, 3].map(|id| bold_of(&none, id)).to_vec(), before);
}

/// The files of the LibreOffice check (run by hand, never in a gate): `SAFEF2_LO_DIR` names a folder of the run, which gets `three.odg` (the VSDX drawing of three texts saved as an `.odg`).
/// LibreOffice converts a copy to `.odg` (a save with no edit), and [`lo_read`] compares what DiaCraft reads of the two.
#[test]
#[ignore]
fn lo_files() {
    let Ok(out) = std::env::var("SAFEF2_LO_DIR") else { return };
    let dir = Dir::new();
    dir.write("a.vsdx", &three_texts());
    trip(&dir, &["a.vsdx", "three.odg"]).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(std::path::Path::new(&out).join("three.odg"), dir.read("three.odg")).unwrap();
}

/// The check of [`lo_files`]: `SAFEF2_LO_DIR` holds `three.odg` and `SAFEF2_LO_OUT` LibreOffice's save of it. **A save with no edit changes no shape's text formatting in DiaCraft's model**:
/// every shape's bold and `Character` rows are read the same from both.
#[test]
#[ignore]
fn lo_read() {
    let (Ok(input), Ok(output)) = (std::env::var("SAFEF2_LO_DIR"), std::env::var("SAFEF2_LO_OUT")) else { return };
    let ours = std::fs::read(std::path::Path::new(&input).join("three.odg")).unwrap();
    let theirs = std::fs::read(std::path::Path::new(&output).join("three.odg")).unwrap();
    for id in [1, 2, 3] {
        assert_eq!(bold_of(&theirs, id), bold_of(&ours, id), "shape {id}");
    }
}

/// **The manifest lists DiaCraft's folder whenever an entry of DiaCraft's is written**, the document file alone included (the measure: a LibreOffice save keeps DiaCraft's entries because the
/// manifest lists the folder). A VSDX drawing whose data an `.odg` carries is in the document file and no part, and its folder had no row: LibreOffice 26.8.2.2 dropped `DiaCraft/document.xml` on a save
/// with no edit (measured, `lo_files`), and with it the items of the document, its pages and its faces. Red on main: no row for the folder.
#[test]
fn the_manifest_lists_diacraft_s_folder_when_only_the_document_file_rides() {
    let dir = Dir::new();
    dir.write("a.vsdx", &three_texts());
    trip(&dir, &["a.vsdx", "b.odg"]).unwrap_or_else(|e| panic!("{e}"));
    let written = dir.read("b.odg");
    let package = diacraft_odg::Package::read(&written, &diacraft_odg::Limits::default()).unwrap();
    assert!(package.part("DiaCraft/document.xml").is_some(), "the document file rides");
    assert!(package.parts().all(|p| !p.starts_with("DiaCraft/blob")), "and no part does: the case of the measure");
    let folder = package
        .manifest()
        .entry("DiaCraft/")
        .unwrap_or_else(|| panic!("a row for the folder: {}", crate::odg_build::part_of(&written, "META-INF/manifest.xml")));
    assert_eq!(folder.media_type.as_deref(), Some("application/vnd.diacraft.parts"));
    // A second save of the file, with no edit, writes the same manifest (no row twice).
    let mut s = opened(&dir.path("b.odg"));
    s.execute("file.save_as", &json!({ "path": dir.path("c.odg") })).unwrap();
    let again = crate::odg_build::part_of(&dir.read("c.odg"), "META-INF/manifest.xml");
    assert_eq!(again.matches("manifest:full-path=\"DiaCraft/\"").count(), 1, "{again}");
}

// ------------------------------------------------------------------------------------------------------------------ item 1: what `carried` names comes back

/// LibreOffice's default graphic styles that hold line ends and gradients, in the forms its saves write them (a black box: LibreOffice 26.8.2.2's save of a drawing of ours holds them so): a
/// `standard` that says the widths of the line ends and no marker, a `Filled Blue` that names its gradient over its parent's `draw:fill="gradient"`, an `Arrow Line` that names its markers.
const LO_COMMON: &str = concat!(
    r##"<draw:gradient draw:name="Filled" draw:style="linear" draw:start-color="#ffffff" draw:end-color="#cccccc" draw:start-intensity="100%" draw:end-intensity="100%" draw:angle="30deg" draw:border="0%"/>"##,
    r##"<draw:gradient draw:name="Filled_20_Blue" draw:display-name="Filled Blue" draw:style="linear" draw:start-color="#729fcf" draw:end-color="#355269" draw:start-intensity="100%" draw:end-intensity="100%" draw:angle="30deg" draw:border="0%"/>"##,
    r##"<draw:gradient draw:name="Filled_20_Red" draw:display-name="Filled Red" draw:style="linear" draw:start-color="#ff6d6d" draw:end-color="#c9211e" draw:start-intensity="100%" draw:end-intensity="100%" draw:angle="30deg" draw:border="0%"/>"##,
    r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"#,
    r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-width="0cm" svg:stroke-color="#3465a4" draw:marker-start-width="0.2cm" draw:marker-start-center="false" draw:marker-end-width="0.2cm" draw:marker-end-center="false" draw:fill="solid" draw:fill-color="#729fcf"/></style:style>"##,
    r##"<style:style style:name="Graphic" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffffff"/></style:style>"##,
    r#"<style:style style:name="Filled" style:family="graphic" style:parent-style-name="Graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="Filled"/></style:style>"#,
    r#"<style:style style:name="Filled_20_Blue" style:display-name="Filled Blue" style:family="graphic" style:parent-style-name="Filled"><style:graphic-properties draw:fill-gradient-name="Filled_20_Blue"/></style:style>"#,
    r##"<style:style style:name="Lines" style:family="graphic" style:parent-style-name="Graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-color="#000000" draw:fill="none"/></style:style>"##,
    r#"<style:style style:name="Arrow_20_Line" style:display-name="Arrow Line" style:family="graphic" style:parent-style-name="Lines"><style:graphic-properties draw:marker-start="Arrow" draw:marker-start-width="0.2cm" draw:marker-end="Arrow" draw:marker-end-width="0.2cm"/></style:style>"#,
    r#"<style:style style:name="End_20_Only" style:display-name="End Only" style:family="graphic"><style:graphic-properties draw:marker-end="Arrow"/></style:style>"#
);

/// The automatic styles of the shapes: line ends and gradients said in each form a style holds them.
const LO_AUTO: &str = concat!(
    r#"<style:style style:name="gr5" style:family="graphic" style:parent-style-name="Arrow_20_Line"><style:graphic-properties draw:marker-end-width="0.5cm"/></style:style>"#,
    r#"<style:style style:name="gr6" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:marker-start="Arrow"/></style:style>"#,
    r#"<style:style style:name="gr7" style:family="graphic" style:parent-style-name="Arrow_20_Line"/>"#,
    r#"<style:style style:name="gr8" style:family="graphic" style:parent-style-name="Filled_20_Blue"/>"#,
    r#"<style:style style:name="gr9" style:family="graphic" style:parent-style-name="Filled"><style:graphic-properties draw:fill-gradient-name="Filled_20_Red"/></style:style>"#,
    r##"<style:style style:name="gr10" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:fill-gradient-name="Filled_20_Red"/></style:style>"##,
    r#"<style:style style:name="gr11" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="Filled_20_Red" draw:marker-end="Arrow" draw:marker-end-width="0.4cm" draw:marker-end-center="true"/></style:style>"#,
    // A marker with no width anywhere in its style's chain (the case): the default width applies, and the marker is still the style's.
    r#"<style:style style:name="gr12" style:family="graphic"><style:graphic-properties draw:marker-start="Arrow"/></style:style>"#,
    r#"<style:style style:name="gr13" style:family="graphic"><style:graphic-properties draw:marker-end="Arrow"/></style:style>"#,
    r#"<style:style style:name="gr14" style:family="graphic" style:parent-style-name="End_20_Only"/>"#
);

/// The drawing: three lines and four rectangles on the styles above.
fn line_ends_and_gradients() -> Vec<u8> {
    let mut d = Odg::new();
    d.common.push_str(LO_COMMON);
    d.auto.push_str(LO_AUTO);
    for (k, style) in ["gr5", "gr6", "gr7", "gr12", "gr13", "gr14"].iter().enumerate() {
        let y = 2 + 2 * k;
        d.shapes.push_str(&format!(
            r#"<draw:line draw:style-name="{style}" draw:layer="layout" draw:name="L{k}" svg:x1="2cm" svg:y1="{y}cm" svg:x2="8cm" svg:y2="{y}cm"><text:p/></draw:line>"#
        ));
    }
    for (k, style) in ["gr8", "gr9", "gr10", "gr11"].iter().enumerate() {
        let y = 14 + 3 * k;
        d.shapes.push_str(&format!(
            r#"<draw:rect draw:style-name="{style}" draw:layer="layout" draw:name="R{k}" svg:x="2cm" svg:y="{y}cm" svg:width="4cm" svg:height="2cm"/>"#
        ));
    }
    d.bytes()
}

/// What the style `name` of `part` says of its line ends and its fill's kind and names (`draw:marker-*`, `draw:fill`, `draw:fill-gradient-name`), sorted: the properties counted.
fn ends_and_fill(part: &str, name: &str) -> Vec<String> {
    let head = format!(r#"style:name="{name}""#);
    let Some(at) = part.find(&head) else { return vec![format!("no style {name}")] };
    let rest = &part[at..];
    let end = rest.find("</style:style>").unwrap_or(rest.len());
    let end = match rest.find("/>") {
        Some(close) if !rest[..close].contains('>') => close,
        _ => end,
    };
    let mut out: Vec<String> = rest[..end]
        .split([' ', '/', '>'])
        .filter(|token| token.starts_with("draw:marker-") || token.starts_with("draw:fill=") || token.starts_with("draw:fill-gradient-name="))
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

/// **What `carried` names comes back**: "line ends" and "gradients" are named by the VSDX leg of every crossing of a drawing that has them (the definitions ride in the VSDX file), and
/// found every one of the 207 trips that lost a line-end property, and of the 272 that lost a gradient reference, named them. **The `.odg` model writer lost them, and the plain visit too**:
/// a style that names its gradient and says no `draw:fill` of its own (LibreOffice's `Filled Blue`) was written `draw:fill="solid"` with the name gone, a name beside `draw:fill="solid"` went, and a
/// marker named alone was given a width the cells work out. Now each style says what the source said, on the visit and at the end of the crossing, and the crossing still names both classes. Red on
/// main: the gradient names and a width.
#[test]
fn line_ends_and_gradient_references_come_back_from_the_visit_and_the_crossing() {
    let source = line_ends_and_gradients();
    let dir = Dir::new();
    dir.write("a.odg", &source);
    trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap_or_else(|e| panic!("{e}"));
    let said = trip(&dir, &["b.diacraft", "d.vsdx", "e.diacraft", "f.odg"]).unwrap_or_else(|e| panic!("{e}"));
    let classes = said[0]["carried"]["classes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert!(classes.contains(&"line ends".to_owned()) && classes.contains(&"gradients".to_owned()), "the VSDX leg names both: {classes:?}");
    let parts = |bytes: &[u8]| (crate::odg_build::part_of(bytes, "content.xml"), crate::odg_build::part_of(bytes, "styles.xml"));
    let (content, styles) = parts(&source);
    for out in ["c.odg", "f.odg"] {
        let (back_content, back_styles) = parts(&dir.read(out));
        for name in ["gr5", "gr6", "gr7", "gr8", "gr9", "gr10", "gr11", "gr12", "gr13", "gr14"] {
            assert_eq!(ends_and_fill(&back_content, name), ends_and_fill(&content, name), "{out}: the automatic style {name}");
        }
        for name in ["standard", "Filled", "Filled_20_Blue", "Arrow_20_Line", "End_20_Only"] {
            assert_eq!(ends_and_fill(&back_styles, name), ends_and_fill(&styles, name), "{out}: the common style {name}");
        }
        for definition in
            [r#"<draw:marker draw:name="Arrow""#, r#"<draw:gradient draw:name="Filled_20_Blue""#, r#"<draw:gradient draw:name="Filled_20_Red""#]
        {
            assert!(back_styles.contains(definition), "{out}: {definition}");
        }
    }
}

/// A drawing of one rectangle on the automatic style `gr3`, whose graphic properties are `props`, beside the common styles `common` and the files `files`.
fn one_styled(common: &str, props: &str, files: Vec<(String, String, Vec<u8>)>) -> Odg {
    let mut d = Odg::new();
    d.common.push_str(common);
    d.auto.push_str(&format!(r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties {props}/></style:style>"#));
    d.shapes.push_str(r#"<draw:rect draw:style-name="gr3" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hi</text:p></draw:rect>"#);
    d.files = files;
    d
}

/// **Each class the crossing names comes back** (`carried` names only what reading back restores). For each word the VSDX leg of a crossing says for an `.odg`'s data, a drawing that
/// holds that class goes `.odg` to `.diacraft` to VSDX to `.diacraft` to `.odg`; the VSDX leg names the class, and the `.odg` at the end holds the markup of the class as the source did.
#[test]
fn each_class_the_crossing_names_comes_back() {
    let hatch = r##"<draw:hatch draw:name="Lines_20_Red" draw:display-name="Lines Red" draw:style="single" draw:color="#ff0000" draw:distance="0.2cm" draw:rotation="450"/>"##;
    let dash = r#"<draw:stroke-dash draw:name="Fine_20_Dashed" draw:display-name="Fine Dashed" draw:style="rect" draw:dots1="1" draw:dots1-length="0.508cm" draw:dots2="1" draw:dots2-length="0.508cm" draw:distance="0.508cm"/>"#;
    let opacity = r#"<draw:opacity draw:name="Fade" draw:style="linear" draw:start="100%" draw:end="0%" draw:angle="0deg" draw:border="0%"/>"#;
    let image = r#"<draw:fill-image draw:name="Img1" xlink:href="Pictures/img1.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#;
    let png_file = || vec![("Pictures/img1.png".to_owned(), "image/png".to_owned(), png())];
    let cases: Vec<(&str, Odg, Vec<&str>)> = vec![
        (
            "hatches",
            one_styled(hatch, r#"draw:fill="hatch" draw:fill-hatch-name="Lines_20_Red""#, Vec::new()),
            vec![hatch, r#"draw:fill="hatch""#, r#"draw:fill-hatch-name="Lines_20_Red""#],
        ),
        (
            "dash patterns",
            one_styled(dash, r#"draw:stroke="dash" draw:stroke-dash="Fine_20_Dashed""#, Vec::new()),
            vec![dash, r#"draw:stroke="dash""#, r#"draw:stroke-dash="Fine_20_Dashed""#],
        ),
        (
            "gradients",
            one_styled(opacity, r##"draw:fill="solid" draw:fill-color="#00ff00" draw:opacity-name="Fade""##, Vec::new()),
            vec![opacity, r#"draw:opacity-name="Fade""#],
        ),
        (
            "fill images",
            one_styled(image, r#"draw:fill="bitmap" draw:fill-image-name="Img1""#, png_file()),
            vec![image, r#"draw:fill="bitmap""#, r#"draw:fill-image-name="Img1""#],
        ),
        (
            "style names",
            one_styled("", r##"draw:fill="solid" draw:fill-color="#00ff00""##, Vec::new()),
            vec![r#"<style:style style:name="gr3" style:family="graphic""#, r#"draw:style-name="gr3""#],
        ),
        (
            "unknown parts",
            one_styled("", r#"draw:fill="none""#, vec![("foo/bar.dat".to_owned(), "application/x-foo".to_owned(), b"0123456789".to_vec())]),
            vec![],
        ),
        ("macro project", one_styled("", r#"draw:fill="none""#, macro_files()), vec![]),
        (
            "document properties",
            one_styled("", r#"draw:fill="none""#, vec![("meta.xml".to_owned(), "text/xml".to_owned(), meta().into_bytes())]),
            vec![],
        ),
    ];
    for (class, drawing, kept) in cases {
        let source = drawing.bytes();
        let dir = Dir::new();
        dir.write("a.odg", &source);
        // A macro project goes into a macro-enabled VSDX type only (Call 17).
        let vsdx = if class == "macro project" { "c.vsdm" } else { "c.vsdx" };
        let said = trip(&dir, &["a.odg", "b.diacraft", vsdx, "d.diacraft", "e.odg"]).unwrap_or_else(|e| panic!("{class}: {e}"));
        let named = said[1]["carried"]["classes"].as_array().unwrap().iter().any(|c| c.as_str() == Some(class));
        assert!(named, "{class}: the VSDX leg names it: {}", said[1]["carried"]);
        let back = dir.read("e.odg");
        let all = format!("{}{}", crate::odg_build::part_of(&back, "content.xml"), crate::odg_build::part_of(&back, "styles.xml"));
        let was = format!("{}{}", crate::odg_build::part_of(&source, "content.xml"), crate::odg_build::part_of(&source, "styles.xml"));
        for markup in kept {
            assert!(was.contains(markup), "{class}: the source holds {markup}");
            assert!(all.contains(markup), "{class}: {markup} comes back: {all}");
        }
        if class == "fill images" {
            assert_eq!(crate::odg_build::part_bytes(&back, "Pictures/img1.png"), Some(png()), "{class}: the picture");
        }
        // The entries of the class, byte for byte.
        let entries: Vec<(String, Vec<u8>)> = match class {
            "unknown parts" => vec![("foo/bar.dat".to_owned(), b"0123456789".to_vec())],
            "macro project" => macro_files().into_iter().map(|(n, _, b)| (n, b)).collect(),
            _ => Vec::new(),
        };
        for (name, bytes) in entries {
            assert_eq!(crate::odg_build::part_bytes(&back, &name), Some(bytes), "{class}: the entry {name} comes back as it was");
        }
        if class == "document properties" {
            let meta = crate::odg_build::part_of(&back, "meta.xml");
            for field in [
                "<dc:title>A title</dc:title>",
                "<dc:subject>A subject</dc:subject>",
                "<meta:keyword>words</meta:keyword>",
                "<dc:description>About it</dc:description>",
            ] {
                assert!(meta.contains(field), "{class}: {field} comes back: {meta}");
            }
        }
    }
}

/// The entries of a macro project of the `.odg` format (`Basic/`): bytes, never run.
fn macro_files() -> Vec<(String, String, Vec<u8>)> {
    vec![
        (
            "Basic/script-lc.xml".to_owned(),
            "text/xml".to_owned(),
            b"<library:libraries xmlns:library=\"http://openoffice.org/2000/library\"/>".to_vec(),
        ),
        (
            "Basic/Standard/Module1.xml".to_owned(),
            "text/xml".to_owned(),
            b"<script:module xmlns:script=\"http://openoffice.org/2000/script\" script:name=\"Module1\">not run</script:module>".to_vec(),
        ),
    ]
}

/// A `meta.xml` with a title, a subject, a keyword and a description.
fn meta() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator><dc:title>A title</dc:title><dc:subject>A subject</dc:subject><meta:keyword>words</meta:keyword><dc:description>About it</dc:description></office:meta></office:document-meta>",
        crate::odg_build::NS
    )
}

// ------------------------------------------------------------------------------------------------------------------ item 4: formulas come back from VSDX through an `.odg`

/// The rows of a rectangle's outline as a VSDX file writes them: every `X` and `Y` a share of the width or height (`Width*0`, `Width*1`, ...).
const RECT_ROWS: &str = concat!(
    r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
    r#"<Row T="LineTo" IX="2"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="0" F="Height*0"/></Row>"#,
    r#"<Row T="LineTo" IX="3"><Cell N="X" V="2" F="Width*1"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
    r#"<Row T="LineTo" IX="4"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="1" F="Height*1"/></Row>"#,
    r#"<Row T="LineTo" IX="5"><Cell N="X" V="0" F="Width*0"/><Cell N="Y" V="0" F="Height*0"/></Row></Section>"#
);

/// A VSDX drawing whose shapes hold the formulas counted lost (`Sheet.N!`, `RGB()`, `GUARD`, expressions over the shape's own cells), on style cells, frame cells, user cells and the
/// outline, with `ID`s that are not the places of the shapes (3, 7, and a group 20 of 21 and 22).
fn formulas_drawing() -> Vec<u8> {
    let three = format!(
        concat!(
            r#"<Shape ID="3" Type="Shape" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="PinX" V="2" F="Sheet.7!PinX-3"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#,
            r##"<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Cell N="FillForegnd" V="#FF0000" F="RGB(255,0,0)"/>"##,
            r##"<Cell N="LineWeight" V="0.02" F="GUARD(0.02)"/><Cell N="LineColor" V="#0000FF" F="Sheet.7!LineColor"/>"##,
            r#"<Section N="User"><Row N="Far"><Cell N="Value" V="4" F="Sheet.7!Width*2"/></Row><Row N="Own"><Cell N="Value" V="3" F="Width+Height"/></Row>"#,
            r#"<Row N="Grp"><Cell N="Value" V="4" F="Sheet.20!Width"/></Row></Section>"#,
            "{rows}<Text>three</Text></Shape>"
        ),
        rows = RECT_ROWS
    );
    let seven = format!(
        concat!(
            r#"<Shape ID="7" Type="Shape" LineStyle="0" FillStyle="0" TextStyle="0"><Cell N="PinX" V="5"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>"#,
            r##"<Cell N="LocPinX" V="1" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Cell N="LineColor" V="#0000FF"/>{rows}<Text>seven</Text></Shape>"##
        ),
        rows = RECT_ROWS
    );
    let member = |id: u32, x: f64| {
        format!(
            r#"<Shape ID="{id}" Type="Shape"><Cell N="PinX" V="{x}" F="Sheet.20!Width*{k}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="LocPinX" V="0.5" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/>{RECT_ROWS}</Shape>"#,
            k = x / 4.0
        )
    };
    let group = format!(
        concat!(
            r#"<Shape ID="20" Type="Group" NameU="Pair"><Cell N="PinX" V="4"/><Cell N="PinY" V="6"/><Cell N="Width" V="4"/><Cell N="Height" V="1"/>"#,
            r#"<Cell N="LocPinX" V="2" F="Width*0.5"/><Cell N="LocPinY" V="0.5" F="Height*0.5"/><Shapes>{a}{b}</Shapes></Shape>"#
        ),
        a = member(21, 1.0),
        b = member(22, 3.0)
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(PAGE_SHEET).shapes(&format!("{three}{seven}{group}"))).build()
}

/// Every cell of every shape of the first page that holds a formula, as (`ID` path, cell, formula), in order: the `ID`s of a group's members after the group's.
fn formulas_of(doc: &diacraft_doc::Document) -> Vec<(String, String, String)> {
    fn walk(list: &diacraft_doc::ShapeList, prefix: &str, out: &mut Vec<(String, String, String)>) {
        for shape in list.iter() {
            let at = format!("{prefix}{}", shape.id);
            for (name, cell) in shape.sheet.cells.iter() {
                if let Some(f) = cell.formula.text() {
                    out.push((at.clone(), name.as_str().to_owned(), f.to_owned()));
                }
            }
            for section in shape.sheet.sections.iter() {
                for row in section.rows.iter() {
                    for (name, cell) in row.cells.iter() {
                        if let Some(f) = cell.formula.text() {
                            out.push((at.clone(), format!("{}[{:?}{:?}].{}", section.name.as_str(), row.ix, row.name, name.as_str()), f.to_owned()));
                        }
                    }
                }
            }
            walk(&shape.children, &format!("{at}/"), out);
        }
    }
    let mut out = Vec::new();
    walk(&doc.pages.iter().next().unwrap().shapes, "", &mut out);
    out.sort();
    out
}

/// **Every formula comes back from VSDX through an `.odg` to VSDX, or the save says how many do not** (a formula that does not come back is a loss, even when its value does), and
/// **the shapes a formula names keep their `ID`s**, so a `Sheet.N!` names the same shape after the trip. Counted 1,808 of 13,267 formulas lost in the 25 trips that finished: the engine
/// handed the `.odg` writer each shape with its style and frame cells as constants of what they resolve to (`RGB()`, `GUARD`, `Sheet.N!` gone), and the writer took an outline whose rows are
/// `Width*0` and `Width*1` for the rows the reader gives back (the constant 0 and `Width`). Now the copy keeps the shape's own formulas with their values, the file's attributes say the values,
/// DiaCraft's data carries the formulas, and the rows ride beside the path where the reader would not give them back. **The place of a member of a group is the reader's to work out from the group's
/// frame**: its formulas are written as their values, and the save says how many. Red on main: `RGB(255,0,0)`, `GUARD(0.02)`, `Sheet.7!PinX-3`, `Sheet.7!LineColor` and the outline's `Width*0` and
/// `Width*1` are gone, and nothing is said.
#[test]
fn formulas_and_ids_come_back_from_vsdx_through_an_odg() {
    let source = formulas_drawing();
    let dir = Dir::new();
    dir.write("a.vsdx", &source);
    let said = trip(&dir, &["a.vsdx", "b.odg", "c.vsdx"]).unwrap_or_else(|e| panic!("{e}"));
    let read = |bytes: &[u8]| diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap().0;
    let (before, after) = (read(&source), read(&dir.read("c.vsdx")));
    let want = formulas_of(&before);
    assert!(want.iter().any(|(_, _, f)| f == "RGB(255,0,0)") && want.iter().any(|(_, _, f)| f == "Width*0"), "the source holds them: {want:?}");
    let got = formulas_of(&after);
    let missing: Vec<(String, String, String)> = want.iter().filter(|w| !got.contains(w)).cloned().collect();
    let pin = |at: &str, f: &str| (at.to_owned(), "PinX".to_owned(), f.to_owned());
    assert_eq!(
        missing,
        [pin("20/21", "Sheet.20!Width*0.25"), pin("20/22", "Sheet.20!Width*0.75")],
        "every formula but the members' places, on the shape of the same `ID`: {got:?}"
    );
    let warnings = said[0]["warnings"].to_string();
    assert!(warnings.contains("2 formulas of the shapes' own cells"), "the save says how many formulas it writes as their values: {warnings}");
    // The file another program reads draws the shape as the drawing does: the attributes say the values the formulas give.
    let odg = dir.read("b.odg");
    let content = crate::odg_build::content_of(&odg);
    assert!(content.contains(r##"draw:fill-color="#ff0000""##), "the fill's value is said natively: {content}");
    assert!(content.contains(r##"svg:stroke-color="#0000ff""##), "the line's colour too");
    // And the `.odg` read again holds them too (the model of the middle of the trip).
    let middle = diacraft_odg::read(&odg, &diacraft_odg::Limits::default()).unwrap().0;
    let in_middle = formulas_of(&middle);
    assert!(want.iter().filter(|w| !missing.contains(w)).all(|w| in_middle.contains(w)), "the `.odg` read again: {in_middle:?}");
}

// ------------------------------------------------------------------------------------------------------------------ R3: a list style in a style's graphic properties

/// A list style of one bulleted level, as a style's graphic properties hold one (LibreOffice writes it so in its default graphic styles).
const LIST_STYLE: &str = concat!(
    r#"<text:list-style style:name="Bulleted"><text:list-level-style-bullet text:level="1" text:bullet-char="&#x2022;">"#,
    r#"<style:list-level-properties text:min-label-width="0.6cm"/><style:text-properties fo:font-size="45%"/></text:list-level-style-bullet></text:list-style>"#
);

/// The drawings of R3: a common style whose `style:graphic-properties` holds a `text:list-style`, with attributes and without, with paragraph and text properties after it, and with a
/// `style:map` after them; and a shape on each.
fn list_style_drawings() -> Vec<(&'static str, Vec<u8>)> {
    let with = |name: &str, graphic_attrs: &str, after: &str| {
        let mut d = Odg::new();
        d.common.push_str(&format!(
            r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties{graphic_attrs}>{LIST_STYLE}</style:graphic-properties>{after}</style:style>"#
        ));
        d.auto.push_str(&format!(r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="{name}"/>"#));
        d.shapes.push_str(r#"<draw:rect draw:style-name="gr3" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hi</text:p></draw:rect>"#);
        d.bytes()
    };
    let props = r#"<style:paragraph-properties fo:margin-left="0cm"/><style:text-properties fo:font-size="18pt"/>"#;
    let after_other = |family: &str| {
        let mut d = Odg::new();
        // (A style of the text family says text properties: graphic properties are no part of it.)
        let own =
            if family == "text" { r#"<style:text-properties fo:font-size="12pt"/>"# } else { r#"<style:graphic-properties draw:fill="none"/>"# };
        d.common.push_str(&format!(r#"<style:style style:name="Other" style:family="{family}">{own}</style:style>"#));
        d.common.push_str(&format!(
            r#"<style:style style:name="Listed" style:family="graphic"><style:graphic-properties draw:fill="none">{LIST_STYLE}</style:graphic-properties>{props}</style:style>"#
        ));
        d.auto.push_str(r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="Listed"/>"#);
        d.shapes.push_str(r#"<draw:rect draw:style-name="gr3" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hi</text:p></draw:rect>"#);
        d.bytes()
    };
    let unused = {
        let mut d = Odg::new();
        d.common.push_str(&format!(r#"<style:style style:name="Listed" style:family="graphic"><style:graphic-properties draw:fill="none">{LIST_STYLE}</style:graphic-properties>{props}</style:style>"#));
        d.shapes.push_str(r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="R" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"><text:p>hi</text:p></draw:rect>"#);
        d.bytes()
    };
    vec![
        ("after a style of the presentation family", after_other("presentation")),
        ("after a style of the text family", after_other("text")),
        ("a list style no shape names", unused),
        ("a list style beside fill attributes", with("Listed", r#" draw:fill="none" draw:stroke="solid""#, "")),
        ("a list style alone", with("Listed", "", "")),
        ("a list style with paragraph and text properties after it", with("Listed", r#" draw:fill="none""#, props)),
        (
            "a list style and a style:map",
            with("Listed", r#" draw:fill="none""#, &format!(r#"{props}<style:map style:condition="true()" style:apply-style-name="standard"/>"#)),
        ),
    ]
}

/// `el` with DiaCraft's and LibreOffice's extension markup taken out (their attributes and elements, by the prefixes `diacraft` and `loext`), as the schema check reads a part.
fn strip_extensions(el: &mut diacraft_container::xml::El) {
    let ours = |name: &str| ["diacraft:", "loext:", "xmlns:diacraft", "xmlns:loext"].iter().any(|p| name.starts_with(p));
    el.attrs.retain(|(k, _)| !ours(k));
    el.children.retain(|n| !matches!(n, diacraft_container::xml::Node::El(e) if ours(&e.name)));
    for n in &mut el.children {
        if let diacraft_container::xml::Node::El(e) = n {
            strip_extensions(e);
        }
    }
}

/// The `style:style` elements of `styles.xml` named `name`, each as the local names of its children in order, and the local names of the children of its `style:graphic-properties`.
fn style_children(styles: &str, name: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let root = diacraft_container::xml::parse(styles.as_bytes()).unwrap();
    let local = |n: &str| n.rsplit(':').next().unwrap_or(n).to_owned();
    let mut out = Vec::new();
    let mut stack: Vec<&diacraft_container::xml::El> = vec![&root];
    while let Some(el) = stack.pop() {
        if local(&el.name) == "style" && el.attr("style:name") == Some(name) {
            let kids: Vec<String> = el.elements().map(|c| local(&c.name)).collect();
            let graphic: Vec<String> =
                el.elements().filter(|c| local(&c.name) == "graphic-properties").flat_map(|g| g.elements().map(|c| local(&c.name))).collect();
            out.push((kids, graphic));
        }
        stack.extend(el.elements());
    }
    out
}

/// **R3 (the full gate's schema check on `9d0daec0`):** a common style whose `style:graphic-properties` holds a `text:list-style` (LibreOffice writes one so) comes back from a visit through `.diacraft`
/// with one `style:graphic-properties`, first, holding the list style, then the paragraph and text properties, then a `style:map`, in the order the ODF 1.4 schema takes (`style-style-content`), and
/// `styles.xml` validates with xmllint against the ODF 1.4 RelaxNG when `ODF_SCHEMA_RNG` names it (DiaCraft's and LibreOffice's markup taken out, as `odg`'s `model_safe::schema_errors` does).
/// **Not red:** the gate found `style:style` "extra content" at a `style:graphic-properties` in 120 files, 118 of which hold such a list style; none of these hand-built forms gives it on this
/// branch, so the sample files' form is still to be told (a vector of it is asked for).
#[test]
fn a_list_style_in_a_style_s_graphic_properties_comes_back_where_the_schema_takes_it() {
    for (what, source) in list_style_drawings() {
        let dir = Dir::new();
        dir.write("a.odg", &source);
        trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap_or_else(|e| panic!("{what}: {e}"));
        let back = dir.read("c.odg");
        let styles = crate::odg_build::part_of(&back, "styles.xml");
        let found = style_children(&styles, "Listed");
        let [(kids, graphic)] = found.as_slice() else { panic!("{what}: one style Listed: {found:?}") };
        let (src_kids, src_graphic) = style_children(&crate::odg_build::part_of(&source, "styles.xml"), "Listed").remove(0);
        assert_eq!(kids, &src_kids, "{what}: the style's children, in the source's order: {styles}");
        assert_eq!(graphic, &src_graphic, "{what}: what the graphic properties hold");
        assert_eq!(kids.iter().filter(|k| *k == "graphic-properties").count(), 1, "{what}");
        if let Ok(rng) = std::env::var("ODF_SCHEMA_RNG") {
            let file = dir.path("styles.xml");
            let mut root = diacraft_container::xml::parse(styles.as_bytes()).unwrap();
            strip_extensions(&mut root);
            std::fs::write(&file, diacraft_container::xml::to_bytes(&root, b"", b"\n", true).unwrap()).unwrap();
            let run = std::process::Command::new("xmllint").arg("--noout").arg("--relaxng").arg(&rng).arg(&file).output().unwrap();
            assert!(run.status.success(), "{what}: {}", String::from_utf8_lossy(&run.stderr));
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ round 2, an earlier version's forms in ODF 1.4's

/// The common style of the drawing (labelled 1.1, our own names and values): `list` is the list style in its graphic properties (or none), `numbering` the paragraph properties'
/// `text:enable-numbering`, `size` the text properties' `fo:font-size`.
fn older_vector(list: &str, numbering: bool, size: &str) -> Vec<u8> {
    older_vector_with(list, numbering, size, "")
}

/// [`older_vector`], and `run` the font size of the shape's text run, in an automatic text style of `content.xml` (empty: the run has none).
fn older_vector_with(list: &str, numbering: bool, size: &str, run: &str) -> Vec<u8> {
    let ns = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
        r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#
    );
    let numbering = if numbering { r#" text:enable-numbering="false""# } else { "" };
    let styles = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {ns} office:version="1.1"><office:styles><style:style style:name="Plain" style:family="graphic">"#,
            r##"<style:graphic-properties draw:fill="solid" draw:fill-color="#c0ffee" draw:stroke="solid" svg:stroke-color="#123456">{list}</style:graphic-properties>"##,
            r#"<style:paragraph-properties fo:margin-left="0cm" fo:margin-right="0cm" fo:text-indent="0cm"{numbering}/><style:text-properties fo:font-size="{size}" fo:font-weight="normal"/></style:style></office:styles>"#,
            r#"<office:automatic-styles><style:page-layout style:name="Lay"><style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" style:print-orientation="portrait"/></style:page-layout></office:automatic-styles>"#,
            r#"<office:master-styles><style:master-page style:name="Base" style:page-layout-name="Lay"/></office:master-styles></office:document-styles>"#
        ),
        ns = ns,
        list = list,
        numbering = numbering,
        size = size
    );
    let (automatic, text) = if run.is_empty() {
        (String::new(), "Vector".to_owned())
    } else {
        (
            format!(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="{run}"/></style:style>"#),
            r#"<text:span text:style-name="T1">Vector</text:span>"#.to_owned(),
        )
    };
    let content = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ns} office:version="1.1"><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:drawing>"#,
            r#"<draw:page draw:name="Sheet" draw:master-page-name="Base"><draw:rect draw:style-name="Plain" svg:x="2cm" svg:y="3cm" svg:width="5cm" svg:height="2cm"><text:p>{text}</text:p></draw:rect></draw:page>"#,
            r#"</office:drawing></office:body></office:document-content>"#
        ),
        ns = ns,
        automatic = automatic,
        text = text
    );
    crate::odg_build::package(
        crate::odg_build::DRAWING,
        vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), styles.into_bytes())],
    )
}

/// The list style of the vector, named or not, with the first level's label width.
fn older_list(name: Option<&str>, width: &str) -> String {
    let name = name.map(|n| format!(r#" style:name="{n}""#)).unwrap_or_default();
    format!(
        concat!(
            r#"<text:list-style{name}><text:list-level-style-bullet text:level="1" text:bullet-char="*"><style:list-level-properties text:space-before="0.1cm" text:min-label-width="{width}"/>"#,
            r#"<style:text-properties fo:font-family="Sans"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="2" text:bullet-char="-">"#,
            r#"<style:list-level-properties text:space-before="0.5cm" text:min-label-width="0.4cm"/></text:list-level-style-bullet></text:list-style>"#
        ),
        name = name,
        width = width
    )
}

/// The drawing and its single-cause variants, and the control: (what, bytes, whether it holds a form 1.4 refuses).
fn older_vectors() -> Vec<(&'static str, Vec<u8>, bool)> {
    let tally = older_list(Some("Tally"), "0.4cm");
    vec![
        ("the vector: text:enable-numbering beside a list style", older_vector(&tally, true, "12pt"), true),
        ("min: text:enable-numbering alone", older_vector("", true, "12pt"), true),
        ("noname: a list style with no style:name", older_vector(&older_list(None, "0.4cm"), false, "12pt"), true),
        ("negw: a negative label width", older_vector(&older_list(Some("Tally"), "-0.4cm"), false, "12pt"), true),
        ("zerofs: a font size of 0", older_vector(&tally, false, "0pt"), true),
        ("zerofs-run: a font size of 0 on a shape's text run", older_vector_with(&tally, false, "12pt", "0pt"), true),
        ("ctl-valid: none of them", older_vector(&tally, false, "12pt"), false),
    ]
}

/// The `Size` of each `Character` row of the first shape a read of `bytes` gives.
fn shape_sizes(bytes: &[u8]) -> Vec<Option<String>> {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let shape = doc.pages.get(0).and_then(|p| p.shapes.iter().next().cloned()).expect("a shape");
    shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == "Character")
        .flat_map(|s| s.rows.iter().map(|r| r.cells.get("Size").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())).collect::<Vec<_>>())
        .collect()
}

/// What a read of `bytes` gives of the style `Plain`: its unmapped properties, its kept children and its `Character` row's `Size`.
fn plain_style(bytes: &[u8]) -> (Vec<(String, String)>, diacraft_doc::Passthrough, Option<String>) {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let style = doc.styles.iter().find(|s| s.ext.get().and_then(|d| d.odf.name.as_deref()) == Some("Plain")).expect("the style Plain").clone();
    let odf = style.ext.get().unwrap().odf.clone();
    let mut unmapped: Vec<(String, String)> = odf.unmapped.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    unmapped.sort();
    let size = style
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .and_then(|s| s.rows.iter().next().cloned())
        .and_then(|r| r.cells.get("Size").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()));
    (unmapped, odf.kept.kept, size)
}

/// **The `.odg` writer writes ODF 1.4-valid markup, and a DiaCraft read gives the source's forms back.** A style of a file labelled 1.1 may hold what the 1.1 schema takes and
/// 1.4's refuses (`text:enable-numbering`, a list style with no name, a negative label width, a font size of 0); the model writer labels the file 1.4 and, writes the style natively,
/// so the visit made a valid file invalid (5 sample files, about 130 with these forms). Now each is written in 1.4's form (left out, named, at 0, a tenth of a point) and the source's rides in the
/// style's entry of DiaCraft's document file. Red on main: `text:enable-numbering`, the list style with no name, the `-0.4cm` and the `0pt` are in the file written.
#[test]
fn an_earlier_versions_style_forms_are_written_in_1_4_s_and_read_back_as_they_were() {
    for (what, source, older) in older_vectors() {
        let dir = Dir::new();
        dir.write("a.odg", &source);
        let results = trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap_or_else(|e| panic!("{what}: {e}"));
        let back = dir.read("c.odg");
        let styles = crate::odg_build::part_of(&back, "styles.xml");
        let content = crate::odg_build::part_of(&back, "content.xml");
        // The card says it in the values group (as the allowances are), and finds nothing lost.
        let checked = &results[1]["checked"];
        assert_eq!(checked["classes"], serde_json::json!([]), "{what}: {checked}");
        if older {
            assert_eq!(checked["as_values"], true, "{what}: {checked}");
        }
        // What 1.4 refuses is not in the file.
        assert!(!content.contains("fo:font-size=\"0pt\""), "{what}: no font size of 0 in a shape's text: {content}");
        assert!(!styles.contains("text:enable-numbering"), "{what}: {styles}");
        assert_eq!(
            styles.matches("<text:list-style").count(),
            styles.matches("<text:list-style style:name=").count(),
            "{what}: every list style is named: {styles}"
        );
        assert!(!styles.contains("min-label-width=\"-"), "{what}: no negative label width: {styles}");
        assert!(!styles.contains("fo:font-size=\"0pt\"") && !styles.contains("fo:font-size=\"0in\""), "{what}: no font size of 0: {styles}");
        // And a DiaCraft read gives the source's forms back.
        assert_eq!(plain_style(&back), plain_style(&source), "{what}: the style as the source had it");
        assert_eq!(shape_sizes(&back), shape_sizes(&source), "{what}: the shape's text as the source had it");
        let docfile = crate::odg_build::part_bytes(&back, "DiaCraft/document.xml").map(|b| String::from_utf8(b).unwrap()).unwrap_or_default();
        assert_eq!(docfile.contains("diacraft:older"), older && !what.starts_with("zerofs"), "{what}: the source's forms ride: {docfile}");
        // Both parts validate against the ODF 1.4 schema, DiaCraft's own extensions taken out (`ODF_SCHEMA_RNG`: the OASIS RelaxNG file; the check is skipped where it is not set).
        if let Ok(rng) = std::env::var("ODF_SCHEMA_RNG") {
            for (part, text) in [("styles.xml", &styles), ("content.xml", &content)] {
                let file = dir.path(part);
                let mut root = diacraft_container::xml::parse(text.as_bytes()).unwrap();
                strip_extensions(&mut root);
                std::fs::write(&file, diacraft_container::xml::to_bytes(&root, b"", b"\n", true).unwrap()).unwrap();
                let run = std::process::Command::new("xmllint").arg("--noout").arg("--relaxng").arg(&rng).arg(&file).output().unwrap();
                assert!(run.status.success(), "{what}: {part}: {}", String::from_utf8_lossy(&run.stderr));
            }
        }
    }
}

/// **The control is written as it was**: a style of a file labelled 1.1 that holds none of the forms 1.4 refuses gives the file it gave before the forms were brought into 1.4's (the parts of the
/// visit are the bytes of `CONTROL_PARTS`, hashed when this round began, on `5ce74a6e`), and nothing rides for it in DiaCraft's document file.
#[test]
fn a_style_of_an_earlier_version_with_no_form_1_4_refuses_is_written_as_before() {
    let (_, source, _) = older_vectors().pop().unwrap();
    let dir = Dir::new();
    dir.write("a.odg", &source);
    trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap();
    let back = dir.read("c.odg");
    let hash = |part: &str| crate::odg_build::part_bytes(&back, part).map(|b| format!("{:016x}", fnv(&b)));
    let found: Vec<(&str, Option<String>)> = CONTROL_PARTS.iter().map(|(part, _)| (*part, hash(part))).collect();
    let wanted: Vec<(&str, Option<String>)> = CONTROL_PARTS.iter().map(|(part, h)| (*part, h.map(str::to_owned))).collect();
    assert_eq!(found, wanted);
}

/// The FNV-1a hash of `bytes` (a fingerprint for a test to pin, no security).
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// The parts of the control's visit and their hashes: the visit writes no document file. The hash of `styles.xml` was `b2c5995fe1947fd1` on `5ce74a6e` (before round 2) and until a later change,
/// which writes the style's weight as the file spelled it (`fo:font-weight="normal"` alone, and not with the four attributes of its group that the file did not state).
const CONTROL_PARTS: [(&str, Option<&str>); 3] =
    [("styles.xml", Some("df64c943396ab4fa")), ("content.xml", Some("5bdea246fc9f3396")), ("DiaCraft/document.xml", None)];

/// The files of the LibreOffice check of round 2 (run by hand): `SAFEF2_LO_DIR` gets each vector visited through `.diacraft`.
#[test]
#[ignore]
fn lo_files_of_round_2() {
    let Ok(out) = std::env::var("SAFEF2_LO_DIR") else { return };
    for (k, (what, source, _)) in older_vectors().into_iter().enumerate() {
        let dir = Dir::new();
        dir.write("a.odg", &source);
        trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap_or_else(|e| panic!("{what}: {e}"));
        std::fs::write(std::path::Path::new(&out).join(format!("older{k}.odg")), dir.read("c.odg")).unwrap();
    }
    // And the files of a folder `SAFEF2_VECTORS` names (built by hand), each visited the same way.
    let Ok(vectors) = std::env::var("SAFEF2_VECTORS") else { return };
    let mut files: Vec<std::path::PathBuf> =
        std::fs::read_dir(&vectors).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "odg")).collect();
    files.sort();
    for file in files {
        let dir = Dir::new();
        dir.write("a.odg", &std::fs::read(&file).unwrap());
        trip(&dir, &["a.odg", "b.diacraft", "c.odg"]).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let (source, back) = (std::fs::read(&file).unwrap(), dir.read("c.odg"));
        assert_eq!(plain_style(&back), plain_style(&source), "{}: the style as the source had it", file.display());
        std::fs::write(std::path::Path::new(&out).join(file.file_name().unwrap()), back).unwrap();
    }
}
