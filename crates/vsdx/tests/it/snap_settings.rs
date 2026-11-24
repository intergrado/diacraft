//! The Snap & Glue words of `DocumentSettings` are saved as files hold them: a word changed in the model is
//! changed in the file's own element and nowhere else, a word added goes where files put it, and a new drawing carries them in the skeleton's
//! settings.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Document, SnapWords};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteError, WriteOptions, write_with};

use crate::read_ok;

fn options() -> WriteOptions {
    WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }
}

fn part(bytes: &[u8], name: &str) -> String {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    let parts: BTreeMap<String, Vec<u8>> = pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect();
    String::from_utf8(parts.get(name).unwrap_or_else(|| panic!("no part {name}: {:?}", parts.keys())).clone()).unwrap()
}

fn words(glue: Option<&str>, snap: Option<&str>, extensions: Option<&str>, dynamic: Option<&str>) -> SnapWords {
    let a = |s: Option<&str>| s.map(Arc::<str>::from);
    SnapWords { glue: a(glue), snap: a(snap), extensions: a(extensions), dynamic_grid: a(dynamic) }
}

const SETTINGS: &str = concat!(
    r#"<DocumentSettings TopPage="0" DefaultTextStyle="3" DefaultLineStyle="3" DefaultFillStyle="3" DefaultGuideStyle="4">"#,
    r#"<GlueSettings>9</GlueSettings><SnapSettings>3</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/>"#,
    r#"<DynamicGridEnabled>0</DynamicGridEnabled><ProtectStyles>0</ProtectStyles></DocumentSettings>"#
);

fn drawing(extra: &str) -> Vec<u8> {
    VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).document_extra(extra).build()
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Result<diacraft_vsdx::Written, WriteError> {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &options())
}

#[test]
fn a_changed_word_is_changed_in_the_files_own_element_and_nowhere_else() {
    let original = drawing(SETTINGS);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert_eq!(doc.snap_words(), words(Some("9"), Some("3"), Some("34"), Some("0")));
    doc.set_snap_words(&words(Some("9"), Some("2"), Some("34"), Some("1")));
    let written = save(&doc, &original, &baseline).unwrap();
    let before = part(&original, "/visio/document.xml");
    let after = part(&written.bytes, "/visio/document.xml");
    // Words that are there and change are replaced in the file's own bytes, so an element the file wrote `<X></X>` is still that (before, the part was
    // written from its tree, which spells it `<X/>`).
    let expected = before
        .replace("<SnapSettings>3</SnapSettings>", "<SnapSettings>2</SnapSettings>")
        .replace("<DynamicGridEnabled>0<", "<DynamicGridEnabled>1<");
    assert_ne!(before, after);
    assert_eq!(after, expected, "only the two words moved, byte for byte");
    // The page part is the original's byte for byte.
    assert_eq!(part(&original, "/visio/pages/page1.xml"), part(&written.bytes, "/visio/pages/page1.xml"));
    // It reads back with the words, and the model the save returns is the one to compare the next save with.
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.snap_words(), doc.snap_words());
    assert_eq!(written.document.snap_words(), doc.snap_words());
    // A second save of the same model, from the new baseline, writes the same document part.
    let second = save(&doc, &written.bytes, &written.document).unwrap();
    assert_eq!(part(&second.bytes, "/visio/document.xml"), after);
}

#[test]
fn a_document_that_changed_no_word_leaves_the_part_alone() {
    let original = drawing(SETTINGS);
    let (doc, _) = read_ok(&original);
    let written = save(&doc, &original, &doc.clone()).unwrap();
    assert_eq!(part(&original, "/visio/document.xml"), part(&written.bytes, "/visio/document.xml"));
}

#[test]
fn a_word_the_file_lacks_is_added_where_files_put_it_and_a_word_set_to_none_is_taken_out() {
    let lacking = SETTINGS.replace("<SnapExtensions>34</SnapExtensions>", "").replace("<GlueSettings>9</GlueSettings>", "");
    let original = drawing(&lacking);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.set_snap_words(&words(Some("9"), Some("3"), Some("34"), None));
    let written = save(&doc, &original, &baseline).unwrap();
    let after = part(&written.bytes, "/visio/document.xml");
    let at = |needle: &str| after.find(needle).unwrap_or_else(|| panic!("no {needle} in {after}"));
    assert!(
        at("<GlueSettings>9<") < at("<SnapSettings>3<")
            && at("<SnapSettings>3<") < at("<SnapExtensions>34<")
            && at("<SnapExtensions>34<") < at("<SnapAngles")
    );
    assert!(!after.contains("DynamicGridEnabled"), "{after}");
    assert!(after.contains("<ProtectStyles>0</ProtectStyles>"), "the child it does not know stays: {after}");
    assert!(after.contains(r#"DefaultGuideStyle="4""#), "the attributes stay: {after}");
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.snap_words(), doc.snap_words());
}

#[test]
fn a_file_with_no_settings_gets_them_first_in_the_document() {
    let original = drawing("");
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert_eq!(doc.snap_words(), SnapWords::default());
    doc.set_snap_words(&words(None, Some("3"), None, Some("1")));
    let written = save(&doc, &original, &baseline).unwrap();
    let after = part(&written.bytes, "/visio/document.xml");
    let settings = after.find("<DocumentSettings").unwrap_or_else(|| panic!("no settings in {after}"));
    let first_other = after.find("<FaceNames").or_else(|| after.find("<StyleSheets")).unwrap_or(after.len());
    assert!(settings < first_other, "the settings come before the tables: {after}");
    assert!(after.contains("<SnapSettings>3</SnapSettings>") && after.contains("<DynamicGridEnabled>1</DynamicGridEnabled>"), "{after}");
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.snap_words(), doc.snap_words());
}

/// What the settings hold beside the words is the model's, written as the model has it: a change to an attribute of the element (it was refused before a change, which writes
/// the unknown elements of the document).
#[test]
fn a_change_to_anything_else_the_settings_hold_is_written_as_the_model_has_it() {
    let original = drawing(SETTINGS);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    // An attribute of the element changes: not a word.
    let nodes = doc.passthrough.nodes_mut();
    for placed in nodes.iter_mut() {
        if let diacraft_doc::RawNode::Element(el) = &mut placed.node {
            el.attrs = diacraft_doc::RawAttrs::from_pairs([("TopPage", "7")]);
        }
    }
    let written = save(&doc, &original, &baseline).unwrap_or_else(|e| panic!("{e}"));
    let after = part(&written.bytes, "/visio/document.xml");
    assert!(after.contains("<DocumentSettings TopPage=\"7\">"), "{after}");
    assert_eq!(read_ok(&written.bytes).0.passthrough, doc.passthrough);
}

#[test]
fn a_new_drawing_carries_its_words_in_the_skeletons_settings() {
    let mut doc = Document::new_blank();
    doc.set_snap_words(&words(Some("8"), Some("3"), None, Some("1")));
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let xml = part(&written.bytes, "/visio/document.xml");
    assert!(xml.contains("<DocumentSettings"), "{xml}");
    assert!(xml.contains("TopPage"), "the skeleton's own attributes are there: {xml}");
    assert!(xml.contains("<GlueSettings>8</GlueSettings>") && xml.contains("<SnapSettings>3</SnapSettings>"), "{xml}");
    assert!(!xml.contains("SnapExtensions"), "a word the model lacks is not made up: {xml}");
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.snap_words(), doc.snap_words());
    // The model the save hands back is the caller's, so the next save compares against what the caller's model said.
    assert_eq!(written.document.snap_words(), doc.snap_words());
    assert_eq!(written.document.passthrough, doc.passthrough);
}

#[test]
fn a_new_drawing_with_no_words_writes_the_skeletons_settings_as_before() {
    let doc = Document::new_blank();
    let written = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    let xml = part(&written.bytes, "/visio/document.xml");
    assert!(xml.contains("<DocumentSettings") && !xml.contains("GlueSettings") && !xml.contains("SnapSettings"), "{xml}");
}

#[test]
fn a_saved_new_drawing_whose_words_change_is_patched_the_next_time() {
    let mut doc = Document::new_blank();
    doc.set_snap_words(&words(None, Some("3"), None, None));
    let first = write_with(&doc, None, &options()).unwrap_or_else(|e| panic!("{e}"));
    doc.set_snap_words(&words(Some("9"), Some("2"), None, None));
    let second = save(&doc, &first.bytes, &first.document).unwrap();
    let xml = part(&second.bytes, "/visio/document.xml");
    assert!(xml.contains("<GlueSettings>9</GlueSettings>") && xml.contains("<SnapSettings>2</SnapSettings>"), "{xml}");
    assert!(xml.contains("TopPage"), "the skeleton's attributes survive: {xml}");
    let (again, _) = read_ok(&second.bytes);
    assert_eq!(again.snap_words(), doc.snap_words());
}

#[test]
fn settings_of_every_odd_shape_are_patched_without_a_panic_and_read_back() {
    // A word with an element inside it, a CDATA text, a comment between children, two of one name, a prefixed element, an empty element, a huge text.
    let big = "7".repeat(1 << 16);
    let odd = [
        r#"<DocumentSettings><SnapSettings><x/>3</SnapSettings></DocumentSettings>"#.to_owned(),
        r#"<DocumentSettings><SnapSettings><![CDATA[3]]></SnapSettings></DocumentSettings>"#.to_owned(),
        r#"<DocumentSettings><!-- a note --><GlueSettings>9</GlueSettings><!-- b --><SnapSettings>3</SnapSettings></DocumentSettings>"#.to_owned(),
        r#"<DocumentSettings><SnapSettings>1</SnapSettings><SnapSettings>2</SnapSettings></DocumentSettings>"#.to_owned(),
        r#"<DocumentSettings/>"#.to_owned(),
        r#"<DocumentSettings></DocumentSettings>"#.to_owned(),
        format!("<DocumentSettings><SnapSettings>{big}</SnapSettings></DocumentSettings>"),
        r#"<DocumentSettings><DynamicGridEnabled>1</DynamicGridEnabled><GlueSettings>9</GlueSettings></DocumentSettings>"#.to_owned(),
    ];
    for xml in odd {
        let original = drawing(&xml);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        doc.set_snap_words(&words(Some("9"), Some("3"), Some("34"), Some("1")));
        let written = save(&doc, &original, &baseline).unwrap_or_else(|e| panic!("{xml:.80}: {e}"));
        let (again, _) = read_ok(&written.bytes);
        assert_eq!(again.snap_words(), doc.snap_words(), "{xml:.80}");
    }
}

#[test]
fn a_word_with_blanks_around_it_keeps_them_and_the_part_keeps_its_line_ends_and_quotes() {
    // The text of the word is replaced in the file's bytes, whatever surrounds it.
    let settings = concat!(
        "<DocumentSettings TopPage='0'>\r\n  <GlueSettings> 9 </GlueSettings>\r\n  <SnapSettings>\r\n 3\r\n</SnapSettings>\r\n  <SnapExtensions>34</SnapExtensions>\r\n",
        "  <SnapAngles/>\r\n  <DynamicGridEnabled>true</DynamicGridEnabled>\r\n</DocumentSettings>"
    );
    let original = drawing(settings);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.set_snap_words(&words(Some("41"), Some("2"), Some("34"), Some("0")));
    let written = save(&doc, &original, &baseline).unwrap();
    let before = part(&original, "/visio/document.xml");
    let expected = before
        .replace("<GlueSettings> 9 </GlueSettings>", "<GlueSettings> 41 </GlueSettings>")
        .replace("<SnapSettings>\r\n 3\r\n</SnapSettings>", "<SnapSettings>\r\n 2\r\n</SnapSettings>")
        .replace("<DynamicGridEnabled>true<", "<DynamicGridEnabled>0<");
    assert_eq!(part(&written.bytes, "/visio/document.xml"), expected);
    assert_eq!(read_ok(&written.bytes).0.snap_words(), doc.snap_words());
}

#[test]
fn a_word_that_is_added_or_written_with_a_reference_is_written_from_the_tree_and_reads_back() {
    // A word the element lacks is added where files put it, and a word written as a character reference is replaced whole: the tree is written (the other
    // bytes are then the writer's spelling), and the file reads back with the words.
    let missing = SETTINGS.replace("<DynamicGridEnabled>0</DynamicGridEnabled>", "");
    let reference = SETTINGS.replace("<SnapSettings>3<", "<SnapSettings>&#51;<");
    for (what, settings) in [("a word added", missing), ("a word written as a reference", reference)] {
        let original = drawing(&settings);
        let (mut doc, _) = read_ok(&original);
        let baseline = doc.clone();
        doc.set_snap_words(&words(Some("9"), Some("2"), Some("34"), Some("1")));
        let written = save(&doc, &original, &baseline).unwrap();
        assert_eq!(read_ok(&written.bytes).0.snap_words(), words(Some("9"), Some("2"), Some("34"), Some("1")), "{what}");
        let xml = part(&written.bytes, "/visio/document.xml");
        assert!(xml.contains("<SnapSettings>2</SnapSettings>") && xml.contains("<DynamicGridEnabled>1</DynamicGridEnabled>"), "{what}: {xml}");
        assert!(
            xml.contains("<ProtectStyles>0</ProtectStyles>") && xml.contains(r#"DefaultGuideStyle="4""#),
            "{what}: the rest of the element stays"
        );
    }
}

#[test]
fn a_word_taken_out_and_a_word_that_does_not_change_are_not_patched_in_the_bytes() {
    let original = drawing(SETTINGS);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.set_snap_words(&words(Some("9"), Some("3"), None, Some("0")));
    let written = save(&doc, &original, &baseline).unwrap();
    assert_eq!(read_ok(&written.bytes).0.snap_words(), words(Some("9"), Some("3"), None, Some("0")));
    assert!(!part(&written.bytes, "/visio/document.xml").contains("SnapExtensions"));
}

const FOREIGN_FIRST: &str = concat!(
    r#"<DocumentSettings TopPage="0"><GlueSettings>9</GlueSettings><SnapSettings xmlns="urn:x">5</SnapSettings><SnapSettings>773</SnapSettings>"#,
    r#"<SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>0</DynamicGridEnabled></DocumentSettings>"#
);

#[test]
fn a_word_in_another_namespace_is_neither_read_nor_written_and_the_real_word_is() {
    // The model read the foreign 5 and the writer wrote 7 over the real 773 (guides, outlines and the alignment box lost, and it read
    // back as 5). Both take the real word now.
    let original = drawing(FOREIGN_FIRST);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert_eq!(doc.snap_words(), words(Some("9"), Some("773"), Some("34"), Some("0")), "the model reads the word of the file's own namespace");
    // The grid on: 773 + 2.
    doc.set_snap_words(&words(Some("9"), Some("775"), Some("34"), Some("0")));
    let written = save(&doc, &original, &baseline).unwrap();
    let before = part(&original, "/visio/document.xml");
    let after = part(&written.bytes, "/visio/document.xml");
    assert_eq!(after, before.replace("<SnapSettings>773<", "<SnapSettings>775<"), "the real word moved, byte for byte, and the foreign one did not");
    assert!(after.contains(r#"<SnapSettings xmlns="urn:x">5</SnapSettings>"#));
    assert!(written.warnings.iter().all(|w| !w.contains("Snap and Glue")), "{:?}", written.warnings);
    let (again, _) = read_ok(&written.bytes);
    assert_eq!(again.snap_words().snap.as_deref(), Some("775"), "and it reads back as what was written");
}

#[test]
fn words_the_model_and_the_writer_would_not_both_take_are_left_alone_with_a_warning() {
    // A prefix bound on the root to another application's namespace: the model, which sees only the settings, takes the first `SnapSettings` by its local name;
    // the writer, which resolves the prefix, takes the second. They are not the same element, so nothing is written, and the save says so.
    let settings = concat!(
        r#"<DocumentSettings TopPage="0"><GlueSettings>9</GlueSettings><x:SnapSettings>5</x:SnapSettings><SnapSettings>773</SnapSettings>"#,
        r#"<SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>0</DynamicGridEnabled></DocumentSettings>"#
    );
    let builder = VsdxBuilder::new().page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1")))).document_extra(settings);
    let original = builder
        .pkg()
        .map_parts(|name, bytes| {
            if name.ends_with("/document.xml") {
                *bytes = String::from_utf8_lossy(bytes).replacen("<VisioDocument ", r#"<VisioDocument xmlns:x="urn:x" "#, 1).into_bytes();
            }
        })
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    assert_eq!(doc.snap_words().snap.as_deref(), Some("5"), "the model takes the first by its local name");
    doc.set_snap_words(&words(Some("9"), Some("7"), Some("34"), Some("0")));
    let written = save(&doc, &original, &baseline).unwrap();
    assert_eq!(part(&written.bytes, "/visio/document.xml"), part(&original, "/visio/document.xml"), "the part is as it was, byte for byte");
    assert!(written.warnings.iter().any(|w| w.contains("Snap and Glue") && w.contains("namespace")), "{:?}", written.warnings);
    assert_eq!(written.document.snap_words().snap.as_deref(), Some("5"), "the baseline is the file's, so the next save says so again");
    // A change of something else in the same save is still written (the styles, say), and the words still are not.
    let mut again = doc.clone();
    again.title = "Renamed".into();
    let second = save(&again, &original, &baseline).unwrap();
    assert_eq!(part(&second.bytes, "/visio/document.xml"), part(&original, "/visio/document.xml"));
    assert!(second.warnings.iter().any(|w| w.contains("Snap and Glue")));
}

#[test]
fn the_read_back_check_catches_a_word_whose_blanks_the_bytes_would_lose() {
    // The tree keeps the blanks round a word's text, Unicode ones too; the bytes were patched with ASCII ones only, so a no-break
    // space before the number was replaced with the number. The check reads the bytes back and finds they are not the tree, and the tree is written. With the
    // check taken out (`Some(out)`) the space is lost and this test fails.
    let settings = SETTINGS.replace("<SnapSettings>3<", "<SnapSettings>\u{a0}773<");
    let original = drawing(&settings);
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    doc.set_snap_words(&words(Some("9"), Some("775"), Some("34"), Some("0")));
    let written = save(&doc, &original, &baseline).unwrap();
    let after = part(&written.bytes, "/visio/document.xml");
    assert!(after.contains("<SnapSettings>\u{a0}775</SnapSettings>"), "the blank stays: {after}");
    assert!(after.contains("<FaceNames/>"), "and the part was written from its tree, which spells an empty element so: {after}");
    assert_eq!(read_ok(&written.bytes).0.snap_words().snap.as_deref(), Some("775"));
}

#[test]
fn a_new_drawing_carries_the_empty_snap_angles_between_the_extensions_and_the_dynamic_grid() {
    // All 1,906 samples hold an empty `SnapAngles` in that slot.
    let mut doc = Document::new_blank();
    doc.set_snap_words(&words(Some("9"), Some("65847"), Some("34"), Some("1")));
    let written = write_with(&doc, None, &options()).unwrap();
    let xml = part(&written.bytes, "/visio/document.xml");
    assert!(
        xml.contains("<GlueSettings>9</GlueSettings><SnapSettings>65847</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled>"),
        "{xml}"
    );
    // A model with no word writes no settings and no angles.
    let plain = write_with(&Document::new_blank(), None, &options()).unwrap();
    assert!(!part(&plain.bytes, "/visio/document.xml").contains("SnapAngles"));
    // It reads back, and an in-place save of it keeps the element in its slot.
    let (mut again, _) = read_ok(&written.bytes);
    let baseline = again.clone();
    again.set_snap_words(&words(Some("9"), Some("65845"), Some("34"), Some("1")));
    let second = save(&again, &written.bytes, &baseline).unwrap();
    let xml = part(&second.bytes, "/visio/document.xml");
    assert!(xml.contains("<SnapSettings>65845</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1<"), "{xml}");
}
