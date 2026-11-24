//! Bug 1: **a save says that data of the other format was left out only when an item of it really is**. The carrier (`crates/vsdx/src/carry.rs`) keys an
//! item to its page or master by `ID` and to its style by name, and an object whose key another has names nothing. Before this fix a save counted every such object as data left out, even
//! one that holds nothing that rides: an unedited save of a file with two `StyleSheet` rows of one `Name` said "the data of 2 pages, masters or styles was left out", and so did a file with
//! two pages of one `ID`, though nothing was carried and the bytes written were the file's. Now an object is counted only when it holds data, and **a style whose name another style has is
//! keyed by its `ID`** (the design's 5.1 keys a style by its name; the `ID`, which a file keys a style by, is taken only where the name is not the style's alone), so its data comes back.
//!
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::{Document, OdfPage, OdfShape, OdfStyle, Style};
use diacraft_opc::{Limits as OpcLimits, Package};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const SOLUTION: &str = "/visio/solutions/solution1.xml";

/// The words of the carrier when it leaves the data of pages, masters, styles or shapes out.
const LEFT_OUT: &str = "was left out";

/// A drawing whose document part's `StyleSheets` holds the rows `ID` 0, 4 and 5, the last two of one `Name` (their `NameU`s differ), and `more` after them.
fn twin_styles(more: &str) -> Vec<u8> {
    VsdxBuilder::new()
        .more_styles(&format!(
            r#"<StyleSheet ID="4" Name="Twin" NameU="Twin"><Cell N="LineWeight" V="0.01"/></StyleSheet><StyleSheet ID="5" Name="Twin" NameU="Twin.5"><Cell N="LineWeight" V="0.02"/></StyleSheet>{more}"#
        ))
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))))
        .build()
}

fn opts() -> WriteOptions {
    WriteOptions { now: Some(NOW), ..WriteOptions::default() }
}

/// Which writer.
#[derive(Clone, Copy, Debug)]
enum Path {
    Splice,
    Model,
}

const PATHS: [Path; 2] = [Path::Splice, Path::Model];

/// The save of `doc` by `path`: into `original` (with `baseline`, the model the file was read as, when given) or by the model writer.
fn save(doc: &Document, original: &[u8], baseline: Option<&Document>, path: Path) -> Written {
    let result = match path {
        Path::Splice => write_with(doc, Some(&OriginalBytes { bytes: original, baseline }), &opts()),
        Path::Model => write_with(doc, None, &opts()),
    };
    result.unwrap_or_else(|e| panic!("{path:?}: the save refused: {e}"))
}

fn left_out(written: &Written) -> Vec<&String> {
    written.warnings.iter().filter(|w| w.contains(LEFT_OUT)).collect()
}

fn style_mut(doc: &mut Document, id: u32) -> &mut Style {
    let at = doc.styles.iter().position(|s| s.id == id).unwrap_or_else(|| panic!("no style {id}"));
    Arc::make_mut(doc.styles.get_mut(at).unwrap())
}

fn odf_style(name: &str) -> OdfStyle {
    OdfStyle { name: Some(Arc::from(name)), family: Some(Arc::from("common:graphic")), ..OdfStyle::default() }
}

/// The ODF data of style `id` of `doc`.
fn odf_of(doc: &Document, id: u32) -> Option<OdfStyle> {
    doc.styles.iter().find(|s| s.id == id).and_then(|s| s.ext.get()).map(|d| d.odf.clone()).filter(|o| *o != OdfStyle::default())
}

fn solution(bytes: &[u8]) -> String {
    String::from_utf8(Package::read(bytes, &OpcLimits::default()).unwrap().part(SOLUTION).unwrap().to_vec()).unwrap()
}

/// The keys of the style items of DiaCraft's part, as the start tags spell them: `kind` and the key attributes, without the stamp.
fn style_items(bytes: &[u8]) -> Vec<String> {
    let part = solution(bytes);
    part.match_indices(r#"kind="style""#)
        .map(|(at, _)| {
            let tag = &part[at..at + part[at..].find('>').unwrap()];
            tag.split(" fp=").next().unwrap().trim_end_matches('/').trim().to_owned()
        })
        .collect()
}

#[test]
fn an_unedited_save_of_styles_that_share_a_name_or_pages_and_masters_that_share_an_id_says_nothing_was_left_out() {
    let third = r#"<StyleSheet ID="6" Name="Twin" NameU="Twin.6"><Cell N="LineWeight" V="0.03"/></StyleSheet>"#;
    let twin_pages = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .page(PageSpec::new(1, "B").shapes(&shape(1, &cell("PinX", "2"))))
        .build();
    let twin_masters = VsdxBuilder::new()
        .master(MasterSpec::new(1, "M").shapes(&shape(1, &cell("Width", "1"))))
        .master(MasterSpec::new(1, "N").shapes(&shape(1, &cell("Width", "2"))))
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))))
        .build();
    for (case, bytes) in [
        ("two styles of one name", twin_styles("")),
        ("three", twin_styles(third)),
        ("two pages of one ID", twin_pages),
        ("two masters of one ID", twin_masters),
    ] {
        let (doc, report) = read_ok(&bytes);
        if case.contains("styles") || case == "three" {
            assert_eq!(report.warning_counts.values().sum::<u64>(), 0, "{case}: the file opens with nothing to say: {:?}", report.warnings);
        }
        for path in PATHS {
            let written = save(&doc, &bytes, None, path);
            assert!(left_out(&written).is_empty(), "{case}, {path:?}: nothing rides, so nothing is left out: {:?}", written.warnings);
            if matches!(path, Path::Splice) && report.warning_counts.values().sum::<u64>() == 0 {
                assert!(written.warnings.is_empty(), "{case}: an unedited save says nothing: {:?}", written.warnings);
            }
            // No DiaCraft part is made for data there is none of.
            assert!(Package::read(&written.bytes, &OpcLimits::default()).unwrap().part(SOLUTION).is_none(), "{case}, {path:?}");
        }
    }
}

#[test]
fn the_data_of_the_second_of_two_styles_that_share_a_name_is_keyed_by_its_id_and_comes_back_after_a_save_and_a_reopen() {
    let bytes = twin_styles("");
    for path in PATHS {
        let (mut doc, _) = read_ok(&bytes);
        style_mut(&mut doc, 5).ext.make_mut().odf = odf_style("Twin");
        let written = save(&doc, &bytes, None, path);
        let case = format!("{path:?}");
        assert!(left_out(&written).is_empty(), "{case}: the style's data rides: {:?}", written.warnings);
        assert_eq!(style_items(&written.bytes), [r#"kind="style" id="5""#], "{case}: the item is keyed by the ID");
        let (back, report) = read_ok(&written.bytes);
        assert_eq!((report.carry.kept, report.carry.gone, report.carry.stale, report.carry.left_out), (1, 0, 0, 0), "{case}: {:?}", report.carry);
        assert_eq!(odf_of(&back, 5), Some(odf_style("Twin")), "{case}: the data is on the style it was on");
        assert_eq!(odf_of(&back, 4), None, "{case}: and not on the other of that name");
        // A save of the drawing read back, unedited, into the file it was read from, and a reopen: the data is there still, and nothing is said.
        let again = save(&back, &written.bytes, Some(&back), Path::Splice);
        assert!(again.warnings.is_empty(), "{case}: {:?}", again.warnings);
        let (third, _) = read_ok(&again.bytes);
        assert_eq!(odf_of(&third, 5), Some(odf_style("Twin")), "{case}: after the second save");
        // An edit of the drawing read back (its page renamed) saved into the file keeps it too.
        let mut edited = back.clone();
        Arc::make_mut(edited.pages.get_mut(0).unwrap()).name = Arc::from("Q");
        let saved = save(&edited, &written.bytes, Some(&back), Path::Splice);
        assert!(left_out(&saved).is_empty(), "{case}: {:?}", saved.warnings);
        assert_eq!(odf_of(&read_ok(&saved.bytes).0, 5), Some(odf_style("Twin")), "{case}: after an edited save");
    }
}

#[test]
fn a_style_keyed_by_a_name_another_program_gave_a_second_style_is_still_found_by_its_id() {
    // A style that is alone in its name is keyed by it (the design's 5.1); one whose name two styles share is found by its ID, whatever the other program named the others.
    let bytes = twin_styles(r#"<StyleSheet ID="7" Name="Solo" NameU="Solo"/>"#);
    let (mut doc, _) = read_ok(&bytes);
    style_mut(&mut doc, 7).ext.make_mut().odf = odf_style("Solo");
    style_mut(&mut doc, 4).ext.make_mut().odf = odf_style("Twin");
    let written = save(&doc, &bytes, None, Path::Model);
    assert_eq!(style_items(&written.bytes), [r#"kind="style" id="4""#, r#"kind="style" name="Solo""#]);
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(report.carry.kept, 2, "{:?}", report.carry);
    assert_eq!((odf_of(&back, 7), odf_of(&back, 4), odf_of(&back, 5)), (Some(odf_style("Solo")), Some(odf_style("Twin")), None));
}

#[test]
fn data_whose_key_another_object_has_too_is_still_left_out_and_the_save_says_so_in_words() {
    // Two styles of one name and one ID: neither key is the style's alone.
    let bytes = twin_styles("");
    let (mut doc, _) = read_ok(&bytes);
    style_mut(&mut doc, 5).id = 4;
    let at = doc.styles.iter().rposition(|s| s.id == 4).unwrap();
    Arc::make_mut(doc.styles.get_mut(at).unwrap()).ext.make_mut().odf = odf_style("Twin");
    for path in PATHS {
        let written = save(&doc, &bytes, None, path);
        assert_eq!(
            left_out(&written),
            ["the data of 1 pages, masters or styles was left out: another has the same ID or name"],
            "{path:?}: {:?}",
            written.warnings
        );
        let (back, _) = read_ok(&written.bytes);
        assert!(back.styles.iter().all(|s| s.ext.get().is_none_or(|d| d.odf == OdfStyle::default())), "{path:?}: nothing rides for it");
    }
    // Two pages of one ID, the second with data of its own and on a shape: both are left out, and said.
    let pages = VsdxBuilder::new()
        .page(PageSpec::new(1, "A").shapes(&shape(1, &cell("PinX", "1"))))
        .page(PageSpec::new(1, "B").shapes(&shape(1, &cell("PinX", "2"))))
        .build();
    let (mut doc, _) = read_ok(&pages);
    {
        let page = Arc::make_mut(doc.pages.get_mut(1).unwrap());
        page.ext.make_mut().odf = OdfPage { master_page: Some(Arc::from("Default")), ..OdfPage::default() };
        let s = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        s.ext.make_mut().odf = OdfShape { style_name: Some(Arc::from("gr1")), ..OdfShape::default() };
    }
    for path in PATHS {
        let written = save(&doc, &pages, None, path);
        assert_eq!(
            left_out(&written),
            [
                "the data of 1 pages, masters or styles was left out: another has the same ID or name",
                "the data of 1 shapes was left out: another page or master has the ID of theirs"
            ],
            "{path:?}: {:?}",
            written.warnings
        );
    }
}
