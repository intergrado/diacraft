//! What a read made of a drawing, beside the document: the [`ShapeIndex`], what was kept but not modelled, counted by kind, the warnings by
//! class, who wrote the file, and the work the read did, counted in the units each loop walks.
//!
//! **Kept, not modelled.** The model has slots for what the specs map, and `RawAttrs` and `Passthrough` for the attributes and elements of a
//! modelled shape or page it does not map. What it has no slot for at all (a property with no cell, a style of a family the model does not
//! hold) stays in the file's bytes, which the caller keeps, and is counted here by kind ([`ReadReport::kept`]), so that a test can hold every
//! kind of markup to "in the model, or counted".

use std::collections::BTreeMap;

use crate::Variant;
use crate::index::ShapeIndex;

/// The longest text from the file a report repeats (the generator, a warning's detail), in characters.
pub(crate) const MOST_SHOWN: usize = 256;

/// What a read cost, in the units each loop walks: a test holds a read to these and not to a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadWork {
    /// The package's own read: the archive, the manifest and the two trees (the container's counts).
    pub package: diacraft_container::Work,
    /// Elements of the trees the reader visited.
    pub elements: u64,
    /// Attributes the reader resolved, each once.
    pub attributes: u64,
    /// Steps along `style:parent-style-name` links, and lookups of a property along a chain of styles.
    pub style_steps: u64,
    /// Path commands, coordinate pairs and transform terms parsed.
    pub path_units: u64,
    /// Pieces of text walked: text runs, spaces, tabs, line breaks and spans.
    pub text_units: u64,
    /// Items charged to [`crate::Limits::max_items`]: cells, rows, sections, shapes, text pieces, and the markup kept unmodelled.
    pub items: u64,
    /// Bytes of `meta.xml` and `settings.xml` parsed.
    pub small_part_bytes: u64,
    /// Bytes of text made for the shapes, charged to [`crate::Limits::max_text_bytes`] before it is made.
    pub text_bytes: u64,
    /// Names looked up in one step each: a shape's layer, a page's master page.
    pub name_lookups: u64,
    /// Bytes of names hashed to look a list style, a font face, a dash, a gradient, a marker or a linked style up by name: each name's
    /// length, for each lookup made. A list that names no style of its own reads its enclosing list's, a face name held in one place is
    /// looked up once, and the dash, gradient, marker and link names a style holds once when its part is read, however many lists, runs
    /// and shapes show them.
    pub name_bytes: u64,
    /// Style property values parsed: each value a style holds as a length, a percentage, a colour, a size and so on, once, when its style is
    /// read, whatever the number of runs, paragraphs, list items and shapes that show it. A value is read into its typed form then, and
    /// every use reads the typed form (`read/typed.rs`).
    pub value_parses: u64,
    /// Levels of list styles visited to choose the one a list's depth names: the most a binary search of the style's levels can take, for
    /// each list that has a style.
    pub list_levels: u64,
    /// Bytes of texts compared one by one to tell whether two cells hold the same value or the same formula: the length of each pair of
    /// texts of one length, held in two places, that is no longer than a short text (64 bytes). A longer text is a style's text (a font's
    /// family, a bullet), one shared value for every cell that shows it, so two cells show the same one or two different ones and no byte
    /// is read. What the count holds is the short texts (colours, numbers), at most 64 bytes a comparison, and a style's long text does
    /// not add to it however many shape-style pairs show it.
    pub compared_bytes: u64,
    /// Tab stops walked to tell whether two lists of them are the same: the length of a style's list when the style is read, to share it
    /// with an equal list (and again for each equal-length list of the same hash it is compared with), and nothing for a shape-style pair,
    /// which compares the lists by identity.
    pub tab_stops_walked: u64,
    /// Entries of one conversion's cell list visited by the search for the cell of the same name in another's, for each cell of the
    /// first: bounded by the cells a conversion can give, and not by anything a style holds.
    pub cells_scanned: u64,
    /// Equations of enhanced geometries evaluated: each once, in dependency order, whatever number of equations and parameters read it.
    pub equations: u64,
    /// Steps of the walk over the equations' dependencies: each dependency looked at once and each equation once, however they refer to one
    /// another (cycles and back-edges included).
    pub equation_steps: u64,
    /// Operations run to evaluate equations and the parameters of paths, handles, glue points and text areas.
    pub equation_ops: u64,
    /// Bytes of expression text compiled for enhanced geometries: each equation's formula and each parameter of a path, a glue point or a
    /// text area that is not a plain number, once. A parameter's bytes are charged to the items (one for 64 bytes) when it is read, so an
    /// arc that makes no row still pays for its parameters.
    pub expression_bytes: u64,
    /// Nodes of the formula trees the reader parsed for cells, each tree once however many cells share it. Each node is charged to the items
    /// at the formula crate's bytes a node, so that a tree costs by its nodes and not by the length of its text.
    pub formula_nodes: u64,
    /// Bytes of pictures the model holds: each part once, however many frames name it, and each inline picture as decoded.
    pub media_bytes: u64,
    /// The items of [`ReadWork::items`] that DiaCraft's document file cost: its bytes, the entries it holds for groups (read ahead of the pages) and the rest of it, read over the
    /// document. A save asks for it: what the rest of the file costs is `items` less this, and what is left of the reader's items for a document file the save writes.
    pub docfile_items: u64,
}

impl ReadWork {
    /// What the read cost, as a save's memory estimate counts it ([`crate::index::ReadCost`]): the tree is the larger of what the read
    /// counted (its elements and attributes, and the events it parsed), at most the reader's cap.
    pub(crate) fn cost(&self) -> crate::index::ReadCost {
        let nodes = self.elements.saturating_add(self.attributes).saturating_add(self.package.xml_events);
        crate::index::ReadCost {
            items: self.items,
            media_bytes: self.media_bytes,
            tree_nodes: nodes.min(crate::limits::MAX_READ_XML_NODES as u64),
            inflated_bytes: self.package.inflated,
            figures: crate::figures::Figures::default(),
        }
    }
}

/// Figures of the document read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Pages, background pages included.
    pub pages: usize,
    /// Of those, background pages made from master pages that hold shapes.
    pub background_pages: usize,
    /// Shapes modelled, over all pages.
    pub shapes: usize,
    /// Shape elements kept unmodelled, over all pages (a group's members counted inside it).
    pub kept_shapes: usize,
    /// Style sheets, the root included.
    pub styles: usize,
    /// Cells written, over every sheet.
    pub cells: usize,
    /// The most shapes on one page, kept ones included.
    pub max_shapes_in_a_page: usize,
}

/// What a read of the items DiaCraft's document file carries of the VSDX file's data did: how many it found and what became of each. Every item is judged against the file it
/// now sits in; an item whose object is gone is dropped silently, and its count is here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CarryReport {
    /// Items found (the shapes' own, on their styles, are not among them).
    pub items: u64,
    /// Items kept.
    pub kept: u64,
    /// Items dropped because their stamp no longer matches what the file says.
    pub stale: u64,
    /// Items dropped because their object is gone from the file (silently).
    pub gone: u64,
    /// Items, records and parts left out with a note: damaged, ambiguous, naming an entry the package does not have, or a part no package could hold.
    pub left_out: u64,
    /// Parts kept.
    pub parts_kept: u64,
}

/// What a read made of a drawing. See the [module documentation](self).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// Each page's shapes by the element they came from.
    pub index: ShapeIndex,
    /// What was kept but not modelled, by kind (`custom_shape`, `group`, `property`, ...): how many.
    pub kept: BTreeMap<&'static str, u64>,
    /// `meta:generator`, cut to [`MOST_SHOWN`] characters; `None` when the file names none.
    pub generator: Option<String>,
    /// `office:version` on `content.xml`'s root, cut short likewise; `None` when it has none.
    pub office_version: Option<String>,
    /// A drawing or a template, by what the package says; `None` when it says neither. A flat drawing is told by its root: one that
    /// says nothing this crate knows is read as a drawing.
    pub variant: Option<Variant>,
    /// How the drawing was written: a package, or one flat XML document (`.fodg`).
    pub form: crate::Form,
    /// Pictures a flat drawing links and does not hold: a `draw:image` with an `xlink:href` and no inline data, never opened, drawn as a
    /// placeholder, its href kept as written. Always 0 for a package.
    pub linked_pictures: u64,
    /// Objects DiaCraft cannot draw and has no picture of, each given a label for the placeholder the renderer draws. **Not in
    /// [`ReadReport::kept`]**: each is already kept there under its own kind (`frame_object`, `measure`, ...), and a second entry would count it twice.
    pub object_placeholders: u64,
    /// Custom shapes of a kind this reader does not know (a type and no path, or no geometry at all) that are drawn as their frame's box:
    /// what `unknown_shape_type` says once for the document. A `rectangle`, which is that box exactly, is not among them. **Not in [`ReadReport::kept`]**, where
    /// each is already counted under `shape_type`.
    pub boxed_shapes: u64,
    /// Whether the package holds a signature file. Nothing is verified.
    pub signed: bool,
    /// Whether the package holds macros or scripts. Nothing is run.
    pub macros: bool,
    /// **What the drawing does not hold, by class**: markup the read neither keeps in the model nor carries in DiaCraft's data, so that a copy saved from the
    /// drawing (a `.diacraft` file, or a file of another format) is without it, while a save into this file keeps its bytes. Counted as the read meets each, never by a walk of its own:
    /// `element_kind` (a shape of a kind the model keeps only as a placeholder, which a copy writes as another element), `font_declaration` (a font declaration the one font table of the
    /// model cannot hold apart: a repeat within one part, or a name the two parts declare differently), `shape_order` (a shape element kept unmodelled, a hyperlink wrapper, with a
    /// modelled shape after it: a copy writes it after them), `style_property` (a property a cell reads and no copy writes: a default tab
    /// distance) and `container_markup` (what the containers of a flat drawing hold that no field reads). [`ReadReport::not_kept_note`] says each in a line, which the read puts first
    /// among its warnings.
    pub not_kept: BTreeMap<&'static str, u64>,
    /// Warnings by class, counted.
    pub warning_counts: BTreeMap<&'static str, u64>,
    /// The first warnings, in words, at most `Limits::max_report_entries`.
    pub warnings: Vec<String>,
    pub stats: Stats,
    pub work: ReadWork,
    /// What the read of the items of DiaCraft's document file did.
    pub carry: CarryReport,
    /// What the document's native markup alone gives, before DiaCraft's document file was put over it: what a save compares the model with to
    /// say what the file holds. Bookkeeping: two reports are equal whatever it holds.
    pub(crate) native: crate::ext::docfile::Native,
}

impl ReadReport {
    /// How many of `kind` were kept unmodelled.
    pub fn kept(&self, kind: &str) -> u64 {
        self.kept.get(kind).copied().unwrap_or(0)
    }

    /// How many warnings of `class` the read gave.
    pub fn warned(&self, class: &str) -> u64 {
        self.warning_counts.get(class).copied().unwrap_or(0)
    }

    /// How many of `class` the drawing does not hold ([`ReadReport::not_kept`]).
    pub fn not_kept(&self, class: &str) -> u64 {
        self.not_kept.get(class).copied().unwrap_or(0)
    }

    /// **The open's words for what the drawing does not hold**: a first line that says what it means, then a line for each class with its count, whatever the count; none
    /// when the drawing holds everything the file says. See [`not_kept_lines`].
    pub fn not_kept_note(&self) -> Vec<String> {
        not_kept_lines(&self.not_kept)
    }
}

/// The class of [`ReadReport::warning_counts`] the note's lines are counted under, one for each line.
pub(crate) const NOT_KEPT_WARNING: &str = "not_kept";

/// The first line of the note: what the lines after it mean, for a drawing that has the file and for a new one a template made.
pub const NOT_KEPT_HEAD: &str = "What DiaCraft does not hold of this file: a save into the file keeps it, and a copy saved as another file (.diacraft or another format), or a new drawing \
                                 made from it, is without it";

/// What each class of [`ReadReport::not_kept`] is, for one and for many, in the order the note names them.
const NOT_KEPT_CLASSES: [(&str, &str, &str); 5] = [
    (
        "element_kind",
        "shape of a kind DiaCraft keeps only as a placeholder (a dimension line, a callout, a form control or a 3-D scene), which a copy writes as a plain shape",
        "shapes of kinds DiaCraft keeps only as placeholders (dimension lines, callouts, form controls or 3-D scenes), which a copy writes as plain shapes",
    ),
    (
        "font_declaration",
        "font declaration that repeats a name, which DiaCraft's one table of fonts holds once",
        "font declarations that repeat a name, which DiaCraft's one table of fonts holds once",
    ),
    (
        "shape_order",
        "shape in a hyperlink, which a copy writes over the shapes that were over it",
        "shapes in hyperlinks, which a copy writes over the shapes that were over them",
    ),
    ("style_property", "style property that no copy writes (a default tab distance)", "style properties that no copy writes (default tab distances)"),
    (
        "container_markup",
        "piece of the file's markup that no part of the drawing holds",
        "pieces of the file's markup that no part of the drawing holds",
    ),
];

/// The lines of the note for `counts` (class to count): [`NOT_KEPT_HEAD`], then a line for each class whose count is not 0, in a fixed order ("3 shapes of kinds DiaCraft keeps only as
/// placeholders ..."); nothing when every count is 0. Each line is at most [`NOT_KEPT_LINE_CHARS`] characters whatever the count, so that the window shows it whole. A class this build
/// has no words for is named by its word, so that no count is left out.
pub(crate) fn not_kept_lines(counts: &BTreeMap<&'static str, u64>) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (class, one, many) in NOT_KEPT_CLASSES {
        match counts.get(class).copied().unwrap_or(0) {
            0 => {}
            1 => lines.push(format!("1 {one}")),
            n => lines.push(format!("{n} {many}")),
        }
    }
    for (class, n) in counts.iter().filter(|(c, n)| **n > 0 && !NOT_KEPT_CLASSES.iter().any(|(k, ..)| k == *c)) {
        lines.push(format!("{n} of {} that no part of the drawing holds", class.replace('_', " ")));
    }
    if !lines.is_empty() {
        lines.insert(0, NOT_KEPT_HEAD.to_owned());
    }
    lines
}

/// The longest line of the note, in characters: what a line of the window's list shows whole.
pub const NOT_KEPT_LINE_CHARS: usize = 200;

/// `text` cut to [`MOST_SHOWN`] characters, control characters left out.
pub(crate) fn shown(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MOST_SHOWN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_is_a_line_for_each_class_with_its_count_after_what_it_means() {
        let mut counts = BTreeMap::new();
        assert!(not_kept_lines(&counts).is_empty());
        counts.insert("font_declaration", 0);
        assert!(not_kept_lines(&counts).is_empty(), "a count of 0 says nothing");
        counts.insert("font_declaration", 100_000);
        counts.insert("element_kind", 1);
        assert_eq!(
            not_kept_lines(&counts),
            [
                NOT_KEPT_HEAD,
                "1 shape of a kind DiaCraft keeps only as a placeholder (a dimension line, a callout, a form control or a 3-D scene), which a copy writes as a plain shape",
                "100000 font declarations that repeat a name, which DiaCraft's one table of fonts holds once",
            ]
        );
    }

    #[test]
    fn every_line_fits_the_window_s_line_whatever_the_count() {
        let mut counts: BTreeMap<&'static str, u64> = NOT_KEPT_CLASSES.iter().map(|(c, ..)| (*c, u64::MAX)).collect();
        counts.insert("some_new_class_of_markup", u64::MAX);
        let lines = not_kept_lines(&counts);
        assert_eq!(lines.len(), NOT_KEPT_CLASSES.len() + 2);
        for line in &lines {
            assert!(line.chars().count() <= NOT_KEPT_LINE_CHARS, "{} characters: {line}", line.chars().count());
            assert!(!line.contains('\n'));
        }
        assert_eq!(lines.last().unwrap(), &format!("{} of some new class of markup that no part of the drawing holds", u64::MAX));
    }
}
