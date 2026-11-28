//! The door sweep of the `.diacraft` reader: deterministic hostile files in every `cargo test` run, each on a thread of its own with a wall-clock budget.
//!
//! Four seeds are what the engine writes (the whole-model codec's output, with the kinds of held data the formats it came from leave): from a small VSDX drawing, from one with a
//! little of everything, from one whose styles and masters inherit, and from an `.odg` drawing. Every class of the testkit's catalogue is applied (see `vsdx`'s `door.rs` for the
//! list and for what a mutant that the reader lets in goes through). Loops of styles and masters, formulas that read each other and a chain of shapes are made in the codec's own
//! XML (the cells of the ShapeSheet under the prefix of DiaCraft's namespace), and a mutant of a package's `state.xml`, `manifest.xml` or `document.xml` reaches the reader
//! with right checksums.
//!
//! Before the engine meets a mutant, the reader reads it on its own and the container's counts of the read are held (`held_to_its_counts`): linear in the file (a scan for the end
//! record of at most 65,536 places, no more rows than the file has room for, no more bytes of names than the file holds).
//!
//! `DIACRAFT_DOOR_CASES=5000` looks further; `DIACRAFT_DOOR_CASE=17 DIACRAFT_DOOR_DUMP=1` replays one case and keeps its file. `docs/development.md`, "Hostile-file sweeps".

use std::sync::Arc;

use diacraft_native::{Limits, read};
use diacraft_testkit::door::{Config, Door, engine_door, seeds, sweep};

/// The cases a run makes for the format when the environment says nothing.
const CASES: usize = 300;

/// The container's counts of a read of `bytes` that was let in: linear in the file.
fn held_to_its_counts(bytes: &[u8]) {
    let Ok((_, opened)) = read(bytes, &Limits::default()) else { return };
    let (len, work) = (bytes.len() as u64, opened.work);
    assert!(work.end_scan <= 65_536 + 22, "{} places looked at for the end record", work.end_scan);
    assert!(work.rows.saturating_mul(46) <= len, "{} rows in a file of {len} bytes", work.rows);
    assert!(work.name_bytes <= len.saturating_mul(2), "{} bytes of names in a file of {len} bytes", work.name_bytes);
}

fn door() -> Door {
    let pipeline = |bytes: &[u8], extension: &str, case: usize| {
        held_to_its_counts(bytes);
        engine_door(bytes, extension, case)
    };
    Door { name: "diacraft", seeds: seeds::diacraft(), pipeline: Arc::new(pipeline) }
}

#[test]
fn mutants_of_the_diacraft_seeds_end_as_a_read_or_a_refusal_in_words_and_the_ones_that_get_in_go_through_the_rest_of_the_program() {
    let door = door();
    let report = sweep(&door, &Config::from_env(CASES));
    report.assert_clean();
    report.assert_classes(&door.seeds);
    report.assert_door(25, 11);
}
