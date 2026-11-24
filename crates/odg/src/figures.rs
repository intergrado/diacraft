//! What a document holds, counted from the document itself: its shapes, its cells and the bytes of text in them. The reader takes these figures
//! of the model it made ([`crate::ReadCost`]), and a save takes them again of the model it is given, so that **what the caller's model grew by
//! since it was read** is known before the file is read again (a model grown by 2 GB of cell text after the read was not
//! charged by a gate that went by the cost of the read alone). Both walks are this one function, so the two figures are comparable.
//!
//! **Every byte a model can hold is in the walk** (1,200 `User` rows named 1 MiB each, 1.26 GB the walk did not see, passed
//! the gate and aborted the save): the names and text of cells, rows, sections, shapes, pages, styles, masters and the document, the attributes and
//! the elements a model keeps beside its fields, the pictures and themes, the colours and the fonts.
//!
//! The walk is iterative (a stack of the lists still to walk, as deep as the shapes nest and no more), touches each shape, cell, row and node
//! once, and is counted ([`Figures::steps`]).

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{
    Cell, Cells, Connects, Document, Ext, Held, Kept, Master, OdfDoc, OdfPage, OdfRow, OdfShape, OdfStyle, OdfText, OdfTextStyle, Passthrough,
    Placed, RawAttrs, RawElement, RawNode, Shape, ShapeList, Sheet, Style, TextPart,
};

/// The most levels of nested shape lists the walk follows: the model's own cap is 200 ([`diacraft_doc::MAX_SHAPE_DEPTH`]); past this the
/// document is one the walk will not count, and says so by saturating.
const MOST_DEPTH: usize = 1_024;

/// What a document holds, as the walk counts it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Figures {
    /// Shapes, groups and their members included.
    pub shapes: u64,
    /// Cells: those of the document's, pages', shapes' and styles' sheets, in their sections and rows too.
    pub cells: u64,
    /// Rows, sections, connections, styles, masters, and the attributes and nodes a model keeps beside its fields: what a model holds besides
    /// cells and shapes, each a small allocation of its own.
    pub nodes: u64,
    /// Bytes of text the model holds: in cells (formula, value, unit, error, extra text, and the cell's name), rows (name, local name, type),
    /// sections (name), names, the text of shapes, the attributes and nodes kept whole, the names and properties of the document, its styles and
    /// masters, the colours and the fonts.
    pub text_bytes: u64,
    /// Bytes of pictures and themes the model holds (a picture shown by many shapes is counted once).
    pub media_bytes: u64,
    /// The units the walk went through: shapes, cells, rows, nodes and text parts, each once.
    pub steps: u64,
}

impl Figures {
    fn text(&mut self, text: &str) {
        self.text_bytes = self.text_bytes.saturating_add(text.len() as u64);
    }

    fn opt(&mut self, text: Option<&Arc<str>>) {
        if let Some(t) = text {
            self.text(t);
        }
    }

    fn node(&mut self) {
        self.nodes = self.nodes.saturating_add(1);
        self.steps = self.steps.saturating_add(1);
    }

    /// The attributes a model keeps beside its fields: each a node, with its name and its value.
    fn raw(&mut self, attrs: &RawAttrs) {
        for (name, value) in attrs.iter() {
            self.node();
            self.text(name);
            self.text(value);
        }
    }

    /// The elements a model keeps whole, counted by a walk that holds the nodes still to be seen and nothing deeper than that (no recursion).
    fn passthrough(&mut self, kept: &Passthrough) {
        self.nodes(kept.iter().map(|p| &p.node));
    }

    /// An element kept whole, as [`Figures::passthrough`] counts its kin.
    fn element(&mut self, element: &RawElement) {
        self.node();
        self.text(&element.name);
        self.raw(&element.attrs);
        self.nodes(element.children.iter());
    }

    /// The nodes of `list` and everything in them, by the walk of [`Figures::passthrough`].
    fn nodes<'a>(&mut self, list: impl Iterator<Item = &'a RawNode>) {
        let mut stack: Vec<&RawNode> = list.collect();
        while let Some(node) = stack.pop() {
            self.node();
            match node {
                RawNode::Element(e) => {
                    self.text(&e.name);
                    self.raw(&e.attrs);
                    stack.extend(e.children.iter());
                }
                RawNode::Text(t) | RawNode::Comment(t) => self.text(t),
                RawNode::Pi { target, data } => {
                    self.text(target);
                    self.text(data);
                }
            }
        }
    }

    /// What an object keeps beside its fields, tagged by format: the attributes and nodes of a [`Kept`].
    fn kept(&mut self, kept: &Kept) {
        self.raw(&kept.raw);
        self.passthrough(&kept.kept);
    }

    /// An extension slot, whatever it holds: the slot is a node, its `odf` side is counted by `odf`, and the VSDX elements and the unknown `.diacraft` content by their own
    /// walks. A slot that holds nothing costs nothing (it is a null pointer). **Every slot of the model is counted here**, so that a model that holds a great deal in one is
    /// no smaller to the gate than one that holds it in a field (12 masters of one shape with 100 MB of kept `ext` text passed a gate that
    /// held 125 KB, and the save aborted).
    fn slot<T: Default + Clone + PartialEq>(&mut self, ext: &Ext<T>, odf: impl FnOnce(&mut Figures, &T)) {
        let Some(data) = ext.get() else { return };
        self.node();
        odf(self, &data.odf);
        for (key, kept) in &data.vsdx.0 {
            self.node();
            self.text(key);
            self.kept(kept);
        }
        self.kept(&data.native);
    }

    /// A slot that holds markup alone.
    fn kept_slot(&mut self, ext: &Ext<Kept>) {
        self.slot(ext, |f, kept| f.kept(kept));
    }

    /// A held value: its attributes and the nodes inside it (its stamp is a number).
    fn held(&mut self, held: &Held) {
        self.node();
        self.raw(&held.attrs);
        self.passthrough(&held.kept);
    }

    /// What a style a paragraph or run named holds.
    fn text_style(&mut self, style: &OdfTextStyle) {
        self.node();
        self.opt(style.name.as_ref());
        for value in &style.held {
            self.node();
            self.raw(&value.attrs);
            self.passthrough(&value.kept);
        }
    }

    fn odf_text(&mut self, text: &OdfText) {
        self.kept(&text.kept);
        for paragraph in &text.paragraphs {
            self.text_style(&paragraph.style);
            if let Some(element) = &paragraph.element {
                self.element(element);
            }
            for level in &paragraph.levels {
                self.node();
                for element in [&level.list, &level.item].into_iter().flatten() {
                    self.element(element);
                }
            }
            for run in &paragraph.runs {
                self.text_style(&run.style);
                if let Some(element) = &run.element {
                    self.element(element);
                }
            }
        }
    }

    fn odf_shape(&mut self, shape: &OdfShape) {
        self.kept(&shape.kept);
        self.opt(shape.style_name.as_ref());
        for held in &shape.held {
            self.held(held);
        }
        self.raw(&shape.units);
    }

    fn odf_row(&mut self, row: &OdfRow) {
        self.kept(&row.kept);
        self.opt(row.style_name.as_ref());
        if let Some(held) = &row.held {
            self.held(held);
        }
    }

    fn odf_page(&mut self, page: &OdfPage) {
        self.kept(&page.kept);
        self.opt(page.master_page.as_ref());
        self.opt(page.layout.as_ref());
        self.raw(&page.master_page_attrs);
        self.raw(&page.layout_props);
        if let Some(style) = &page.drawing_page_style {
            self.element(style);
        }
        self.raw(&page.units);
    }

    fn odf_style(&mut self, style: &OdfStyle) {
        self.kept(&style.kept);
        for t in [&style.name, &style.display_name, &style.parent, &style.family] {
            self.opt(t.as_ref());
        }
        self.raw(&style.unmapped);
        self.raw(&style.units);
        for held in &style.held {
            self.held(held);
        }
        for value in &style.text_held {
            self.node();
            self.raw(&value.attrs);
            self.passthrough(&value.kept);
        }
    }

    /// The document's own `.odg` data and its parts: the roots, the manifest's rows, `meta.xml`'s fields, `settings.xml`, the definitions (a fill image's bytes are media), the
    /// layer set and the other style families.
    fn odf_doc(&mut self, doc: &OdfDoc, pictures: &mut HashSet<usize>) {
        self.kept(&doc.kept);
        self.opt(doc.mimetype.as_ref());
        for (name, attrs) in doc.roots.iter().chain(doc.manifest_rows.iter()) {
            self.node();
            self.text(name);
            self.raw(attrs);
        }
        self.passthrough(&doc.meta);
        if let Some(settings) = &doc.settings {
            self.element(settings);
        }
        for definition in &doc.definitions {
            self.node();
            self.text(&definition.name);
            self.element(&definition.element);
            if let Some(bytes) = &definition.bytes
                && pictures.insert(Arc::as_ptr(bytes).cast::<u8>() as usize)
            {
                self.media_bytes = self.media_bytes.saturating_add(bytes.len() as u64);
            }
        }
        for layer in &doc.layers {
            self.element(layer);
        }
        for style in &doc.styles {
            self.node();
            self.text(&style.family);
            self.text(&style.name);
            self.element(&style.element);
        }
    }

    /// One node of a text, the way the kept ones are counted: a part is a step, and what a kept part holds is walked.
    fn text_part(&mut self, part: &TextPart) {
        self.steps = self.steps.saturating_add(1);
        match part {
            TextPart::Chars(chars) => self.text_bytes = self.text_bytes.saturating_add(chars.len() as u64),
            TextPart::Fld { text, .. } => self.text_bytes = self.text_bytes.saturating_add(text.len() as u64),
            TextPart::Raw(node) => self.nodes(std::iter::once(node)),
            TextPart::OdfRaw(node) | TextPart::NativeRaw(node) => self.nodes(std::iter::once(&**node)),
            TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_) => {}
        }
    }

    fn cell(&mut self, cell: &Cell) {
        self.cells = self.cells.saturating_add(1);
        self.steps = self.steps.saturating_add(1);
        let held = cell.formula.text().map_or(0, str::len)
            + cell.value.as_ref().map_or(0, |v| v.text().len())
            + cell.unit.as_deref().map_or(0, str::len)
            + cell.error.as_deref().map_or(0, str::len)
            + cell.text().map_or(0, str::len);
        self.text_bytes = self.text_bytes.saturating_add(held as u64);
        self.raw(&cell.raw);
        if let Some(extra) = &cell.extra {
            self.passthrough(&extra.passthrough);
            self.kept_slot(&extra.ext);
        }
    }

    fn cells(&mut self, cells: &Cells) {
        for (name, cell) in cells.iter() {
            self.text(name.as_str());
            self.cell(cell);
        }
    }

    fn sheet(&mut self, sheet: &Sheet) {
        self.cells(&sheet.cells);
        self.raw(&sheet.raw);
        self.kept_slot(&sheet.ext);
        for section in &sheet.sections {
            self.node();
            self.text(section.name.as_str());
            self.cells(&section.cells);
            self.raw(&section.raw);
            self.passthrough(&section.passthrough);
            self.kept_slot(&section.ext);
            for row in section.rows.iter() {
                self.node();
                self.opt(row.name.as_ref());
                self.opt(row.local_name.as_ref());
                self.opt(row.row_type.as_ref());
                self.cells(&row.cells);
                self.raw(&row.raw);
                self.passthrough(&row.passthrough);
                self.slot(&row.ext, |f, odf| f.odf_row(odf));
            }
        }
    }

    fn connects(&mut self, connects: &Connects) {
        for c in connects {
            self.node();
            self.opt(c.from_cell.as_ref());
            self.opt(c.to_cell.as_ref());
            self.raw(&c.raw);
            self.kept_slot(&c.ext);
        }
    }

    fn shape(&mut self, shape: &Shape, pictures: &mut HashSet<usize>) {
        self.shapes = self.shapes.saturating_add(1);
        self.steps = self.steps.saturating_add(1);
        self.sheet(&shape.sheet);
        self.opt(shape.name.as_ref());
        self.opt(shape.name_u.as_ref());
        self.opt(shape.data1.as_ref());
        self.opt(shape.data2.as_ref());
        self.opt(shape.data3.as_ref());
        self.raw(&shape.raw);
        self.passthrough(&shape.passthrough);
        self.slot(&shape.ext, |f, odf| f.odf_shape(odf));
        if let Some(foreign) = &shape.foreign {
            self.node();
            self.raw(&foreign.attrs);
            self.opt(foreign.rel_id.as_ref());
            self.opt(foreign.text.as_ref());
            self.passthrough(&foreign.passthrough);
            self.kept_slot(&foreign.ext);
            if let Some(picture) = &foreign.picture
                && pictures.insert(Arc::as_ptr(picture) as usize)
            {
                self.text(&picture.part);
                self.text(&picture.content_type);
                self.media_bytes = self.media_bytes.saturating_add(picture.bytes.len() as u64);
            }
        }
        if let Some(text) = &shape.text {
            for part in &text.parts {
                self.text_part(part);
            }
            self.slot(&text.ext, |f, odf| f.odf_text(odf));
        }
    }

    /// The shapes of a list and of the lists inside them: a stack of the lists still to walk, as deep as the shapes nest. `false` for a list nested
    /// deeper than the walk follows.
    fn shapes_of(&mut self, shapes: &ShapeList, pictures: &mut HashSet<usize>) -> bool {
        let mut stack: Vec<(&ShapeList, usize)> = vec![(shapes, 0)];
        while let Some((list, at)) = stack.pop() {
            let Some(shape) = list.get(at) else { continue };
            stack.push((list, at + 1));
            self.shape(shape, pictures);
            if !shape.children.is_empty() {
                if stack.len() >= MOST_DEPTH {
                    return false;
                }
                stack.push((&shape.children, 0));
            }
        }
        true
    }

    fn style(&mut self, style: &Style) {
        self.node();
        self.opt(style.name.as_ref());
        self.opt(style.name_u.as_ref());
        self.sheet(&style.sheet);
        self.raw(&style.raw);
        self.passthrough(&style.passthrough);
        self.slot(&style.ext, |f, odf| f.odf_style(odf));
    }

    fn master(&mut self, master: &Master, pictures: &mut HashSet<usize>) -> bool {
        self.node();
        for t in [&master.name, &master.name_u, &master.unique_id, &master.rel_id, &master.icon] {
            self.opt(t.as_ref());
        }
        self.sheet(&master.sheet);
        self.connects(&master.connects);
        self.raw(&master.raw);
        self.passthrough(&master.passthrough);
        self.kept_slot(&master.ext);
        self.opt(master.part_name.as_ref());
        self.shapes_of(&master.shapes, pictures)
    }

    /// The figures saturated, for a document nested deeper than the walk follows: any comparison with it is a refusal.
    fn saturated() -> Figures {
        let most = u64::MAX / 4;
        Figures { shapes: most, cells: most, nodes: most, text_bytes: most, media_bytes: most, steps: 0 }
    }
}

/// The figures of `masters` alone, counted as [`of`] counts them (a picture shown by many shapes is counted once, over all the masters): what a writer that makes trees of
/// the masters' bodies knows before it makes them. `saturated` for masters nested deeper than the walk follows.
pub(crate) fn of_masters<'a>(masters: impl IntoIterator<Item = &'a Arc<Master>>) -> Figures {
    let mut figures = Figures::default();
    let mut pictures: HashSet<usize> = HashSet::new();
    for master in masters {
        if !figures.master(master, &mut pictures) {
            return Figures::saturated();
        }
    }
    figures
}

/// The figures of one sheet.
pub(crate) fn of_sheet(sheet: &Sheet) -> Figures {
    let mut figures = Figures::default();
    figures.sheet(sheet);
    figures
}

/// The figures of `doc`.
pub fn of(doc: &Document) -> Figures {
    let mut figures = Figures::default();
    let mut pictures: HashSet<usize> = HashSet::new();
    figures.sheet(&doc.sheet);
    figures.text(&doc.title);
    let p = &*doc.props;
    for t in [
        &p.subject,
        &p.creator,
        &p.last_modified_by,
        &p.keywords,
        &p.description,
        &p.category,
        &p.company,
        &p.manager,
        &p.hyperlink_base,
        &p.app_version,
        &p.created,
        &p.modified,
        &p.last_printed,
        &p.language,
    ] {
        figures.opt(t.as_ref());
    }
    for style in doc.styles.iter() {
        figures.style(style);
    }
    for theme in doc.themes.iter() {
        figures.node();
        figures.text(&theme.part);
        figures.media_bytes = figures.media_bytes.saturating_add(theme.bytes.len() as u64);
    }
    for colour in doc.colors.iter() {
        figures.node();
        figures.text(&colour.rgb);
    }
    for face in doc.fonts.iter() {
        figures.node();
        figures.text(&face.name_u);
        figures.raw(&face.raw);
        figures.kept_slot(&face.ext);
    }
    for prop in &doc.props.custom {
        figures.node();
        figures.text(&prop.name);
        figures.text(&prop.value_type);
        figures.text(&prop.value);
        figures.raw(&prop.raw);
    }
    figures.slot(&doc.ext, |f, odf| f.odf_doc(odf, &mut pictures));
    for part in doc.parts.iter() {
        figures.node();
        figures.text(&part.name);
        figures.text(&part.media_type);
        figures.raw(&part.manifest);
        for rel in part.rels.iter() {
            figures.node();
            for t in [&rel.source, &rel.id, &rel.kind, &rel.target] {
                figures.text(t);
            }
        }
        if pictures.insert(Arc::as_ptr(&part.bytes).cast::<u8>() as usize) {
            figures.media_bytes = figures.media_bytes.saturating_add(part.bytes.len() as u64);
        }
    }
    if let Some(held) = &doc.vsdx {
        for rel in &held.rels {
            figures.node();
            for t in [&rel.source, &rel.id, &rel.kind, &rel.target] {
                figures.text(t);
            }
        }
        for row in &held.content_types {
            figures.node();
            match row {
                diacraft_doc::ContentTypeRow::Default { extension, content_type } => {
                    figures.text(extension);
                    figures.text(content_type);
                }
                diacraft_doc::ContentTypeRow::Override { part, content_type } => {
                    figures.text(part);
                    figures.text(content_type);
                }
            }
        }
        for (name, kept) in &held.props_rest {
            figures.node();
            figures.text(name);
            figures.kept(kept);
        }
        if let Some(windows) = &held.windows {
            figures.element(windows);
        }
    }
    figures.passthrough(&doc.passthrough);
    if let Some(kept) = &doc.diacraft {
        figures.passthrough(&Passthrough::from_nodes(vec![Placed { after: 0, node: RawNode::Element((**kept).clone()) }]));
    }
    for master in doc.masters.iter() {
        if !figures.master(master, &mut pictures) {
            return Figures::saturated();
        }
    }
    for page in doc.pages.iter() {
        figures.sheet(&page.sheet);
        figures.text(&page.name);
        figures.opt(page.name_u.as_ref());
        figures.opt(page.rel_id.as_ref());
        figures.raw(&page.raw);
        figures.passthrough(&page.passthrough);
        figures.slot(&page.ext, |f, odf| f.odf_page(odf));
        figures.opt(page.part_name.as_ref());
        figures.connects(&page.connects);
        if !figures.shapes_of(&page.shapes, &mut pictures) {
            return Figures::saturated();
        }
    }
    figures
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{CellName, DocProps, Page, Row, Section, Text, Value};

    use super::*;

    fn shape_with(id: u32, cells: &[(&str, &str)], text: Option<&str>) -> Shape {
        let mut shape = Shape::new(id);
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, value) in cells {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
        }
        shape.text = text.map(|t| Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(t))], ..Default::default() }));
        shape
    }

    #[test]
    fn the_figures_are_the_shapes_cells_and_bytes_of_text_the_document_holds() {
        let mut doc = Document::empty();
        let mut page = Page::new(0, "pg");
        let mut group = shape_with(1, &[("A", "12345")], Some("hello"));
        group.children.push(Arc::new(shape_with(2, &[("B", "xy"), ("C", "z")], None)));
        page.shapes.push(Arc::new(group));
        page.shapes.push(Arc::new(shape_with(3, &[], None)));
        doc.pages.push(Arc::new(page));
        let f = of(&doc);
        assert_eq!((f.shapes, f.cells), (3, 3), "a group's member and the shape after the group are counted");
        // 5 + 2 + 1 bytes of values, 1 + 1 + 1 of the cells' names, 5 of the text, and the page's name and universal name of 2 each.
        assert_eq!(f.text_bytes, 5 + 2 + 1 + 3 + 5 + 2 + 2);
        // Each shape, cell and text part is a step.
        assert_eq!(f.steps, 3 + 3 + 1);
    }

    #[test]
    fn the_growth_of_a_cells_text_is_in_the_figures_and_nothing_else_moves() {
        let mut doc = Document::empty();
        let mut page = Page::new(0, "p");
        page.shapes.push(Arc::new(shape_with(1, &[("A", "1")], None)));
        doc.pages.push(Arc::new(page));
        let before = of(&doc);
        let Some(page) = doc.pages.get_mut(0) else { panic!("no page") };
        let Some(shape) = Arc::make_mut(page).shapes.get_mut(0) else { panic!("no shape") };
        let sheet = Arc::make_mut(&mut Arc::make_mut(shape).sheet);
        let Some(cell) = sheet.cells.get_mut("A") else { panic!("no cell") };
        cell.value = Some(Value::from_text(&"x".repeat(1_000)));
        let after = of(&doc);
        assert_eq!((after.shapes, after.cells), (before.shapes, before.cells));
        assert_eq!(after.text_bytes - before.text_bytes, 999);
    }

    #[test]
    fn a_document_nested_past_the_walk_is_counted_as_past_every_cap() {
        let mut doc = Document::empty();
        let mut page = Page::new(0, "p");
        let mut inner = Shape::new(1);
        for id in 2..(MOST_DEPTH as u32 + 10) {
            let mut outer = Shape::new(id);
            outer.children.push(Arc::new(inner));
            inner = outer;
        }
        page.shapes.push(Arc::new(inner));
        doc.pages.push(Arc::new(page));
        assert_eq!(of(&doc), Figures::saturated());
    }

    /// A document with one page of one shape, and `grow` made of that shape.
    fn grown(grow: impl FnOnce(&mut Shape)) -> Figures {
        let mut doc = Document::empty();
        let mut page = Page::new(0, "p");
        let mut shape = shape_with(1, &[("A", "1")], None);
        grow(&mut shape);
        page.shapes.push(Arc::new(shape));
        doc.pages.push(Arc::new(page));
        of(&doc)
    }

    /// What the walk did not see. Each place a model holds text, grown by a megabyte, is a megabyte more in the figures:
    /// a row's name, its local name and type, a section's name, a cell's name, the attributes kept beside a shape's fields, the elements kept
    /// whole (a deep one too), a shape's data and names, a picture (once for the shapes that show it).
    #[test]
    fn every_place_a_model_holds_text_is_counted_byte_for_byte() {
        let mb = "m".repeat(1 << 20);
        let base = grown(|_| {});
        let with = |f: &dyn Fn(&mut Shape, &str)| {
            let g = grown(|s| f(s, &mb));
            (g.text_bytes - base.text_bytes, g.media_bytes - base.media_bytes)
        };
        let big = 1u64 << 20;
        let row = |name: Option<&str>, local: Option<&str>, kind: Option<&str>, s: &mut Shape| {
            let mut section = Section::new("User", None);
            section.rows.push(Arc::new(Row {
                name: name.map(Arc::from),
                local_name: local.map(Arc::from),
                row_type: kind.map(Arc::from),
                ..Row::default()
            }));
            Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
        };
        assert_eq!(with(&|s, t| row(Some(t), None, None, s)).0, big + "User".len() as u64, "a row's name");
        assert_eq!(with(&|s, t| row(None, Some(t), None, s)).0, big + 4);
        assert_eq!(with(&|s, t| row(None, None, Some(t), s)).0, big + 4);
        assert_eq!(with(&|s, t| Arc::make_mut(&mut s.sheet).sections.push(Arc::new(Section::new(t, None)))).0, big);
        assert_eq!(with(&|s, t| drop(Arc::make_mut(&mut s.sheet).cells.insert(CellName::new(t), Cell::new()))).0, big);
        assert_eq!(with(&|s, t| s.raw.push("k", t)).0, big + 1, "an attribute kept: its name and value");
        assert_eq!(with(&|s, t| s.name = Some(Arc::from(t))).0, big);
        assert_eq!(with(&|s, t| s.data2 = Some(Arc::from(t))).0, big);
        assert_eq!(with(&|s, t| s.passthrough.push(0, RawNode::Text(Arc::from(t)))).0, big);
        assert_eq!(with(&|s, t| s.passthrough.push(0, RawNode::Comment(Arc::from(t)))).0, big);
        // An element kept whole, with the text three levels down.
        let deep = |t: &str| {
            let leaf = RawNode::Text(Arc::from(t));
            let mid = RawNode::Element(diacraft_doc::RawElement { name: Arc::from("m"), attrs: RawAttrs::new(), children: vec![leaf] });
            RawNode::Element(diacraft_doc::RawElement { name: Arc::from("o"), attrs: RawAttrs::new(), children: vec![mid] })
        };
        assert_eq!(with(&|s, t| s.passthrough.push(0, deep(t))).0, big + 2, "the names of the elements and the text");
        // A picture's bytes, once for two shapes that show it.
        let picture =
            Arc::new(diacraft_doc::Media { part: Arc::from("p.png"), content_type: Arc::from("image/png"), bytes: Arc::from(vec![0u8; 1 << 20]) });
        let foreign = |s: &mut Shape| {
            s.foreign = Some(Arc::new(diacraft_doc::ForeignData { picture: Some(Arc::clone(&picture)), ..diacraft_doc::ForeignData::default() }));
        };
        let g = with(&|s, _| foreign(s));
        assert_eq!(g.1, big, "a picture's bytes");
        let mut doc = Document::empty();
        let mut page = Page::new(0, "p");
        for id in 1..=2 {
            let mut shape = Shape::new(id);
            foreign(&mut shape);
            page.shapes.push(Arc::new(shape));
        }
        doc.pages.push(Arc::new(page));
        assert_eq!(of(&doc).media_bytes, big, "a picture two shapes show is counted once");
    }

    /// The rest of a document: the names of styles, the connections of a page, the names and the shapes of masters; and a master nested past the walk
    /// is past every cap too, as a page is (each of the figures, saturated, and no steps).
    #[test]
    fn styles_connections_and_masters_are_in_the_figures_and_a_master_nested_past_the_walk_saturates() {
        let mb = "m".repeat(1 << 20);
        let big = 1u64 << 20;
        let base = of(&Document::empty());
        let mut doc = Document::empty();
        doc.styles.push(Arc::new(diacraft_doc::Style { name: Some(Arc::from(mb.as_str())), ..diacraft_doc::Style::default() }));
        let f = of(&doc);
        assert_eq!((f.text_bytes - base.text_bytes, f.nodes - base.nodes), (big, 1), "a style's name");
        let mut doc = Document::empty();
        let mut page = Page::new(0, "p");
        let page_only = {
            let mut d = Document::empty();
            d.pages.push(Arc::new(page.clone()));
            of(&d)
        };
        page.connects = diacraft_doc::Connects::from_vec(vec![diacraft_doc::Connect {
            from_cell: Some(Arc::from(mb.as_str())),
            ..diacraft_doc::Connect::default()
        }]);
        doc.pages.push(Arc::new(page));
        let f = of(&doc);
        assert_eq!((f.text_bytes - page_only.text_bytes, f.nodes - page_only.nodes), (big, 1), "a connection of a page");
        let mut doc = Document::empty();
        let mut master = diacraft_doc::Master { name: Some(Arc::from(mb.as_str())), ..diacraft_doc::Master::default() };
        master.shapes.push(Arc::new(shape_with(1, &[("A", "1")], None)));
        doc.masters.push(Arc::new(master));
        let f = of(&doc);
        assert_eq!((f.shapes, f.cells, f.nodes - base.nodes), (1, 1, 1), "a master's shape, its cell and the master");
        assert!(f.text_bytes - base.text_bytes >= big, "a master's name");
        assert_ne!(f, Figures::saturated());
        // A master nested past the walk.
        let mut inner = Shape::new(1);
        for id in 2..(MOST_DEPTH as u32 + 10) {
            let mut outer = Shape::new(id);
            outer.children.push(Arc::new(inner));
            inner = outer;
        }
        let mut master = diacraft_doc::Master::default();
        master.shapes.push(Arc::new(inner));
        let mut doc = Document::empty();
        doc.masters.push(Arc::new(master));
        let most = u64::MAX / 4;
        let f = of(&doc);
        assert_eq!((f.shapes, f.cells, f.nodes, f.text_bytes, f.media_bytes, f.steps), (most, most, most, most, most, 0));
    }

    /// A megabyte: what each test of the slots adds to a place and expects the figures to grow by, at the least.
    const BIG: u64 = 1 << 20;

    fn kept_with(t: &str) -> diacraft_doc::Kept {
        diacraft_doc::Kept {
            raw: RawAttrs::from_pairs([("k", t)]),
            kept: Passthrough::from_nodes(vec![Placed { after: 0, node: RawNode::Text(Arc::from("")) }]),
        }
    }

    fn node(t: &str) -> RawNode {
        RawNode::Comment(Arc::from(t))
    }

    fn element(t: &str) -> Arc<RawElement> {
        Arc::new(RawElement { name: Arc::from("e"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(t))] })
    }

    /// A held value with nothing in it.
    fn plain_held() -> Held {
        Held { group: diacraft_doc::HeldGroup::Fill, stamp: None, attrs: RawAttrs::new(), kept: Passthrough::new() }
    }

    /// The text a megabyte more makes in the figures of a shape that `f` has changed, beside the plain shape's.
    fn on_shape(f: &dyn Fn(&mut Shape, &str)) -> u64 {
        let mb = "m".repeat(BIG as usize);
        let (base, with) = (grown(|_| {}), grown(|s| f(s, &mb)));
        with.text_bytes - base.text_bytes
    }

    /// The same for a document `f` has changed.
    fn on_doc(f: &dyn Fn(&mut Document, &str)) -> u64 {
        let mb = "m".repeat(BIG as usize);
        let mut doc = Document::empty();
        f(&mut doc, &mb);
        of(&doc).text_bytes - of(&Document::empty()).text_bytes
    }

    /// The extension slots were no figure of the walk, and a model that held 100 MB of kept text in them passed the gate of a save and
    /// aborted it. **Every place a slot holds text is counted byte for byte**, whichever side of the slot holds it (the `.odg`'s, the VSDX elements', a newer build's), on
    /// every object that has one, and so is every kept part of a shape's text; and the parts of the package, the custom properties and the VSDX held data.
    #[test]
    fn every_extension_slot_of_a_shape_and_its_sheet_is_counted_byte_for_byte() {
        // A shape's slot: each side, the names it holds, its held values and its units.
        assert!(on_shape(&|s, t| s.ext.make_mut().odf.kept.raw.push("k", t)) >= BIG, "the shape's ODF attributes");
        assert!(on_shape(&|s, t| s.ext.make_mut().odf.kept.kept.push(0, node(t))) >= BIG, "the shape's ODF children");
        assert!(on_shape(&|s, t| s.ext.make_mut().odf.style_name = Some(Arc::from(t))) >= BIG, "the style name the shape named");
        assert!(on_shape(&|s, t| s.ext.make_mut().odf.units.push("k", t)) >= BIG, "the units");
        assert!(
            on_shape(&|s, t| s.ext.make_mut().odf.held.push(Held { attrs: RawAttrs::from_pairs([("a", t)]), ..plain_held() })) >= BIG,
            "a held value's attributes"
        );
        assert!(
            on_shape(&|s, t| s
                .ext
                .make_mut()
                .odf
                .held
                .push(Held { kept: Passthrough::from_nodes(vec![Placed { after: 0, node: node(t) }]), ..plain_held() }))
                >= BIG,
            "a held value's nodes"
        );
        assert!(on_shape(&|s, t| s.ext.make_mut().native = kept_with(t)) >= BIG, "a newer build's content");
        assert!(on_shape(&|s, t| s.ext.make_mut().vsdx.0.push((Arc::from("Rest"), kept_with(t)))) >= BIG, "the VSDX elements");
        // The cells' and the sheet's.
        assert!(on_shape(&|s, t| Arc::make_mut(&mut s.sheet).ext.make_mut().odf = kept_with(t)) >= BIG, "a sheet's slot");
        assert!(
            on_shape(&|s, t| {
                let mut section = Section::new("User", None);
                section.ext.make_mut().odf = kept_with(t);
                Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
            }) >= BIG,
            "a section's slot"
        );
        assert!(
            on_shape(&|s, t| {
                let mut section = Section::new("User", None);
                let mut row = Row::default();
                row.ext.make_mut().odf.kept = kept_with(t);
                section.rows.push(Arc::new(row));
                Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
            }) >= BIG,
            "a row's ODF data"
        );
        assert!(
            on_shape(&|s, t| {
                let mut section = Section::new("Connection", None);
                let mut row = Row::default();
                row.ext.make_mut().odf.held = Some(Held { attrs: RawAttrs::from_pairs([("a", t)]), ..plain_held() });
                section.rows.push(Arc::new(row));
                Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
            }) >= BIG,
            "a glue point held on a row"
        );
        assert!(
            on_shape(&|s, t| {
                let mut cell = Cell::new();
                let mut extra = diacraft_doc::CellExtra::default();
                extra.ext.make_mut().odf = kept_with(t);
                cell.extra = Some(Arc::new(extra));
                Arc::make_mut(&mut s.sheet).cells.insert(CellName::new("Z"), cell);
            }) >= BIG,
            "a cell's slot"
        );
        assert!(
            on_shape(&|s, t| s.foreign = Some(Arc::new(diacraft_doc::ForeignData {
                ext: {
                    let mut e = Ext::default();
                    e.make_mut().odf = kept_with(t);
                    e
                },
                ..Default::default()
            }))) >= BIG,
            "a picture's slot"
        );
    }

    /// The kept parts of a shape's text, of each kind, and the text's own slot, with the style each paragraph and each run named.
    #[test]
    fn every_kept_part_of_a_text_and_its_slot_is_counted_byte_for_byte() {
        use diacraft_doc::{OdfParagraph, OdfRun, OdfText, OdfTextStyle, Text};
        // The text: the parts of every kind a kept node is, and its slot.
        let text_of = |parts: Vec<TextPart>, ext: Ext<OdfText>| Some(Arc::new(Text { parts, ext }));
        assert!(on_shape(&|s, t| s.text = text_of(vec![TextPart::Raw(node(t))], Ext::default())) >= BIG, "a kept VSDX node of a text");
        assert!(on_shape(&|s, t| s.text = text_of(vec![TextPart::OdfRaw(Box::new(node(t)))], Ext::default())) >= BIG, "a kept ODF node of a text");
        assert!(
            on_shape(&|s, t| s.text = text_of(vec![TextPart::NativeRaw(Box::new(node(t)))], Ext::default())) >= BIG,
            "a newer build's node of a text"
        );
        assert!(
            on_shape(&|s, t| {
                let mut ext: Ext<OdfText> = Ext::default();
                ext.make_mut().odf.kept = kept_with(t);
                s.text = text_of(Vec::new(), ext);
            }) >= BIG,
            "the text's slot"
        );
        assert!(
            on_shape(&|s, t| {
                let style = OdfTextStyle { name: Some(Arc::from(t)), ..Default::default() };
                let mut ext: Ext<OdfText> = Ext::default();
                ext.make_mut().odf.paragraphs =
                    vec![OdfParagraph { style: style.clone(), runs: vec![OdfRun { style, ..Default::default() }], ..Default::default() }];
                s.text = text_of(Vec::new(), ext);
            }) >= 2 * BIG,
            "the style each paragraph and each run named, each its own"
        );
    }

    /// The document's own ODF data, its parts, its custom properties and the VSDX data it holds.
    #[test]
    fn every_slot_and_part_of_the_document_is_counted_byte_for_byte() {
        use diacraft_doc::{ContentTypeRow, CustomProp, DefFamily, Definition, Format, KeptStyle, Part, PartRole, Rel, VsdxHeld};
        let mb = "m".repeat(1 << 20);
        // The document, its pages, masters, styles, connections and faces, and its parts.
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.kept = kept_with(t)) >= BIG, "the document's ODF data");
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.mimetype = Some(Arc::from(t))) >= BIG, "the mimetype");
        assert!(
            on_doc(&|d, t| d.ext.make_mut().odf.roots.push((Arc::from("content.xml"), RawAttrs::from_pairs([("a", t)])))) >= BIG,
            "a root's attributes"
        );
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.manifest_rows.push((Arc::from("x"), RawAttrs::from_pairs([("a", t)])))) >= BIG, "a manifest row");
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.meta.push(0, node(t))) >= BIG, "meta.xml's fields");
        let element = |t: &str| Arc::new(RawElement { name: Arc::from("e"), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(t))] });
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.settings = Some(element(t))) >= BIG, "settings.xml");
        assert!(on_doc(&|d, t| d.ext.make_mut().odf.layers.push(element(t))) >= BIG, "a layer");
        assert!(
            on_doc(&|d, t| d.ext.make_mut().odf.definitions.push(Definition {
                family: DefFamily::Hatch,
                name: Arc::from("n"),
                element: element(t),
                bytes: None
            })) >= BIG,
            "a definition"
        );
        assert!(
            on_doc(&|d, t| d.ext.make_mut().odf.definitions.push(Definition {
                family: DefFamily::FillImage,
                name: Arc::from("n"),
                element: element(""),
                bytes: Some(Arc::from(t.as_bytes()))
            })) < 64,
            "a fill image's bytes are media, not text"
        );
        let mut doc = Document::empty();
        doc.ext.make_mut().odf.definitions.push(Definition {
            family: DefFamily::FillImage,
            name: Arc::from("n"),
            element: element(""),
            bytes: Some(Arc::from(mb.as_bytes())),
        });
        assert_eq!(of(&doc).media_bytes, BIG, "and are counted as media");
        let shared: Arc<[u8]> = Arc::from(mb.as_bytes());
        for name in ["a", "b"] {
            doc.ext.make_mut().odf.definitions.push(Definition {
                family: DefFamily::FillImage,
                name: Arc::from(name),
                element: element(""),
                bytes: Some(Arc::clone(&shared)),
            });
        }
        assert_eq!(of(&doc).media_bytes, 2 * BIG, "once for the bytes two definitions share");
        assert!(
            on_doc(&|d, t| d.ext.make_mut().odf.styles.push(KeptStyle { family: Arc::from("f"), name: Arc::from("n"), element: element(t) })) >= BIG,
            "a style of another family"
        );
        assert!(
            on_doc(&|d, t| d.props = Arc::new(DocProps {
                custom: vec![CustomProp {
                    format: Format::Odf,
                    name: Arc::from("n"),
                    value_type: Arc::from("string"),
                    value: Arc::from(t),
                    raw: RawAttrs::new()
                }],
                ..DocProps::default()
            })) >= BIG,
            "a custom property"
        );
        assert!(
            on_doc(&|d, t| d.parts.push(Arc::new(Part {
                format: Format::Odf,
                name: Arc::from(t),
                media_type: Arc::from(""),
                role: PartRole::Other,
                manifest: RawAttrs::new(),
                rels: Arc::new(Vec::new()),
                bytes: Arc::from(Vec::new())
            }))) >= BIG,
            "a part's name"
        );
        assert!(
            on_doc(&|d, t| d.parts.push(Arc::new(Part {
                format: Format::Odf,
                name: Arc::from("p"),
                media_type: Arc::from(""),
                role: PartRole::Other,
                manifest: RawAttrs::from_pairs([("a", t)]),
                rels: Arc::new(Vec::new()),
                bytes: Arc::from(Vec::new())
            }))) >= BIG,
            "a part's manifest row"
        );
        assert!(
            on_doc(&|d, t| d.vsdx = Some(Arc::new(VsdxHeld {
                rels: vec![Rel { source: Arc::from("/"), id: Arc::from("rId1"), kind: Arc::from("k"), target: Arc::from(t), external: false }],
                ..VsdxHeld::default()
            }))) >= BIG,
            "a held relationship"
        );
        assert!(
            on_doc(&|d, t| d.vsdx = Some(Arc::new(VsdxHeld {
                content_types: vec![ContentTypeRow::Override { part: Arc::from(t), content_type: Arc::from("x") }],
                ..VsdxHeld::default()
            }))) >= BIG,
            "a held content-type row"
        );
        assert!(
            on_doc(&|d, t| d.vsdx = Some(Arc::new(VsdxHeld { windows: Some(element(t)), ..VsdxHeld::default() }))) >= BIG,
            "the held windows part"
        );
        assert!(
            on_doc(&|d, t| d.vsdx = Some(Arc::new(VsdxHeld { props_rest: vec![(Arc::from("p"), kept_with(t))], ..VsdxHeld::default() }))) >= BIG,
            "the rest of a properties part"
        );
        assert!(
            on_doc(&|d, t| Arc::make_mut(&mut d.fonts).push(diacraft_doc::FaceName {
                name_u: Arc::from("F"),
                raw: RawAttrs::new(),
                ext: {
                    let mut e = Ext::default();
                    e.make_mut().odf = kept_with(t);
                    e
                }
            })) >= BIG,
            "a font face's slot"
        );
        assert!(
            on_doc(&|d, t| d.sheet = Arc::new({
                let mut sh = Sheet::default();
                sh.ext.make_mut().odf = kept_with(t);
                sh
            })) >= BIG,
            "the document sheet's slot"
        );
    }

    /// The pages', masters', styles' and connections' slots.
    #[test]
    fn every_slot_of_a_page_a_master_a_style_and_a_connection_is_counted_byte_for_byte() {
        use diacraft_doc::OdfPage;
        let page = |t: &str| {
            let mut p = Page::new(0, "p");
            p.ext.make_mut().odf = OdfPage { master_page: Some(Arc::from(t)), ..Default::default() };
            p
        };
        assert!(on_doc(&|d, t| d.pages.push(Arc::new(page(t)))) >= BIG, "a page's master page name");
        assert!(
            on_doc(&|d, t| {
                let mut p = Page::new(0, "p");
                p.ext.make_mut().odf.drawing_page_style = Some(element(t));
                d.pages.push(Arc::new(p));
            }) >= BIG,
            "a page's drawing-page style"
        );
        assert!(
            on_doc(&|d, t| {
                let mut p = Page::new(0, "p");
                p.ext.make_mut().odf.layout_props.push("k", t);
                d.pages.push(Arc::new(p));
            }) >= BIG,
            "a page's layout properties"
        );
        assert!(
            on_doc(&|d, t| {
                let mut m = diacraft_doc::Master::default();
                m.ext.make_mut().odf = kept_with(t);
                d.masters.push(Arc::new(m));
            }) >= BIG,
            "a master's slot"
        );
        assert!(
            on_doc(&|d, t| {
                let mut st = diacraft_doc::Style::default();
                st.ext.make_mut().odf.unmapped.push("k", t);
                d.styles.push(Arc::new(st));
            }) >= BIG,
            "a style's unmapped properties"
        );
        assert!(
            on_doc(&|d, t| {
                let mut p = Page::new(0, "p");
                let mut c = diacraft_doc::Connect::default();
                c.ext.make_mut().odf = kept_with(t);
                p.connects = diacraft_doc::Connects::from_vec(vec![c]);
                d.pages.push(Arc::new(p));
            }) >= BIG,
            "a connection's slot"
        );
    }

    /// A slot that holds nothing costs nothing.
    #[test]
    fn a_slot_that_holds_nothing_costs_the_walk_nothing() {
        // A slot that holds nothing costs nothing: an untidied empty one is no figure.
        let empty = grown(|s| {
            s.ext.make_mut();
        });
        assert_eq!(empty, grown(|_| {}), "a slot of defaults is a null pointer to the walk");
    }

    /// The structure of a model is counted too, and the walk of an element kept whole does not recurse: a chain of two thousand elements.
    #[test]
    fn rows_sections_attributes_and_kept_nodes_are_nodes_and_a_deep_chain_is_walked_without_recursion() {
        let base = grown(|_| {});
        let g = grown(|s| {
            let mut section = Section::new("User", None);
            for k in 0..3 {
                section.rows.push(Arc::new(Row { ix: Some(k), ..Row::default() }));
            }
            Arc::make_mut(&mut s.sheet).sections.push(Arc::new(section));
            s.raw.push("a", "b");
        });
        assert_eq!(g.nodes - base.nodes, 1 + 3 + 1, "a section, its rows, an attribute");
        let mut chain = RawNode::Text(Arc::from("x"));
        for _ in 0..2_000 {
            chain = RawNode::Element(diacraft_doc::RawElement { name: Arc::from("e"), attrs: RawAttrs::new(), children: vec![chain] });
        }
        let deep = grown(|s| s.passthrough.push(0, chain));
        assert_eq!(deep.nodes - base.nodes, 2_001);
        assert_eq!(deep.text_bytes - base.text_bytes, 2_000 + 1);
    }
}
