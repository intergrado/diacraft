//! The door sweep of the draw.io reader: deterministic hostile files in every `cargo test` run, each on a thread of its own with a wall-clock budget.
//!
//! Three seeds are built in code by the testkit's encoder (a plain page, a compressed page and a file of two pages: vertices, an edge with a point, a container and a label of HTML).
//! Every class of the testkit's catalogue that needs no package is applied (see `vsdx`'s `door.rs` for the list): bytes flipped, overwritten, cut and spliced; XML with a flood of
//! attributes, copies, entity references, numbers past every range, text that is no number, loops of `parent`, `source` and `target`, and a chain of cells that are each in the one
//! before. The engine does not open draw.io files yet, so the door here is the reader itself: the drawing it makes is written as a `.diacraft` file, which goes through the
//! engine's open, one page drawn, a move, the saves and the opens of what they wrote (`diacraft_testkit::door::document_door`).
//!
//! The reader is held to its own meter on every mutant, whether it reads or refuses: the work it counted (`ReadWork::total`, the sum of the units each loop charges before it runs) is
//! within `Limits::max_read_work`.
//!
//! `DIACRAFT_DOOR_CASES=5000` looks further; `DIACRAFT_DOOR_CASE=17 DIACRAFT_DOOR_DUMP=1` replays one case and keeps its file. `docs/development.md`, "Hostile-file sweeps".

use std::sync::Arc;

use diacraft_mxfile::{Limits, ReadWork, read_counted};
use diacraft_testkit::door::{Config, Door, document_door, seeds, sweep};

/// The cases a run makes for the format when the environment says nothing.
const CASES: usize = 300;

fn door() -> Door {
    let parse = |bytes: &[u8]| {
        let limits = Limits::default();
        let mut work = ReadWork::default();
        let read = read_counted(bytes, &limits, &mut work);
        assert!(work.total() <= limits.max_read_work, "{} steps of work against a limit of {}", work.total(), limits.max_read_work);
        read.map(|read| read.doc).map_err(|e| e.to_string())
    };
    Door { name: "mxfile", seeds: seeds::drawio(), pipeline: Arc::new(move |bytes, _extension, case| document_door(&parse, bytes, case)) }
}

#[test]
fn mutants_of_the_drawio_seeds_end_as_a_read_or_a_refusal_in_words_and_the_ones_that_get_in_go_through_the_rest_of_the_program() {
    let door = door();
    let report = sweep(&door, &Config::from_env(CASES));
    report.assert_clean();
    report.assert_classes(&door.seeds);
    report.assert_door(25, 6);
}
