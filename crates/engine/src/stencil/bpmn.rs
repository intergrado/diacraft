//! BPMN Shapes (`bpmn`, GUID series 15): a stub that batch B fills.
//!
//! The stencil is registered in the catalogue with no masters, so it is **not shipped**: no listing, search, `stencil.open`, More Shapes menu or `template.list` answer reaches it
//! until it has masters (`StencilSpec::shipped`). The task fills `MASTERS`, in the order the Shapes window lists them, with ids `bpmn.<name>` and the identity
//! `(SERIES, ordinal)` (a `const SERIES: u8 = 15;` of its own), and the search words of its masters in `keywords`. It touches no other family's file.

use super::MasterSpec;

pub(super) static MASTERS: [MasterSpec; 0] = [];

/// The search words of the master with this id, space separated, or `""`.
pub(super) fn keywords(_master_id: &str) -> &'static str {
    ""
}
