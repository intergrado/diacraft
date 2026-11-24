//! What the model has no typed place for, and how the reader keeps it: unknown attributes, elements, comments and processing
//! instructions.
//!
//! **The rules are `diacraft-sheetxml`'s** (`passthrough.rs` there): what a typed field holds, where an element's unknown children go,
//! why layout is not content, and what a CDATA section becomes. They are the model's rules and not this format's, so the functions that
//! carry them are written once and used here, in the reader and the writer, and by any other format that keeps a sheet.

pub(crate) use diacraft_sheetxml::passthrough::{all_attrs, canonical_u32, element_cost, is_blank, raw_element, read_flag, read_u32};
pub use diacraft_sheetxml::passthrough::{node_to_tree, to_tree};
