//! Fuzzing what the reader and the writer of DiaCraft's own data take from a file (AGENTS.md: importers are fuzzed): the value of any attribute of the
//! extension on a shape's own automatic style, any text put in the document file's root, and the document file itself of any bytes. A read gives a document or an
//! error of a known class, and what it gives holds the model's caps; **an unedited save of what was read is the file, byte for byte, where a copy keeps the archive
//! whole, and the copy of it where it does not** (a local header that is not its row's, bytes no header announces); and a model with one shape
//! moved is saved, or refused in words, and what was saved reads again. These run under `proptest` with a fixed budget of cases; the long run is the cargo-fuzz
//! target `odg_tier2` (`crates/odg/fuzz`).

use diacraft_odg::{Limits, OriginalPackage, WriteOptions, read_partial, write_with};
use proptest::prelude::*;

use crate::drawing::Drawing;

/// `xml` as the value of an attribute.
fn value(xml: &str) -> String {
    xml.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "&#10;")
}

/// A drawing of two rectangles on a style that carries `attrs`.
fn drawing(attrs: &str) -> Vec<u8> {
    let style = format!(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="none" {attrs}/></style:style>"#);
    let rect = |k: u32| {
        format!(
            r#"<draw:rect draw:style-name="gr1" draw:name="R{k}" svg:x="{k}cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p/></draw:rect>"#
        )
    };
    // A custom shape on the same style beside the rectangles: tier 3 judges an outline, and a rectangle has none.
    let custom = r#"<draw:custom-shape draw:style-name="gr1" draw:name="C" svg:x="9cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p>t</text:p><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 Z N"/></draw:custom-shape>"#;
    Drawing::a4().auto(&style).page(&format!("{}{}{custom}", rect(1), rect(4))).bytes()
}

/// A saved drawing that has a document file, with the file's text replaced by `text`.
fn with_file(text: &str) -> Vec<u8> {
    let mut o = crate::write_core::open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    o.doc.masters.push(std::sync::Arc::new(diacraft_doc::Master { id: 1, ..diacraft_doc::Master::default() }));
    let saved = crate::write_core::real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    crate::docfile::rebuild(
        &saved.bytes,
        |name, b| Some(if name == "DiaCraft/document.xml" { text.as_bytes().to_vec() } else { b.to_vec() }),
        |m| m.to_owned(),
    )
}

/// `bytes` with the name in the local header of `entry` overwritten by `noise`, as far as it goes (the length of the name is the row's, so that every offset stands).
fn damaged(bytes: &[u8], entry: &str, noise: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(range) = crate::local_name::local_name_range(bytes, entry) {
        for (at, byte) in range.zip(noise) {
            if let Some(slot) = out.get_mut(at) {
                *slot = *byte;
            }
        }
    }
    out
}

/// What every read, save and second read of a file must hold.
fn check(bytes: &[u8]) {
    let limits = Limits::default();
    let (result, report) = read_partial(bytes, &limits);
    let doc = match result {
        Err(e) => {
            assert!(crate::read_fuzz::CLASSES.contains(&e.class()), "an unknown class: {e:?}");
            let _shown = e.to_string();
            return;
        }
        Ok(doc) => doc,
    };
    assert!(doc.check_limits().is_ok());
    assert!(report.work.items <= limits.max_items as u64);
    let original = OriginalPackage { bytes, index: &report.index };
    // An unedited save is the file, or, where the copy does not keep the archive whole (a local header that is not its row's, bytes no header announces), the
    // copy of it, which the account of the copy says.
    let saved = match write_with(&doc, Some(&original), &WriteOptions::default()) {
        Ok(saved) => saved,
        Err(e) => {
            assert!(!e.to_string().is_empty());
            return;
        }
    };
    assert!(!saved.edited, "a model that was read and not touched is no edit");
    crate::local_name::unedited_save_is_the_file_or_its_copy(bytes, &saved.bytes, &limits);
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
    match write_with(&moved, Some(&original), &WriteOptions::default()) {
        Ok(saved) => assert!(read_partial(&saved.bytes, &limits).0.is_ok(), "what a save made reads again"),
        Err(e) => assert!(!e.to_string().is_empty()),
    }
}

/// The local names of the extension's attributes, and any other.
fn local() -> impl Strategy<Value = String> {
    let known = [
        "cells",
        "s-User",
        "s-Geometry-0",
        "text",
        "gone",
        "name",
        "name-u",
        "master",
        "master-shape",
        "id",
        "version",
        // The fingerprints of tier 3: of each kind of item, and one of a key this build does not know.
        "fp-text",
        "fp-name",
        "fp-master",
        "fp-s-Geometry-0",
        "fp-c-EndArrow",
        "fp-c-FillForegnd",
        "fp-future",
    ];
    prop_oneof![prop::sample::select(known.to_vec()).prop_map(str::to_owned), "[a-z][a-z0-9-]{0,12}"]
}

/// The pieces a value of the extension is made of: fragments of the grammar and noise.
fn piece() -> impl Strategy<Value = String> {
    let fragments = [
        "<Sheet>",
        "</Sheet>",
        r#"<Cell N="Note" V="x" F="Width*2" U="IN"/>"#,
        r#"<Cell N="PinX" V="9"/>"#,
        r#"<Section N="User"><Row N="a"><Cell N="Value" V="3"/></Row></Section>"#,
        r#"<Section N="Geometry" IX="0"/>"#,
        r#"<Text><cp IX="1"/>ab<pp IX="2"/>c<fld IX="0">3</fld><!--k--></Text>"#,
        "c.PinX s.User._ name name-u text",
        // A fingerprint of 16 digits, one that is not, and a carried frame.
        "0123456789abcdef",
        "0123456789ABCDEF",
        r#"<Sheet><Cell N="EndArrow" V="254"/><Cell N="Width" V="9"/><Cell N="LocPinX" V="0" F="Width*0"/></Sheet>"#,
        "c.a_ s.User zz",
    ];
    prop_oneof![
        prop::sample::select(fragments.to_vec()).prop_map(str::to_owned),
        "[ -~]{0,24}",
        any::<u32>().prop_map(|n| n.to_string()),
        "[\u{80}-\u{3FF}]{0,4}",
    ]
}

fn attr_value() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..8).prop_map(|v| v.concat())
}

/// Elements and text for the root of the document file.
fn file_piece() -> impl Strategy<Value = String> {
    let elements = [
        // The masters are the codec's own (a shape directly in its master, a kept element as data, a picture by its blob), and the pictures are the file's list.
        r#"<diacraft:master id="1"><diacraft:sheet/><diacraft:connect from-sheet="1" to-sheet="2"/></diacraft:master>"#,
        r#"<diacraft:master id="2" name="M"><diacraft:shape id="1"><diacraft:sheet/><diacraft:shape id="2"/></diacraft:shape></diacraft:master>"#,
        r#"<diacraft:master id="3"><diacraft:shape id="1" type="Foreign"><diacraft:foreign><diacraft:picture part="p.png" type="image/png" blob="0"/></diacraft:foreign></diacraft:shape></diacraft:master>"#,
        r#"<diacraft:master id="4"><diacraft:shape id="1"><diacraft:kept after="0"><diacraft:el name="x:k"><diacraft:attr name="xmlns:x" value="urn:x"/></diacraft:el></diacraft:kept></diacraft:shape></diacraft:master>"#,
        r#"<diacraft:master id="5"><diacraft:shape type="Bogus"/></diacraft:master>"#,
        r#"<diacraft:blob id="0">QUJD</diacraft:blob>"#,
        r#"<diacraft:blob id="0">not base64 !</diacraft:blob>"#,
        r#"<diacraft:docsheet gone="c.PageWidth s.User._"><diacraft:sheet><Cell N="a" V="1"/></diacraft:sheet></diacraft:docsheet>"#,
        r#"<diacraft:docsheet><diacraft:sheet><diacraft:Cell N="a" V="1"/></diacraft:sheet></diacraft:docsheet>"#,
        r#"<diacraft:page name="Page-1" id="4" name-u="P"/>"#,
        r#"<diacraft:page name="nowhere"><diacraft:sheet/></diacraft:page>"#,
        r#"<diacraft:theme part="theme/theme1.xml">QUJD</diacraft:theme>"#,
        r#"<diacraft:theme part="t">not base64 !</diacraft:theme>"#,
        r##"<diacraft:colors><diacraft:color ix="1" rgb="#112233"/><diacraft:color ix="x" rgb="y"/></diacraft:colors>"##,
        r#"<diacraft:data><x:thing xmlns:x="urn:x" k="v">text<!--c--></x:thing></diacraft:data>"#,
        r#"<diacraft:future kept="yes"/>"#,
        "<unrelated/>",
    ];
    prop_oneof![prop::sample::select(elements.to_vec()).prop_map(str::to_owned), "[ -~]{0,20}"]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Any value on any attribute of the extension, on the two rectangles' shared style.
    #[test]
    fn any_value_of_any_attribute_of_the_extension_reads_and_saves_or_is_refused_in_words(
        local in local(),
        v in attr_value(),
        id in prop_oneof![Just(String::new()), Just(r#" diacraft:id="7""#.to_owned()), Just(r#" diacraft:id="x""#.to_owned())],
    ) {
        let attrs = format!(r#"diacraft:version="1"{id} diacraft:{local}="{}""#, value(&v));
        check(&drawing(&attrs));
    }

    /// Tier 3: a style that carries one valid item of each kind that is judged, and a fingerprint of any value on any key, read, saved, saved with a shape
    /// moved, and read again: the items it holds that are stale are left out and the shape is the markup's, and nothing breaks.
    #[test]
    fn a_fingerprint_of_any_value_on_a_style_that_carries_each_item_reads_and_saves(
        key in prop::sample::select(vec!["fp-text", "fp-name", "fp-master", "fp-s-Geometry-0", "fp-c-EndArrow", "fp-c-FillForegnd", "fp-s-Geometry-7"]),
        v in prop_oneof!["[0-9a-f]{16}", "[ -~]{0,40}", Just("0000000000000000".to_owned()), Just("ffffffffffffffff".to_owned())],
        more in prop::collection::vec(prop::sample::select(vec!["fp-text", "fp-name", "fp-master", "fp-c-LineCap"]), 0..3),
    ) {
        let items = concat!(
            r#"diacraft:version="1" diacraft:id="7" diacraft:name="N" diacraft:master="2" diacraft:text="&lt;Text&gt;&lt;cp IX=&quot;0&quot;/&gt;hi&lt;/Text&gt;" "#,
            r#"diacraft:cells="&lt;Sheet&gt;&lt;Cell N=&quot;EndArrow&quot; V=&quot;254&quot;/&gt;&lt;Cell N=&quot;FillForegnd&quot; V=&quot;#FF0000&quot;/&gt;&lt;Cell N=&quot;Width&quot; V=&quot;3&quot; F=&quot;Height*3&quot;/&gt;&lt;/Sheet&gt;" "#,
            r#"diacraft:s-Geometry-0="&lt;Sheet&gt;&lt;Section N=&quot;Geometry&quot; IX=&quot;0&quot;&gt;&lt;Cell N=&quot;NoFill&quot; V=&quot;1&quot;/&gt;&lt;/Section&gt;&lt;/Sheet&gt;""#
        );
        let mut attrs = format!(r#"{items} diacraft:{key}="{}""#, value(&v));
        let mut seen = std::collections::HashSet::from([key]);
        for k in more {
            if seen.insert(k) {
                attrs.push_str(&format!(r#" diacraft:{k}="{}""#, value(&v)));
            }
        }
        check(&drawing(&attrs));
    }

    /// Several attributes at once, so that they meet.
    #[test]
    fn attributes_of_the_extension_that_meet_read_and_save(
        a in (local(), attr_value()),
        b in (local(), attr_value()),
        c in (local(), attr_value()),
    ) {
        let mut attrs = String::from(r#"diacraft:version="1" diacraft:id="3""#);
        let mut seen = std::collections::HashSet::new();
        for (l, v) in [a, b, c] {
            if seen.insert(l.clone()) && l != "version" && l != "id" {
                attrs.push_str(&format!(r#" diacraft:{l}="{}""#, value(&v)));
            }
        }
        check(&drawing(&attrs));
    }

    /// Any elements in the root of the document file.
    #[test]
    fn any_children_of_the_document_files_root_read_and_save_or_are_refused_in_words(pieces in prop::collection::vec(file_piece(), 0..8)) {
        let file = format!(
            r#"<diacraft:document xmlns:diacraft="urn:app.diacraft:extension" version="2">{}</diacraft:document>"#,
            pieces.concat()
        );
        check(&with_file(&file));
    }

    /// An honest drawing whose local header of one entry has any bytes of the row's length for a name (the central header wins): read,
    /// and saved as the file or as its copy, whichever the account of the copy says.
    #[test]
    fn a_drawing_whose_local_header_names_an_entry_in_any_bytes_is_saved_as_the_file_or_its_copy(
        entry in prop::sample::select(vec!["content.xml", "styles.xml", "META-INF/manifest.xml", "mimetype"]),
        noise in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        check(&damaged(&drawing(r#"diacraft:version="1""#), entry, &noise));
    }

    /// The document file of any bytes at all.
    #[test]
    fn a_document_file_of_any_bytes_is_read_with_a_warning_or_an_error_and_never_a_panic(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        check(&with_file(&text));
    }
}
