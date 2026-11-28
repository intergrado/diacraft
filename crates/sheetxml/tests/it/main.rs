//! The `sheetxml` crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Every input is written by hand from or built in code: no file from anyone is read. The codec was carved
//! out of `diacraft-vsdx` with no change in behaviour; the tests of that crate, which read whole packages, pass unchanged on it and are
//! what pins the behaviour. These are the tests of the codec's own door: the reader on an element, the vocabulary, the cell given as
//! attributes, the writer and the patcher, the delta, and hostile and random input. Tests may unwrap and panic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod attrs;
mod bytes;
mod delta;
mod ext;
mod fuzz;
mod hostile;
mod lookahead;
mod model_amplify;
mod model_capacity;
mod model_capacity_a;
mod model_capacity_review;
mod model_caps;
mod model_comments;
mod model_cost;
mod model_diff;
mod model_exact;
mod model_ext;
mod model_found;
mod model_hostile;
mod model_legacy;
mod model_memory;
mod model_parts;
mod model_pools;
mod model_review;
mod model_save;
mod model_save_threads;
mod model_scopes;
mod model_share;
mod model_share_speed;
mod model_share_value;
mod model_sparse;
mod model_stream;
mod model_stream_write;
mod model_table;
mod model_units;
mod model_util;
mod model_vectors;
mod model_weigh;
mod namespaces;
mod patch_moved;
mod patchlist;
mod reading;
mod shared_read;
mod write;

use diacraft_container::xml::{El, Node};
use diacraft_doc::Sheet;
use diacraft_sheetxml::{Caps, Cx, LimitError, Scope, SheetReader, Vocabulary, Warnings};

/// The allocator of this binary: the system's, with a count of the bytes and the allocations asked for and given back. `model_memory` holds what a
/// decode is charged to what the model keeps, **counted**, which the resident size of the process does not show: a vector's room that nothing has touched is not
/// resident, and what is resident depends on the platform's allocator. Every call goes on to the system's allocator unchanged (`realloc` and `alloc_zeroed` too), so the
/// tests that measure the process measure what they did before.
#[global_allocator]
static GLOBAL: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

/// The namespace the tests' main vocabulary reads, beside no namespace at all (as a VSDX part does).
pub const MAIN: &str = "urn:test:main";
/// The namespace of the extension vocabulary: it reads this and nothing else, not even no namespace.
pub const EXT: &str = "urn:test:ext";

/// A vocabulary like a VSDX part's: one namespace, and none at all.
#[derive(Clone, Copy, Debug, Default)]
pub struct Main;

impl Vocabulary for Main {
    fn reads(self, uri: Option<&str>) -> bool {
        matches!(uri, None | Some("") | Some(MAIN))
    }
}

/// A vocabulary like an extension's: its own namespace and no other.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ext;

impl Vocabulary for Ext {
    fn reads(self, uri: Option<&str>) -> bool {
        uri == Some(EXT)
    }
}

/// The budgets of a test that is not about budgets: more than any test spends.
pub const BIG: Caps = Caps { cells: 10_000_000, items: 100_000_000, formula_nodes: 10_000_000 };

/// The tree of `xml`, which must parse.
pub fn parse(xml: &str) -> El {
    diacraft_container::xml::parse(xml.as_bytes()).unwrap()
}

/// What reading an element as a sheet gave.
pub struct Read {
    pub sheet: Sheet,
    pub warnings: Warnings,
    pub reader: SheetReader,
}

/// Reads `xml` as an element that is only a sheet, in the vocabulary `V`, with budgets `caps`.
pub fn read_with<V: Vocabulary + Default>(xml: &str, caps: Caps) -> Result<Read, LimitError> {
    let el = parse(xml);
    read_el::<V>(&el, caps)
}

/// As [`read_with`] for a tree.
pub fn read_el<V: Vocabulary + Default>(el: &El, caps: Caps) -> Result<Read, LimitError> {
    read_scoped::<V>(el, &Scope::<V>::empty().enter(el), caps)
}

/// Reads the first element inside the root of `xml` as a sheet, in the scope of the root and of that element: for a namespace that is declared
/// above the sheet and not on it.
pub fn read_in<V: Vocabulary + Default>(xml: &str, caps: Caps) -> Result<Read, LimitError> {
    let root = parse(xml);
    let child = root.elements().next().unwrap();
    let sc = Scope::<V>::empty().enter(&root).enter(child);
    read_scoped::<V>(child, &sc, caps)
}

fn read_scoped<V: Vocabulary>(el: &El, sc: &Scope<V>, caps: Caps) -> Result<Read, LimitError> {
    let mut reader = SheetReader::new(caps);
    let mut warnings = Warnings::new(50);
    let sheet = {
        let mut cx = Cx::new(&mut warnings, "/page1.xml");
        reader.read_sheet_only(&mut cx, el, sc)?
    };
    Ok(Read { sheet, warnings, reader })
}

/// Reads `xml` in the main vocabulary with all the budgets a test wants, which must succeed.
pub fn read(xml: &str) -> Read {
    read_with::<Main>(xml, BIG).unwrap()
}

/// The elements among the children of `el`, by name.
pub fn names(el: &El) -> Vec<&str> {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::El(e) => Some(e.name.as_str()),
            _ => None,
        })
        .collect()
}
