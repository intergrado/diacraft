//! The shells: what of an object rides in the DiaCraft solution part, and how it is put back.
//!
//! **A shell is a copy of an object with nothing in it but what the VSDX file cannot say**: the `odf` and `native` sides of its extension slots (the `vsdx` side is the file's own
//! and is written natively), and the places that hold them. Every cell, value and formula stays out of it (no cell is copied). A shell is encoded by the codec of the model
//! (`diacraft_sheetxml::model`, the fragment of one shape, page, master, style or document) and read back by it, so that whatever the codec can say, the carrier can carry.
//!
//! **Where a shell finds its place again** is by position and not by name, since the stamp of the native outputs beside it (`stamp.rs`) says that the file's structure is the one the
//! shell was made beside: a section is its place in the sheet's list, a row its place in the section's, a connect its place in the list (a font is found by its name). The position is
//! the `at` attribute of the element's `raw` attributes in the shell (the shell is thrown away once its slots are copied, so the attribute is the carrier's own and is never put on
//! an object). A text part that rides (a bookmark, a field the `.odg` reader kept) sits between the characters, which VSDX writes natively; its place is the number of markers (`Cp`,
//! `Pp`, `Tp`, `Fld`, VSDX's own `Raw`) before it and the bytes of characters since the last of them.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellExtra, Cells, Connect, Connects, DocProps, Document, Ext, FaceName, Format, Kept, Master, OdfShape, Page, Part, Passthrough, Placed,
    RawAttrs, RawNode, Row, Rows, Section, Shape, SharedVec, Sheet, Style, Text, TextPart,
};

use crate::work;

/// The name of the attribute of a shell element that says where it stood: its position in its list.
pub(crate) const AT: &str = "at";

/// What of a slot rides: its `odf` and `native` sides, and nothing of its `vsdx` side.
pub(crate) fn carried<T: Default + Clone + PartialEq>(slot: &Ext<T>) -> Ext<T> {
    let mut out: Ext<T> = Ext::default();
    if let Some(data) = slot.get() {
        let side = out.make_mut();
        side.odf = data.odf.clone();
        side.native = data.native.clone();
        out.tidy();
    }
    out
}

/// Whether a slot holds something that rides.
pub(crate) fn rides<T: Default + Clone + PartialEq>(slot: &Ext<T>) -> bool {
    slot.get().is_some_and(|data| data.odf != T::default() || data.native != Kept::default())
}

/// Puts what rides in `shell` into `slot`: the `odf` and `native` sides, replaced; the `vsdx` side, which the file gave, as it is.
pub(crate) fn put<T: Default + Clone + PartialEq>(slot: &mut Ext<T>, shell: &Ext<T>) {
    let Some(data) = shell.get() else { return };
    let side = slot.make_mut();
    side.odf = data.odf.clone();
    side.native = data.native.clone();
    slot.tidy();
}

fn at(position: usize) -> RawAttrs {
    RawAttrs::from_pairs([(AT, position.to_string())])
}

/// The position a shell element says it stood at.
pub(crate) fn position(raw: &RawAttrs) -> Option<usize> {
    raw.get(AT).and_then(|text| text.parse().ok())
}

// ------------------------------------------------------------------------------------------------------------------------ cells and sheets

/// The cells of a place that hold something that rides in the extra of the cell, as cells with nothing else in them.
fn cells_shell(cells: &Cells) -> Cells {
    let mut out = Cells::new();
    for (name, cell) in cells {
        work::carry_walked(1);
        if let Some(extra) = &cell.extra
            && rides(&extra.ext)
        {
            let shell = Cell {
                extra: Some(Arc::new(CellExtra { text: None, passthrough: Default::default(), ext: carried(&extra.ext) })),
                ..Cell::default()
            };
            out.insert(name.clone(), shell);
        }
    }
    out
}

/// Puts what rides on the cells of `shell` into the cells of `real`, by name. → how many could not be put (a cell the file no longer has).
fn cells_put(real: &mut Cells, shell: &Cells) -> usize {
    let mut lost = 0;
    for (name, cell) in shell {
        let Some(extra) = &cell.extra else { continue };
        match real.get_mut(name) {
            Some(target) => {
                let slot = Arc::make_mut(target.extra.get_or_insert_with(Default::default));
                put(&mut slot.ext, &extra.ext);
            }
            None => lost += 1,
        }
    }
    lost
}

fn row_shell(row: &Row, position: usize) -> Option<Row> {
    work::carry_walked(1);
    let cells = cells_shell(&row.cells);
    if !rides(&row.ext) && cells.is_empty() {
        return None;
    }
    Some(Row { name: row.name.clone(), ix: row.ix, cells, raw: at(position), ext: carried(&row.ext), ..Row::default() })
}

fn section_shell(section: &Section, position: usize) -> Option<Section> {
    work::carry_walked(1);
    let cells = cells_shell(&section.cells);
    let rows: Vec<Arc<Row>> = section.rows.iter().enumerate().filter_map(|(i, r)| row_shell(r, i)).map(Arc::new).collect();
    if !rides(&section.ext) && cells.is_empty() && rows.is_empty() {
        return None;
    }
    Some(Section {
        name: section.name.clone(),
        ix: section.ix,
        del: false,
        cells,
        rows: Rows::from(rows),
        raw: at(position),
        passthrough: Default::default(),
        ext: carried(&section.ext),
    })
}

/// The shell of a sheet, or `None` when nothing in it rides.
pub(crate) fn sheet_shell(sheet: &Sheet) -> Option<Sheet> {
    work::carry_walked(1);
    let cells = cells_shell(&sheet.cells);
    let sections: Vec<Arc<Section>> = sheet.sections.iter().enumerate().filter_map(|(i, s)| section_shell(s, i)).map(Arc::new).collect();
    if !rides(&sheet.ext) && cells.is_empty() && sections.is_empty() {
        return None;
    }
    Some(Sheet { cells, sections, ext: carried(&sheet.ext), ..Sheet::default() })
}

/// Whether the shell of a sheet holds anything to put: a place, a cell or a slot. (A shell with none leaves the sheet it is put on as it is, and shared.)
pub(crate) fn touches(shell: &Sheet) -> bool {
    !diacraft_sheetxml::build::sheet_is_empty(shell) || !shell.ext.is_empty()
}

/// Puts what rides in the shell of a sheet into `real`. → how many places it could not find.
pub(crate) fn sheet_put(real: &mut Sheet, shell: &Sheet) -> usize {
    let mut lost = 0;
    put(&mut real.ext, &shell.ext);
    lost += cells_put(&mut real.cells, &shell.cells);
    for section in &shell.sections {
        let Some(slot) = position(&section.raw).and_then(|i| real.sections.get_mut(i)) else {
            lost += 1;
            continue;
        };
        if slot.name != section.name || slot.ix != section.ix {
            lost += 1;
            continue;
        }
        let target = Arc::make_mut(slot);
        put(&mut target.ext, &section.ext);
        lost += cells_put(&mut target.cells, &section.cells);
        for row in &section.rows {
            let Some(place) = position(&row.raw).and_then(|i| target.rows.get_mut(i)) else {
                lost += 1;
                continue;
            };
            if place.name != row.name || place.ix != row.ix {
                lost += 1;
                continue;
            }
            let row_target = Arc::make_mut(place);
            put(&mut row_target.ext, &row.ext);
            lost += cells_put(&mut row_target.cells, &row.cells);
        }
    }
    lost
}

// ------------------------------------------------------------------------------------------------------------------------ text

/// Whether a part of a text is one that rides: ODF's or a newer build's, which VSDX never writes.
pub(crate) fn rides_in_text(part: &TextPart) -> bool {
    matches!(part, TextPart::OdfRaw(_) | TextPart::NativeRaw(_))
}

/// The shell of a text and the place of each part of it that rides, or `None`: the text's slot, and the parts VSDX has no place for.
pub(crate) fn text_shell(text: &Text) -> Option<(Text, Vec<(u32, u32)>)> {
    let (mut markers, mut since) = (0u32, 0u32);
    let (mut parts, mut places) = (Vec::new(), Vec::new());
    for part in &text.parts {
        work::carry_walked(1);
        match part {
            part if rides_in_text(part) => {
                parts.push(part.clone());
                places.push((markers, since));
            }
            TextPart::Chars(chars) => since = since.saturating_add(u32::try_from(chars.len()).unwrap_or(u32::MAX)),
            _ => {
                markers = markers.saturating_add(1);
                since = 0;
            }
        }
    }
    if parts.is_empty() && !rides(&text.ext) {
        return None;
    }
    Some((Text { parts, ext: carried(&text.ext) }, places))
}

/// Puts the text's slot and the parts that ride back into the text the file gave, each where it stood. → how many parts could not be put (the text is not what they were
/// beside), which are left out.
pub(crate) fn text_put(real: &mut Text, shell: &Text, places: &[(u32, u32)]) -> usize {
    put(&mut real.ext, &shell.ext);
    if shell.parts.is_empty() {
        return 0;
    }
    let old = std::mem::take(&mut real.parts);
    let mut out: Vec<TextPart> = Vec::with_capacity(old.len().saturating_add(shell.parts.len()));
    let mut next = 0usize;
    let (mut markers, mut since) = (0u32, 0u32);
    // The parts that ride and stand at (`markers`, `since` .. `since + room`), the first of which is placed at the offset it says.
    let place = |out: &mut Vec<TextPart>, next: &mut usize, markers: u32, since: u32| {
        while let (Some(part), Some(&(m, k))) = (shell.parts.get(*next), places.get(*next)) {
            if m == markers && k == since {
                out.push(part.clone());
                *next += 1;
            } else {
                break;
            }
        }
    };
    place(&mut out, &mut next, markers, since);
    for part in old {
        work::carry_walked(1);
        match part {
            TextPart::Chars(chars) => {
                let mut rest: &str = &chars;
                let mut taken = 0u32;
                loop {
                    // The next part that rides stands inside these characters, at an offset that is a boundary of the text.
                    let inside = match (places.get(next), shell.parts.get(next)) {
                        (Some(&(m, k)), Some(_)) if m == markers && k > since.saturating_add(taken) => {
                            let offset = usize::try_from(k - since.saturating_add(taken)).unwrap_or(usize::MAX);
                            (offset <= rest.len() && rest.is_char_boundary(offset)).then_some(offset)
                        }
                        _ => None,
                    };
                    let Some(offset) = inside else { break };
                    let (head, tail) = rest.split_at(offset);
                    if !head.is_empty() {
                        out.push(TextPart::Chars(Arc::from(head)));
                    }
                    taken = taken.saturating_add(u32::try_from(head.len()).unwrap_or(u32::MAX));
                    rest = tail;
                    place(&mut out, &mut next, markers, since.saturating_add(taken));
                }
                if !rest.is_empty() {
                    out.push(TextPart::Chars(Arc::from(rest)));
                }
                since = since.saturating_add(u32::try_from(chars.len()).unwrap_or(u32::MAX));
            }
            marker => {
                out.push(marker);
                markers = markers.saturating_add(1);
                since = 0;
                place(&mut out, &mut next, markers, since);
            }
        }
    }
    // Parts that stood after the last character, and any that stood where this text has nothing: the first are placed, the rest are left out.
    place(&mut out, &mut next, markers, since);
    real.parts = out;
    shell.parts.len().saturating_sub(next)
}

// ------------------------------------------------------------------------------------------------------------------------ the objects

/// The shell of a shape, the places of its text's parts that ride, or `None` when nothing of it rides. Its children are not in it: each has an item of its own.
/// The slot of a shape whose object frame the save wrote as an embedding: what rides, **without the frame's `draw:object-ole` element**. The embedding is the object, and a model read
/// from the file must not hold markup that names an entry of the package that is no longer there.
fn without_object(slot: &Ext<OdfShape>) -> Ext<OdfShape> {
    let mut out = carried(slot);
    let kept = out.get().map(|data| &data.odf.kept.kept);
    let at = kept.and_then(|list| list.iter().position(|p| matches!(&p.node, RawNode::Element(e) if &*e.name == "draw:object-ole")));
    if let Some(at) = at {
        let nodes: Vec<Placed> =
            out.get().map_or_else(Vec::new, |data| data.odf.kept.kept.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, p)| p.clone()).collect());
        work::carry_walked(nodes.len());
        out.make_mut().odf.kept.kept = Passthrough::from_nodes(nodes);
        out.tidy();
    }
    out
}

/// The shell of a shape. `object` says the save wrote the shape's object frame as an embedding: its slot then holds no `draw:object-ole` element.
pub(crate) fn shape_shell(shape: &Shape, sheets: &mut Sheets, object: bool) -> Option<(Shape, Vec<(u32, u32)>)> {
    work::carry_walked(1);
    let sheet = sheets.shell(&shape.sheet);
    let text = shape.text.as_deref().and_then(text_shell);
    let foreign = shape
        .foreign
        .as_deref()
        .filter(|f| rides(&f.ext))
        .map(|f| Arc::new(diacraft_doc::ForeignData { ext: carried(&f.ext), ..diacraft_doc::ForeignData::default() }));
    let ext = if object { without_object(&shape.ext) } else { carried(&shape.ext) };
    if !rides(&ext) && sheet.is_none() && text.is_none() && foreign.is_none() {
        return None;
    }
    let (text, places) = match text {
        Some((text, places)) => (Some(Arc::new(text)), places),
        None => (None, Vec::new()),
    };
    let shell = Shape { id: shape.id, sheet: sheet.unwrap_or_default(), text, foreign, ext, ..Shape::default() };
    Some((shell, places))
}

/// Puts what rides in a shape's shell into the shape the file gave. → how many places it could not find.
pub(crate) fn shape_put(real: &mut Shape, shell: &Shape, places: &[(u32, u32)]) -> usize {
    let mut lost = 0;
    put(&mut real.ext, &shell.ext);
    if !diacraft_sheetxml::build::sheet_is_empty(&shell.sheet) || !shell.sheet.ext.is_empty() {
        lost += sheet_put(Arc::make_mut(&mut real.sheet), &shell.sheet);
    }
    if let Some(text) = &shell.text {
        let target = Arc::make_mut(real.text.get_or_insert_with(Default::default));
        lost += text_put(target, text, places);
    }
    if let Some(foreign) = &shell.foreign {
        match real.foreign.as_mut() {
            Some(target) => put(&mut Arc::make_mut(target).ext, &foreign.ext),
            None => lost += 1,
        }
    }
    lost
}

fn connects_shell(connects: &Connects) -> Connects {
    let list: Vec<Connect> = connects
        .iter()
        .enumerate()
        .filter(|(_, c)| rides(&c.ext))
        .map(|(i, c)| Connect { raw: at(i), ext: carried(&c.ext), ..Connect::default() })
        .collect();
    work::carry_walked(connects.len());
    Connects::from_vec(list)
}

fn connects_put(real: &mut Connects, shell: &Connects) -> usize {
    if shell.is_empty() {
        return 0;
    }
    let mut list: Vec<Connect> = real.iter().cloned().collect();
    let mut lost = 0;
    for connect in shell {
        match position(&connect.raw).and_then(|i| list.get_mut(i)) {
            Some(target) => put(&mut target.ext, &connect.ext),
            None => lost += 1,
        }
    }
    *real = Connects::from_vec(list);
    lost
}

/// The shell of a page: its slot, its sheet's places and its connects' slots. `None` when nothing rides.
pub(crate) fn page_shell(page: &Page, sheets: &mut Sheets) -> Option<Page> {
    let sheet = sheets.shell(&page.sheet);
    let connects = connects_shell(&page.connects);
    if !rides(&page.ext) && sheet.is_none() && connects.is_empty() {
        return None;
    }
    let mut shell = Page::new(page.id, "");
    shell.name_u = None;
    shell.sheet = sheet.unwrap_or_default();
    shell.connects = connects;
    shell.ext = carried(&page.ext);
    Some(shell)
}

pub(crate) fn page_put(real: &mut Page, shell: &Page) -> usize {
    put(&mut real.ext, &shell.ext);
    let mut lost = 0;
    if !diacraft_sheetxml::build::sheet_is_empty(&shell.sheet) || !shell.sheet.ext.is_empty() {
        lost += sheet_put(Arc::make_mut(&mut real.sheet), &shell.sheet);
    }
    lost + connects_put(&mut real.connects, &shell.connects)
}

pub(crate) fn master_shell(master: &Master, sheets: &mut Sheets) -> Option<Master> {
    let sheet = sheets.shell(&master.sheet);
    let connects = connects_shell(&master.connects);
    if !rides(&master.ext) && sheet.is_none() && connects.is_empty() {
        return None;
    }
    Some(Master { id: master.id, sheet: sheet.unwrap_or_default(), connects, ext: carried(&master.ext), ..Master::default() })
}

pub(crate) fn master_put(real: &mut Master, shell: &Master) -> usize {
    put(&mut real.ext, &shell.ext);
    let mut lost = 0;
    if !diacraft_sheetxml::build::sheet_is_empty(&shell.sheet) || !shell.sheet.ext.is_empty() {
        lost += sheet_put(Arc::make_mut(&mut real.sheet), &shell.sheet);
    }
    lost + connects_put(&mut real.connects, &shell.connects)
}

pub(crate) fn style_shell(style: &Style, sheets: &mut Sheets) -> Option<Style> {
    let sheet = sheets.shell(&style.sheet);
    if !rides(&style.ext) && sheet.is_none() {
        return None;
    }
    Some(Style { id: style.id, name: style.name.clone(), sheet: sheet.unwrap_or_default(), ext: carried(&style.ext), ..Style::default() })
}

pub(crate) fn style_put(real: &mut Style, shell: &Style) -> usize {
    put(&mut real.ext, &shell.ext);
    if !diacraft_sheetxml::build::sheet_is_empty(&shell.sheet) || !shell.sheet.ext.is_empty() {
        sheet_put(Arc::make_mut(&mut real.sheet), &shell.sheet)
    } else {
        0
    }
}

/// The shell of the document: its slot, the faces that hold something, and its sheet's places. `None` when nothing rides.
pub(crate) fn document_shell(doc: &Document, sheets: &mut Sheets) -> Option<Document> {
    // A face is found again by its name: a drawing that was never saved is given the faces its cells name beside its own, so the table's order is not the model's.
    let fonts: Vec<FaceName> = doc
        .fonts
        .iter()
        .filter(|f| rides(&f.ext))
        .map(|f| FaceName { name_u: Arc::clone(&f.name_u), raw: RawAttrs::new(), ext: carried(&f.ext) })
        .collect();
    work::carry_walked(doc.fonts.len());
    let sheet = sheets.shell(&doc.sheet);
    if !rides(&doc.ext) && fonts.is_empty() && sheet.is_none() {
        return None;
    }
    let mut shell = Document::empty();
    shell.ext = carried(&doc.ext);
    shell.fonts = Arc::new(fonts);
    shell.sheet = sheet.unwrap_or_default();
    Some(shell)
}

/// The shell of the parts and custom properties of the other formats: `parts` are the ones that ride (judged by the caller). `None` when there are none.
pub(crate) fn parts_shell(doc: &Document, parts: &[Arc<Part>]) -> Option<Document> {
    let custom: Vec<_> = doc.props.custom.iter().filter(|c| c.format != Format::Vsdx).cloned().collect();
    work::carry_walked(doc.props.custom.len());
    if parts.is_empty() && custom.is_empty() {
        return None;
    }
    let mut shell = Document::empty();
    shell.parts = SharedVec::from_vec(parts.to_vec());
    shell.props = Arc::new(DocProps { custom, ..DocProps::default() });
    Some(shell)
}

/// Puts what rides in a document's shell into `real` (the parts and the custom properties of the other formats are added by the caller, which judged them).
pub(crate) fn document_put(real: &mut Document, shell: &Document) -> usize {
    put(&mut real.ext, &shell.ext);
    let mut lost = 0;
    if !shell.fonts.is_empty() {
        let mut fonts: Vec<FaceName> = real.fonts.as_ref().clone();
        let mut first: HashMap<Arc<str>, usize> = HashMap::with_capacity(fonts.len());
        for (i, face) in fonts.iter().enumerate() {
            first.entry(Arc::clone(&face.name_u)).or_insert(i);
        }
        for face in shell.fonts.iter() {
            work::carry_walked(1);
            match first.get(&face.name_u).and_then(|&i| fonts.get_mut(i)) {
                Some(target) => put(&mut target.ext, &face.ext),
                None => lost += 1,
            }
        }
        real.fonts = Arc::new(fonts);
    }
    if !diacraft_sheetxml::build::sheet_is_empty(&shell.sheet) || !shell.sheet.ext.is_empty() {
        lost += sheet_put(Arc::make_mut(&mut real.sheet), &shell.sheet);
    }
    lost
}

// ------------------------------------------------------------------------------------------------------------------------ shared sheets

/// The shells of the sheets of a model, **one for each distinct sheet** (a drawing of a million shapes that hold one sheet is one walk of it): by the sheet's address, which a model
/// being walked holds for as long as the walk goes.
#[derive(Default)]
pub(crate) struct Sheets {
    shells: HashMap<usize, Option<Arc<Sheet>>>,
}

impl Sheets {
    /// The shell of `sheet` (one `Arc` for all the shapes that share it), or `None` when nothing in it rides.
    pub(crate) fn shell(&mut self, sheet: &Arc<Sheet>) -> Option<Arc<Sheet>> {
        let key = Arc::as_ptr(sheet).addr();
        if let Some(found) = self.shells.get(&key) {
            return found.clone();
        }
        let made = sheet_shell(sheet).map(Arc::new);
        self.shells.insert(key, made.clone());
        made
    }
}
