//! The tests of `diacraft-native`: the package a `.diacraft` file is. One module for each concern of the plan's step 1; every package is built by this
//! code, from the public specification, and no file here is one another program made. Tests may unwrap and panic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod blobs;
mod capacity;
mod cost;
mod determinism;
mod door;
mod hostile;
mod ledger_cost;
mod memory;
mod old_namespace;
mod package;
mod par_read;
mod par_write;
mod review;
mod room;
mod sniff;
mod state;
mod util;
mod vectors;
mod versions;
mod zipbuild;
