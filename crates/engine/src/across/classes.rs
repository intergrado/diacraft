//! What a file carries, in plain words (the native-format design's 5.6: "class names are plain words").
//!
//! A save across the formats routes what the target cannot say into its carrier, and **names each kind of it** in the `carried` field of the save result, so that nothing is carried (or left out)
//! without a word. The classes are found **in the model, by one counted walk**:
//!
//! * **Into a VSDX file** ([`into_vsdx`]): everything ODF-tagged (the `odf` side of every slot, the held values, the definitions, the layer set, the settings, the metadata, the style
//!   names), what a newer DiaCraft wrote that this build does not know (the `native` side), the parts and custom properties of the other formats. The walk **destructures each type of ODF data
//!   without `..`**, so a field the model gains is a compile error until it is classed, and a slot that holds anything but defaults is named by a class even when no finer word fits
//!   ("shape data", "page data", "style data", "document data").
//! * **Into an `.odg`**: the `.odg` writer says what it carried (`SaveNote::Carried`), in its own words ("shape data", "page data", "style data", "document data", "package parts", "macro
//!   project", "embedded objects"), which [`plain`] turns into the vocabulary; the window settings of the drawing are added here ([`into_odf`]).
//!
//! The words are the design's list ("line ends", "gradients", "hatches", "glue directions", "layer settings", "style names", "document properties", "settings", "macro project", "embedded
//! objects", "window settings", "unknown parts") and, for what it does not list, plain words of the same kind.

use std::collections::HashSet;

use diacraft_doc::{
    Cell, DefFamily, Document, Ext, Format, HeldGroup, Kept, OdfDoc, OdfPage, OdfRow, OdfShape, OdfStyle, OdfText, PartRole, RawAttrs, Row, Shape,
    ShapeList, Sheet, TextPart,
};

use super::hold::{is_macro_part, is_object_part};
use super::{MAX_STEPS, TOO_BIG};

/// The words, in the order a note lists them.
pub const VOCABULARY: [&str; 21] = [
    "line ends",
    "gradients",
    "hatches",
    "glue directions",
    "layer settings",
    "style names",
    "document properties",
    "settings",
    "macro project",
    "embedded objects",
    "window settings",
    "unknown parts",
    "dash patterns",
    "fill images",
    "page layouts",
    "package details",
    "digital signatures",
    "shape data",
    "page data",
    "style data",
    "document data",
];

const LINE_ENDS: usize = 0;
const GRADIENTS: usize = 1;
const HATCHES: usize = 2;
const GLUE_DIRECTIONS: usize = 3;
const LAYER_SETTINGS: usize = 4;
const STYLE_NAMES: usize = 5;
const DOCUMENT_PROPERTIES: usize = 6;
const SETTINGS: usize = 7;
const MACRO_PROJECT: usize = 8;
const EMBEDDED_OBJECTS: usize = 9;
const WINDOW_SETTINGS: usize = 10;
const UNKNOWN_PARTS: usize = 11;
const DASH_PATTERNS: usize = 12;
const FILL_IMAGES: usize = 13;
const PAGE_LAYOUTS: usize = 14;
const PACKAGE_DETAILS: usize = 15;
const DIGITAL_SIGNATURES: usize = 16;
const SHAPE_DATA: usize = 17;
const PAGE_DATA: usize = 18;
const STYLE_DATA: usize = 19;
const DOCUMENT_DATA: usize = 20;

/// A set of classes: a bit for each word of the [`VOCABULARY`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Classes(u32);

impl Classes {
    /// Puts the "document data" class in: something rides that no finer word names (the catch-all that keeps a carrier from carrying anything unnamed).
    pub(crate) fn catch_all(&mut self) {
        self.add(DOCUMENT_DATA);
    }

    pub(crate) fn add(&mut self, class: usize) {
        self.0 |= Classes::bit(class);
    }

    /// Whether the class is in.
    pub(crate) fn has(self, class: usize) -> bool {
        self.0 & Classes::bit(class) != 0
    }

    /// The bit of a class, and none for a class past the bits of the set (a class past `u32::MAX` was taken for the bit 31).
    fn bit(class: usize) -> u32 {
        u32::try_from(class).ok().and_then(|class| 1u32.checked_shl(class)).unwrap_or(0)
    }

    /// The words, in the order of the vocabulary.
    pub(crate) fn words(self) -> Vec<String> {
        VOCABULARY.iter().enumerate().filter(|(at, _)| self.has(*at)).map(|(_, word)| (*word).to_owned()).collect()
    }

    /// Whether no class is in.
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The class a word of the `.odg` writer's own (`diacraft_odg::SaveNote::Carried`) stands for, in the vocabulary: its "package parts" are the parts no other word names.
pub(crate) fn plain(writer_class: &str) -> Option<usize> {
    Some(match writer_class {
        "package parts" => UNKNOWN_PARTS,
        other => VOCABULARY.iter().position(|word| *word == other)?,
    })
}

/// The classes `words` name (the writer's words, made plain) among `into`.
pub(crate) fn from_writer<'a>(words: impl IntoIterator<Item = &'a str>, into: &mut Classes) {
    for word in words {
        if let Some(class) = plain(word) {
            into.add(class);
        }
    }
}

/// What the walk of the model counts: the steps taken, held to [`MAX_STEPS`] before the work is done.
struct Walk {
    classes: Classes,
    steps: u64,
    /// The sheets already looked at, by address: a sheet that many shapes share is looked at once.
    sheets: HashSet<usize>,
}

impl Walk {
    fn step(&mut self, n: usize) -> Result<(), &'static str> {
        self.steps = self.steps.saturating_add(u64::try_from(n).unwrap_or(u64::MAX)).saturating_add(1);
        if self.steps > MAX_STEPS { Err(TOO_BIG) } else { Ok(()) }
    }

    fn kept(&mut self, kept: &Kept, class: usize) {
        let Kept { raw, kept } = kept;
        if !raw.is_empty() || !kept.is_empty() {
            self.classes.add(class);
        }
    }

    /// A slot of `Kept` that held something of another format than VSDX: the class it is for. What a newer DiaCraft wrote is unknown.
    fn slot_kept(&mut self, slot: &Ext<Kept>, class: usize) {
        if let Some(data) = slot.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            self.kept(odf, class);
            self.kept(native, UNKNOWN_PARTS);
        }
    }

    /// The class a held value of one group is in; a fill's is told by what it says it fills with.
    fn held(&mut self, group: HeldGroup, attrs: &RawAttrs) {
        let class = match group {
            HeldGroup::Fill => match attrs.iter().find(|(name, _)| name.rsplit_once(':').map_or(*name, |(_, local)| local) == "fill").map(|(_, v)| v)
            {
                Some("hatch") => HATCHES,
                Some("bitmap") => FILL_IMAGES,
                _ => GRADIENTS,
            },
            HeldGroup::Line => DASH_PATTERNS,
            HeldGroup::LineEnds => LINE_ENDS,
            HeldGroup::Glue => GLUE_DIRECTIONS,
            HeldGroup::Shadow | HeldGroup::Text | HeldGroup::Frame | HeldGroup::Geometry => SHAPE_DATA,
        };
        self.classes.add(class);
    }

    fn row(&mut self, row: &Row, other: usize) {
        if let Some(data) = row.ext.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            let OdfRow { kept, style_name, held } = odf;
            self.kept(kept, other);
            if style_name.is_some() {
                self.classes.add(STYLE_NAMES);
            }
            if let Some(h) = held {
                self.held(h.group, &h.attrs);
            }
            self.kept(native, UNKNOWN_PARTS);
        }
    }

    fn cell(&mut self, cell: &Cell, other: usize) {
        if let Some(extra) = &cell.extra {
            self.slot_kept(&extra.ext, other);
        }
    }

    /// A sheet: its slot, its sections' and rows' slots and its cells', named `other` unless a finer word fits.
    fn sheet(&mut self, sheet: &Sheet, other: usize) -> Result<(), &'static str> {
        if !self.sheets.insert(std::ptr::from_ref(sheet).addr()) {
            return Ok(());
        }
        self.step(sheet.cells.len())?;
        self.slot_kept(&sheet.ext, other);
        for cell in sheet.cells.values() {
            self.cell(cell, other);
        }
        for section in &sheet.sections {
            self.step(section.rows.len().saturating_add(section.cells.len()))?;
            self.slot_kept(&section.ext, other);
            for cell in section.cells.values() {
                self.cell(cell, other);
            }
            for row in section.rows.iter() {
                self.row(row, other);
                for cell in row.cells.values() {
                    self.cell(cell, other);
                }
            }
        }
        Ok(())
    }

    fn text(&mut self, text: &diacraft_doc::Text) {
        for part in &text.parts {
            if matches!(part, TextPart::OdfRaw(_)) {
                self.classes.add(SHAPE_DATA);
            }
            if matches!(part, TextPart::NativeRaw(_)) {
                self.classes.add(UNKNOWN_PARTS);
            }
        }
        if let Some(data) = text.ext.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            let OdfText { kept, paragraphs } = odf;
            self.kept(kept, SHAPE_DATA);
            if !paragraphs.is_empty() {
                self.classes.add(STYLE_NAMES);
            }
            self.kept(native, UNKNOWN_PARTS);
        }
    }

    fn shape(&mut self, shape: &Shape) -> Result<(), &'static str> {
        self.step(1)?;
        if let Some(data) = shape.ext.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            let OdfShape { kept, style_name, held, units } = odf;
            self.kept(kept, SHAPE_DATA);
            if style_name.is_some() {
                self.classes.add(STYLE_NAMES);
            }
            for h in held {
                self.held(h.group, &h.attrs);
            }
            if !units.is_empty() {
                self.classes.add(SHAPE_DATA);
            }
            self.kept(native, UNKNOWN_PARTS);
        }
        if let Some(foreign) = &shape.foreign {
            self.slot_kept(&foreign.ext, SHAPE_DATA);
        }
        if let Some(text) = &shape.text {
            self.text(text);
        }
        self.sheet(&shape.sheet, SHAPE_DATA)
    }

    fn shapes(&mut self, list: &ShapeList) -> Result<(), &'static str> {
        // A walk with a stack of its own: a chain of groups as deep as the model allows is no recursion.
        let mut stack: Vec<&ShapeList> = vec![list];
        while let Some(list) = stack.pop() {
            for shape in list.iter() {
                self.shape(shape)?;
                if !shape.children.is_empty() {
                    stack.push(&shape.children);
                }
            }
        }
        Ok(())
    }

    fn page(&mut self, page: &diacraft_doc::Page) -> Result<(), &'static str> {
        self.step(1)?;
        if let Some(data) = page.ext.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            let OdfPage { kept, master_page, layout, master_page_attrs, layout_props, drawing_page_style, units, stamp: _ } = odf;
            if master_page.is_some()
                || layout.is_some()
                || !master_page_attrs.is_empty()
                || !layout_props.is_empty()
                || drawing_page_style.is_some()
                || !units.is_empty()
            {
                self.classes.add(PAGE_LAYOUTS);
            }
            self.kept(kept, PAGE_DATA);
            self.kept(native, UNKNOWN_PARTS);
        }
        for connect in page.connects.iter() {
            self.slot_kept(&connect.ext, PAGE_DATA);
        }
        self.sheet(&page.sheet, PAGE_DATA)?;
        self.shapes(&page.shapes)
    }

    fn style(&mut self, style: &diacraft_doc::Style) -> Result<(), &'static str> {
        self.step(1)?;
        if let Some(data) = style.ext.get() {
            let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
            let OdfStyle { kept, name, display_name, parent, family, unmapped, units, held, text_held } = odf;
            if name.is_some()
                || display_name.is_some()
                || parent.is_some()
                || family.is_some()
                || !unmapped.is_empty()
                || !units.is_empty()
                || !text_held.is_empty()
            {
                self.classes.add(STYLE_NAMES);
            }
            self.kept(kept, STYLE_DATA);
            for h in held {
                self.held(h.group, &h.attrs);
            }
            self.kept(native, UNKNOWN_PARTS);
        }
        self.sheet(&style.sheet, STYLE_DATA)
    }
}

/// The classes of the data of the `.odg` format (and of a newer DiaCraft) that `doc` holds, which a VSDX file has no place for: found by one walk of the model, held to [`MAX_STEPS`].
/// `macros` and `objects` say a macro project or objects of the other format ride (the VSDX writer says; some objects are written as embeddings and ride nowhere): their words are in the classes
/// then. The parts that ride are named by their roles: "digital signatures" and "unknown parts".
pub(crate) fn into_vsdx(doc: &Document, macros: bool, objects: bool) -> Result<Classes, &'static str> {
    let mut walk = Walk { classes: Classes::default(), steps: 0, sheets: HashSet::new() };
    if macros {
        walk.classes.add(MACRO_PROJECT);
    }
    if objects {
        walk.classes.add(EMBEDDED_OBJECTS);
    }
    // The document's own data.
    if let Some(data) = doc.ext.get() {
        let diacraft_doc::ExtData { odf, vsdx: _, native } = data;
        let OdfDoc { kept, mimetype, roots, manifest_rows, meta, settings, definitions, layers, styles } = odf;
        walk.step(definitions.len().saturating_add(styles.len()).saturating_add(layers.len()))?;
        walk.kept(kept, DOCUMENT_DATA);
        if mimetype.is_some() || !roots.is_empty() || !manifest_rows.is_empty() {
            walk.classes.add(PACKAGE_DETAILS);
        }
        if !meta.is_empty() {
            walk.classes.add(DOCUMENT_PROPERTIES);
        }
        if settings.is_some() {
            walk.classes.add(SETTINGS);
        }
        for definition in definitions {
            walk.classes.add(match definition.family {
                DefFamily::Marker => LINE_ENDS,
                DefFamily::Gradient | DefFamily::Opacity => GRADIENTS,
                DefFamily::Hatch => HATCHES,
                DefFamily::StrokeDash => DASH_PATTERNS,
                DefFamily::FillImage => FILL_IMAGES,
            });
        }
        if !layers.is_empty() {
            walk.classes.add(LAYER_SETTINGS);
        }
        if !styles.is_empty() {
            walk.classes.add(STYLE_NAMES);
        }
        walk.kept(native, UNKNOWN_PARTS);
    }
    // The parts and the properties of the other formats.
    walk.step(doc.parts.len())?;
    for part in doc.parts.iter().filter(|p| p.format != Format::Vsdx) {
        if is_macro_part(part) || is_object_part(part) {
            // (Said by the writer: a macro project may be left out, and an object may be an embedding.)
            continue;
        }
        walk.classes.add(match part.role {
            PartRole::Signature => DIGITAL_SIGNATURES,
            _ => UNKNOWN_PARTS,
        });
    }
    if doc.props.custom.iter().any(|c| c.format != Format::Vsdx) {
        walk.classes.add(DOCUMENT_PROPERTIES);
    }
    for face in doc.fonts.iter() {
        walk.slot_kept(&face.ext, DOCUMENT_DATA);
    }
    walk.sheet(&doc.sheet, DOCUMENT_DATA)?;
    for style in doc.styles.iter() {
        walk.style(style)?;
    }
    for page in doc.pages.iter() {
        walk.page(page)?;
    }
    for master in doc.masters.iter() {
        walk.step(1)?;
        walk.slot_kept(&master.ext, DOCUMENT_DATA);
        walk.sheet(&master.sheet, DOCUMENT_DATA)?;
        walk.shapes(&master.shapes)?;
    }
    Ok(walk.classes)
}

/// The classes the drawing adds to what the `.odg` writer says it carried: the window settings the model holds (the windows part of a VSDX file, which the document file carries).
pub(crate) fn into_odf(doc: &Document) -> Classes {
    let mut classes = Classes::default();
    if doc.vsdx.as_ref().is_some_and(|held| held.windows.is_some()) {
        classes.add(WINDOW_SETTINGS);
    }
    classes
}

/// Whether the class is the one that rides on shapes (the rest ride in DiaCraft's document data).
pub(crate) fn is_shape_class(word: &str) -> bool {
    word == VOCABULARY[SHAPE_DATA]
}

/// The number of digital signature parts of the format `format` that `doc` holds: the signatures of a file of that format, which no longer match the file the model writer makes.
pub(crate) fn signatures_of(doc: &Document, format: Format) -> u32 {
    let count = doc
        .parts
        .iter()
        .filter(|p| {
            p.format == format
                && match format {
                    // An `.odg`'s signature entry (`META-INF/documentsignatures.xml`): the file is one signature record. (The macros' own belongs to the macro project.)
                    Format::Odf => p.role == PartRole::Signature,
                    // A VSDX package's XML signature parts, as the writer counts them (`write/held.rs`).
                    Format::Vsdx => p.role == PartRole::Signature && p.media_type.to_ascii_lowercase().contains("xmlsignature"),
                    Format::Native => false,
                }
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class past the bits of the set is put in as none and is never found (a class past `u32::MAX` was taken for the bit 31, which no word names, and found there).
    #[test]
    fn a_class_past_the_bits_of_the_set_is_never_put_in_or_found() {
        let mut set = Classes::default();
        for class in [32, 64, usize::MAX / 2, usize::MAX] {
            set.add(class);
            assert!(!set.has(class), "{class}");
        }
        assert!(set.is_empty() && !set.has(31));
        set.add(31);
        assert!(set.has(31) && !set.has(usize::MAX));
        set.add(DOCUMENT_DATA);
        assert_eq!(set.words(), vec!["document data".to_owned()]);
    }
}
