//! Templates: the templates DiaCraft ships, a drawing started from one, and how a template names the stencils it opens.
//!
//! * [`catalogue`]: the templates of our own (**originals**, `docs/provenance.md`): four from the first batches and fourteen registered for the second stencil batch, **listed once their stencils ship**. A template is a
//!   small description in code (`TemplateSpec`: a page size, an orientation, a theme and the built-in stencils it opens), like the built-in
//!   stencils, and not a file shipped with the app: it works on the web, it needs no asset row, and there is one place that says what it holds.
//! * [`build`]: the description made into a document, then into the bytes of a `.vstx` **on demand**, by the one writer
//!   (`diacraft_vsdx::write_with`). A drawing started from a built-in template is read from those bytes exactly as one started from a template
//!   file is: there is no second path for the built-in ones, so what a test of the built-in templates shows is what any template file does.
//! * [`names`]: **how a template names the stencils it opens**. The specs give the format one record of open stencils, the
//!   stencil windows of the windows part, whose `Document` is a file path; the model does not keep that part
//!   A built-in stencil has no file to name, and nothing says that a reader opens a template's stencil windows (R32 is `unknown`).
//!   So the project rule is a child `stencils` of **DiaCraft's own solution part** (`Document::set_diacraft_child`, where the Snap & Glue options live
//!   too), whose attribute `open` holds the ids of the built-in stencils to open, separated by `;`. A name that is not a built-in id is **never
//!   followed**: a template is a hostile file, and a path in it is not opened.
//!
//! The commands are in `cmd/template.rs` (`template.list`) and `cmd/file.rs` (`file.new {template}`, and `file.open` of a template file, which
//! opens the same stencils).

mod build;
mod catalogue;
mod names;

pub use build::{Built, build};
pub use catalogue::{Seed, TemplateSpec, find, is_path, registered, templates};
pub use names::{
    DrawingStencils, MAX_NAMES, STENCILS_CHILD, StencilNames, set_stencil_names, stencil_names, stencils_of_drawing, stencils_of_masters,
    stencils_opened_by,
};

#[cfg(test)]
mod tests;
