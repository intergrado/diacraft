//! The content check for VSDX packages: what "the same content" means for two `.vsdx` files, as the lists the container's check
//! ([`diacraft_container::canon`]) is given (design section 4.3).
//!
//! The rules are the container's: the same set of parts, the part names folded by ASCII case as OPC folds them; XML parts compared with
//! spacing, attribute order, prefixes, namespace declarations and comments ignored; every other part byte for byte. What this module adds is
//! what only the format can say, and each entry below has its source.
//!
//! # The lists
//!
//! **Siblings whose order has no meaning** ([`CANON_RULES`]`.keyed`):
//! * **`Cell` by `N`**, in each of the namespaces a part can be written in (`2012/main`, `2011/1/core`, and none, `ns.rs`). Source: measured.
//!   A sheet's cells are kept by name in the model, so the model writer writes them in the order of their names and not the file's
//!   (`crates/sheetxml/src/build.rs`, "Order"); and a cell is found by its `N` (R9 of the pages and shapes spec), never by its place. Every
//!   other child of a shape, section or row is in order: shapes are the drawing's z-order, and rows are rows.
//! * **`Default` rows of `[Content_Types].xml` by `Extension`**, and **`Override` rows by `PartName`**, each key **folded as OPC's reader folds
//!   it** (`crates/opc/src/content_types.rs`): an extension by ASCII case, a part name by ASCII case, the `%` escapes that decode and a slash
//!   in front. Source: OPC says the order of the rows has no meaning, and its schema makes the root a repeated choice of the two
//!   Rows the reader takes for one (`/a.xml` and `/A.XML`) are one key, and the reader
//!   takes the first, so those are compared in the order written: two rows for one part in another order are another type for the part.
//! * **`Relationship` rows by `Id`**. Source: OPC, as for the rows above (ignores their order).
//!
//! `Section` is not keyed. The model keeps a sheet's sections in a `Vec`, in the order the reader found them (`Sheet::sections` in
//! `crates/doc/src/sheet.rs`), and writes them in that order, so a section's place survives a trip; the design's "by `N` and `IX`, where the
//! model does not keep their order" does not apply. (If a writer is ever shown to reorder them, the entry is `Section` by `N` and `IX`,
//! with the measured case as its source.)
//!
//! **Attributes whose value names a prefix** (`Rules::qnames`), compared by the namespace the prefix is bound to: `xsi:type` (a name, as
//! `dcterms:W3CDTF` in `docProps/core.xml`, which `write/props.rs` emits), the markup compatibility `Ignorable` and `MustUnderstand` (prefixes)
//! and `ProcessContent` (names), and `Requires` of `mc:Choice` (prefixes). Source: the markup compatibility and schema instance specifications
//! (a `Choice` is read only if the namespaces it requires are understood), so a prefix declared again
//! changes what the part says.
//!
//! **Number-typed attributes: none** (NP8). The model keeps every VSDX value as the string the file gave, so no
//! writer of ours respells one, and an attribute is listed only with the measured case that needs it. `V`, `F`, `U`, `E`, `IX`, `ID` and `N`
//! hold text and are never listed: if `V` were, `007` changed to `7` would pass as the same content (Review Focus 3).
//!
//! **Paragraphs: none** (there is no ODF text in a VSDX package). **Mixed content: `Text`**, the one element that holds both text and
//! children (the `cp`, `pp`, `tp` and `fld` markers): the text of a shape may be a space beside a marker, which is content and not spacing.
//!
//! **What is no content.** The project ruled that three things a writer may do
//! differently say nothing, and the check counts them as the same; each has a control test that real content in the same place still differs
//! (`tests/it/canon_empty_parts.rs`):
//! * **An empty container is the same as an absent one** ([`CANON_RULES`]`.absent_when_empty_under`): `Colors`, `FaceNames`, `StyleSheets` and `DocumentSheet`
//!   as children of the document part's root (`VisioDocument`), `Connects` and `Shapes` as children of a page's or a master's root (`PageContents`,
//!   `MasterContents`), and a group's `Shapes` as a child of its `Shape`, in each of the three namespaces, when they hold no attribute, no child, no
//!   instruction and no text but white space (the container's rule, `diacraft_container::canon`). The model holds no colour, face, style, cell, row or shape
//!   for either, and a container that holds one is compared as before. **Only under those parents**: the same name, empty, anywhere else
//!   (a `Shapes` inside an element the reader does not know, a `Colors` in a page) is an element of the part, which a save keeps.
//! * **An empty relationships part is the same as none** ([`CANON_RULES`]`.parts_absent_when_empty`): a part on one side only that is a relationships part by
//!   its name (`diacraft_opc::name::is_rels_part`: `.rels` in a folder `_rels`, ASCII case folded), whose root is OPC's `Relationships` and holds no row
//!   (ECMA-376 Part 2, 9.3: a part's relationships are the rows of its relationships part, and a part with none needs none). A part with a row, an attribute
//!   or anything else in it is the part it is, and **so is a part of any other name, whatever its root holds**: a part the reader does not
//!   know is kept by a save, so dropping one is a difference.
//! * **The order of a shape's `Section`, `Text` and `ForeignData` among themselves** ([`CANON_RULES`]`.unordered`): the writer writes the sections, then
//!   the text, then the foreign data, and files have the text first, or the foreign data before the text. Each is read by its name and once (`shape.rs`),
//!   so their order says nothing; **the sections keep their order among themselves** (a sheet's sections are a list, `Section` is not keyed), and every other
//!   child of a shape (`Shapes`, `Data1` to `Data3`, a foreign element) keeps its place.
//!
//! **What else is no content.** The project ruled that three more things say nothing
//! (`tests/it/canon_implied_rows.rs`, each with controls that a real difference in the same place still differs):
//! * **A `Row` with no `IX`** ([`CANON_RULES`]`.placed`) is the same as one whose `IX` is the count of the rows before it, in the
//!   sections the reader keys rows by `IX` and, in any other section, for a row with no `N`; the rows of a section keyed by name keep an absent `IX` absent. The
//!   lists are those of `diacraft_doc::keying`, and a test holds the two together.
//! * **A relationship row that a page's own parts imply** and the other package lacks, and **a part's content type given where it had none**, are not
//!   questions that one part can answer, so they are taken out of the package's copy before the comparison (`implied`): see that module for what each is and what
//!   it will not read. Nothing is added; a row that is not shown to be implied stays, and is compared as ever.
//!
//! The rules of are each as narrow as their definition: none of them lets go an unknown part with an empty root, or any other row.
//!
//! # Part names fold, and so do the keys of rows, but values are as written
//!
//! The set of parts is compared with ASCII case folded, as OPC compares part names, and non-ASCII letters do not fold. The rows that name a
//! part (`Override`'s `PartName`) are matched by a key folded as OPC's reader folds it, but their values are compared as written (and a
//! relationship's `Target` is a value): a part named in another case with a row for it is the same part with a row whose `PartName` differs,
//! one difference in the content types. The check may call a re-cased part different, and never calls different content the same.
//!
//! # What is XML
//!
//! A part whose content type (its `Override`, else its `Default` by extension) ends in `xml`, which covers `+xml`, is XML, with two
//! exceptions: a type that starts `image/` (a picture is bytes, and `image/svg+xml` stays one: the same bytes for every picture), and a
//! part with no content type, which is XML only if its name ends in `.rels`. The content types stream, which is not a part, is XML. Every
//! other part is bytes. A part meant as XML that does not parse is compared as bytes (the container's rule).

use diacraft_container::canon::{
    CanonError, ContentDiff, EmptyPart, EmptyUnder, Entry, KeyFold, Keyed, MAX_COMPARE_BYTES, MAX_COMPARE_ITEMS, Name, Placed, QKind, Qualified,
    Rules, Unordered, compare_entries,
};
use diacraft_container::{Work, XmlLimits};
use diacraft_opc::name::fold;
use diacraft_opc::{CONTENT_TYPES_ITEM, Package};

mod implied;

use crate::error::{LimitKind, VsdxError};
use crate::limits::Limits;
use crate::ns::{NS_2011, NS_2012, NS_MC};

/// The namespace of `[Content_Types].xml` (ECMA-376 Part 2, 10.1.2).
const NS_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
/// The namespace of a relationships part (ECMA-376 Part 2, 9.3).
const NS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// An attribute of no namespace.
const fn attr(local: &'static str) -> Name {
    Name { ns: "", local }
}

const CELL_KEY: &[Name] = &[attr("N")];
const EXTENSION: &[Name] = &[attr("Extension")];
const PART_NAME: &[Name] = &[attr("PartName")];
const ID: &[Name] = &[attr("Id")];
/// The schema instance namespace, whose `type` names a type by a prefixed name (`xsi:type="dcterms:W3CDTF"`).
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// The lists of the content check for VSDX: see the [module documentation](self) for each entry and its source.
pub const CANON_RULES: Rules = Rules {
    keyed: &[
        Keyed { child: Name { ns: NS_2012, local: "Cell" }, key: CELL_KEY, fold: KeyFold::Exact },
        Keyed { child: Name { ns: NS_2011, local: "Cell" }, key: CELL_KEY, fold: KeyFold::Exact },
        Keyed { child: Name { ns: "", local: "Cell" }, key: CELL_KEY, fold: KeyFold::Exact },
        Keyed { child: Name { ns: NS_TYPES, local: "Default" }, key: EXTENSION, fold: KeyFold::AsciiCase },
        Keyed { child: Name { ns: NS_TYPES, local: "Override" }, key: PART_NAME, fold: KeyFold::PartName },
        Keyed { child: Name { ns: NS_RELS, local: "Relationship" }, key: ID, fold: KeyFold::Exact },
    ],
    numbers: &[],
    lengths: &[],
    qnames: &[
        Qualified { attr: Name { ns: NS_XSI, local: "type" }, on: None, kind: QKind::QNames },
        Qualified { attr: Name { ns: NS_MC, local: "Ignorable" }, on: None, kind: QKind::Prefixes },
        Qualified { attr: Name { ns: NS_MC, local: "MustUnderstand" }, on: None, kind: QKind::Prefixes },
        Qualified { attr: Name { ns: NS_MC, local: "ProcessContent" }, on: None, kind: QKind::QNames },
        Qualified { attr: attr("Requires"), on: Some(Name { ns: NS_MC, local: "Choice" }), kind: QKind::Prefixes },
    ],
    paragraphs: &[],
    exact_text: &[Name { ns: NS_2012, local: "Text" }, Name { ns: NS_2011, local: "Text" }, Name { ns: "", local: "Text" }],
    // See the module documentation: these containers, empty, are the same as absent, and only under their parents.
    absent_when_empty: &[],
    absent_when_empty_under: EMPTY_CONTAINERS,
    unordered: &[
        Unordered { parent: Name { ns: NS_2012, local: "Shape" }, children: &SHAPE_CHILDREN_2012 },
        Unordered { parent: Name { ns: NS_2011, local: "Shape" }, children: &SHAPE_CHILDREN_2011 },
        Unordered { parent: Name { ns: "", local: "Shape" }, children: &SHAPE_CHILDREN_NONE },
    ],
    // A relationships part, by its name, that holds no row; no other part.
    parts_absent_when_empty: &[EmptyPart {
        named: diacraft_opc::name::is_rels_part,
        root: Name { ns: NS_RELS, local: "Relationships" },
        children: &[],
    }],
    ignored_attributes: &[],
    ignored_children: &[],
    ignored_when_extra: &[],
    free_names: &[],
    // Class F7: a `Row` with no `IX` has the count of the rows before it.
    placed: &[placed(NS_2012), placed(NS_2011), placed("")],
    // APPID: the namespace that builds made before the id moved to `diacraft.app` wrote, in DiaCraft's own part and in the type of its relationships to the parts it carries, is
    // the one that is written now: a file one build wrote and the other wrote again are the same content.
    aliases: ALIASES,
};

/// The spellings DiaCraft's own namespace has had (`Rules::aliases`): the earlier one reads as the current one. The type of the relationship to a carried part is the namespace and
/// `:blob`, which the same pair covers.
const ALIASES: &[(&str, &str)] = &[(diacraft_doc::DIACRAFT_URN_LEGACY, diacraft_doc::DIACRAFT_URN)];

/// The sections whose rows are keyed by `IX`, and the ones keyed by `N` (`diacraft_doc::keying`, which the reader finds a row by: a test holds the two together).
const INDEXED_SECTIONS: &[&str] =
    &["Character", "Field", "FillGradient", "Geometry", "Layer", "LineGradient", "Paragraph", "Reviewer", "Scratch", "Tabs"];
const NAMED_SECTIONS: &[&str] = &["User", "Property", "Actions", "Control", "Hyperlink", "ActionTag"];

/// The rule for `IX` in one namespace (class F7; an absent `IX` is the count of the rows before the row).
const fn placed(ns: &'static str) -> Placed {
    Placed {
        parent: Name { ns, local: "Section" },
        kind: attr("N"),
        child: Name { ns, local: "Row" },
        place: attr("IX"),
        named_by: attr("N"),
        always: INDEXED_SECTIONS,
        never: NAMED_SECTIONS,
    }
}

/// The containers of the document part's root that are the same as absent when empty, in the namespaces a part can be written in: the reader
/// reads a child of any of the three under a root of any of them, so each parent below holds the children of all three.
const DOCUMENT_CONTAINERS: &[Name] = &[
    Name { ns: NS_2012, local: "Colors" },
    Name { ns: NS_2012, local: "FaceNames" },
    Name { ns: NS_2012, local: "StyleSheets" },
    Name { ns: NS_2012, local: "DocumentSheet" },
    Name { ns: NS_2011, local: "Colors" },
    Name { ns: NS_2011, local: "FaceNames" },
    Name { ns: NS_2011, local: "StyleSheets" },
    Name { ns: NS_2011, local: "DocumentSheet" },
    Name { ns: "", local: "Colors" },
    Name { ns: "", local: "FaceNames" },
    Name { ns: "", local: "StyleSheets" },
    Name { ns: "", local: "DocumentSheet" },
];
/// The containers of a page's or a master's root.
const PART_CONTAINERS: &[Name] = &[
    Name { ns: NS_2012, local: "Connects" },
    Name { ns: NS_2012, local: "Shapes" },
    Name { ns: NS_2011, local: "Connects" },
    Name { ns: NS_2011, local: "Shapes" },
    Name { ns: "", local: "Connects" },
    Name { ns: "", local: "Shapes" },
];
/// The container of a group's `Shape`.
const GROUP_CONTAINERS: &[Name] = &[Name { ns: NS_2012, local: "Shapes" }, Name { ns: NS_2011, local: "Shapes" }, Name { ns: "", local: "Shapes" }];

/// Each parent of the containers, in each namespace, with the containers it may hold empty.
const EMPTY_CONTAINERS: &[EmptyUnder] = &[
    EmptyUnder { parent: Name { ns: NS_2012, local: "VisioDocument" }, children: DOCUMENT_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2011, local: "VisioDocument" }, children: DOCUMENT_CONTAINERS },
    EmptyUnder { parent: Name { ns: "", local: "VisioDocument" }, children: DOCUMENT_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2012, local: "PageContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2011, local: "PageContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: "", local: "PageContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2012, local: "MasterContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2011, local: "MasterContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: "", local: "MasterContents" }, children: PART_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2012, local: "Shape" }, children: GROUP_CONTAINERS },
    EmptyUnder { parent: Name { ns: NS_2011, local: "Shape" }, children: GROUP_CONTAINERS },
    EmptyUnder { parent: Name { ns: "", local: "Shape" }, children: GROUP_CONTAINERS },
];

/// A shape's children whose order among themselves is no content: the sections, the text and the foreign data, in one namespace.
const fn shape_children(ns: &'static str) -> [Name; 3] {
    [Name { ns, local: "Section" }, Name { ns, local: "Text" }, Name { ns, local: "ForeignData" }]
}

const SHAPE_CHILDREN_2012: [Name; 3] = shape_children(NS_2012);
const SHAPE_CHILDREN_2011: [Name; 3] = shape_children(NS_2011);
const SHAPE_CHILDREN_NONE: [Name; 3] = shape_children("");

/// Whether a part is XML by its content type (or its name, when it has none): see the [module documentation](self).
fn is_xml(package: &Package, name: &str) -> bool {
    match package.content_type(name) {
        Some(content_type) => {
            let kind = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            kind.ends_with("xml") && !kind.starts_with("image/")
        }
        None => name.to_ascii_lowercase().ends_with(".rels"),
    }
}

/// The parts of a package as the check sees them: the content types stream, which is no part, and each part. What [`implied`] took out of the stream or of a part
/// (`adjusted`) is what the check sees in its place.
fn entries_of<'a>(package: &'a Package, adjusted: &'a implied::Adjusted) -> Vec<Entry<'a>> {
    let types = adjusted.content_types.as_deref().unwrap_or_else(|| package.content_types_bytes());
    let mut entries = vec![Entry { name: CONTENT_TYPES_ITEM, xml: true, bytes: types }];
    for name in package.parts() {
        if let Some(bytes) = package.part(name) {
            let bytes = if adjusted.parts.is_empty() { bytes } else { adjusted.parts.get(&fold(name)).map_or(bytes, Vec::as_slice) };
            entries.push(Entry { name, xml: is_xml(package, name), bytes });
        }
    }
    entries
}

/// Whether the packages in `a` and `b` have the same content, or what differs: each is read with `limits.opc` (so a package that cannot be
/// read is an error, with its class), then compared by [`CANON_RULES`]. `shown` of the answer lists the first 200 differences, with the
/// part and a path of element names and positions, and no name a person chose.
pub fn compare_content(a: &[u8], b: &[u8], limits: &Limits) -> Result<ContentDiff, VsdxError> {
    compare_content_counted(a, b, limits, &mut Work::default())
}

/// [`compare_content`], and what the comparison cost is added to `work`: `canon_steps` and `canon_bytes`, and `xml_events` for the parts
/// read, for the tests that hold a cost to a count.
pub fn compare_content_counted(a: &[u8], b: &[u8], limits: &Limits, work: &mut Work) -> Result<ContentDiff, VsdxError> {
    // Each package may inflate to half of what a comparison holds: the refusal is the container's, from the sizes the archive states, before
    // anything is inflated.
    let mut opc = limits.opc.clone();
    opc.max_package_bytes = opc.max_package_bytes.min(MAX_COMPARE_BYTES / 2);
    let (pa, pb) = (Package::read(a, &opc)?, Package::read(b, &opc)?);
    // What each package's own parts imply and the other lacks is taken out of its copy first (classes F3 and F4).
    let xml = XmlLimits { max_nodes: limits.opc.xml.max_nodes.min(MAX_COMPARE_ITEMS), ..limits.opc.xml.clone() };
    let (adjusted_a, adjusted_b) = implied::adjust(&pa, &pb, &xml, work);
    let (ea, eb) = (entries_of(&pa, &adjusted_a), entries_of(&pb, &adjusted_b));
    compare_entries(&ea, &eb, true, &CANON_RULES, &limits.opc.xml, work).map_err(|error| match error {
        CanonError::Xml { part, error } => VsdxError::Xml { part, error },
        CanonError::TooLarge { limit, .. } => VsdxError::Limit { kind: LimitKind::XmlBytes, max: limit },
        // An attribute whose value, read through the namespaces in scope, is longer than the limit: the XML error of its kind, naming the part.
        CanonError::Values { part, limit } => {
            VsdxError::Xml { part, error: diacraft_opc::XmlError::AttributeTooLarge { limit: usize::try_from(limit).unwrap_or(usize::MAX) } }
        }
    })
}
