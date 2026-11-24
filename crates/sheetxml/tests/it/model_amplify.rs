//! **Memory amplification through sharing.** A drawing of many shapes that hold one big sheet is a sheet and a pointer for each
//! shape, as a model, and **a file says the sheet once** (`same`, the vocabulary's 3.19), so that what is written is what the model holds. A write to a shape makes the
//! shape's own copy of the sheet, and that is the edit's to charge (the engine's `Session::edit_if` refuses an edit past what the model may hold; its test is in
//! `crates/engine`); what a save does with a model that the edits made too large is to count it, as it is held, before it makes a tree, and to refuse it in words.
//! Each case here runs in a child under `ulimit -v 3000000`, and ends refused in words or within memory; an abort is the failure.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;

use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_doc::{Document, Page, Section, Shape, ShapeList, Sheet};
use diacraft_sheetxml::model::{
    Budget, CodecError, MemBlobs, OPEN_MODEL_BYTES, PartSource, URN, decode_from, encode, encode_for_save, held_bytes, open_limits, save_parts,
};

const SCENARIO: &str = "SHEETXML_AMPLIFY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

/// The sections of a sheet at the model's cap: 8,192, each of a name of its own and one cell: 32,771 items for the shape, and 14 MB of model once nothing is shared
/// (a map of cells makes a leaf of eleven at its first cell, so a section of one cell is 1.5 KB and one of none is 160 bytes).
const SECTIONS: usize = diacraft_doc::MAX_SECTIONS_PER_SHEET;

fn sheet_inside() -> String {
    (0..SECTIONS).map(|k| format!("<Section N=\"s{k}\"><Cell N=\"c\"/></Section>")).collect()
}

/// A part of `shapes` shapes that each hold the same sheet: written in full for each (the file of a writer that does not say it once), or once, named, and as a `same`
/// for the others.
fn page_text(shapes: usize, first: usize, sheet: &str, said_once: bool) -> String {
    let body: String = (0..shapes)
        .map(|i| {
            let id = first + i;
            match (said_once, i) {
                (false, _) => format!("<shape id=\"{id}\"><sheet>{sheet}</sheet></shape>"),
                (true, 0) => format!("<shape id=\"{id}\"><sheet def=\"1\">{sheet}</sheet></shape>"),
                (true, _) => format!("<shape id=\"{id}\"><same of=\"1\"/></shape>"),
            }
        })
        .collect();
    format!("<page xmlns=\"{URN}\">{body}</page>")
}

/// A source that makes each page's text when it is asked for it and parses it at the limits of an open.
struct Made {
    pages: usize,
    shapes: usize,
    sheet: String,
    said_once: bool,
}

impl PartSource for Made {
    type Tree = El;

    fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
        if index >= self.pages {
            return Err(CodecError::Malformed { part: "page".to_owned(), why: "no such part" });
        }
        let text = page_text(self.shapes, index * self.shapes + 1, &self.sheet, self.said_once);
        parse_with(text.as_bytes(), &open_limits().xml)
            .map_err(|_| CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" })
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "no such part" })
    }
}

fn document_text(pages: usize) -> El {
    let refs: String = (0..pages).map(|_| "<page-ref/>").collect();
    parse_with(format!("<document xmlns=\"{URN}\">{refs}</document>").as_bytes(), &open_limits().xml).unwrap()
}

/// Gives every shape, section and sheet of the drawing a copy of its own, as a program that edits each of them does: the model once nothing is shared.
fn unshare_everything(doc: &mut Document) -> usize {
    let mut sections = 0;
    for p in 0..doc.pages.len() {
        let Some(page) = doc.pages.get_mut(p) else { continue };
        let page = Arc::make_mut(page);
        for s in 0..page.shapes.len() {
            let Some(shape) = page.shapes.get_mut(s) else { continue };
            let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
            for section in &mut sheet.sections {
                let section = Arc::make_mut(section);
                section.del = !section.del;
                sections += 1;
            }
        }
    }
    sections
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn amplify_child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    match scenario.as_str() {
        // The most-shared file there is: the same sheet on 900 shapes, 13 GB of model once nothing is shared, in a file that says it once for each page: it is opened
        // as what it holds (a sheet for each page and a pointer for each shape).
        "said_once" => {
            let mut source = Made { pages: 9, shapes: 100, sheet: sheet_inside(), said_once: true };
            let mut budget = Budget::for_open(&open_limits());
            let doc =
                decode_from(&document_text(9), &mut source, &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("a file that says its sheet once");
            let charged = OPEN_MODEL_BYTES - budget.bytes;
            let held = held_bytes(&doc);
            println!("AMPLIFY said_once: charged {} MiB, held {} MiB, VmPeak {} MiB", charged >> 20, held >> 20, status_kib("VmPeak") >> 10);
            assert!(charged < 40 << 20, "nine sheets and nine hundred pointers: {} MiB", charged >> 20);
            // And it is saved as what it holds: each page names its sheet once.
            let mut written = 0u64;
            let mut parts = 0usize;
            save_parts(&doc, &mut MemBlobs::new(), &open_limits(), &mut |p| {
                written += u64::try_from(to_bytes(p.tree, &[], &[], true).map_err(CodecError::from)?.len()).unwrap_or(u64::MAX);
                parts += 1;
                Ok(())
            })
            .expect("saved as it is held");
            println!("AMPLIFY said_once: {parts} parts, {} KiB written, VmPeak {} MiB", written >> 10, status_kib("VmPeak") >> 10);
            assert_eq!(parts, 10);
            assert!(written < 8 << 20, "nine sheets and nine hundred references: {written} bytes");
            let held_whole = encode(&doc, &mut MemBlobs::new()).expect("the whole document encoder too");
            assert_eq!(held_whole.pages.len(), 9);
        }
        // The same file in a writer's other spelling (the sheet in full for each shape, 33,000 items each): the decode shares the equal sheets it reads, and what is
        // saved is what is held.
        "written_in_full" => {
            let mut source = Made { pages: 2, shapes: 50, sheet: sheet_inside(), said_once: false };
            let mut budget = Budget::for_open(&open_limits());
            let doc = decode_from(&document_text(2), &mut source, &MemBlobs::new(), &mut budget, &mut Vec::new()).expect("equal sheets, read once");
            let charged = OPEN_MODEL_BYTES - budget.bytes;
            println!("AMPLIFY written_in_full: charged {} MiB, VmPeak {} MiB", charged >> 20, status_kib("VmPeak") >> 10);
            assert!(charged < 40 << 20);
            drop(encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("and the save that holds every part"));
        }
        // The most-shared drawing, every shape of it edited (a copy of the sheet for each): the model held is 1.2 GB, past what an open takes of a file that says
        // each in full, and within what a save holds. A save writes it as what it holds (each sheet in full: they are copies of one another that no pointer says), and the
        // check of the save reads it as an open does, **which shares what is equal**, so what the open would be charged is a sheet and pointers: it is saved, a tree at
        // a time, and the open of it takes what it holds.
        "edited_everywhere" => {
            let mut source = Made { pages: 2, shapes: 50, sheet: sheet_inside(), said_once: true };
            let mut doc = decode_from(&document_text(2), &mut source, &MemBlobs::new(), &mut Budget::for_open(&open_limits()), &mut Vec::new())
                .expect("the file that says its sheet once");
            let sections = unshare_everything(&mut doc);
            let held = held_bytes(&doc);
            println!("AMPLIFY edited_everywhere: {sections} sections unshared, held {} MiB, VmPeak {} MiB", held >> 20, status_kib("VmPeak") >> 10);
            assert!(
                held > OPEN_MODEL_BYTES && held < diacraft_sheetxml::model::EDIT_MODEL_BYTES,
                "within what a save holds, past what an open takes as it is held"
            );
            let (mut parts, mut written) = (0usize, 0u64);
            save_parts(&doc, &mut MemBlobs::new(), &open_limits(), &mut |p| {
                written += u64::try_from(to_bytes(p.tree, &[], &[], true).map_err(CodecError::from)?.len()).unwrap_or(u64::MAX);
                parts += 1;
                Ok(())
            })
            .expect("saved a part at a time: what is equal is shared by the check, as an open shares it");
            assert_eq!(parts, 3);
            println!("AMPLIFY edited_everywhere: {parts} parts, {} MiB written, VmPeak {} MiB", written >> 20, status_kib("VmPeak") >> 10);
        }
        // A big model that shares nothing, as a reader that builds each shape's sheet and sections apart makes one (the `.vsdx` reader's model of 140,000 shapes is two
        // gigabytes): the save counts what it is before it makes a tree, and refuses it in words, where it once ran out of address space beside the first part.
        "big_model" => {
            let base = super::model_capacity::base_shape("typical");
            let mut doc = Document::empty();
            let per = 50_000usize;
            for p in 0..6 {
                let mut page = Page::new(u32::try_from(p + 1).unwrap(), "P");
                let shapes: Vec<Arc<Shape>> = (0..per)
                    .map(|i| {
                        let mut shape = base.clone();
                        shape.id = u32::try_from(p * per + i + 1).unwrap();
                        // Every sheet, section and row its own, with a cell of its own value: nothing is shared with any other.
                        let sheet = Arc::make_mut(&mut shape.sheet);
                        for section in &mut sheet.sections {
                            let section = Arc::make_mut(section);
                            for row in section.rows.iter_mut() {
                                Arc::make_mut(row).del = false;
                            }
                        }
                        Arc::new(shape)
                    })
                    .collect();
                page.shapes = ShapeList::from_vec(shapes);
                doc.pages.push(Arc::new(page));
            }
            println!("AMPLIFY big_model: the model is built, VmRSS {} MiB", status_kib("VmRSS") >> 10);
            let mut parts = 0usize;
            let saved = save_parts(&doc, &mut MemBlobs::new(), &open_limits(), &mut |_| {
                parts += 1;
                Ok(())
            });
            println!(
                "AMPLIFY big_model: {parts} parts handed over, then {:?}, VmPeak {} MiB",
                saved.as_ref().err().map(ToString::to_string),
                status_kib("VmPeak") >> 10
            );
            assert_eq!(parts, 0, "refused before the first part is made");
            match saved {
                Err(CodecError::Limit { what: "bytes of the model in memory", .. }) => {}
                other => panic!("the save of a model of 300,000 shapes that share nothing: {:?}", other.err()),
            }
            let held = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect_err("and the save that holds every part");
            assert!(matches!(held, CodecError::Limit { what: "bytes of the model in memory", .. }), "{held}");
        }
        other => panic!("no scenario {other}"),
    }
    println!("amplification: no abort");
}

#[cfg(target_os = "linux")]
fn run(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_amplify::amplify_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("amplification: no abort"), "{scenario}: {text}");
    eprintln!("{}", text.lines().filter(|l| l.contains("AMPLIFY")).collect::<Vec<_>>().join("\n"));
    text
}

#[test]
#[cfg(target_os = "linux")]
fn a_file_of_one_sheet_on_nine_hundred_shapes_that_says_it_once_is_opened_as_what_it_holds_and_saved_as_what_it_holds() {
    run("said_once");
}

#[test]
#[cfg(target_os = "linux")]
fn the_same_sheet_written_in_full_on_every_shape_is_shared_at_the_open_and_saved_as_what_is_held() {
    run("written_in_full");
}

#[test]
#[cfg(target_os = "linux")]
fn the_most_shared_drawing_edited_everywhere_is_saved_as_what_it_holds_and_the_open_of_it_shares_what_is_equal() {
    run("edited_everywhere");
}

#[test]
#[cfg(target_os = "linux")]
fn a_big_model_that_shares_nothing_is_refused_in_words_before_any_part_is_made_and_does_not_run_out_of_address_space() {
    run("big_model");
}

/// The unit that a model that shares is saved as: a sheet that three shapes hold is written once with a name and the others are `same` elements that name it.
#[test]
fn a_sheet_that_three_shapes_hold_is_written_once_and_named_and_the_others_say_so() {
    let sheet = Arc::new(Sheet { sections: vec![Arc::new(Section::new("User", None))], ..Sheet::default() });
    let mut doc = Document::empty();
    let mut page = Page::new(1, "P");
    let shapes: Vec<Arc<Shape>> = (1..=3)
        .map(|id| {
            let mut shape = Shape::new(id);
            shape.sheet = Arc::clone(&sheet);
            Arc::new(shape)
        })
        .collect();
    page.shapes = ShapeList::from_vec(shapes);
    doc.pages.push(Arc::new(page));
    let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
    let text = String::from_utf8(to_bytes(&encoded.pages[0].1, &[], &[], false).unwrap()).unwrap();
    assert_eq!(text.matches("<diacraft:sheet def=\"1\" fp=\"").count(), 1, "a name, and the fingerprint of the unit: {text}");
    assert_eq!(text.matches("<diacraft:same of=\"1\"/>").count(), 2, "{text}");
    assert_eq!(text.matches("<diacraft:Section").count(), 1, "the section is written once: {text}");
}
