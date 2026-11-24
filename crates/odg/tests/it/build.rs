//! Honest packages, written by hand from the ODF standard (Part 2 for the package and the manifest, Part 3 for the parts' roots and the
//! drawing's elements) and put together by the container's writer: `mimetype` first and stored, the parts after it, and a manifest that
//! lists them. The XML is the smallest that says what a test needs; it is no producer's output.

use diacraft_container::zip::{ArchiveWriter, Method};

/// A drawing's media type.
pub const DRAWING: &str = "application/vnd.oasis.opendocument.graphics";
/// A drawing template's media type.
pub const TEMPLATE: &str = "application/vnd.oasis.opendocument.graphics-template";
/// A presentation's media type.
pub const PRESENTATION: &str = "application/vnd.oasis.opendocument.presentation";

/// The namespace declarations of a part's root.
pub const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:script="urn:oasis:names:tc:opendocument:xmlns:script:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink""#
);

/// `content.xml` around `body`, the pages of `office:drawing`, indented as a writer might.
pub fn content(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\">\n <office:body>\n  \
         <office:drawing>{body}\n  </office:drawing>\n </office:body>\n</office:document-content>\n"
    )
}

/// A page holding `shapes`.
pub fn page(name: &str, shapes: &str) -> String {
    format!("\n   <draw:page draw:name=\"{name}\" draw:master-page-name=\"Default\">{shapes}\n   </draw:page>")
}

/// A custom shape at `x` (a length as written).
pub fn shape(name: &str, x: &str) -> String {
    format!(
        "\n    <draw:custom-shape draw:name=\"{name}\" svg:x=\"{x}\" svg:y=\"1cm\" svg:width=\"2cm\" svg:height=\"1cm\">\n     \
         <text:p/>\n     <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"rectangle\"/>\n    </draw:custom-shape>"
    )
}

/// `n` custom shapes named `s1` to `sn`, the `k`-th at `k` cm.
pub fn shapes(n: usize) -> String {
    (1..=n).map(|k| shape(&format!("s{k}"), &format!("{k}cm"))).collect()
}

/// The other three XML parts, each with its fixed root.
pub fn styles() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:styles/></office:document-styles>\n"
    )
}

pub fn meta() -> String {
    concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        r#"<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
        r#"xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.3">"#,
        "<office:meta><meta:generator>TestWriter/1</meta:generator></office:meta></office:document-meta>\n"
    )
    .to_owned()
}

pub fn settings() -> String {
    concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        r#"<office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" office:version="1.3">"#,
        "<office:settings/></office:document-settings>\n"
    )
    .to_owned()
}

/// The four XML parts of a drawing whose pages are `body`.
pub fn files(body: &str) -> Vec<(String, Vec<u8>)> {
    vec![
        ("content.xml".to_owned(), content(body).into_bytes()),
        ("styles.xml".to_owned(), styles().into_bytes()),
        ("meta.xml".to_owned(), meta().into_bytes()),
        ("settings.xml".to_owned(), settings().into_bytes()),
    ]
}

/// A manifest whose root row says `media`, with one row for each of `rows` (full path, media type).
pub fn manifest(media: &str, rows: &[(&str, &str)]) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
        "\n"
    ));
    out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"{media}\"/>\n"));
    for (path, media_type) in rows {
        out.push_str(&format!(" <manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{media_type}\"/>\n"));
    }
    out.push_str("</manifest:manifest>\n");
    out
}

/// The media type a manifest row gives an entry of this name.
fn media_type_of(name: &str) -> &'static str {
    if name.ends_with('/') {
        ""
    } else if name.ends_with(".png") {
        "image/png"
    } else {
        "text/xml"
    }
}

/// `files`, then `META-INF/manifest.xml` with a row for each of them and a root row of `media`.
pub fn with_manifest(media: &str, mut files: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let rows: Vec<(String, &str)> = files.iter().map(|(name, _)| (name.clone(), media_type_of(name))).collect();
    let rows: Vec<(&str, &str)> = rows.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    files.push(("META-INF/manifest.xml".to_owned(), manifest(media, &rows).into_bytes()));
    files
}

/// An archive of `mimetype` holding `media` (first and stored), when given, then `entries` in order: a name ending in `/` a folder,
/// a `.png` stored, anything else deflated.
pub fn write(media: Option<&str>, entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = ArchiveWriter::new();
    if let Some(media) = media {
        w.add("mimetype", media.as_bytes(), Method::Stored).unwrap();
    }
    for (name, data) in entries {
        if name.ends_with('/') {
            w.add_directory(name).unwrap();
        } else if name.ends_with(".png") {
            w.add(name, data, Method::Stored).unwrap();
        } else {
            w.add(name, data, Method::Deflated).unwrap();
        }
    }
    w.finish().unwrap()
}

/// A package of `media` holding `files` and a manifest listing them.
pub fn package(media: &str, files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    write(Some(media), &with_manifest(media, files))
}

/// A drawing whose pages are `body`.
pub fn drawing(body: &str) -> Vec<u8> {
    package(DRAWING, files(body))
}

/// `files` with the bytes of `name` replaced.
pub fn replace(mut files: Vec<(String, Vec<u8>)>, name: &str, bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    for (n, data) in &mut files {
        if n == name {
            *data = bytes.clone();
        }
    }
    files
}
