//! A template as a document and as the bytes of a `.vstx` (ours).
//!
//! The document is a new drawing (`Document::new_blank`: the root style and one page, "Page-1") with the template's size and orientation written by the
//! page-setup rules (`draw::set_size`, `draw::set_orientation`), its theme applied by the theme rules (`theme::apply`: the theme's part, the `Theme`
//! style and the page's selector cells), and the list that names its stencils (a child of DiaCraft's part: [`super::names`]). Then the one writer (`diacraft_vsdx::write_with`)
//! makes the package, under the template content type, with no times (`now` is `None`: a template is the same bytes each time it is made, and the
//! drawing made from it takes the time of its first save, `WriteOptions::new_copy`). The document returned is **read from those bytes by the
//! reader**, as `file.open` reads a template file (and not the writer's own account of the baseline, which is massaged for the next save of a new
//! drawing), so that a drawing started from a built-in template and one started from its file are made of the same thing. **The drawing made from it keeps the document and not the bytes**
//! (NP10): the package is let go once the document is read, and the model holds everything it held.

use diacraft_doc::Document;
use diacraft_vsdx::{Kind, Limits, Variant, WriteOptions, read, write_with};

use super::catalogue::{Seed, TemplateSpec};
use super::names::set_stencil_names;
use crate::draw::container::Dir;
use crate::draw::container::swimlane::{self, Create};
use crate::draw::paper::size_preset;
use crate::draw::{self, SizeChoice};
use crate::error::{EngineError, Result, failed, internal};
use crate::stencil::swimlane::{LANE_LABEL, PHASE_LABEL};
use crate::theme::{self, Choice};

/// A template made: the package, the document it reads as, and what the writer said.
pub struct Built {
    /// The `.vstx` package (a new drawing made from the template does not keep it).
    pub bytes: Vec<u8>,
    /// The document those bytes read as: the new drawing's model.
    pub document: Document,
    /// What was odd about the write, in words. Empty for a template of ours.
    pub warnings: Vec<String>,
}

/// Makes `spec` into a document and the bytes of a `.vstx`. `cmd` names the command in an error.
pub fn build(cmd: &str, spec: &TemplateSpec) -> Result<Built> {
    let mut doc = crate::snap::blank_drawing();
    let page = doc.pages.iter().next().map(|p| p.id).ok_or_else(|| internal(cmd, "a new document without a page"))?;
    let size = size_preset(spec.size).ok_or_else(|| internal(cmd, format!("template `{}` names a size that does not exist", spec.id)))?;
    let wrong = |e: draw::Problem| -> EngineError { e.into_engine(cmd) };
    draw::set_size(&mut doc, page, SizeChoice::Preset(size)).map_err(wrong)?;
    draw::set_orientation(&mut doc, page, spec.orientation).map_err(wrong)?;
    if let Some((id, variant)) = spec.theme {
        let found = theme::find(id).ok_or_else(|| internal(cmd, format!("template `{}` names a theme that does not exist", spec.id)))?;
        theme::apply(&mut doc, &[page], Choice::Theme(found, variant), theme::Base::Root).map_err(wrong)?;
    }
    if let Some(seed) = spec.seed {
        seed_page(&mut doc, page, seed).map_err(wrong)?;
    }
    if !set_stencil_names(&mut doc, spec.stencils) {
        return Err(internal(cmd, format!("template `{}` could not name its stencils", spec.id)));
    }
    let options = WriteOptions { variant: Some(Variant { kind: Kind::Template, macro_enabled: false }), ..WriteOptions::default() };
    let written = write_with(&doc, None, &options).map_err(|e| failed(cmd, crate::shown(&format!("the template could not be written: {e}"), 300)))?;
    let (document, report) = read(&written.bytes, &Limits::default())
        .map_err(|e| internal(cmd, crate::shown(&format!("the template could not be read back: {e}"), 300)))?;
    let mut warnings = written.warnings;
    warnings.extend(report.warnings);
    Ok(Built { bytes: written.bytes, document, warnings })
}

/// What a template puts on its page (ours): the lanes of a cross-functional flowchart, 0.5 inch in from the page's top left corner, as wide as the page less a margin each side and as
/// thick as the page lets them be, at most 2 inches, on a page that has Auto Size on; or an org chart's Executive, in the middle of the page, an inch below its top edge.
fn seed_page(doc: &mut Document, page: u32, seed: Seed) -> std::result::Result<(), draw::Problem> {
    match seed {
        Seed::Swimlanes { lanes, phases } => {
            let setup = draw::read_setup(doc, page)?;
            let k = draw::units_per_inch(doc, page);
            let band = crate::draw::container::PHASE_BAND_INCHES;
            let length = (setup.width - 1.0).max(4.0);
            let thickness = ((setup.height - 1.0 - band) / lanes.max(1) as f64).clamp(crate::draw::container::MIN_INCHES, 2.0);
            // **The page has Auto Size on** (ours: the specifications say nothing of a template's page): a lane added past its last sheet makes the page grow, as any shape does.
            draw::set_auto_size(doc, page, true)?;
            swimlane::create(
                doc,
                page,
                Create {
                    dir: Dir::Down,
                    lanes,
                    phases,
                    left: 0.5 * k,
                    top: (setup.height - 0.5 - if phases > 0 { band } else { 0.0 }) * k,
                    length: length * k,
                    thickness: thickness * k,
                    lane_label: LANE_LABEL,
                    phase_label: PHASE_LABEL,
                    above: 0,
                },
            )
            .map(|_| ())
        }
        Seed::OrgChart => {
            let setup = draw::read_setup(doc, page)?;
            let k = draw::units_per_inch(doc, page);
            let master = crate::stencil::find_master("orgchart.executive").ok_or_else(|| draw::Problem::failed("the Executive master is missing"))?;
            draw::drop_shape(doc, page, master, setup.width / 2.0 * k, (setup.height - 1.0) * k).map(|_| ())
        }
    }
}
