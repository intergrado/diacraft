//! The door sweep of the `.vsdx` reader: deterministic hostile files in every `cargo test` run, each on a thread of its own with a wall-clock budget.
//!
//! Three seeds built in code (a small drawing, one with a little of everything the reader reads, one whose styles and masters inherit) are changed by every class of the testkit's
//! catalogue (`diacraft_testkit::door::mutate`): bytes flipped, overwritten, cut and spliced; entries written twice, taken out, moved, renamed and swapped for a bomb; a package
//! whose headers lie about sizes, offsets, checksums, names, methods and the central directory; and XML with a nest of groups, a flood of attributes, copies, entity references,
//! numbers past every range, text that is no number, formulas that read each other, loops of styles and masters, and a chain of shapes. A mutant that the reader lets in goes on
//! through the engine: a page is drawn, a shape is moved, and the drawing is saved (as the file it came from, as `.diacraft` and as `.odg`) and each file is opened again. The
//! sweep fails on a panic, on a bug the engine's guard caught, on a frame that met a bug, on a refusal with no words, on a file the engine wrote and will not read, and on a case
//! that does not end in its budget; and it fails if too few mutants get past the reader (a sweep that only exercises the parser).
//!
//! Before the engine meets a mutant, the reader reads it on its own and its own meters are held to its own limits (`held_to_its_limits`): a read that is let in has stayed within the
//! items, cells, shapes, formula nodes, bytes of XML, depth and memory it is given, in the units it charges, as the crate's cost tests hold it.
//!
//! `DIACRAFT_DOOR_CASES=5000` looks further; `DIACRAFT_DOOR_CASE=17 DIACRAFT_DOOR_DUMP=1` replays one case and keeps its file. `docs/development.md`, "Hostile-file sweeps".

use std::sync::Arc;

use diacraft_testkit::door::{Config, Door, Outcome, engine_door, seeds, sweep};
use diacraft_vsdx::{Limits, read_partial};

/// The cases a run makes for the format when the environment says nothing.
const CASES: usize = 300;

/// The reader's own counts of a read of `bytes`, held to the limits it was given: a read that was let in stayed within every one.
fn held_to_its_limits(bytes: &[u8]) {
    let limits = Limits::default();
    let (result, report) = read_partial(bytes, &limits);
    if result.is_err() {
        return;
    }
    let s = &report.stats;
    assert!(s.items <= limits.max_items, "{} items against a limit of {}", s.items, limits.max_items);
    assert!(s.cells <= limits.max_cells, "{} cells against a limit of {}", s.cells, limits.max_cells);
    assert!(s.shapes <= limits.max_shapes, "{} shapes against a limit of {}", s.shapes, limits.max_shapes);
    assert!(s.formula_nodes <= limits.max_formula_nodes, "{} formula nodes against a limit of {}", s.formula_nodes, limits.max_formula_nodes);
    assert!(s.xml_bytes <= limits.max_xml_bytes, "{} bytes of XML against a limit of {}", s.xml_bytes, limits.max_xml_bytes);
    assert!(s.max_shape_depth <= limits.max_shape_depth, "shapes {} deep against a limit of {}", s.max_shape_depth, limits.max_shape_depth);
    assert!(s.memory_peak <= limits.max_memory, "a pool of {} bytes against a limit of {}", s.memory_peak, limits.max_memory);
}

fn door() -> Door {
    let pipeline = |bytes: &[u8], extension: &str, case: usize| {
        held_to_its_limits(bytes);
        engine_door(bytes, extension, case)
    };
    Door { name: "vsdx", seeds: seeds::vsdx(), pipeline: Arc::new(pipeline) }
}

#[test]
fn mutants_of_the_vsdx_seeds_end_as_a_read_or_a_refusal_in_words_and_the_ones_that_get_in_go_through_the_rest_of_the_program() {
    let door = door();
    let report = sweep(&door, &Config::from_env(CASES));
    report.assert_clean();
    report.assert_classes(&door.seeds);
    report.assert_door(25, 12);
}

// ---------------------------------------------------------------------------------------------------------------- found by the sweeps

/// **A finding of the sweeps, fixed.** A package whose `docProps/app.xml` is a megabyte of one repeated pattern (it is not well formed, and the reader keeps
/// such a part as the bytes it is) opened, and then **no save of it worked**: `file.save` was refused in words ("the file just made does not read back, so it was not kept: part
/// `docProps/app.xml` inflates more than 1000 times its size") and so was `file.save_as` a `.diacraft` (the same of `blobs/0.xml`). The writer deflated the part harder than the file
/// it came from was (the test's archive writer deflates fast), and the container's own check of what was written refuses a ratio past 1,000. The same of `docProps/core.xml`, and of
/// a megabyte of spaces; 64 KiB of `<a/>` was saved. Found by case 81 of the `.vsdx` sweep at the stream the sweep has by default (a bomb: the entry replaced by 1,048,576 bytes of
/// empty elements). No file was lost (the engine's check refuses its own output), but a drawing that opens was one that could not be saved. The container's writer now stores an entry
/// that deflate would make a thousand times smaller and more (`ArchiveWriter::add_smallest` and `pack`: `readable_ratio`), which the OPC writer of every part writes through.
#[test]
fn a_properties_part_the_writer_deflates_past_the_ratio_is_saved_all_the_same() {
    // The two properties parts, each a megabyte of one pattern: empty elements (the sweep's case) and spaces; every save (`file.save`, as a `.diacraft` and as an `.odg`) is made.
    for part in ["docProps/app.xml", "docProps/core.xml"] {
        for (what, data) in [("empty elements", "<a/>".repeat(262_144)), ("spaces", " ".repeat(1_048_576))] {
            let mut archive = seeds::vsdx().remove(0).archive.unwrap();
            let at = archive.find(part).unwrap();
            archive.entries[at].data = data.into_bytes();
            match engine_door(&archive.honest(), "vsdx", 1) {
                Outcome::Passed(passed) => {
                    let ratio: Vec<&String> = passed.refused.iter().map(|(_, whole)| whole).filter(|w| w.contains("inflates more than")).collect();
                    assert!(ratio.is_empty(), "{part}, {what}: saves refused for the ratio: {ratio:?}");
                    assert!(passed.saved >= 2, "{part}, {what}: the saves were made: {}", passed.saved);
                }
                other => panic!("{part}, {what}: the package should open and go through: {other:?}"),
            }
        }
    }
}
