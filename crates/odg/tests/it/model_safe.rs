//! The `.odg` model writer never loses data silently (the owner's "ship safe" for 0.2).
use std::io::Write;

use diacraft_doc::Document;

use crate::drawing::Drawing;

/// A real PNG of 2 by 2 pixels (red, green, blue, white), encoded here from the PNG standard (ISO/IEC 15948: signature, `IHDR`, one `IDAT` of zlib data, `IEND`), so that another
/// program decodes it and shows it.
pub(crate) fn png_2x2() -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.finalize().to_be_bytes());
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    // Width 2, height 2, 8 bits, colour type 2 (RGB), deflate, no filter method, no interlace.
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    // Each row: filter type 0, then three bytes a pixel.
    let raw = [0u8, 255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 255, 255, 255];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).unwrap();
    chunk(&mut out, b"IDAT", &z.finish().unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A fill image definition of `office:styles` named `name`, whose picture is the package entry `href`.
pub(crate) fn fill_image(name: &str, href: &str) -> String {
    format!(r#"<draw:fill-image draw:name="{name}" xlink:href="{href}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#)
}

/// A rectangle on the style `style`.
pub(crate) fn rect(style: &str, x: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="{x}" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)
}

/// The vectors of the task, each one construct.
pub(crate) fn vectors() -> Vec<(&'static str, Drawing)> {
    let bitmap_auto = r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/></style:style>"#;
    let with_png = |mut d: Drawing| {
        d.files.push(("Pictures/img1.png".to_owned(), png_2x2()));
        d
    };
    let shape_fill = with_png(Drawing::a4().common(&fill_image("Img1", "Pictures/img1.png")).auto(bitmap_auto).page(&rect("gr1", "2cm")));
    let two_shapes = with_png(
        Drawing::a4()
            .common(&fill_image("Img1", "Pictures/img1.png"))
            .auto(bitmap_auto)
            .auto(r#"<style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1" style:repeat="stretch"/></style:style>"#)
            .page(&format!("{}{}", rect("gr1", "2cm"), rect("gr2", "8cm"))),
    );
    let sheet_fill = with_png(
        Drawing::a4()
            .common(&fill_image("Img1", "Pictures/img1.png"))
            .common(r#"<style:style style:name="Pictured" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/></style:style>"#)
            .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="Pictured"><style:graphic-properties draw:stroke="solid"/></style:style>"#)
            .page(&rect("gr1", "2cm")),
    );
    let unused = with_png(Drawing::a4().common(&fill_image("Img1", "Pictures/img1.png")).page(&rect("", "2cm")));
    vec![
        ("a shape with a bitmap fill", shape_fill),
        ("two shapes sharing one bitmap", two_shapes),
        ("a style sheet with a bitmap fill", sheet_fill),
        ("a fill image no shape names", unused),
        ("style sheets as a drawing program writes them", style_sheets()),
    ]
}

/// A face declaration, in both parts as programs write them.
pub(crate) const FACE: &str = r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'" style:font-family-generic="swiss" style:font-pitch="variable"/>"#;

/// The common styles a drawing program writes for a new drawing, in the way it writes them (each written here from ODF 1.4 Part 3 §16: a default style, a `standard` graphic style with
/// a fill, a line, padding, paragraph and text properties, a style whose parent it is and that says a few things of its own, and a paragraph style), and shapes that name them.
pub(crate) fn style_sheets() -> Drawing {
    let standard = r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-width="0cm" svg:stroke-color="#3465a4" draw:marker-start-width="0.2cm" draw:marker-start-center="false" draw:marker-end-width="0.2cm" draw:marker-end-center="false" draw:fill="solid" draw:fill-color="#729fcf" draw:textarea-horizontal-align="justify" fo:padding-top="0.125cm" fo:padding-bottom="0.125cm" fo:padding-left="0.25cm" fo:padding-right="0.25cm" draw:shadow="hidden" draw:shadow-offset-x="0.2cm" draw:shadow-offset-y="0.2cm" draw:shadow-color="#808080"><text:list-style style:name="standard"><text:list-level-style-bullet text:level="1" text:bullet-char="&#x25CF;"><style:list-level-properties text:min-label-width="0.6cm"/><style:text-properties fo:font-family="OpenSymbol" style:use-window-font-color="true" fo:font-size="45%"/></text:list-level-style-bullet></text:list-style></style:graphic-properties><style:paragraph-properties fo:margin-left="0cm" fo:margin-right="0cm" fo:margin-top="0cm" fo:margin-bottom="0cm" fo:line-height="100%" fo:text-indent="0cm"/><style:text-properties fo:font-variant="normal" fo:text-transform="none" style:use-window-font-color="true" loext:opacity="0%" style:text-outline="false" style:text-line-through-style="none" style:font-name="Liberation Sans" fo:font-size="18pt" fo:font-style="normal" fo:text-shadow="none" style:text-underline-style="none" fo:font-weight="normal" style:font-size-asian="18pt" style:font-style-asian="normal" style:font-weight-asian="normal" style:font-size-complex="18pt" style:font-style-complex="normal" style:font-weight-complex="normal" style:text-emphasize="none" style:font-relief="none" style:text-overline-style="none" style:text-overline-color="font-color"/></style:style>"##;
    let filled = r##"<style:style style:name="Filled" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffd428" draw:stroke="none"/><style:text-properties fo:font-size="14pt" fo:font-weight="bold"/></style:style>"##;
    let paragraph = r#"<style:style style:name="Heading" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="24pt"/></style:style>"#;
    let defaults = r##"<style:default-style style:family="graphic"><style:graphic-properties svg:stroke-color="#3465a4" draw:fill-color="#729fcf"/><style:text-properties style:font-name="Liberation Sans" fo:font-size="24pt"/></style:default-style>"##;
    let mut d = Drawing::a4()
        .common(defaults)
        .common(standard)
        .common(filled)
        .common(paragraph)
        .auto(r#"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:textarea-vertical-align="middle"/></style:style>"#)
        .auto(r#"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="Filled"/>"#)
        .page(&format!(
            "{}{}",
            r#"<draw:custom-shape draw:style-name="gr1" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"><text:p>One</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle"/></draw:custom-shape>"#,
            r#"<draw:custom-shape draw:style-name="gr2" svg:x="8cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"><text:p>Two</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="ellipse"/></draw:custom-shape>"#
        ));
    d.fonts = FACE.to_owned();
    d.ns.push_str(r#" xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0""#);
    d
}

/// Whether two models are one drawing (the package's bookkeeping left out), and where they differ when they are not.
pub(crate) fn model_same(a: &Document, b: &Document) -> Result<(), Vec<String>> {
    let (a, b) = (crate::drawing::without_package(a), crate::drawing::without_package(b));
    let mut differ = Vec::new();
    if a.pages.len() != b.pages.len() {
        differ.push("page count".to_owned());
    }
    for (x, y) in a.pages.iter().zip(b.pages.iter()) {
        if x != y {
            differ.push(format!("page {}", x.id));
        }
    }
    for (word, same) in [
        ("styles", a.styles == b.styles),
        ("fonts", a.fonts == b.fonts),
        ("ext", a.ext == b.ext),
        ("masters", a.masters == b.masters),
        ("sheet", a.sheet == b.sheet),
        ("props", a.props == b.props),
    ] {
        if !same {
            differ.push(word.to_owned());
        }
    }
    if differ.is_empty() { Ok(()) } else { Err(differ) }
}

/// The part `name` of a package, as text.
pub(crate) fn part(bytes: &[u8], name: &str) -> Option<String> {
    let pkg = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).ok()?;
    pkg.part(name).map(|b| String::from_utf8_lossy(b).into_owned())
}

/// A drawing read and written from the model alone (`write_with(doc, None, ..)`), the content as it was read: the written bytes and the notes.
pub(crate) fn visit(bytes: &[u8]) -> diacraft_odg::Written {
    let (doc, _) = diacraft_odg::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions { content_unchanged: true, ..diacraft_odg::WriteOptions::default() }).unwrap()
}

/// The foreign namespaces whose markup is taken out before a part is held to the schema (a document must be valid once foreign markup is taken out): DiaCraft's, and
/// the extension of a drawing program that the vectors write as such files hold it (`loext`).
const FOREIGN_NS: [&str; 2] = ["urn:app.diacraft:extension", "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0"];

/// `el` with the foreign elements, attributes and declarations taken out, at any depth.
fn strip(el: &mut diacraft_container::xml::El, bound: &[String]) {
    let mut mine: Vec<String> = bound.to_vec();
    for (k, v) in &el.attrs {
        if FOREIGN_NS.contains(&v.as_str())
            && let Some(p) = k.strip_prefix("xmlns:")
        {
            mine.push(p.to_owned());
        }
    }
    let ours = |name: &str| name.split_once(':').is_some_and(|(p, _)| mine.iter().any(|m| m == p));
    el.attrs.retain(|(k, v)| !(FOREIGN_NS.contains(&v.as_str()) && k.starts_with("xmlns")) && !ours(k));
    el.children.retain(|n| !matches!(n, diacraft_container::xml::Node::El(e) if ours(&e.name)));
    for n in &mut el.children {
        if let diacraft_container::xml::Node::El(e) = n {
            strip(e, &mine);
        }
    }
}

/// Each XML part of the package `bytes` that the ODF 1.4 schema is asked about (`content.xml`, `styles.xml`, `meta.xml`, `settings.xml`), DiaCraft's markup taken out, held to the schema
/// at `ODF_SCHEMA_RNG` (OASIS's `OpenDocument-v1.4-schema.rng`, <https://docs.oasis-open.org/office/OpenDocument/v1.4/os/schemas/OpenDocument-v1.4-schema.rng>) by `xmllint --relaxng`:
/// `Ok` when every part is valid, else the parts that are not with what xmllint said. `None` when no schema is given (the check is run by hand).
pub(crate) fn schema_errors(bytes: &[u8], what: &str) -> Option<Result<(), String>> {
    let rng = std::env::var("ODF_SCHEMA_RNG").ok()?;
    let dir = std::path::PathBuf::from(std::env::var("ODF_SCHEMA_OUT").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failed = Vec::new();
    for name in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        let Some(text) = part(bytes, name) else { continue };
        let mut root = diacraft_container::xml::parse(text.as_bytes()).unwrap();
        strip(&mut root, &[]);
        let out = diacraft_container::xml::to_bytes(&root, b"", b"\n", true).unwrap();
        let file = dir.join(format!("{}__{name}", what.replace([' ', '/'], "_")));
        std::fs::write(&file, out).unwrap();
        let run = std::process::Command::new("xmllint").arg("--noout").arg("--relaxng").arg(&rng).arg(&file).output().unwrap();
        if !run.status.success() {
            failed.push(format!("{name}: {}", String::from_utf8_lossy(&run.stderr).lines().take(6).collect::<Vec<_>>().join(" | ")));
        }
    }
    Some(if failed.is_empty() { Ok(()) } else { Err(failed.join("\n")) })
}

/// A drawing that holds no settings is written with a `settings.xml` whose root holds nothing: the ODF 1.4 schema takes it (an empty `office:settings` it refuses), the
/// manifest lists it, counts it as none in the content check, and a second save gives the same bytes.
#[test]
fn a_drawing_with_no_settings_gets_a_settings_part_that_holds_nothing() {
    let source = Drawing::a4().page(&rect("", "2cm"));
    assert!(source.settings.is_none());
    let bytes = source.bytes();
    let written = visit(&bytes);
    let settings = part(&written.bytes, "settings.xml").expect("a settings.xml");
    assert!(!settings.contains("office:settings"), "no empty office:settings: {settings}");
    assert!(settings.contains("<office:document-settings") && settings.trim_end().ends_with("/>"), "{settings}");
    assert!(part(&written.bytes, "META-INF/manifest.xml").unwrap().contains("manifest:full-path=\"settings.xml\""));
    let diff = diacraft_odg::compare_content(&bytes, &written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert!(diff.shown.iter().all(|d| d.part != "settings.xml"), "the part counts as none: {:?}", diff.shown);
    let again = visit(&written.bytes);
    assert_eq!(part(&again.bytes, "settings.xml"), Some(settings), "a second save writes the same part");
    // A new drawing, which holds no settings either.
    let blank = diacraft_doc::Document::new_blank();
    let made = diacraft_odg::write_with(&blank, None, &diacraft_odg::WriteOptions::default()).unwrap();
    assert!(!part(&made.bytes, "settings.xml").unwrap().contains("office:settings"));
    if let Some(checked) = schema_errors(&written.bytes, "no settings") {
        checked.unwrap();
    }
}

/// The bytes of the entry `name` of a package.
pub(crate) fn entry(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let pkg = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).ok()?;
    pkg.part(name).map(<[u8]>::to_vec)
}

/// The vector called `name` of [`vectors`], as a package.
fn vector(name: &str) -> Vec<u8> {
    vectors().into_iter().find(|(n, _)| *n == name).map(|(_, d)| d.bytes()).unwrap()
}

/// The differences the content check finds in the drawing's own parts and its pictures: what a visit must not change (the package's bookkeeping, the manifest's version and `meta.xml`'s
/// generator, is the document level's).
fn drawing_differences(source: &[u8], written: &[u8]) -> Vec<String> {
    let diff = diacraft_odg::compare_content(source, written, &diacraft_odg::Limits::default()).unwrap();
    diff.shown
        .iter()
        .filter(|d| d.part == "content.xml" || d.part == "styles.xml" || d.part.starts_with("Pictures/"))
        .map(|d| format!("{} {} {:?}", d.part, d.path, d.found))
        .collect()
}

/// The picture of a bitmap fill, written from the model: the definition in `office:styles`, its `Pictures/` entry with the bytes the file had and a manifest row of the
/// picture's type, and no note (85 visits lost every picture, with no word). Each is held for a shape, two shapes sharing one, and a fill no shape names.
#[test]
fn a_bitmap_fill_is_written_with_its_picture_and_its_row() {
    for name in ["a shape with a bitmap fill", "two shapes sharing one bitmap", "a fill image no shape names"] {
        let source = vector(name);
        let written = visit(&source);
        assert!(written.notes.is_empty(), "{name}: {:?}", written.notes);
        let styles = part(&written.bytes, "styles.xml").unwrap();
        assert!(styles.contains("<draw:fill-image draw:name=\"Img1\" xlink:href=\"Pictures/img1.png\""), "{name}: {styles}");
        assert_eq!(entry(&written.bytes, "Pictures/img1.png"), Some(png_2x2()), "{name}: the picture's bytes");
        let manifest = part(&written.bytes, "META-INF/manifest.xml").unwrap();
        assert_eq!(
            manifest.matches("manifest:full-path=\"Pictures/img1.png\" manifest:media-type=\"image/png\"").count(),
            1,
            "{name}: one row: {manifest}"
        );
        assert_eq!(drawing_differences(&source, &written.bytes), Vec::<String>::new(), "{name}");
        let (before, _) = diacraft_odg::read(&source, &diacraft_odg::Limits::default()).unwrap();
        let (after, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
        assert_eq!(model_same(&before, &after), Ok(()), "{name}");
        // A second save is the same bytes.
        assert_eq!(visit(&written.bytes).bytes, visit(&written.bytes).bytes, "{name}");
        if let Some(checked) = schema_errors(&written.bytes, name) {
            checked.unwrap();
        }
    }
    // Two shapes of one picture: one entry, both styles name it, and each keeps what it said beside the picture (the second's repeat mode, which the stand-in pattern's ops took out).
    let written = visit(&vector("two shapes sharing one bitmap"));
    let content = part(&written.bytes, "content.xml").unwrap();
    assert_eq!(content.matches("draw:fill-image-name=\"Img1\"").count(), 2, "{content}");
    assert!(content.contains("style:repeat=\"stretch\""), "{content}");
    let pictures = diacraft_odg::package::Package::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(pictures.entries().filter(|e| e.name.starts_with("Pictures/")).count(), 1);
}

/// A style sheet with a bitmap fill is written with it: the common style it was says the fill natively, so that another program shows it on a shape that names the style, and
/// DiaCraft's document file carries nothing for it.
#[test]
fn a_style_sheet_with_a_bitmap_fill_says_it_natively() {
    let written = visit(&vector("a style sheet with a bitmap fill"));
    let styles = part(&written.bytes, "styles.xml").unwrap();
    assert!(
        styles.contains(r#"<style:style style:name="Pictured" style:family="graphic"><style:graphic-properties draw:fill="bitmap" draw:fill-image-name="Img1"/></style:style>"#),
        "{styles}"
    );
    assert_eq!(part(&written.bytes, "DiaCraft/document.xml"), None, "nothing to carry");
}

/// The style sheets a drawing program writes are written as the common styles they were (found the common styles of 703 of 719 visits written empty, their cells
/// in DiaCraft's data alone, so another program drew every shape that named one without the fill, the line and the font it gave): the graphic properties, the paragraph and text properties of
/// the cells, the values no cell reads, the children, the display name and the parent, and the model read back is the one written. **The `Style` cell of a bold style wrote every attribute of
/// its group** (the font's style, variant and the complex script's weight and style as `normal`), where the file stated the weight alone; holds the spelling of the group, so
/// nothing is left that differs.
#[test]
fn style_sheets_are_written_as_the_common_styles_they_were() {
    let source = style_sheets().bytes();
    let written = visit(&source);
    assert!(written.notes.is_empty(), "{:?}", written.notes);
    let styles = part(&written.bytes, "styles.xml").unwrap();
    for wanted in [
        r##"draw:fill-color="#729fcf""##,
        r##"svg:stroke-color="#3465a4""##,
        r#"fo:padding-left="0.25cm""#,
        r#"draw:textarea-horizontal-align="justify""#,
        r#"<text:list-style style:name="standard">"#,
        r#"fo:line-height="100%""#,
        r#"style:font-name="Liberation Sans""#,
        r#"fo:font-size="18pt""#,
        r#"loext:opacity="0%""#,
        r#"<style:style style:name="Filled" style:family="graphic" style:parent-style-name="standard">"#,
        r##"draw:fill-color="#ffd428""##,
        r#"<style:style style:name="Heading" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="24pt"/></style:style>"#,
    ] {
        assert!(styles.contains(wanted), "{wanted}: {styles}");
    }
    let left = drawing_differences(&source, &written.bytes);
    assert!(left.is_empty(), "the bold style says the weight alone, as the file did: {left:?}");
    let (before, _) = diacraft_odg::read(&source, &diacraft_odg::Limits::default()).unwrap();
    let (after, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(model_same(&before, &after), Ok(()));
    assert_eq!(part(&written.bytes, "DiaCraft/document.xml"), None, "nothing to carry");
    assert_eq!(visit(&written.bytes).bytes, visit(&written.bytes).bytes, "a second save is the same bytes");
    if let Some(checked) = schema_errors(&written.bytes, "style sheets") {
        checked.unwrap();
    }
}

/// A style sheet whose cells the markup cannot all say is written natively for what it can, and DiaCraft's document file carries the rest (the difference between the sheet the markup
/// gives and the model's): a cell edited in DiaCraft is written as the new value, a formula rides in the document file, and the drawing reads back as the model was.
#[test]
fn a_style_sheet_edited_in_diacraft_is_written_and_what_the_markup_cannot_say_is_carried() {
    let (mut doc, _) = diacraft_odg::read(&style_sheets().bytes(), &diacraft_odg::Limits::default()).unwrap();
    let at = doc.styles.iter().position(|s| s.name_u.as_deref() == Some("Filled")).unwrap();
    {
        let style = std::sync::Arc::make_mut(doc.styles.get_mut(at).unwrap());
        let sheet = std::sync::Arc::make_mut(&mut style.sheet);
        sheet
            .set_cell(&diacraft_doc::CellAddr::direct("FillForegnd"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("#0000FF")))
            .unwrap();
        let formula = diacraft_doc::Cell::with_formula("0.01*2", diacraft_doc::Value::from_number(0.02));
        sheet.set_cell(&diacraft_doc::CellAddr::direct("LineWeight"), formula).unwrap();
    }
    let written = diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions::default()).unwrap();
    let styles = part(&written.bytes, "styles.xml").unwrap();
    assert!(styles.contains(r##"<style:style style:name="Filled" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill-color="#0000ff""##), "{styles}");
    let docfile = part(&written.bytes, "DiaCraft/document.xml").expect("the formula is carried");
    assert!(docfile.contains("diacraft:style name=\"Filled\"") && docfile.contains("LineWeight"), "{docfile}");
    let (back, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    let style = back.styles.iter().find(|s| s.name_u.as_deref() == Some("Filled")).unwrap();
    assert_eq!(style.sheet, doc.styles.get(at).unwrap().sheet, "the sheet reads back as the model has it");
}

/// A custom shape as a drawing program writes one, in the order the ODF 1.4 schema gives its children (`draw-custom-shape`: an accessible title and description, the event listeners, the glue
/// points, the text, the geometry): `before` goes before the text, `geometry_inner` in the geometry.
fn custom(style: &str, before: &str, geometry_inner: &str) -> String {
    // A path that names the equation only where the geometry holds one.
    let path =
        if geometry_inner.is_empty() { "M 5400 0 L 21600 0 21600 21600 5400 21600 Z N" } else { "M ?f0 0 L 21600 0 21600 21600 ?f0 21600 Z N" };
    format!(
        r#"<draw:custom-shape draw:style-name="{style}" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{before}<text:p>Hi</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="mso-spt100" draw:modifiers="5400" draw:enhanced-path="{path}">{geometry_inner}</draw:enhanced-geometry></draw:custom-shape>"#
    )
}

/// The schema's classes, each one construct, written as a drawing program writes it and as the schema takes it: a custom shape's glue points before its text; a geometry's
/// handles (`draw:handle-position`, the ranges); a page's forms before its shapes; a title, a description and event listeners first in a custom shape and in a rectangle; glue points on a
/// rectangle.
pub(crate) fn schema_vectors() -> Vec<(&'static str, Drawing)> {
    let gr1 = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#99ccff"/></style:style>"##;
    let glue = r#"<draw:glue-point draw:id="4" svg:x="0cm" svg:y="0cm" draw:escape-direction="up"/><draw:glue-point draw:id="5" svg:x="1cm" svg:y="0.5cm" draw:escape-direction="auto"/>"#;
    let handles = r#"<draw:equation draw:name="f0" draw:formula="$0 "/><draw:handle draw:handle-position="$0 top" draw:handle-range-x-minimum="0" draw:handle-range-x-maximum="10800"/>"#;
    let listeners = r#"<office:event-listeners><presentation:event-listener script:event-name="dom:click" presentation:action="next-page"/></office:event-listeners>"#;
    let titled = format!("<svg:title>A title</svg:title><svg:desc>A description</svg:desc>{listeners}");
    let forms = r#"<office:forms form:automatic-focus="false" form:apply-design-mode="false"/>"#;
    let mut with_forms = Drawing::a4().auto(gr1).page(&format!("{forms}{}", custom("gr1", "", "")));
    with_forms.ns.push_str(r#" xmlns:form="urn:oasis:names:tc:opendocument:xmlns:form:1.0""#);
    let a4 = || Drawing { version: "1.4".to_owned(), ..Drawing::a4() };
    with_forms.version = "1.4".to_owned();
    vec![
        ("a custom shape with glue points", a4().auto(gr1).page(&custom("gr1", glue, ""))),
        ("a custom shape with handles", a4().auto(gr1).page(&custom("gr1", "", handles))),
        ("a page with forms", with_forms),
        ("a custom shape with a title and listeners", a4().auto(gr1).page(&custom("gr1", &titled, ""))),
        (
            "a rectangle with a title, listeners and glue points",
            a4().auto(gr1).page(&format!(
                r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{titled}{glue}<text:p>Hi</text:p></draw:rect>"#
            )),
        ),
        ("everything on one custom shape", a4().auto(gr1).page(&custom("gr1", &format!("{titled}{glue}"), handles))),
    ]
}

/// Where the first `needle` stands in `hay`, which must hold it.
#[track_caller]
fn at(hay: &str, needle: &str) -> usize {
    hay.find(needle).unwrap_or_else(|| panic!("no {needle} in {hay}"))
}

/// The schema's four classes of in what the model writer writes, each on a vector the ODF 1.4 schema takes: a shape's glue points, title, description and
/// event listeners come before its text (`draw-custom-shape`, `draw-rect`); a handle says its place as ODF 1.4 asks (`draw:handle-position-x` and `-y`, the deprecated
/// `draw:handle-position` kept); a page's forms come before its shapes (`draw-page`). With `ODF_SCHEMA_RNG` set every part written is held to the schema too.
#[test]
fn what_the_model_writer_writes_is_in_the_order_and_form_the_schema_gives() {
    let vectors = schema_vectors();
    let written = |name: &str| {
        let drawing = &vectors.iter().find(|(n, _)| *n == name).unwrap().1;
        let made = visit(&drawing.bytes());
        assert!(made.notes.is_empty(), "{name}: {:?}", made.notes);
        if let Some(checked) = schema_errors(&made.bytes, name) {
            checked.unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        part(&made.bytes, "content.xml").unwrap()
    };
    let glued = written("a custom shape with glue points");
    assert!(at(&glued, "<draw:glue-point draw:id=\"4\"") < at(&glued, "<text:p"), "glue points before the text: {glued}");
    assert!(at(&glued, "<draw:glue-point draw:id=\"5\"") < at(&glued, "<text:p"), "{glued}");
    let handled = written("a custom shape with handles");
    assert!(
        handled.contains(r#"draw:handle-position="$0 top" draw:handle-range-x-minimum="0" draw:handle-range-x-maximum="10800" draw:handle-position-x="$0" draw:handle-position-y="top""#),
        "{handled}"
    );
    let forms = written("a page with forms");
    assert!(at(&forms, "<office:forms") < at(&forms, "<draw:custom-shape"), "the forms before the shapes: {forms}");
    for name in ["a custom shape with a title and listeners", "a rectangle with a title, listeners and glue points", "everything on one custom shape"]
    {
        let text = written(name);
        let (title, desc, listeners, text_p) =
            (at(&text, "<svg:title"), at(&text, "<svg:desc"), at(&text, "<office:event-listeners"), at(&text, "<text:p"));
        assert!(title < desc && desc < listeners && listeners < text_p, "{name}: {text}");
        if text.contains("<draw:glue-point") {
            assert!(listeners < at(&text, "<draw:glue-point") && at(&text, "<draw:glue-point") < text_p, "{name}: {text}");
        }
    }
}

/// Groups with no name whose own markup gives them data (the forms: a graphic style of its own, a glue point of its own), alone and beside a group with a name, as a
/// drawing program writes them.
pub(crate) fn group_vectors() -> Vec<(&'static str, Drawing)> {
    let gr = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffcc00"/></style:style>"##;
    let members = r#"<draw:rect svg:x="1cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/><draw:rect svg:x="4cm" svg:y="1cm" svg:width="2cm" svg:height="2cm"/>"#;
    let a4 = || Drawing { version: "1.4".to_owned(), ..Drawing::a4() };
    vec![
        ("an unnamed group with a style of its own", a4().auto(gr).page(&format!(r#"<draw:g draw:style-name="gr1">{members}</draw:g>"#))),
        (
            "an unnamed group with a glue point of its own",
            a4().page(&format!(r#"<draw:g><draw:glue-point draw:id="4" svg:x="1cm" svg:y="1cm" draw:escape-direction="auto"/>{members}</draw:g>"#)),
        ),
        (
            "two unnamed groups with styles of their own",
            a4().auto(gr).page(&format!(r#"<draw:g draw:style-name="gr1">{members}</draw:g><draw:g draw:style-name="gr1">{members}</draw:g>"#)),
        ),
    ]
}

/// A group with no name of two rectangles named `a` and `b`, at `x` centimetres.
fn unnamed_group(a: &str, b: &str, x: u32) -> String {
    format!(
        r#"<draw:g><draw:rect draw:name="{a}" svg:x="{x}cm" svg:y="3cm" svg:width="2cm" svg:height="2cm"/><draw:rect draw:name="{b}" svg:x="{}cm" svg:y="3cm" svg:width="2cm" svg:height="2cm"/></draw:g>"#,
        x + 3
    )
}

/// The package `bytes` with the first two `draw:g` of its `content.xml` swapped, as another program that reorders two groups writes it; every other entry as it is.
fn groups_swapped(bytes: &[u8]) -> Vec<u8> {
    let content = part(bytes, "content.xml").unwrap();
    let first = content.find("<draw:g>").unwrap();
    let first_end = content[first..].find("</draw:g>").unwrap() + first + "</draw:g>".len();
    let second = content[first_end..].find("<draw:g>").unwrap() + first_end;
    let second_end = content[second..].find("</draw:g>").unwrap() + second + "</draw:g>".len();
    let swapped = format!(
        "{}{}{}{}{}",
        &content[..first],
        &content[second..second_end],
        &content[first_end..second],
        &content[first..first_end],
        &content[second_end..]
    );
    let pkg = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let files: Vec<(String, Vec<u8>)> = pkg
        .entries()
        .filter(|e| e.name != "mimetype")
        .map(|e| {
            (
                e.name.clone(),
                if e.name == "content.xml" { swapped.clone().into_bytes() } else { pkg.part(&e.name).map(<[u8]>::to_vec).unwrap_or_default() },
            )
        })
        .collect();
    crate::build::write(Some(crate::build::DRAWING), &files)
}

/// The `UniqueID` (VSDX data an `.odg` carries for a group) of each group of the first page, in document order.
fn unique_ids(doc: &Document) -> Vec<Option<String>> {
    let page = doc.pages.get(0).unwrap();
    page.shapes.iter().filter(|s| !s.children.is_empty()).map(|s| s.raw.get("UniqueID").map(str::to_owned)).collect()
}

/// The concern 2: **the VSDX data of a group with no name is judged ambiguous, and left out with a note, when its stamp (its members' names) is the
/// stamp of more than one group with no name of the page read.** Two such groups whose members have the same names, reordered by another program, gave each other's data with no word: now
/// neither gets the other's, and the read says why. The control: groups whose members' names differ keep their own data through the same reorder (the item of the other group's place is
/// stale, as it always was), and in place.
#[test]
fn the_data_of_a_group_with_no_name_that_another_such_group_shares_a_stamp_with_is_left_out_with_a_note() {
    let carried = |groups: [(&str, &str); 2]| {
        let drawing = Drawing::a4().page(&format!("{}{}", unnamed_group(groups[0].0, groups[0].1, 1), unnamed_group(groups[1].0, groups[1].1, 10)));
        let (mut doc, _) = diacraft_odg::read(&drawing.bytes(), &diacraft_odg::Limits::default()).unwrap();
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        for (at, id) in ["{FIRST}", "{SECOND}"].iter().enumerate() {
            let group = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            group.raw = diacraft_doc::RawAttrs::from_pairs([("UniqueID", *id)]);
        }
        let written = diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions::default()).unwrap();
        assert!(part(&written.bytes, "DiaCraft/document.xml").unwrap().contains("{FIRST}"));
        written.bytes
    };
    // Members of the same names: neither group keeps data another program may have moved, reordered or not, and the read says so for each.
    let same = carried([("A", "B"), ("A", "B")]);
    for (what, bytes) in [("as written", same.clone()), ("reordered", groups_swapped(&same))] {
        let (doc, report) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
        assert_eq!(unique_ids(&doc), vec![None, None], "{what}: no group gets the other's data");
        assert_eq!(
            report.warnings.iter().filter(|w| w.contains("which group it was written for cannot be told")).count(),
            1,
            "{what}: {:?}",
            report.warnings
        );
        assert_eq!(report.carry.left_out, 2, "{what}: both items are left out");
    }
    // Members of other names: each keeps its own in place, and through the reorder neither takes the other's.
    let other = carried([("A", "B"), ("C", "D")]);
    let (doc, report) = diacraft_odg::read(&other, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(unique_ids(&doc), vec![Some("{FIRST}".to_owned()), Some("{SECOND}".to_owned())], "{:?}", report.warnings);
    let (doc, _) = diacraft_odg::read(&groups_swapped(&other), &diacraft_odg::Limits::default()).unwrap();
    let ids = unique_ids(&doc);
    assert!(
        ids.iter().all(|id| id.is_none()) || ids == vec![Some("{SECOND}".to_owned()), Some("{FIRST}".to_owned())],
        "never the other's data: {ids:?}"
    );
}

/// The concern 3, as closed it: **a group with no name whose own markup gives it a style or glue points is written** with them, from the model alone
/// (the style as the file named it, the points as elements, both read again as the cells the group has), so that no entry of DiaCraft's document file is made for what the markup says. **A
/// group with no name that holds data of DiaCraft's own** (here a cell the markup gives no group) **still cannot be written**: its entry would be found by its place, and the refusal is in
/// words and names the group (its number among the groups with no name of its page, and the page), so that a person can name it, and nothing is written. The way out holds: the same drawing
/// with the group named is written.
#[test]
fn a_group_with_no_name_whose_markup_gives_it_data_is_written_and_one_with_data_of_its_own_is_refused_in_words_that_name_it() {
    for (name, drawing) in group_vectors() {
        let (doc, _) = diacraft_odg::read(&drawing.bytes(), &diacraft_odg::Limits::default()).unwrap();
        let written = diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let content = part(&written.bytes, "content.xml").unwrap();
        let (styled, points) = (name.contains("style"), name.contains("glue point"));
        assert_eq!(content.contains("draw:style-name=\"gr1\""), styled, "{name}: the group's style is written with it: {content}");
        assert_eq!(content.matches("<draw:glue-point ").count(), usize::from(points), "{name}: the point is written with the group: {content}");
        // A cell of its own on each group: DiaCraft's data, which has no key to be found by.
        let mut own = doc.clone();
        let page = std::sync::Arc::make_mut(own.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            std::sync::Arc::make_mut(&mut shape.sheet)
                .cells
                .insert(diacraft_doc::CellName::new("Cost"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")));
        }
        let refused = diacraft_odg::write_with(&own, None, &diacraft_odg::WriteOptions::default()).unwrap_err();
        let words = refused.to_string();
        assert!(words.contains("a group with no name") && words.contains("name the group"), "{name}: {words}");
        assert!(words.contains("it is the group with no name numbered 1 on the page \"page1\""), "{name}: the group is named: {words}");
        assert_eq!(refused.class(), "write_unsupported", "{name}");
        // Named, it is written.
        let page = std::sync::Arc::make_mut(own.pages.get_mut(0).unwrap());
        for at in 0..page.shapes.len() {
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            shape.name_u = Some(std::sync::Arc::from(format!("Group {at}").as_str()));
            shape.name = shape.name_u.clone();
        }
        assert!(diacraft_odg::write_with(&own, None, &diacraft_odg::WriteOptions::default()).is_ok(), "{name}: named, the groups are written");
    }
}

/// The other difference classes, each one construct as a drawing program writes it, for the tests that tell whether a visit loses what they hold or writes it differently:
/// `meta.xml`'s fields, a master page with no layer set, automatic styles of names a program gives them (`gr5`, `P3`) and a text style's sizes of the three scripts.
pub(crate) fn class_vectors() -> Vec<(&'static str, Drawing)> {
    let meta = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" ",
        "xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"1.4\"><office:meta>",
        "<meta:initial-creator>Ann Author</meta:initial-creator><meta:creation-date>2024-01-02T03:04:05</meta:creation-date><dc:creator>Bob Editor</dc:creator>",
        "<dc:date>2024-02-03T04:05:06</dc:date><meta:editing-cycles>7</meta:editing-cycles><meta:editing-duration>PT1H2M3S</meta:editing-duration>",
        "<meta:generator>A Drawing Program/1.0</meta:generator><meta:document-statistic meta:object-count=\"1\"/></office:meta></office:document-meta>"
    );
    let gr = r##"<style:style style:name="gr5" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#66cc66"/></style:style>"##;
    let p3 = r#"<style:style style:name="P3" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="14pt" style:font-size-asian="20pt" style:font-size-complex="22pt"/></style:style>"#;
    let shape = r#"<draw:custom-shape draw:style-name="gr5" draw:text-style-name="P3" svg:x="2cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"><text:p text:style-name="P3">Hi</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#;
    let a4 = || Drawing { version: "1.4".to_owned(), ..Drawing::a4() };
    let mut with_meta = a4().auto(gr).page(&rect("gr5", "2cm"));
    with_meta.meta = Some(meta.to_owned());
    let mut no_layers = a4().page(&rect("", "2cm"));
    no_layers.masters = r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#.to_owned();
    let mut picture = a4()
        .common(r##"<style:style style:name="Pictures" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="none"/></style:style>"##)
        .auto(r#"<style:style style:name="gr9" style:family="graphic" style:parent-style-name="Pictures"/>"#)
        .page(r#"<draw:frame draw:style-name="gr9" svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#);
    picture.files.push(("Pictures/a.png".to_owned(), png_2x2()));
    vec![
        ("a picture on a style sheet", picture),
        ("meta.xml fields", with_meta),
        ("a master page with no layer set", no_layers),
        ("automatic styles of a program's names", a4().auto(gr).auto(p3).page(shape)),
    ]
}

/// The vector called `name` of [`class_vectors`], as a package.
fn class_vector(name: &str) -> Vec<u8> {
    class_vectors().into_iter().find(|(n, _)| *n == name).map(|(_, d)| d.bytes()).unwrap()
}

/// A picture on a style sheet is written with the style sheet as the parent of its own style (counted 279 visits refused for it): another program draws it with
/// what the style sheet gives, and the drawing reads back as it was.
#[test]
fn a_picture_on_a_style_sheet_is_written_with_the_style_sheet_as_its_parent() {
    let source = class_vector("a picture on a style sheet");
    let written = visit(&source);
    let content = part(&written.bytes, "content.xml").unwrap();
    assert!(content.contains(r#"<style:style style:name="gr9" style:family="graphic" style:parent-style-name="Pictures"/>"#), "{content}");
    assert_eq!(entry(&written.bytes, "Pictures/a.png"), Some(png_2x2()));
    let (before, _) = diacraft_odg::read(&source, &diacraft_odg::Limits::default()).unwrap();
    let (after, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(model_same(&before, &after), Ok(()));
    assert_eq!(drawing_differences(&source, &written.bytes), Vec::<String>::new());
}

/// What a paragraph's automatic style held that no cell reads is written with it (found the sizes of the Asian and complex scripts of 406 visits dropped with no
/// word): the sizes of the other scripts, which no cell holds, stay in the paragraph style's text properties when the size the cells hold goes to the run.
#[test]
fn a_paragraph_style_keeps_what_no_cell_reads() {
    let written = visit(&class_vector("automatic styles of a program's names"));
    let content = part(&written.bytes, "content.xml").unwrap();
    let p3 = &content[content.find(r#"<style:style style:name="P3""#).unwrap()..];
    let p3 = &p3[..p3.find("</style:style>").unwrap()];
    assert!(p3.contains(r#"style:font-size-asian="20pt""#) && p3.contains(r#"style:font-size-complex="22pt""#), "{p3}");
    let (back, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    let kept = back.ext.get().unwrap().odf.styles.iter().find(|k| &*k.name == "P3").map(|k| format!("{:?}", k.element)).unwrap();
    assert!(kept.contains("font-size-asian") && kept.contains("20pt"), "{kept}");
}

/// A text box whose paragraph names the paragraph style `P4`, written in content.xml's automatic styles beside `more`.
fn text_box_on(styles: &str, body: &str) -> Drawing {
    let mut d = Drawing { version: "1.4".to_owned(), ..Drawing::a4() }
        .auto(styles)
        .page(&format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="8cm" svg:height="3cm"><draw:text-box>{body}</draw:text-box></draw:frame>"#));
    d.ns.push_str(r#" xmlns:s2="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#);
    d
}

/// The security scan's first finding on the remainder of a text style: **an attribute is written once on an element**, though the kept style holds two `text-properties`
/// of one name, or the attribute under two prefixes of one namespace (each prefix is read through the declarations in force, never by its spelling): the part written is well formed and
/// reads again.
#[test]
fn a_kept_text_style_holding_an_attribute_twice_is_written_with_one_copy() {
    let p4 = r#"<style:style style:name="P4" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="14pt" style:font-size-asian="20pt" s2:font-size-complex="23pt"/><style:text-properties style:font-size-asian="21pt" style:font-size-complex="22pt"/></style:style>"#;
    let source = text_box_on(p4, r#"<text:p text:style-name="P4">Hi</text:p>"#).bytes();
    let written = visit(&source);
    let content = part(&written.bytes, "content.xml").unwrap();
    // The writer writes the style namespace under the least prefix the part binds to it (`s2` here).
    let p4 = &content[content.find(r#":name="P4""#).unwrap_or_else(|| panic!("no P4: {content}"))..];
    let p4 = &p4[..p4.find(":style>").unwrap()];
    assert_eq!(p4.matches("font-size-asian=").count(), 1, "one copy: {p4}");
    assert_eq!(p4.matches("font-size-complex=").count(), 1, "one copy under one prefix: {p4}");
    diacraft_container::xml::parse(content.as_bytes()).expect("the part is well formed");
    assert!(diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).is_ok(), "the file reads again");
}

/// A drawing whose one paragraph holds `n` runs, each naming a text style of its own; `kept` says whether each style holds a property no cell reads (a kept remainder).
fn runs_on_styles(n: usize, kept: bool) -> Drawing {
    let extra = if kept { r#" style:font-size-asian="20pt""# } else { "" };
    let styles: String = (1..=n)
        .map(|i| {
            format!(
                r#"<style:style style:name="T{i}" style:family="text"><style:text-properties fo:font-size="{}pt"{extra}/></style:style>"#,
                10 + i % 7
            )
        })
        .collect();
    let runs: String = (1..=n).map(|i| format!(r#"<text:span text:style-name="T{i}">a</text:span>"#)).collect();
    text_box_on(&styles, &format!("<text:p>{runs}</text:p>"))
}

/// The steps a model save of `drawing` counts (`WriteWork::cells_compared`).
fn save_steps(drawing: &Drawing) -> u64 {
    visit(&drawing.bytes()).work.cells_compared
}

/// The security scan's second finding: **the kept text styles are found by a map made once for the save, and charged**: each style written under its own name looked for its
/// kept style by a walk of every kept style, uncharged (50,000 runs on 50,000 styles walked 2.5 billion). Counted: 50,000 such styles cost within a small constant of the same drawing whose
/// styles keep nothing, and each style's walk is charged (more steps than the yardstick, by at least a step a style).
#[test]
fn fifty_thousand_kept_text_styles_are_found_by_a_map_made_once() {
    const N: usize = 50_000;
    let case = save_steps(&runs_on_styles(N, true));
    let yardstick = save_steps(&runs_on_styles(N, false));
    println!("COUNTED kept text styles: {case} steps against {yardstick}");
    // The map is made once (a step for each kept style) and each style's walk is charged before it is made (its children, its attributes and its properties' attributes: four here).
    assert!(case >= yardstick + 3 * N as u64, "the map and each kept style's walk are charged: {case} against {yardstick}");
    assert!(case <= 2 * yardstick + 1024, "within a small constant of the yardstick: {case} against {yardstick}");
}

/// The time of the same (`#[ignore]`: run by hand with `--nocapture`): the case beside its yardstick.
#[test]
#[ignore = "timed: run by hand"]
fn fifty_thousand_kept_text_styles_timed() {
    const N: usize = 50_000;
    let time = |d: &Drawing| {
        let bytes = d.bytes();
        let (doc, _) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
        let start = std::time::Instant::now();
        diacraft_odg::write_with(&doc, None, &diacraft_odg::WriteOptions { content_unchanged: true, ..diacraft_odg::WriteOptions::default() })
            .unwrap();
        start.elapsed().as_secs_f64()
    };
    let (case, yardstick) = (time(&runs_on_styles(N, true)), time(&runs_on_styles(N, false)));
    println!("TIMED kept text styles: {case:.3} s against {yardstick:.3} s");
    assert!(case < 3.0 * yardstick + 1.0, "{case} against {yardstick}");
}

/// Three difference classes of that a visit **writes differently and loses nothing by** (for a ruling): `meta.xml`'s fields come back each
/// one, in the schema's order where the file had its own (`office:meta` is an interleave in the ODF 1.4 schema); a master page with no layer set comes back with the five layers the
/// package is made with (the drawing's pages, shapes and cells are the same); a folder and an entry the manifest lists with an empty media type come back, every entry with its bytes.
/// Each is pinned, so that a change of what is written differently shows.
#[test]
fn the_classes_written_differently_lose_nothing() {
    let entries = |bytes: &[u8]| -> Vec<(String, Vec<u8>)> {
        let pkg = diacraft_odg::package::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
        pkg.entries().filter(|e| !e.name.ends_with('/')).map(|e| (e.name.clone(), pkg.part(&e.name).unwrap_or_default().to_vec())).collect()
    };
    // meta.xml: every field, the order the only difference.
    let source = class_vector("meta.xml fields");
    let written = visit(&source);
    let meta = part(&written.bytes, "meta.xml").unwrap();
    for field in [
        "Ann Author",
        "2024-01-02T03:04:05",
        "Bob Editor",
        "2024-02-03T04:05:06",
        "<meta:editing-cycles>7<",
        "PT1H2M3S",
        "A Drawing Program/1.0",
        "meta:object-count=\"1\"",
    ] {
        assert!(meta.contains(field), "{field}: {meta}");
    }
    let diff = diacraft_odg::compare_content(&source, &written.bytes, &diacraft_odg::Limits::default()).unwrap();
    let meta_diffs: Vec<String> = diff.shown.iter().filter(|d| d.part == "meta.xml").map(|d| format!("{} {:?}", d.path, d.found)).collect();
    // The order of the fields (and the generator) is no content so the part is the same: before it, the one difference was `/document-meta[0]/meta[0] Order`.
    assert!(meta_diffs.is_empty(), "the order of the fields is no content: {meta_diffs:?}");
    // No layer set: the package's five come back; the drawing is the same.
    let source = class_vector("a master page with no layer set");
    let written = visit(&source);
    assert!(part(&written.bytes, "styles.xml").unwrap().contains("<draw:layer-set>"), "the layer set the package is made with");
    let (before, _) = diacraft_odg::read(&source, &diacraft_odg::Limits::default()).unwrap();
    let (after, _) = diacraft_odg::read(&written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(model_same(&before, &after), Err(vec!["ext".to_owned()]), "only the document's layers differ");
    assert_eq!(after.ext.get().unwrap().odf.layers.len(), 5);
    assert_eq!(before.pages, after.pages, "the pages and their shapes are the same");
    // Manifest rows: every entry of the source comes back with its bytes, whatever its row said.
    let mut drawing = Drawing::a4().page(&rect("", "2cm"));
    drawing.files.push(("Configurations2/".to_owned(), Vec::new()));
    drawing.files.push(("Configurations2/accelerator/current.xml".to_owned(), b"<a/>".to_vec()));
    let source = drawing.bytes();
    let written = visit(&source);
    let kept = entries(&written.bytes);
    for (name, bytes) in entries(&source) {
        if matches!(name.as_str(), "content.xml" | "styles.xml" | "meta.xml" | "settings.xml" | "META-INF/manifest.xml" | "mimetype") {
            continue;
        }
        assert!(kept.iter().any(|(n, b)| *n == name && *b == bytes), "{name} comes back with its bytes");
    }
    let diff = diacraft_odg::compare_content(&source, &written.bytes, &diacraft_odg::Limits::default()).unwrap();
    assert!(diff.shown.iter().all(|d| !matches!(d.found, diacraft_container::canon::Found::MissingPart)), "no part is missing: {:?}", diff.shown);
}
