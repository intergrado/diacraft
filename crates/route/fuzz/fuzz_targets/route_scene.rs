//! `cargo fuzz run route_scene`: obstacles, wires, settings and a request from bytes, routed within a work budget.
//!
//! How the input is read, and the properties it is held to, are the library's (`src/lib.rs`), which the router's tests include: no panic, no hang, the work never past its
//! budget, the loops walking what they charge, the same answer twice, and a route that holds the router's invariants.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| diacraft_route_fuzz::check(data));
