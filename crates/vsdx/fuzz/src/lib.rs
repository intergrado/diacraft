//! The parts of the VSDX fuzz targets (`vsdx_read`, `vsdx_carry`, `vsdx_roundtrip`) that more than one uses: the seeds (packages the tests of `crates/vsdx` build), how a target's bytes become a
//! package (`inputs`, `plan`) and the properties they hold (`checks`).
pub mod checks;
pub mod edits;
pub mod inputs;
#[path = "../../../testkit/src/package.rs"]
pub mod package;
pub mod plan;
pub mod seeds;
