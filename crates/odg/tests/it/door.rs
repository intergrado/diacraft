//! The door sweeps of the OpenDocument reader, packaged (`.odg`) and flat (`.fodg`): deterministic hostile files in every `cargo test` run, each on a thread of its own
//! with a wall-clock budget.
//!
//! The packaged seeds are two drawings written by hand from the standard (a plain page; two pages with groups, a frame, an ellipse, a line and a connector glued to shapes, under styles
//! that name their parents) and one the engine writes from a VSDX drawing (with DiaCraft's own data in it). The flat seeds are the two by hand as one XML document. Every class of
//! the testkit's catalogue is applied (for a flat file, those that need no package): see `vsdx`'s `door.rs` for the list and for what a mutant that the reader lets in goes through.
//! Loops of styles (`style:parent-style-name`), of shapes a connector is glued to (`draw:start-shape`) and of master pages, a chain of glued connectors, and groups nested
//! thousands deep are among the XML mutations.
//!
//! Before the engine meets a mutant, the reader reads it on its own and its own meters are held (`held_to_its_counts`): the container's counts are linear in the file (a scan for the end
//! record of at most 65,536 places, no more rows than the file has room for, no more bytes of names than the file holds), and a read that is let in has stayed within the items and
//! the bytes of text it is given.
//!
//! `DIACRAFT_DOOR_CASES=5000` looks further; `DIACRAFT_DOOR_CASE=17 DIACRAFT_DOOR_DUMP=1` replays one case and keeps its file. `docs/development.md`, "Hostile-file sweeps".

use std::sync::Arc;

use diacraft_odg::{Limits, read_partial};
use diacraft_testkit::door::{Config, Door, Outcome, engine_door, seeds, sweep};

/// The cases a run makes for each of the two forms when the environment says nothing.
const CASES: usize = 300;

/// The reader's own counts of a read of `bytes`: the package's are linear in the file whether the read ended or was refused, and a drawing that was let in stayed within its limits.
fn held_to_its_counts(bytes: &[u8]) {
    let limits = Limits::default();
    let (result, report) = read_partial(bytes, &limits);
    let (len, package) = (bytes.len() as u64, report.work.package);
    assert!(package.end_scan <= 65_536 + 22, "{} places looked at for the end record", package.end_scan);
    assert!(package.rows.saturating_mul(46) <= len, "{} rows in a file of {len} bytes", package.rows);
    assert!(package.name_bytes <= len.saturating_mul(2), "{} bytes of names in a file of {len} bytes", package.name_bytes);
    if result.is_ok() {
        assert!(report.work.items <= limits.max_items as u64, "{} items against a limit of {}", report.work.items, limits.max_items);
        assert!(
            report.work.text_bytes <= limits.max_text_bytes as u64,
            "{} bytes of text against a limit of {}",
            report.work.text_bytes,
            limits.max_text_bytes
        );
    }
}

fn door(name: &'static str, seeds: Vec<diacraft_testkit::door::Seed>) -> Door {
    let pipeline = |bytes: &[u8], extension: &str, case: usize| {
        held_to_its_counts(bytes);
        engine_door(bytes, extension, case)
    };
    Door { name, seeds, pipeline: Arc::new(pipeline) }
}

#[test]
fn mutants_of_the_odg_seeds_end_as_a_read_or_a_refusal_in_words_and_the_ones_that_get_in_go_through_the_rest_of_the_program() {
    let door = door("odg", seeds::odg());
    let report = sweep(&door, &Config::from_env(CASES));
    report.assert_clean();
    report.assert_classes(&door.seeds);
    report.assert_door(25, 12);
}

#[test]
fn mutants_of_the_flat_odg_seeds_end_as_a_read_or_a_refusal_in_words_and_the_ones_that_get_in_go_through_the_rest_of_the_program() {
    let door = door("fodg", seeds::fodg());
    let report = sweep(&door, &Config::from_env(CASES));
    report.assert_clean();
    report.assert_classes(&door.seeds);
    report.assert_door(25, 7);
}

// ---------------------------------------------------------------------------------------------------------------- found by the sweeps

/// The namespace of the `svg:` attributes of an OpenDocument drawing.
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";

/// **A bug the sweeps found, fixed.** A drawing whose root binds the prefix `svg` to another namespace than OpenDocument's, with a connector glued to a
/// shape, opens; after the shape is moved (the connector is routed again), `file.save` wrote a `content.xml` that was not well formed: the new attributes of the connector are written
/// in the `svg` namespace under a prefix of the writer's own (`svg1`), and the declaration `xmlns:svg1` was written twice on the one element (a duplicate attribute), once by each of the
/// two edits of the connector (its ends, then its path). The save said it had written the file, and the engine would not open the file it wrote. Found by case 192 of the flat sweep at
/// `DIACRAFT_DOOR_SEED=11` (3 bits flipped in `fodg-plain`, one of them in this namespace); the vector is the seed with the namespace changed, flat or packaged. Another prefix for the
/// right namespace (`s:`) was saved right, and so was the drawing with no connector. `write_prefixes.rs` has the vectors of the file's text (the prefixes of each namespace, nested scopes,
/// a page added with a comment), and the writer now refuses a part with a tag that names an attribute twice (`write/unique.rs`).
#[test]
fn a_drawing_whose_svg_prefix_names_another_namespace_is_saved_after_a_move_as_a_file_that_opens_again() {
    let flat = String::from_utf8(seeds::fodg().remove(0).bytes).unwrap().replace(SVG, "urn:x:other");
    let outcome = engine_door(flat.as_bytes(), "fodg", 0);
    assert!(!matches!(outcome, Outcome::Failed(_)), "flat: {outcome:?}");
    let seed = seeds::odg().remove(0);
    let mut archive = seed.archive.unwrap();
    let at = archive.find("content.xml").unwrap();
    archive.entries[at].data = String::from_utf8(archive.entries[at].data.clone()).unwrap().replace(SVG, "urn:x:other").into_bytes();
    let outcome = engine_door(&archive.honest(), "odg", 0);
    assert!(!matches!(outcome, Outcome::Failed(_)), "packaged: {outcome:?}");
}

/// **A finding of the sweeps, fixed.** A line (`draw:line`) whose `svg:y2` is `infcm`, `NaNcm`, `1e999cm` or even `1e308cm` opens (none of them is a length: the
/// reader read the end as absent and **held its text**), and then `file.save_as` an `.odg` was refused in words: "the file just made does not read back, so it was not kept: the reader
/// does not read what the save wrote as y2". The same of `svg:x1`, `svg:y1` and `svg:x2`; an ellipse with `svg:width="infcm"` is saved. Found by case 161 of the flat sweep at the stream
/// the sweep has by default (`not numbers`). No file was lost (the engine's check refuses its own output, and `file.save` of the flat file still worked), but a drawing the reader let in
/// could not be saved as an `.odg`. The reader no longer holds a frame attribute it does not read, and says it read it as absent (`read_unread_frames.rs` has the vectors of the model).
#[test]
fn a_line_with_a_coordinate_that_is_no_number_is_refused_at_the_open_or_saved_as_an_odg() {
    let seed = String::from_utf8(seeds::fodg().remove(1).bytes).unwrap();
    for value in ["infcm", "NaNcm", "1e999cm", "1e308cm"] {
        let changed = seed.replacen("svg:y2=\"13cm\"", &format!("svg:y2=\"{value}\""), 1);
        assert_ne!(changed, seed, "the seed has the attribute this test changes");
        match engine_door(changed.as_bytes(), "fodg", 1) {
            Outcome::Passed(passed) => {
                let unread: Vec<&String> =
                    passed.refused.iter().map(|(_, whole)| whole).filter(|w| w.contains("does not read what the save wrote")).collect();
                assert!(unread.is_empty(), "{value}: {unread:?}");
            }
            Outcome::Refused(_) => {}
            Outcome::Failed(what) => panic!("{value}: {what}"),
        }
    }
}
