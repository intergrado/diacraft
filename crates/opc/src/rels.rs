//! Relationships: the rows of a `.rels` part, with each internal target resolved to a part name.
//!
//! A relationship is data. Its target may name a part that is not in the package, a part that points back at its source, or itself;
//! nothing here follows a target, so none of that can loop or fail. A walker over a package (the `vsdx` crate's) keeps its own set of
//! the parts it has seen. `Type` is a string and is compared exactly: `.../Page` is not `.../page`.

use std::collections::HashSet;

use crate::error::{OpcError, Warning, XmlError};
use crate::limits::Limits;
use crate::name::resolve_within;
use crate::xml;

/// Whether a relationship points inside the package or out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    /// A part of the package: the default.
    Internal,
    /// Something outside it, named by a URI that is kept and never followed.
    External,
}

/// One relationship, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rel {
    /// `Id`: never renumbered by a save.
    pub id: String,
    /// `Type`, exactly as written.
    pub ty: String,
    /// `Target`, exactly as written.
    pub target_raw: String,
    /// `TargetMode`: external only when the attribute says `External` in any ASCII case (a labelled choice: an external URL read as a
    /// part name is harmless, and a part name read as an external URL would drop a part), and internal for any other value.
    pub mode: TargetMode,
    /// For an internal relationship, the part name its target resolves to, whether or not the package has such a part; `None`
    /// when the target names no part (see [`resolve`](crate::name::resolve)) and for an external one. When the package has the part, the
    /// name is spelled as the package holds it, which may differ in ASCII case from what the target says ([`Package::rels`](crate::Package::rels)).
    pub target: Option<String>,
}

/// The relationships of one part, in file order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rels {
    /// Every relationship that has an `Id`, a `Type` and a `Target`, in order.
    pub items: Vec<Rel>,
    /// What was odd about the part: rows skipped for a missing attribute, and repeated `Id`s.
    pub warnings: Vec<Warning>,
}

impl Rels {
    /// The first relationship with this `Id`.
    pub fn get(&self, id: &str) -> Option<&Rel> {
        self.items.iter().find(|r| r.id == id)
    }

    /// The relationships whose `Type` is exactly `ty`, in order.
    pub fn of_type<'a>(&'a self, ty: &str) -> impl Iterator<Item = &'a Rel> + use<'a> {
        let ty = ty.to_owned();
        self.items.iter().filter(move |r| r.ty == ty)
    }

    /// How many relationships there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Reads the relationships part `rels_part` (named only for the error), whose source part is `source`.
pub(crate) fn parse(bytes: &[u8], rels_part: &str, source: &str, limits: &Limits, max_nodes: usize) -> Result<Rels, OpcError> {
    let xml_error = |error: XmlError| OpcError::Xml { part: rels_part.to_owned(), error };
    let package = limits.package_xml();
    let root =
        xml::parse_with(bytes, &diacraft_container::XmlLimits { max_nodes: package.max_nodes.min(max_nodes), ..package }).map_err(xml_error)?;
    if !root.is("Relationships") {
        return Err(xml_error(XmlError::Malformed { offset: 0, message: format!("the root element is {:?}, not Relationships", root.name) }));
    }
    let mut rels = Rels::default();
    let mut seen = HashSet::new();
    let mut rows = 0usize;
    for row in root.children_named("Relationship") {
        rows += 1;
        if rows > limits.max_relationships {
            return Err(OpcError::TooManyRelationships { part: rels_part.to_owned(), limit: limits.max_relationships });
        }
        let (Some(id), Some(ty), Some(target_raw)) = (row.attr("Id"), row.attr("Type"), row.attr("Target")) else {
            rels.warnings.push(Warning::IncompleteRelationship { part: rels_part.to_owned() });
            continue;
        };
        if !seen.insert(id) {
            rels.warnings.push(Warning::DuplicateRelationshipId { part: rels_part.to_owned(), id: id.to_owned() });
        }
        let external = row.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
        let target = if external { None } else { resolve_within(source, target_raw, limits.max_part_name_bytes) };
        let mode = if external { TargetMode::External } else { TargetMode::Internal };
        rels.items.push(Rel { id: id.to_owned(), ty: ty.to_owned(), target_raw: target_raw.to_owned(), mode, target });
    }
    Ok(rels)
}
