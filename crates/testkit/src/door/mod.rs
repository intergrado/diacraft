//! Hostile-file sweeps for the test run: deterministic mutants of a few seeds for each format, each put through the reader and the rest of the program on a thread of its
//! own with a wall-clock budget, so that a hang or a crash fails every `cargo test` and not only a fuzzer's run.
//!
//! Why this exists: a clean fuzz run and the costs measured on a fixture have both missed hangs here (chains), and a crafted input that was timed found
//! them. The fuzz targets stay (they look at random, for hours); these sweeps look at the same places on every run, in seconds, and they look **past** the reader.
//!
//! * [`seeds`]: a few files for each format, built in code (VSDX packages, OpenDocument drawings as packages and as one flat file, `.diacraft` files, draw.io files);
//! * [`zipfile`]: archives taken apart and written again, honestly or with lies in any header;
//! * [`xml`]: the XML mutations (nesting, floods, copies, entities, numbers past every range, text that is no number, formulas that read each other, loops of references, chains);
//! * [`mutate`]: the catalogue of classes, a mutant of a seed from a fixed stream of random numbers ([`rng`]);
//! * [`pipeline`]: what a mutant is put through: open, one page drawn, an edit, the saves and the opens of what they wrote;
//! * [`run`]: the sweep, the budget, the count of what got past the reader and the dump of what failed (`DIACRAFT_DOOR_DUMP`).
//!
//! A format's test file (`crates/<format>/tests/it/door.rs`) builds a [`run::Door`] from its seeds and a pipeline and calls [`run::sweep`]. `docs/development.md`, "Hostile-file
//! sweeps", has the environment variables, the numbers and how to replay a case.

pub mod mutate;
pub mod pipeline;
pub mod rng;
pub mod run;
pub mod seeds;
pub mod xml;
pub mod zipfile;

pub use mutate::{Class, Group, Mutant};
pub use pipeline::{Outcome, Passed, document_door, engine_door, engine_move_and_save};
pub use run::{Config, Door, Report, sweep};
pub use seeds::Seed;
