//! The `container` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code: by hand from the public PKWARE APPNOTE and from the package specifications (whose vectors carry their id),
//! by this crate's own writer, or as a hostile case that no honest writer
//! makes (`zipbuild.rs`). No file made by any other program is read. The `opc` crate's tests read the same code through `opc` and stay
//! there; these hold what the container promises to any caller. Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod canon;
mod canon_alias;
mod canon_empty;
mod canon_extra;
mod canon_free;
mod canon_free_random;
mod canon_fuzz;
mod canon_ignored;
mod canon_memory;
mod canon_ns;
mod canon_order_parts;
mod canon_placed;
mod canon_props;
mod canon_scale;
mod copy;
mod cost;
mod fuzz;
mod par_read;
mod par_write;
mod read;
mod splice;
mod stream;
mod write;
mod xml;
mod xml_pull;
mod zipbuild;

use diacraft_container::zip::Archive;
use diacraft_container::{ContainerError, Limits};

/// The class of the error reading `bytes` gives with `limits` and the plain judge, or `"read"`.
pub fn class_with(bytes: &[u8], limits: &Limits) -> &'static str {
    match Archive::read(bytes, limits) {
        Ok(_) => "read",
        Err(e) => e.class(),
    }
}

/// The class of the error reading `bytes` with the default limits, or `"read"`.
pub fn class(bytes: &[u8]) -> &'static str {
    class_with(bytes, &Limits::default())
}

/// Every entry of the archive in `bytes`, read with the default limits, as `(name, bytes)`.
pub fn read_all(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, ContainerError> {
    let mut archive = Archive::read(bytes, &Limits::default())?;
    let mut work = diacraft_container::Work::default();
    let mut out = Vec::new();
    for index in 0..archive.entries().len() {
        let data = archive.inflate(index, &mut work)?;
        out.push((archive.entries()[index].name.clone(), data));
    }
    Ok(out)
}
