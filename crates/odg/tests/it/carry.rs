//! DiaCraft's data in `.odg` files (the native format's design, 5.1, 5.3, 5.4 and 5.6; `docs/diacraft-format.md` 7.3). A drawing that holds VSDX data and is written as an `.odg`
//! carries that data inside the file, so that it comes back when the file is read again: on a shape's own automatic style (tier 2), in DiaCraft's document file, and in the `DiaCraft/`
//! entries. **Every read-back goes through the real reader (`read`) and every file through the real writer (`write_with`)**; a file another program edited is the written one changed by hand at
//! the text of one part and packed again, so that the reader meets what such an edit leaves.

use diacraft_container::zip::{ArchiveWriter, Method};
use diacraft_doc::{Document, Shape, TextPart};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::carry_model::{assert_same_data, vsdx_data_of, vsdx_model};
use crate::model_text::{chars, content_of, doc_with, first_shape, row, section};

pub(crate) fn written(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the writer refused: {e} ({})", e.class())).bytes
}

// ------------------------------------------------------------------------------------------------------------------------------ helpers

/// The package `bytes` with the entry `name` changed by `f` (text in, text out), which must change it, packed again with right checksums.
pub(crate) fn repack(bytes: &[u8], name: &str, f: impl FnOnce(String) -> String) -> Vec<u8> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let mut writer = ArchiveWriter::new();
    let mut changed = false;
    let mut f = Some(f);
    for entry in package.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        let mut data = package.part(&entry.name).unwrap().to_vec();
        if entry.name == name {
            let before = String::from_utf8(data).unwrap();
            let after = f.take().map(|f| f(before.clone())).unwrap();
            changed = before != after;
            data = after.into_bytes();
        }
        let method = if entry.name == "mimetype" { Method::Stored } else { Method::Deflated };
        writer.add(&entry.name, &data, method).unwrap();
    }
    assert!(changed, "the edit of {name} changed nothing");
    writer.finish().unwrap()
}

/// The `Style` cell of the run that starts with `starts` in the first shape of the model: the row its `Cp` marker names.
fn run_style(doc: &Document, starts: &str) -> Option<f64> {
    let shape: &Shape = first_shape(doc);
    let text = shape.text.as_ref()?;
    let mut row_ix: Option<u32> = None;
    for part in &text.parts {
        match part {
            TextPart::Cp(ix) => row_ix = Some(*ix),
            TextPart::Chars(c) if c.starts_with(starts) => {
                let ix = row_ix?;
                let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character")?;
                let row = section.rows.iter().find(|r| r.ix == Some(ix))?;
                return row.cells.get("Style")?.value.as_ref()?.number();
            }
            _ => {}
        }
    }
    None
}

/// 2026-11-22: a carried item stands only while its stamp holds (the design's 5.4), and the stamp of a carried text covers its formatting as the markup says it,
/// not only its characters. A text DiaCraft made is carried with its rows, and written natively as spans of automatic text styles; a formatting edit made in another program changes the markup
/// and not the characters, and the file wins.
#[test]
fn a_formatting_only_edit_of_a_carried_text_in_another_program_wins() {
    // A run in italics, as DiaCraft made it: the model's row says italic, the span says so too.
    let parts = vec![chars("plain "), TextPart::Cp(1), chars("it"), TextPart::Cp(0), chars(" end")];
    let doc = doc_with(parts, vec![section("Character", vec![row(1, &[("Style", 2.0)])])]);
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the save is refused: {e}"));
    // Unedited, the carried text is the model's: the run is italic, and only italic.
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(run_style(&back, "it"), Some(2.0));
    // Another program makes the run bold as well: the markup says so, and the characters are the same.
    let xml = content_of(&written.bytes);
    assert!(xml.contains("fo:font-style=\"italic\""), "{xml}");
    assert!(
        xml.contains("diacraft:fp-text=") && xml.contains("diacraft:fp-format="),
        "the carried text is stamped with its characters and its formatting: {xml}"
    );
    let edited = repack(&written.bytes, "content.xml", |text| {
        // The run's style says its weight as it says the rest: whichever way it says it, it now says bold.
        if text.contains("fo:font-weight=\"normal\"") {
            text.replacen("fo:font-weight=\"normal\"", "fo:font-weight=\"bold\"", 1)
        } else {
            text.replacen("fo:font-style=\"italic\"", "fo:font-style=\"italic\" fo:font-weight=\"bold\"", 1)
        }
    });
    let (back, report) = read(&edited, &Limits::default()).unwrap();
    assert_eq!(run_style(&back, "it"), Some(3.0), "the file's bold wins over the carried text's italic alone");
    assert_eq!(report.warned("diacraft_stale"), 1, "the carried text is told stale: {:?}", report.warning_counts);
}

// ------------------------------------------------------------------------------------------------------------------------------ the plan's tests

#[test]
fn vsdx_data_rides_and_comes_back() {
    let model = vsdx_model();
    let before = vsdx_data_of(&model);
    assert!(before.len() > 45, "the fixture holds data in many places: {before:#?}");
    let bytes = written(&model);
    let (back, report) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(report.warned("diacraft_stale"), 0, "{:?}", report.warnings);
    assert_eq!(report.carry.stale + report.carry.gone + report.carry.left_out, 0, "{:?} {:?}", report.carry, report.warnings);
    assert_same_data(&vsdx_data_of(&back), &before, "the trip");
}
