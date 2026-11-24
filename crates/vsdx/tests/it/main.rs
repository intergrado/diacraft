//! The `vsdx` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is built in code by `diacraft-testkit` from the specs: the vectors of `-*.md` (each test names its
//! vector, `V5` of the pages spec, say) and hostile cases that no honest writer makes. No file from the reference app, or from anyone, is read. Tests may
//! unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod canon;
mod canon_empty_parts;
mod canon_implied_rows;
mod carry;
mod carry_keys;
mod cells;
mod comments;
mod content_types;
mod diacraft_part;
mod document;
mod drops;
mod edges;
mod emit;
mod formats;
mod fuzz;
mod held;
mod held_hostile;
mod held_more;
mod holds;
mod hostile;
mod kept_bytes;
mod kept_foreign;
mod limit_words;
mod masters;
mod memory_charges;
mod memory_counted;
mod memory_price;
mod model_classes;
mod model_document;
mod model_hostile;
mod model_rows;
mod model_spelling;
mod model_tags;
mod model_writer;
mod native_capacity;
mod new_drawing;
mod new_page_pictures;
mod new_pictures;
mod objects;
mod old_namespace;
mod order;
mod package;
mod pages;
mod par_read;
mod par_write;
mod part_case;
mod positions;
mod preservation;
mod reading;
mod round_trip;
mod scope;
mod shared_embedding;
mod snap_settings;
mod theme_entries;
mod theme_write;
mod timing;
mod vectors;
mod write;
mod write_more;

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Shape, Sheet};
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use diacraft_vsdx::{Limits, ReadReport, VsdxError, read};

/// The allocator of this binary: the system's, with a count of the bytes and the allocations asked for and given back (as made
/// `crates/sheetxml`'s). `memory_counted` holds what a read is charged to what the model it built keeps, **counted**, which the resident size of the process does
/// not show: a vector's room that nothing has touched is not resident, and what is resident depends on the platform's allocator. Every call goes on to the system's
/// allocator unchanged (`realloc` and `alloc_zeroed` too), so the tests that measure the process (`memory_price`) measure what they did before.
#[global_allocator]
static GLOBAL: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

/// Reads a package with the default limits, which must succeed.
pub fn read_ok(bytes: &[u8]) -> (Document, ReadReport) {
    match read(bytes, &Limits::default()) {
        Ok(read) => read,
        Err(e) => panic!("the package should read, and gave {} ({e})", e.class()),
    }
}

/// Reads with `limits`, which must succeed.
pub fn read_with(bytes: &[u8], limits: &Limits) -> (Document, ReadReport) {
    match read(bytes, limits) {
        Ok(read) => read,
        Err(e) => panic!("the package should read, and gave {} ({e})", e.class()),
    }
}

/// The error reading `bytes` with `limits` gives. Panics if it reads.
pub fn read_err(bytes: &[u8], limits: &Limits) -> VsdxError {
    match read(bytes, limits) {
        Ok((doc, _)) => panic!("expected an error and read a document of {} pages", doc.pages.len()),
        Err(e) => e,
    }
}

/// A one-page drawing whose page holds `shapes`, read.
pub fn doc_of(shapes: &str) -> (Document, ReadReport) {
    read_ok(&VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(shapes)).build())
}

/// The first top-level shape of the first page.
pub fn first_shape(doc: &Document) -> Arc<Shape> {
    doc.pages.get(0).and_then(|p| p.shapes.get(0)).cloned().expect("a first shape on a first page")
}

/// The cell `name` directly under a sheet.
pub fn direct<'a>(sheet: &'a Sheet, name: &str) -> &'a Cell {
    sheet.cell(name).unwrap_or_else(|| panic!("the sheet has no cell {name}"))
}
