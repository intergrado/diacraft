//! `cargo fuzz run vsdx_roundtrip`: read, then splice-save unedited, then read again.
//!
//! The package is a recipe (`diacraft_vsdx_fuzz::inputs`: a part of the plain or of the carried seed replaced or spliced into) or, for a first byte that is 3 modulo 8, a plan of `vsdx_carry`
//! (`diacraft_vsdx_fuzz::plan`) on the rest. A package that is refused is left. For one that reads:
//!
//! 1. **an unedited save into the file** (`write_with` with the original bytes and the model as the baseline) is a package that reads, **the second read equals the first**, and the save wrote
//!    nothing but the name of the writer in the extended properties: no part is rewritten, added or taken out (a part no edit touched is the original's, byte for byte), so **it never writes a part
//!    the first read refused**, and the parts are the original's with the same bytes. A refusal is in words and is not a self-check that failed. (The second read equals the first but for the
//!    `Application` element of the extended properties, which every save sets to `DiaCraft`, edit or not, and the content type of a part that had none, which every save gives one: R22.)
//!    **A file that names `DiaCraft` in its extended properties is the exception**: the save gives the `Value` of its `User` and `Field` rows that hold a text the unit `STR`
//!    (`diacraft_vsdx::string_units`), so it may rewrite the pages, the masters, the document part and the time of the change (`docProps/core.xml`), and the second read is the first read with those units (but for that time).
//! 2. **a save from the model alone** (what a `.diacraft` file saved as a package gets) is a package that reads, of the same bytes when written twice, whose parts of the other format are
//!    the first read's (the same names, roles and bytes: a part the first read refused does not come back, and one it kept is not lost), and that the judge refuses nothing of: none of the
//!    carrier's words of refusal (a macro project, a name, a part twice or missing) are in the read of what the writer made (these two for a package whose solution part the first read took for
//!    DiaCraft's: a part left unread for what its relationships are is written as it was, and may be read by the next read once the writer has kept the first of two rows of one `Id`);
//! 3. **a save into the file of a model whose DiaCraft data changed** (the document's own data is the edit: the solution part is written again) never takes out a part that is not
//!    DiaCraft's: every part of the original that is not under `/diacraft/`, a relationships part, a solution part or a part of the properties is in the package with its bytes. A refusal is
//!    in words.
//!
//! 4. **a model a program made** (a first byte that is 5 modulo 8: the seed's model with parts of the other format added, and object frames of an `.odg` that name them or name nothing, written as a
//!    type with macros or one without) is written as a package that reads, whose parts are the parts that ride (a part an object frame carried back is its embedding instead, byte for byte, a
//!    compound file, and nothing is lost or made) and whose read the judge refuses nothing of, or is refused in words.
//!
//! 5. **a sheet edit** (`diacraft_vsdx_fuzz::edits`: a row inserted or deleted with the rows after it numbered again, a section taken out, a cell written, picked by the last bytes
//!    of the input), saved into the file, and a second edit saved into the file the first made with the file as it reads for the baseline (what the engine keeps after a save): each is a package
//!    that reads or a refusal in words that is not the writer's own check failing and does not say it cannot write a change to unknown nodes.
//!
//! A save costs as much as ten reads, so an input takes kind 2 or kind 3 or both, by its length modulo 3: each is made on two inputs in three. Kind 5 is made on two inputs in four (two saves).
#![no_main]

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_vsdx::{Limits, OriginalBytes, ReadReport, WriteError, WriteOptions, read, write_with};
use diacraft_vsdx_fuzz::plan::{ModelPlan, Plan};
use diacraft_vsdx_fuzz::{checks, inputs};
use libfuzzer_sys::fuzz_target;

const NOW: i64 = 1_700_000_000;

/// A refusal of the writer is in words, and is not a package that the writer made and could not read.
fn refusal_in_words(e: &WriteError) {
    assert!(checks::in_words(&e.to_string()), "a refusal that is not in words: {e:?}");
    assert!(!matches!(e, WriteError::SelfCheck(_) | WriteError::Mismatch(_)), "a save of a model that read: {e:?}");
}

/// Where two models differ, as the text of each around the first difference of their debug forms (for the message of a failed comparison).
fn first_difference(was: &Document, now: &Document) -> String {
    let (a, b) = (format!("{was:?}"), format!("{now:?}"));
    let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(0);
    let around = |text: &str| text.get(at.saturating_sub(160)..(at + 200).min(text.len())).unwrap_or_default().to_owned();
    format!("at {at}: the first read {:?}, the second {:?}", around(&a), around(&b))
}

/// Kind 1: a save into the file with no edit.
fn unedited(bytes: &[u8], doc: &Document, report: &ReadReport) {
    let options = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let original = OriginalBytes { bytes, baseline: Some(doc) };
    // A file that names `DiaCraft` as its writer is given the unit of its `User` and `Field` text by a save, edit or none: what the save writes is the model with those units, and it
    // is an edit of the file, so it also writes the time of the change (`docProps/core.xml`).
    let ours = report.application.as_deref().is_some_and(|name| name.trim() == "DiaCraft");
    let given = ours.then(|| diacraft_vsdx::string_units_in_file(doc)).flatten();
    let expected = given.as_ref().map_or(doc, |fitted| &fitted.doc);
    match write_with(doc, Some(&original), &options) {
        Ok(saved) => {
            // The save names itself as the writer in `docProps/app.xml` (`Application`), whether or not an edit was made: that part may be written, and no other, but for the parts of the drawing
            // that hold a cell the unit was given on (a page, a master or the document's own part) and the time of the change.
            let stamp = |parts: &[String]| parts.iter().all(|p| p.eq_ignore_ascii_case("/docProps/app.xml"));
            let drawing = |p: &String| given.is_some() && (checks::is_drawing_part(p) || p.eq_ignore_ascii_case("/docProps/core.xml"));
            assert!(
                saved.rewritten.iter().all(|p| stamp(std::slice::from_ref(p)) || drawing(p)) && stamp(&saved.added) && saved.removed.is_empty(),
                "an unedited save wrote: rewritten {:?}, added {:?}, removed {:?}",
                saved.rewritten,
                saved.added,
                saved.removed
            );
            let (again, second) = read(&saved.bytes, &Limits::default()).expect("a package the writer made reads");
            // (The time of the change is the save's when the save gave a unit: the read of it is not the file's own in that.)
            let read_as = |d: &Document| if given.is_some() { checks::after_an_edited_save(d) } else { checks::after_a_save(d) };
            let (now, was) = (read_as(&again), read_as(expected));
            assert!(now == was, "the second read is not the first: {}", first_difference(&was, &now));
            assert_eq!(second.carry, report.carry, "the carrier judged the saved package otherwise");
            let rewritten: Vec<String> = if given.is_some() { saved.rewritten.clone() } else { Vec::new() };
            checks::same_parts_but(bytes, &saved.bytes, &rewritten);
            assert!(saved.warnings.iter().all(|w| checks::in_words(w)), "{:?}", saved.warnings);
        }
        Err(e) => refusal_in_words(&e),
    }
}

/// Kind 2: a save from the model alone. **The parts and the judge's words are held to the first read only when it took the solution part for DiaCraft's** (`carry.items` is more than none): a part it
/// left unread for what its relationships are (another relationship naming a part of DiaCraft's, a row that is not of the type, two rows of one `Id`) is written as the package had it, and the
/// model writer keeps the first of two rows of one `Id`, so the next read may take the part and read its items, which are the file's own and are judged as any: not a part the first read
/// refused, and none written that the package did not hold.
fn from_the_model(doc: &Document, report: &ReadReport) {
    let model = WriteOptions { now: Some(NOW), content_unchanged: true, ..WriteOptions::default() };
    match write_with(doc, None, &model) {
        Ok(saved) => {
            let (back, second) = read(&saved.bytes, &Limits::default()).expect("a package the model writer made reads");
            if report.carry.items > 0 {
                assert_eq!(checks::other_parts(&back), checks::other_parts(doc), "the parts of the other format are not the first read's");
                checks::nothing_refused_in(&second);
            }
            checks::carried_parts_are_sound(&saved.bytes, &back, &second);
            let again = write_with(doc, None, &model).expect("a model that was written is written again");
            assert!(again.bytes == saved.bytes, "one model, two writes, two packages");
        }
        Err(e) => refusal_in_words(&e),
    }
}

/// Kind 3: a save into the file of a model whose DiaCraft data changed.
fn data_changed(bytes: &[u8], doc: &Document, report: &ReadReport) {
    let options = WriteOptions { now: Some(NOW), ..WriteOptions::default() };
    let original = OriginalBytes { bytes, baseline: Some(doc) };
    let mut edited = doc.clone();
    edited.ext.make_mut().odf.mimetype = Some(Arc::from("application/x-fuzz"));
    match write_with(&edited, Some(&original), &options) {
        Ok(saved) => {
            let (back, second) = read(&saved.bytes, &Limits::default()).expect("a package the writer made reads");
            let ours = report.application.as_deref().is_some_and(|name| name.trim() == "DiaCraft");
            checks::kept_what_is_not_diacraft_s(bytes, &saved.bytes, ours);
            checks::carried_parts_are_sound(&saved.bytes, &back, &second);
            assert!(saved.warnings.iter().all(|w| checks::in_words(w)), "{:?}", saved.warnings);
        }
        Err(e) => refusal_in_words(&e),
    }
}

/// Kind 4: a model that a program made (not a read), written as a package: the carrier's writer on parts a drawing may hold that no package could (a name that reaches outside, one that differs only by
/// case, a control character, a name at the cap and past it), a macro project written as a type that allows none, a role the name does not say. The write is a package that reads, with the parts
/// that ride (a macro project not among them when the type allows none, a part whose name another has by case not twice), none of the carrier's words of refusal in its read, and every part
/// sound; or it is a refusal in words (a name no package can hold, a macro project the caller did not choose to leave out).
fn written_model(plan: &ModelPlan) {
    let doc = plan.model();
    match write_with(&doc, None, &plan.options()) {
        Ok(saved) => {
            let (back, report) = read(&saved.bytes, &Limits::default()).expect("a package the model writer made reads");
            checks::nothing_refused_in(&report);
            checks::carried_parts_are_sound(&saved.bytes, &back, &report);
            assert_eq!(report.variant.map(|v| v.macro_enabled), Some(plan.allows_macros()), "the type of the package is the one asked for");
            assert_eq!(!saved.macros_dropped.is_empty(), !plan.allows_macros(), "the macro project is left out of a type that allows none, and said");
            if plan.has_frames() {
                // An object frame an `.odg` made may be written as an embedding: the parts that came back are parts that ride, and a part that did not is an embedding.
                checks::objects_lose_nothing(&doc, &back, plan.allows_macros());
            } else {
                assert_eq!(
                    checks::parts_read(&back),
                    checks::parts_that_ride(&doc, plan.allows_macros()),
                    "the parts that came back are not the parts that ride"
                );
            }
        }
        Err(e) => refusal_in_words(&e),
    }
}

/// Kind 5: a sheet edit, saved, and a second one saved after it. The last four bytes of the input say the edit.
fn sheet_edited(bytes: &[u8], doc: &Document, data: &[u8]) {
    let says = data.get(data.len().saturating_sub(4)..).unwrap_or_default();
    if let Err(why) = diacraft_vsdx_fuzz::edits::save_after_edit(bytes, doc, says) {
        panic!("{why}");
    }
}

fuzz_target!(|data: &[u8]| {
    if data.first().is_some_and(|first| first % 8 == 5) {
        return written_model(&ModelPlan::of(data.get(1..).unwrap_or_default()));
    }
    let bytes = match data.first() {
        Some(first) if first % 8 == 3 => match Plan::of(data.get(1..).unwrap_or_default()).package() {
            Some(bytes) => bytes,
            None => return,
        },
        _ => match inputs::input(data) {
            Some(input) => input.bytes,
            None => return,
        },
    };
    let Ok((doc, report)) = read(&bytes, &Limits::default()) else { return };
    unedited(&bytes, &doc, &report);
    if data.len() % 3 != 1 {
        from_the_model(&doc, &report);
    }
    if !data.len().is_multiple_of(3) {
        data_changed(&bytes, &doc, &report);
    }
    if data.len() % 4 >= 2 {
        sheet_edited(&bytes, &doc, data);
    }
});
