//! `cargo fuzz run odg_tier2`: DiaCraft's own data in a drawing, read and saved. A first byte of 0 makes the rest the text of `DiaCraft/document.xml` of an
//! otherwise honest package; 1 makes the rest, split at its first line feed, the value of an attribute of DiaCraft's extension on a rectangle's own automatic
//! style (the line before it names which: 0 `cells`, 1 a section `s-User`, 2 `text`, 3 `gone`, 4 `name`, 5 `master`, 6 `id`, 7 `version`, 8 to 12 the fingerprints
//! `fp-text`, `fp-name`, `fp-master`, `fp-s-Geometry-0` and `fp-c-EndArrow`, any other byte a name of its own); 2 puts the rest in the document file's root as one more
//! child, among masters the fuzzer cannot break; 3 gives the style one valid item of each kind tier 3 judges (a text, a name, a link to a master, a `Geometry` section,
//! a cell `EndArrow`) and puts the rest, as above, in the fingerprint the line before it names (tier 3: a fingerprint of any length and any bytes, and the
//! item it judges stale or fresh, read and saved); 4 puts the rest, split as 1 is, in the attribute the line names of the carrier of VSDX data on a shape (0 `vsdx`,
//! 1 `foreign`, 2 `fp-vsdx`, 3 `fp-foreign`, 4 `fp-format`, with a valid text item beside it); 5 makes the rest the inside of the document file's `parts` item (records of parts,
//! custom properties, blob rows), beside three honest `DiaCraft/` entries the manifest lists with the folder's row; any other first byte reads the bytes as they are. A read
//! gives a document or an error of a known class, and what it gives holds the model's caps; **an unedited save of what was read is the file, byte for byte, where a
//! copy keeps the archive whole, and the copy of it where it does not** (a local header that is not its row's, bytes no header announces,
//! the copy's account says which), the baseline read the save makes reads the same model, whatever the extension held, and a model with one
//! shape moved saves, or is refused in words, and reads again. Anything else is a crash the fuzzer reports.
#![no_main]

use diacraft_container::zip::{Archive, ArchiveWriter, Entry, Method};
use diacraft_odg::{Form, Limits, OriginalPackage, WriteOptions, copy_account, copy_unchanged, read_partial, write_with};
use libfuzzer_sys::fuzz_target;

/// The package with the namespace of DiaCraft's own markup spelled as builds made before the id moved to `diacraft.app` spelled it (APPID), in every part that holds it. The readers read
/// both spellings as one, so that every law of this target holds of the one as of the other. `None` for bytes that are no package.
fn spelled_as_before(bytes: &[u8]) -> Option<Vec<u8>> {
    const CURRENT: &str = "urn:app.diacraft:extension";
    const BEFORE: &str = "urn:io.github.one_bit.diacraft:extension";
    let mut archive = Archive::read(bytes, &diacraft_container::Limits::default()).ok()?;
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let mut out = ArchiveWriter::new();
    for (at, name) in names.iter().enumerate() {
        let part = archive.inflate(at, &mut diacraft_container::Work::default()).ok()?;
        let part = match String::from_utf8(part) {
            Ok(text) => text.replace(CURRENT, BEFORE).into_bytes(),
            Err(bytes) => bytes.into_bytes(),
        };
        out.add(name, &part, if name == "mimetype" || name.ends_with('/') { Method::Stored } else { Method::Deflated }).ok()?;
    }
    out.finish().ok()
}

/// Whether this input is met in the spelling of before: one input in two, by the parity of its bytes.
fn as_before(data: &[u8]) -> bool {
    data.iter().fold(0u8, |a, b| a ^ b) & 1 == 1
}

const MEDIA: &str = "application/vnd.oasis.opendocument.graphics";
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:diacraft="urn:app.diacraft:extension""#
);
const STYLES: &str = concat!(
    r#"<office:document-styles "#,
    "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" ",
    "xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" ",
    "xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.3\"><office:automatic-styles>",
    "<style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout>",
    "</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set>",
    "<style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
);

/// `STYLES` with six common styles in `office:styles`: `S1` on the root, `S2` on `S1` and so on.
const STYLES_NUMBERED: &str = concat!(
    r#"<office:document-styles "#,
    "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" ",
    "xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" ",
    "xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.3\"><office:styles>",
    "<style:style style:name=\"S1\" style:family=\"graphic\"/>",
    "<style:style style:name=\"S2\" style:family=\"graphic\" style:parent-style-name=\"S1\"/>",
    "<style:style style:name=\"S3\" style:family=\"graphic\" style:parent-style-name=\"S2\"/>",
    "<style:style style:name=\"S4\" style:family=\"graphic\" style:parent-style-name=\"S3\"/>",
    "<style:style style:name=\"S5\" style:family=\"graphic\" style:parent-style-name=\"S4\"/>",
    "<style:style style:name=\"S6\" style:family=\"graphic\" style:parent-style-name=\"S5\"/>",
    "</office:styles><office:automatic-styles>",
    "<style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\"/></style:page-layout>",
    "</office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set>",
    "<style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
);

/// The package: `content.xml` made of `style_attrs` on the style `gr1` of two rectangles and `file`, when given, as the document file.
fn package(style_attrs: &str, file: Option<&[u8]>) -> Option<Vec<u8>> {
    package_with(style_attrs, file, &[])
}

/// [`package`] with [`STYLES_NUMBERED`], and the rectangles' automatic style on `S3`.
fn package_numbered(file: &[u8]) -> Option<Vec<u8>> {
    package_full("", Some(file), &[], STYLES_NUMBERED, " style:parent-style-name=\"S3\"")
}

/// [`package`] with `blobs` as entries of DiaCraft's folder, each listed in the manifest, and the folder's own row (the shape of a file that carries parts).
fn package_with(style_attrs: &str, file: Option<&[u8]>, blobs: &[(&str, &[u8])]) -> Option<Vec<u8>> {
    package_full(style_attrs, file, blobs, STYLES, "")
}

/// [`package_with`] with `styles` as `styles.xml` and `parent` (an attribute, or nothing) on the style `gr1`.
fn package_full(style_attrs: &str, file: Option<&[u8]>, blobs: &[(&str, &[u8])], styles: &str, parent: &str) -> Option<Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>\
         <style:style style:name=\"gr1\" style:family=\"graphic\"{parent}><style:graphic-properties draw:fill=\"solid\" {style_attrs}/></style:style></office:automatic-styles>\
         <office:body><office:drawing><draw:page draw:name=\"p\" draw:master-page-name=\"Default\">\
         <draw:custom-shape draw:style-name=\"gr1\" draw:name=\"a\" svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"4cm\" svg:height=\"2cm\"><text:p/>\
         <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"/></draw:custom-shape>\
         <draw:custom-shape draw:style-name=\"gr1\" draw:name=\"b\" svg:x=\"6cm\" svg:y=\"1cm\" svg:width=\"4cm\" svg:height=\"2cm\"><text:p/>\
         <draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 Z N\"/></draw:custom-shape>\
         </draw:page></office:drawing></office:body></office:document-content>"
    );
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\
         <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MEDIA}\"/>\
         <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
         <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>"
    );
    if file.is_some() {
        manifest.push_str(
            "<manifest:file-entry manifest:full-path=\"DiaCraft/document.xml\" manifest:media-type=\"application/vnd.diacraft.document+xml\"/>",
        );
    }
    for (name, _) in blobs {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"application/octet-stream\"/>"));
    }
    if !blobs.is_empty() {
        manifest.push_str("<manifest:file-entry manifest:full-path=\"DiaCraft/\" manifest:media-type=\"application/vnd.diacraft.parts\"/>");
    }
    manifest.push_str("</manifest:manifest>");
    let mut w = ArchiveWriter::new();
    w.add("mimetype", MEDIA.as_bytes(), Method::Stored).ok()?;
    w.add("content.xml", content.as_bytes(), Method::Deflated).ok()?;
    w.add("styles.xml", styles.as_bytes(), Method::Deflated).ok()?;
    if let Some(file) = file {
        w.add("DiaCraft/document.xml", file, Method::Deflated).ok()?;
    }
    for (name, data) in blobs {
        w.add(name, data, Method::Deflated).ok()?;
    }
    w.add("META-INF/manifest.xml", manifest.as_bytes(), Method::Deflated).ok()?;
    w.finish().ok()
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "&#10;")
}

/// A root with masters that hold what the fuzzer puts, and an element of its own.
const FILE_HEAD: &str = "<diacraft:document xmlns:diacraft=\"urn:app.diacraft:extension\" version=\"2\">";

/// What an unedited save of `bytes` is, by the account of its copy: **the file byte for byte when the account is exact**, and
/// otherwise the copy of it, which is the same entries under headers and a directory the rows give, is not the file, is itself exact, and holds every entry
/// of the file in the bytes the archive holds. A twin of `unedited_save_is_the_file_or_its_copy` in `crates/odg/tests/it/local_name.rs`, which this crate
/// cannot call: keep them as one.
fn unedited_save_is_the_file_or_its_copy(bytes: &[u8], saved: &[u8], limits: &Limits) {
    // A flat drawing has no entries to copy: its unedited save is its bytes. A package that was read has an account.
    let account = match Form::of(bytes) {
        Form::Flat => None,
        Form::Package => Some(copy_account(bytes, limits).unwrap_or_else(|e| panic!("a package that was read has no account of its copy: {e}"))),
    };
    let Some(account) = account.filter(|a| !a.is_exact()) else {
        assert_eq!(saved, bytes, "an unedited save of an archive a copy keeps whole is the file");
        return;
    };
    let copy = copy_unchanged(bytes, limits).unwrap_or_else(|e| panic!("a package that was saved cannot be copied: {e} ({account:?})"));
    assert_eq!(saved, copy, "an unedited save of an archive a copy does not keep whole is its copy: {account:?}");
    assert_ne!(saved, bytes, "an account that is not exact says the copy is not the file: {account:?}");
    assert!(copy_account(saved, limits).is_ok_and(|a| a.is_exact()), "a copy of the copy is the copy");
    // Nothing is inflated: a damaged picture's bytes are copied as they are.
    let (was, now) = (Archive::read(bytes, &limits.container).unwrap(), Archive::read(saved, &limits.container).unwrap());
    let raw = |bytes: &[u8], e: &Entry| {
        usize::try_from(e.compressed).ok().and_then(|n| bytes.get(e.data_offset..e.data_offset.checked_add(n)?)).map(<[u8]>::to_vec)
    };
    for entry in was.entries() {
        let at = now.position(&entry.name).unwrap_or_else(|| panic!("the entry {} is in the copy", entry.name));
        let copied = &now.entries()[at];
        assert_eq!(
            (entry.method, entry.crc, entry.compressed, entry.size),
            (copied.method, copied.crc, copied.compressed, copied.size),
            "{}",
            entry.name
        );
        assert!(raw(bytes, entry).is_some() && raw(bytes, entry) == raw(saved, copied), "{}: the bytes are the file's", entry.name);
    }
    assert_eq!(was.entries().len(), now.entries().len(), "and there are no others");
}

fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    let bytes = match data.split_first() {
        Some((0, rest)) => match package("", Some(rest)) {
            Some(b) => b,
            None => return,
        },
        Some((1, rest)) => {
            let (head, value) = match rest.iter().position(|b| *b == b'\n') {
                Some(at) => (&rest[..at], &rest[at + 1..]),
                None => (&rest[..0], rest),
            };
            let local = match head.first().copied().unwrap_or(0) {
                0 => "cells".to_owned(),
                1 => "s-User".to_owned(),
                2 => "text".to_owned(),
                3 => "gone".to_owned(),
                4 => "name".to_owned(),
                5 => "master".to_owned(),
                6 => "id".to_owned(),
                7 => "version".to_owned(),
                8 => "fp-text".to_owned(),
                9 => "fp-name".to_owned(),
                10 => "fp-master".to_owned(),
                11 => "fp-s-Geometry-0".to_owned(),
                12 => "fp-c-EndArrow".to_owned(),
                _ => String::from_utf8_lossy(head).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect(),
            };
            if local.is_empty() || !local.starts_with(|c: char| c.is_ascii_alphabetic()) {
                return;
            }
            let attrs = format!("diacraft:version=\"1\" diacraft:id=\"7\" diacraft:{local}=\"{}\"", escape(&String::from_utf8_lossy(value)));
            match package(&attrs, None) {
                Some(b) => b,
                None => return,
            }
        }
        Some((3, rest)) => {
            let (head, value) = match rest.iter().position(|b| *b == b'\n') {
                Some(at) => (&rest[..at], &rest[at + 1..]),
                None => (&rest[..0], rest),
            };
            let key = match head.first().copied().unwrap_or(0) % 6 {
                0 => "fp-text",
                1 => "fp-name",
                2 => "fp-master",
                3 => "fp-s-Geometry-0",
                4 => "fp-c-EndArrow",
                _ => "fp-s-Geometry-1",
            };
            // One valid item of each kind tier 3 judges, and a fingerprint of the fuzzer's making for one of them.
            let items = concat!(
                "diacraft:version=\"1\" diacraft:id=\"7\" diacraft:name=\"N\" diacraft:master=\"2\" ",
                "diacraft:text=\"&lt;Text&gt;&lt;cp IX=&quot;0&quot;/&gt;hi&lt;/Text&gt;\" ",
                "diacraft:cells=\"&lt;Sheet&gt;&lt;Cell N=&quot;EndArrow&quot; V=&quot;254&quot;/&gt;&lt;/Sheet&gt;\" ",
                "diacraft:s-Geometry-0=\"&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;0&quot;&gt;&lt;Cell N=&quot;NoFill&quot; V=&quot;1&quot;/&gt;&lt;/Section&gt;&lt;/Sheet&gt;\" ",
                "diacraft:s-Geometry-1=\"&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;1&quot;/&gt;&lt;/Sheet&gt;\""
            );
            let attrs = format!("{items} diacraft:{key}=\"{}\"", escape(&String::from_utf8_lossy(value)));
            match package(&attrs, None) {
                Some(b) => b,
                None => return,
            }
        }
        Some((4, rest)) => {
            let (head, value) = match rest.iter().position(|b| *b == b'\n') {
                Some(at) => (&rest[..at], &rest[at + 1..]),
                None => (&rest[..0], rest),
            };
            let key = match head.first().copied().unwrap_or(0) % 5 {
                0 => "vsdx",
                1 => "foreign",
                2 => "fp-vsdx",
                3 => "fp-foreign",
                _ => "fp-format",
            };
            // A text item to stamp, and a fragment of each carried kind the fuzzer may replace; the other attributes are honest.
            let honest = concat!(
                "diacraft:version=\"1\" diacraft:id=\"7\" diacraft:text=\"&lt;Text&gt;&lt;cp IX=&quot;0&quot;/&gt;hi&lt;/Text&gt;\" ",
                "diacraft:vsdx=\"&lt;diacraft:shape xmlns:diacraft=&quot;urn:app.diacraft:extension&quot; id=&quot;7&quot; type=&quot;Shape&quot;&gt;&lt;diacraft:attr name=&quot;A&quot; value=&quot;b&quot;/&gt;&lt;/diacraft:shape&gt;\" ",
                "diacraft:foreign=\"&lt;diacraft:shape xmlns:diacraft=&quot;urn:app.diacraft:extension&quot;&gt;&lt;diacraft:foreign rel-id=&quot;rId2&quot;&gt;&lt;diacraft:attr name=&quot;ForeignType&quot; value=&quot;Object&quot;/&gt;&lt;/diacraft:foreign&gt;&lt;/diacraft:shape&gt;\""
            );
            let attrs = if key == "vsdx" || key == "foreign" {
                format!("diacraft:version=\"1\" diacraft:id=\"7\" diacraft:{key}=\"{}\"", escape(&String::from_utf8_lossy(value)))
            } else {
                format!("{honest} diacraft:{key}=\"{}\"", escape(&String::from_utf8_lossy(value)))
            };
            match package(&attrs, None) {
                Some(b) => b,
                None => return,
            }
        }
        Some((5, rest)) => {
            let file = format!(
                "{FILE_HEAD}<diacraft:vsdx of=\"parts\"><diacraft:document xmlns:diacraft=\"urn:app.diacraft:extension\">{}</diacraft:document></diacraft:vsdx></diacraft:document>",
                String::from_utf8_lossy(rest)
            );
            let blobs: [(&str, &[u8]); 3] =
                [("DiaCraft/blob0.bin", b"vba bytes"), ("DiaCraft/blob1.bin", b"ole bytes!"), ("DiaCraft/blob2.xml", b"<a/>")];
            match package_with("", Some(file.as_bytes()), &blobs) {
                Some(b) => b,
                None => return,
            }
        }
        Some((6, rest)) => {
            // The `style` entries that say an `ID`: a pair of bytes each (the style, the number), the most that fit in a drawing of six style sheets and a few more.
            let mut file = String::from(FILE_HEAD);
            for pair in rest.chunks_exact(2).take(64) {
                let (name, number) = (format!("S{}", 1 + pair[0] % 7), pair[1]);
                let id = match number % 8 {
                    0 => "seven".to_owned(),
                    1 => "4294967295".to_owned(),
                    2 => "0".to_owned(),
                    3 => String::new(),
                    _ => (u32::from(number) * u32::from(pair[0] | 1)).to_string(),
                };
                let link = if pair[0] & 0x80 != 0 { format!(" line-style=\"{}\"", number % 40) } else { String::new() };
                file.push_str(&format!("<diacraft:style name=\"{name}\" id=\"{id}\"{link}/>"));
            }
            file.push_str("</diacraft:document>");
            match package_numbered(file.as_bytes()) {
                Some(b) => b,
                None => return,
            }
        }
        Some((2, rest)) => {
            let file = format!("{FILE_HEAD}{}</diacraft:document>", String::from_utf8_lossy(rest));
            match package("", Some(file.as_bytes())) {
                Some(b) => b,
                None => return,
            }
        }
        _ => data.to_vec(),
    };
    let bytes = if as_before(data) { spelled_as_before(&bytes).unwrap_or(bytes) } else { bytes };
    let (result, report) = read_partial(&bytes, &limits);
    let Ok(doc) = result else { return };
    assert!(doc.check_limits().is_ok());
    // Every style sheet holds an `ID` of its own, whatever the entries of the document file claimed: a link names one style sheet.
    if data.first() == Some(&6) {
        let ids: std::collections::HashSet<u32> = doc.styles.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), doc.styles.len(), "two style sheets hold one ID: {:?}", doc.styles.iter().map(|s| s.id).collect::<Vec<_>>());
        assert_eq!(doc.styles.get(0).map(|s| s.id), Some(0), "the root is 0");
    }
    // What the carrier took in is bounded by what the file holds and what the caps say: a part that came back is a part of the other format with a name its package can hold.
    assert!(report.carry.parts_kept <= doc.parts.len() as u64, "{:?}", report.carry);
    for part in doc.parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx) {
        assert!(
            part.name.starts_with('/') && part.name.split('/').all(|segment| segment != ".." && segment != ".") && part.bytes.len() <= 256 << 20,
            "{}",
            part.name
        );
    }
    // An unedited save of what was read is the file, byte for byte, or where the copy does not keep the archive whole, the copy of it.
    let saved = match write_with(&doc, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &WriteOptions::default()) {
        Ok(saved) => saved,
        Err(e) => {
            assert!(!e.to_string().is_empty());
            return;
        }
    };
    assert!(!saved.edited, "a model that was read and not touched is no edit");
    unedited_save_is_the_file_or_its_copy(&bytes, &saved.bytes, &limits);
    // One shape moved: saved, or refused in words, and what was saved reads again.
    let mut moved = doc.clone();
    if let Some(page) = moved.pages.get_mut(0) {
        let page = std::sync::Arc::make_mut(page);
        if let Some(shape) = page.shapes.get_mut(0) {
            let shape = std::sync::Arc::make_mut(shape);
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            for name in ["PinX", "PinY"] {
                if let Some(cell) = sheet.cells.get_mut(name)
                    && let Some(n) = cell.value.as_ref().and_then(diacraft_doc::Value::number)
                    && let Some(v) = diacraft_doc::Value::from_number(n + 0.25)
                {
                    cell.value = Some(v);
                    cell.formula = diacraft_doc::FormulaSlot::Absent;
                }
            }
        }
    }
    if let Ok(saved) = write_with(&moved, Some(&OriginalPackage { bytes: &bytes, index: &report.index }), &WriteOptions::default()) {
        assert!(read_partial(&saved.bytes, &limits).0.is_ok(), "what a save made reads again");
    }
});
