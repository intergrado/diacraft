//! What a package holds that a drawing in another format has no place for: the macro project, the embedded objects and the other parts the model does not hold (harvested from the shelved
//! `crates/vsdx/src/holds.rs`, this repository's own code; the native-format design's section 6 and Owner 9). [`holds`] only names and counts them: it runs nothing, opens nothing and
//! decodes nothing, and it reads the package as the reader does, within the same limits.
//!
//! **What it is for now.** An earlier change asked the package, because the model then held none of these parts. The model holds them now (`Document::parts`, each with its role), and the engine's save across
//! asks the model (`diacraft_engine::across::hold`), so a part a person deleted is no longer asked for. This reads the **bytes of a package** still, as an independent measure of what a file holds:
//! the tests hold what the model reads to what this counts ([`holds_of`] is the model's side of it).

use std::collections::HashSet;

use diacraft_opc::{Package, TargetMode};

use crate::error::VsdxError;
use crate::limits::Limits;

/// The relationship type of a VBA project, and the prefix of the types of its signature parts.
const VBA_PROJECT_REL: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject"; // brand-ok: a URI the format defines
/// The content type of a VBA project; its signature parts' types start with it too.
const VBA_CONTENT_TYPE: &str = "application/vnd.ms-office.vbaProject";
/// The relationship type of the document part, from the package.
const DOCUMENT_REL: &str = "http://schemas.microsoft.com/visio/2010/relationships/document"; // brand-ok: a URI the format defines
/// The folders of embedded objects and of ActiveX controls, lower case.
const OBJECT_FOLDERS: [&str; 2] = ["/visio/embeddings/", "/visio/activex/"]; // brand-ok: part names the format defines
/// The names of the parts the model holds, or whose content a drawing in another format carries in another way (the properties, the pages, the masters, the
/// themes and the pictures), lower case: every other part is one the new file would not have.
const MODELLED: [&str; 6] = ["/visio/pages/", "/visio/masters/", "/visio/theme/", "/visio/media/", "/docprops/", "/visio/document.xml"]; // brand-ok: part names the format defines

/// What a package holds. See the [module documentation](self).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Holds {
    /// The parts of the macro project, named with their leading slash: the project, the signature parts its own relationships reach, and any part of a VBA
    /// content type. Empty when the package holds no project (a macro-enabled type without one is normal).
    pub macro_parts: Vec<String>,
    /// How many parts are embedded objects or ActiveX controls, by their folders (not counting relationship parts).
    pub object_parts: usize,
    /// How many parts are none of the project, the objects, the pictures, the pages, the masters, the themes, the properties and the document: parts a
    /// drawing in another format would not carry (solutions, recordsets, custom XML, the windows part and the like). Relationship parts are not counted.
    pub other_parts: usize,
}

/// The most items of relationship trees [`holds`] parses in all (the root's, the document's and each project's): **a total budget**, since the container's limit is for one part and many relationships
/// can name one part. A package past it is `xml_too_complex`, as one part past the container's own limit is.
const MAX_WORK_ITEMS: usize = 2_000_000;

/// Names `part` as a part of the project, once whatever the case it is spelled in. `true` when it is new.
fn add(found: &mut Holds, seen: &mut HashSet<String>, part: &str) -> bool {
    let new = seen.insert(part.to_ascii_lowercase());
    if new {
        found.macro_parts.push(part.to_owned());
    }
    new
}

/// What a call of [`holds_counted`] did, in the units its loops walk: the relationships parts it parsed and the items of their trees. **The count a test holds the cost to.**
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoldsWork {
    /// The relationships parts parsed (the root's, the document's and each project's, once each).
    pub parts_parsed: u64,
    /// The items of those parts' trees, as taken from the total budget.
    pub items: u64,
}

/// The relationships of `part`, parsed within what is left of the total budget `left`, which the items of the tree (and the rows it kept or skipped) are taken from.
fn rels_of(pkg: &Package, part: &str, left: &mut usize, work: &mut HoldsWork) -> Result<diacraft_opc::Rels, VsdxError> {
    let rels = pkg.rels_within(part, *left).map_err(VsdxError::Package)?;
    let items = rels.items.len().saturating_add(rels.warnings.len()).saturating_add(1);
    *left = left.saturating_sub(items);
    work.parts_parsed = work.parts_parsed.saturating_add(1);
    work.items = work.items.saturating_add(u64::try_from(items).unwrap_or(u64::MAX));
    Ok(rels)
}

/// What `bytes`, a package of the six types, holds (see [`Holds`]). Errors are the container's, as [`crate::read`] gives them.
pub fn holds(bytes: &[u8], limits: &Limits) -> Result<Holds, VsdxError> {
    holds_counted(bytes, limits).map(|(found, _)| found)
}

/// [`holds`], and the work it did ([`HoldsWork`]).
pub fn holds_counted(bytes: &[u8], limits: &Limits) -> Result<(Holds, HoldsWork), VsdxError> {
    let mut counted = HoldsWork::default();
    let pkg = Package::read(bytes, &limits.opc).map_err(VsdxError::Package)?;
    let mut found = Holds::default();
    let mut seen: HashSet<String> = HashSet::new();
    // The project by its relationships, from the package and from the document part.
    let mut work = MAX_WORK_ITEMS;
    let root = rels_of(&pkg, "/", &mut work, &mut counted)?;
    let document = root.items.iter().find(|r| r.ty == DOCUMENT_REL && r.target.is_some()).and_then(|r| r.target.clone());
    let document_rels = match &document {
        Some(part) => rels_of(&pkg, part, &mut work, &mut counted)?,
        None => Default::default(),
    };
    let mut projects: Vec<String> = Vec::new();
    for rel in root.items.iter().chain(document_rels.items.iter()) {
        if rel.ty != VBA_PROJECT_REL || rel.mode == TargetMode::External {
            continue;
        }
        if let Some(part) = rel.target.as_deref().filter(|p| pkg.part(p).is_some()) {
            // **A project is looked into once**, however many relationships name it and in whatever case: the signature parts its own relationships reach are those of the first to name it.
            if add(&mut found, &mut seen, part) {
                projects.push(part.to_owned());
            }
        }
    }
    // The signature parts the project's own relationships reach.
    for project in &projects {
        let rels = rels_of(&pkg, project, &mut work, &mut counted)?;
        for rel in rels.items.iter().filter(|r| r.mode != TargetMode::External && r.ty.contains("/vbaProjectSignature")) {
            if let Some(part) = rel.target.as_deref().filter(|p| pkg.part(p).is_some()) {
                add(&mut found, &mut seen, part);
            }
        }
    }
    // Any part of a VBA content type is VBA, whether or not a relationship reaches it; and the others by their folders.
    for name in pkg.parts() {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".rels") {
            continue;
        }
        let is_vba = pkg.content_type(name).is_some_and(|t| t.starts_with(VBA_CONTENT_TYPE));
        if is_vba {
            add(&mut found, &mut seen, name);
        } else if OBJECT_FOLDERS.iter().any(|f| lower.starts_with(f)) {
            found.object_parts = found.object_parts.saturating_add(1);
        } else if !seen.contains(&lower)
            && !MODELLED.iter().any(|m| lower.starts_with(m))
            && !document.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(name))
        {
            found.other_parts = found.other_parts.saturating_add(1);
        }
    }
    Ok((found, counted))
}

/// What the model holds of a VSDX package's: the parts the reader keeps (`Document::parts`, VSDX-tagged), by their roles, **counted as [`holds`] counts the same package**: the parts of the macro project
/// (named with their slash), the parts of embedded objects and controls, and the other parts the model does not rebuild. The thumbnail is none of them (the model rebuilds it or leaves it out).
/// The parts of other formats and of a newer DiaCraft that the model holds are not a VSDX package's and are not counted.
pub fn holds_of(doc: &diacraft_doc::Document) -> Holds {
    use diacraft_doc::{Format, PartRole};
    let mut found = Holds::default();
    for part in doc.parts.iter().filter(|p| p.format == Format::Vsdx) {
        let lower = part.name.to_ascii_lowercase();
        if matches!(part.role, PartRole::MacroProject | PartRole::MacroSignature | PartRole::MacroData) {
            found.macro_parts.push(part.name.to_string());
        } else if part.role == PartRole::Embedding || OBJECT_FOLDERS.iter().any(|f| lower.starts_with(f)) {
            found.object_parts = found.object_parts.saturating_add(1);
        } else if part.role != PartRole::Thumbnail {
            found.other_parts = found.other_parts.saturating_add(1);
        }
    }
    found
}
