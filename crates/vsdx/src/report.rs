//! What the reader says about a file it opened: the kind of package, what the model did not take, what was odd, and the sizes that
//! want confirmed against real files.
//!
//! **Nothing here holds a formula, a cell value or a shape's text.** A warning's text holds words, numbers (an `ID`, a count) and part
//! names, which are the package's structure; a class word (`warning_counts`, `skipped_counts`) is a fixed string. A name a user made up,
//! a page name, a master name, is not in a message.

use std::collections::BTreeMap;

use crate::ns::Ns;

/// What a package is, by its document part's content type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Drawing,
    Stencil,
    Template,
}

/// One of the six variants: a kind, with or without macros allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub kind: Kind,
    pub macro_enabled: bool,
}

impl Variant {
    /// The variant a document part's content type names, or `None` for any other.
    pub fn from_content_type(content_type: &str) -> Option<Variant> {
        let (kind, macro_enabled) = match content_type {
            "application/vnd.ms-visio.drawing.main+xml" => (Kind::Drawing, false), // brand-ok: a content type the format defines
            "application/vnd.ms-visio.drawing.macroEnabled.main+xml" => (Kind::Drawing, true), // brand-ok: a content type the format defines
            "application/vnd.ms-visio.stencil.main+xml" => (Kind::Stencil, false), // brand-ok: a content type the format defines
            "application/vnd.ms-visio.stencil.macroEnabled.main+xml" => (Kind::Stencil, true), // brand-ok: a content type the format defines
            "application/vnd.ms-visio.template.main+xml" => (Kind::Template, false), // brand-ok: a content type the format defines
            "application/vnd.ms-visio.template.macroEnabled.main+xml" => (Kind::Template, true), // brand-ok: a content type the format defines
            _ => return None,
        };
        Some(Variant { kind, macro_enabled })
    }

    /// The content type of the document part of this variant: the inverse of [`Variant::from_content_type`].
    pub fn content_type(self) -> &'static str {
        match (self.kind, self.macro_enabled) {
            (Kind::Drawing, false) => "application/vnd.ms-visio.drawing.main+xml", // brand-ok: a content type the format defines
            (Kind::Drawing, true) => "application/vnd.ms-visio.drawing.macroEnabled.main+xml", // brand-ok: a content type the format defines
            (Kind::Stencil, false) => "application/vnd.ms-visio.stencil.main+xml", // brand-ok: a content type the format defines
            (Kind::Stencil, true) => "application/vnd.ms-visio.stencil.macroEnabled.main+xml", // brand-ok: a content type the format defines
            (Kind::Template, false) => "application/vnd.ms-visio.template.main+xml", // brand-ok: a content type the format defines
            (Kind::Template, true) => "application/vnd.ms-visio.template.macroEnabled.main+xml", // brand-ok: a content type the format defines
        }
    }

    /// The file extension of the variant, without its dot: `vsdx`, `vsdm`, `vssx`, `vssm`, `vstx`, `vstm`.
    pub fn extension(self) -> &'static str {
        match (self.kind, self.macro_enabled) {
            (Kind::Drawing, false) => "vsdx",
            (Kind::Drawing, true) => "vsdm",
            (Kind::Stencil, false) => "vssx",
            (Kind::Stencil, true) => "vssm",
            (Kind::Template, false) => "vstx",
            (Kind::Template, true) => "vstm",
        }
    }

    /// The variant an extension names (any ASCII case, no dot), or `None`.
    pub fn from_extension(extension: &str) -> Option<Variant> {
        let kind_and_macro = match extension.to_ascii_lowercase().as_str() {
            "vsdx" => (Kind::Drawing, false),
            "vsdm" => (Kind::Drawing, true),
            "vssx" => (Kind::Stencil, false),
            "vssm" => (Kind::Stencil, true),
            "vstx" => (Kind::Template, false),
            "vstm" => (Kind::Template, true),
            _ => return None,
        };
        Some(Variant { kind: kind_and_macro.0, macro_enabled: kind_and_macro.1 })
    }

    /// The words for a variant in a sentence: `drawing`, `drawing that allows macros`.
    pub fn kind_word_macros(self) -> String {
        if self.macro_enabled { format!("{} that allows macros", self.kind_word()) } else { self.kind_word().to_owned() }
    }

    /// The words for a kind: `drawing`, `stencil`, `template`.
    pub fn kind_word(self) -> &'static str {
        match self.kind {
            Kind::Drawing => "drawing",
            Kind::Stencil => "stencil",
            Kind::Template => "template",
        }
    }
}

/// Whether the extension a caller saw disagrees with what the package is, in words for the person who opened it, or `None`
/// when they agree. The bytes carry no file name, so the reader cannot do this; the caller that has the name does, with the variant of the
/// report. An extension that is none of the six (a `.vsd`, a `.zip`) disagrees with any package. The package's own kind is read from its
/// content type, and wins (the file is read as what it is, with this warning).
pub fn check_extension(variant: Option<Variant>, extension: &str) -> Option<String> {
    let named = Variant::from_extension(extension);
    match (variant, named) {
        (Some(v), Some(n)) if v == n => None,
        (Some(v), Some(n)) => Some(format!("the extension says a {}, and the package is a {}", n.kind_word_macros(), v.kind_word_macros())),
        (Some(v), None) => Some(format!("the extension is not one of a drawing package's, and the package is a {}", v.kind_word_macros())),
        (None, _) => None,
    }
}

/// Something in the file that the model has no place for. It is not lost: the package keeps every part as bytes, and a write at the tree
/// level patches the part's own tree. This says the *model* does not hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The part it is in.
    pub part: String,
    /// A fixed word for what it is: `master_shortcut`, `colorentry_unreadable`, `page_part_unreadable`, ...
    pub class: &'static str,
}

/// How many VSDX parts were in each vocabulary (`ns.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NamespaceUse {
    pub main_2012: usize,
    pub core_2011: usize,
    /// A part with no default namespace, read as the reference app's.
    pub none: usize,
    /// A part whose root is in another namespace (it is not read as the reference app's).
    pub other: usize,
}

impl NamespaceUse {
    pub(crate) fn count(&mut self, ns: Ns) {
        match ns {
            Ns::Main2012 => self.main_2012 += 1,
            Ns::Core2011 => self.core_2011 += 1,
            Ns::None => self.none += 1,
            Ns::Other => self.other += 1,
        }
    }
}

/// What the DiaCraft solution part (`SolutionXML Name="DiaCraft"`, URN `urn:app.diacraft:extension`) held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiacraftPart {
    /// No solution named `DiaCraft`.
    Absent,
    /// Read: the `dc:version` it states (cut to 32 characters), and how many elements its payload holds.
    Loaded { version: String, children: usize },
    /// A solution named `DiaCraft` that is not in the shape of R27, or is too large: kept, and not loaded
    /// The word says why.
    Unreadable(&'static str),
}

/// What the judge did with the items of the other format's data that DiaCraft's solution part carries (`carry.rs`): how many it met, kept and dropped, and why.
/// A part it left out for what it names (a part the package lacks, two records naming one) is also a warning, in words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CarryReport {
    /// The items of a kind this build reads that the part held.
    pub items: u64,
    /// The items that were good and are in the model.
    pub kept: u64,
    /// The items dropped because the stamp no longer matches what the file says: another program edited what they sit beside.
    pub stale: u64,
    /// The items dropped because the object they are keyed to is not in the file, or because two objects have the key: silently, as R28 says.
    pub gone: u64,
    /// The items left out for being damaged or for what the memory a read may hold could not pay, each also a warning.
    pub left_out: u64,
    /// The parts of DiaCraft's the package held, and the parts of the other format that came into the model from them.
    pub parts_seen: u64,
    pub parts_kept: u64,
}

/// Sizes, for the sample-file run: the maxima that confirm or move the caps of and of [`crate::Limits`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Parts in the package, and the bytes of all of them inflated.
    pub parts: usize,
    pub uncompressed_bytes: u64,
    pub styles: usize,
    pub masters: usize,
    pub pages: usize,
    /// Shapes in the document, nested ones counted, over the page and master parts read, **a part that several rows name counted once for each
    /// of them** (the figure the shapes budget is charged: what a walker of the document meets).
    pub shapes: usize,
    /// Shapes in each page's part, in page order, nested ones counted: one entry for each page, 0 for a page with no part.
    pub shapes_per_page: Vec<usize>,
    pub max_shapes_in_a_page: usize,
    pub max_shapes_in_a_master: usize,
    /// The deepest nesting of `Shape` elements: 1 for a shape on a page.
    pub max_shape_depth: usize,
    pub cells: usize,
    /// XML items kept in the model, against `Limits::max_items`.
    pub items: usize,
    /// Bytes of XML parsed from the document's own parts, against `Limits::max_xml_bytes`.
    pub xml_bytes: u64,
    /// Cells that hold a formula of their own.
    pub formula_cells: usize,
    /// Nodes of the formulas parsed, against `Limits::max_formula_nodes`.
    pub formula_nodes: usize,
    pub max_cells_in_one_place: usize,
    pub max_sections_in_a_sheet: usize,
    pub max_rows_in_a_section: usize,
    pub media_bytes: u64,
    /// Bytes of memory charged to the read's pool and still held when it ended (the model that was built: the trees are given back), against
    /// `Limits::max_memory` less the file and the package (`cost.rs`). **It also holds the bytes of the held parts** (`held_bytes`, as `Arc<[u8]>` they take), which are
    /// the package's, moved into the document when the read is done and not charged to the pool a second time: they are the model's own for the ledger of the process.
    pub memory_bytes: u64,
    /// The most bytes the pool was charged at any moment, the trees of the parts being read held in it: what a sample-file run sets beside the pool, which is
    /// `Limits::max_memory` less `file_bytes` and `uncompressed_bytes`.
    pub memory_peak: u64,
    /// The most bytes the tree of one part held (every element, attribute and text of it, as it is allocated), at the parts read.
    pub max_tree_bytes: u64,
    pub themes: usize,
    pub connects: usize,
    // ---- what confirms the container's caps and the XML limits against real files (the sample-file run's fix list): read for a
    // refused file too, as far as the reader got.
    /// The size of the file.
    pub file_bytes: u64,
    /// Rows of the archive's central directory: parts, folders and the content types stream (`opc::Limits::max_parts` is checked against it).
    pub zip_rows: usize,
    /// The longest part name, in bytes, as the package names it.
    pub max_part_name_bytes: usize,
    /// The largest part, inflated, in bytes.
    pub max_part_bytes: u64,
    /// `Default` and `Override` rows of `[Content_Types].xml`.
    pub content_type_rows: usize,
    /// The largest `.rels` part read, in bytes, and the most rows in one.
    pub max_rels_bytes: u64,
    pub max_rels_rows: usize,
    /// Over the XML parts of the document the reader parsed: the most items in one tree (as `opc` counts them: each element, attribute, text,
    /// comment and instruction), the deepest nesting, and the longest attribute name or value in bytes.
    pub max_xml_items: usize,
    pub max_xml_depth: usize,
    pub max_xml_attr_bytes: usize,
}

/// Where in the file the reader was when it refused it: the part, and the section of the sheet it was reading when there was one. A fixed
/// word and the part's name when the name is package structure (`unheld::part_label`), never a name a person typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedAt {
    pub part: String,
    /// One of the 17 predefined sections' names, `<other section>`, or empty for a cell directly under a sheet or no sheet at all.
    pub section: &'static str,
}

/// What reading a file said besides the document. See the module documentation for what it never holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// What the package is, or `None` when the document part's content type is none of the six (a warning says so).
    pub variant: Option<Variant>,
    /// The name of the document part, as the package has it.
    pub document_part: String,
    /// `Application` of `docProps/app.xml`, cut to 64 characters: the writer, per the census's definition. `None` without the part.
    pub application: Option<String>,
    /// What the model has no slot for: the first [`crate::MAX_REPORT_ENTRIES`], the rest only counted in `skipped_counts`. **Empty since
    /// **Every place the reader used to drop something is held in the model now (`held.rs`: the parts, the relationships, the rest of the
    /// properties, and the elements and attributes of each place `unheld.rs` names, in the `vsdx` side of the extension slots), and the test
    /// `no_position_drops_silently` injects each kind of thing at each position and fails on one that is not in the model.
    pub skipped: Vec<Skipped>,
    /// What was odd, in words, the first [`crate::MAX_REPORT_ENTRIES`]; the rest are only counted in `warning_counts`.
    pub warnings: Vec<String>,
    /// Every warning by class word, exactly counted.
    pub warning_counts: BTreeMap<&'static str, u64>,
    /// Every skipped item by class word, exactly counted. Empty as [`ReadReport::skipped`] is.
    pub skipped_counts: BTreeMap<&'static str, u64>,
    /// Parts the document holds beside its model (`Document::parts`): every part of the package that no field of the model rebuilds, kept as bytes with
    /// its content type and its relationships.
    pub held_parts: usize,
    /// The bytes of those parts, inflated, summed: what they add to the memory the model holds (`Stats::memory_bytes` counts them, once the read has moved them out of the package).
    pub held_bytes: u64,
    pub namespaces: NamespaceUse,
    pub stats: Stats,
    /// Where the reader was when it refused the file (`read_partial`); `None` for a file that was read.
    pub refused_at: Option<RefusedAt>,
    pub diacraft: Option<DiacraftPart>,
    /// What the judge did with the items DiaCraft's part carries.
    pub carry: CarryReport,
    /// Formulas that did not parse, by `ParseErrorKind::name`, and the ones the budget could not afford (`too_many_nodes`).
    pub formula_errors: BTreeMap<&'static str, u64>,
}

impl ReadReport {
    /// How many warnings of this class there were.
    pub fn warning_count(&self, class: &str) -> u64 {
        self.warning_counts.get(class).copied().unwrap_or(0)
    }

    /// How many items of this class were skipped.
    pub fn skipped_count(&self, class: &str) -> u64 {
        self.skipped_counts.get(class).copied().unwrap_or(0)
    }
}

/// The collector that fills the lists of a report, each bounded.
pub(crate) struct Notes {
    pub report: ReadReport,
    cap: usize,
}

impl Notes {
    pub(crate) fn new(cap: usize) -> Notes {
        Notes { report: ReadReport::default(), cap }
    }

    /// A warning: counted always, worded while the list has room. `message` is built only when it is kept.
    pub(crate) fn warn(&mut self, class: &'static str, message: impl FnOnce() -> String) {
        let n = self.report.warning_counts.entry(class).or_insert(0);
        *n = n.saturating_add(1);
        if self.report.warnings.len() < self.cap {
            self.report.warnings.push(message());
        }
    }
}

/// What the sheet codec says goes in the report like everything else the reader says: the warnings in one list, in the order they were made.
impl diacraft_sheetxml::Diagnostics for Notes {
    fn warn(&mut self, class: &'static str, message: impl FnOnce() -> String) {
        Notes::warn(self, class, message);
    }

    fn formula_error(&mut self, class: &'static str) {
        let n = self.report.formula_errors.entry(class).or_insert(0);
        *n = n.saturating_add(1);
    }
}
