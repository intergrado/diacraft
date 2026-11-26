//! The shells: what of an object rides in the DiaCraft solution part, and how it is put back.
//!
//! **A shell is a copy of an object with nothing in it but what the VSDX file cannot say**: the `odf` and `native` sides of its extension slots (the `vsdx` side is the file's own
//! and is written natively), and the places that hold them. Every cell, value and formula stays out of it (no cell is copied). A shell is encoded by the codec of the model
//! (`diacraft_sheetxml::model`, the fragment of one shape, page, master, style or document) and read back by it, so that whatever the codec can say, the carrier can carry.
//!
//! **Where a shell finds its place again** is by position and not by name, since the stamp of the native outputs beside it (`stamp.rs`) says that the file's structure is the one the
//! shell was made beside: a section is its place in the sheet's list, a row its place in the section's, a connect its place in the list (a font is found by its name, and by its place in the table when the table holds more than one of a name: the faces an `.odg` declares share a family, which
//! is the name a table of VSDX calls a face by). The position is
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
    // A face is found again by its name, and by its place in the table among the faces of that name: a drawing that was never saved is given the faces its cells name beside its own, so the
    // table's order is not always the model's, and the faces an `.odg` declares are several of one name (GATE1622-CROSS-FONTNAME: three declarations of one family were put on one face).
    let fonts: Vec<FaceName> = doc
        .fonts
        .iter()
        .enumerate()
        .filter(|(_, f)| rides(&f.ext))
        .map(|(i, f)| FaceName { name_u: Arc::clone(&f.name_u), raw: at(i), ext: carried(&f.ext) })
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

/// Puts the faces of a document's shell on the faces of `real`'s table, **each on the face it stood on**: the face at the place the shell says, when it has the shell face's name, and else the first face
/// of that name that no shell face took (a shell of an earlier build says no place). Two shell faces of one name are two faces: the faces an `.odg` declares are several of one family, the name a VSDX
/// table finds a face by, and each is put on a face of its own (GATE1622-CROSS-FONTNAME: all three went on the first, and the last declaration was the only one left). **A shell face that the table has
/// no face left for** (another program rewrote the table, and left fewer faces of the name) **is a face of the table's own again, at its end**, with the data it carried, as the shell of an `.odg` does
/// (`faces_put` of the `odg` crate's shell): a style the model holds whole names its declaration, and a table without it would leave the name with no face to be written. → how many are left out, which is
/// the ones past the most faces a table holds ([`diacraft_doc::MAX_DEFINITIONS`]). Each is a step, and each face of the table is looked at twice at most.
fn faces_put(real: &mut Document, shell: &Document) -> usize {
    let mut fonts: Vec<FaceName> = real.fonts.as_ref().clone();
    let mut taken = vec![false; fonts.len()];
    let mut lost = 0;
    // The faces that stood where the shell says; the others are put by name below.
    let mut unplaced: Vec<&FaceName> = Vec::new();
    for face in shell.fonts.iter() {
        work::carry_walked(1);
        let place = position(&face.raw).filter(|&i| fonts.get(i).is_some_and(|f| f.name_u == face.name_u) && taken.get(i) == Some(&false));
        match place.and_then(|i| fonts.get_mut(i).zip(taken.get_mut(i))) {
            Some((target, slot)) => {
                *slot = true;
                put(&mut target.ext, &face.ext);
            }
            None => unplaced.push(face),
        }
    }
    if !unplaced.is_empty() {
        // The faces of the table by name, in its order, for the shell faces that stood nowhere the table says.
        let mut by_name: HashMap<Arc<str>, std::collections::VecDeque<usize>> = HashMap::with_capacity(fonts.len());
        for (i, face) in fonts.iter().enumerate() {
            work::carry_walked(1);
            if taken.get(i) == Some(&false) {
                by_name.entry(Arc::clone(&face.name_u)).or_default().push_back(i);
            }
        }
        for face in unplaced {
            work::carry_walked(1);
            let at = by_name.get_mut(&face.name_u).and_then(std::collections::VecDeque::pop_front);
            if let Some(target) = at.and_then(|i| fonts.get_mut(i)) {
                put(&mut target.ext, &face.ext);
            } else if fonts.len() < diacraft_doc::MAX_DEFINITIONS {
                let mut own = FaceName { name_u: Arc::clone(&face.name_u), raw: RawAttrs::new(), ext: Ext::default() };
                put(&mut own.ext, &face.ext);
                fonts.push(own);
            } else {
                lost += 1;
            }
        }
    }
    real.fonts = Arc::new(fonts);
    lost
}

/// Puts what rides in a document's shell into `real` (the parts and the custom properties of the other formats are added by the caller, which judged them).
pub(crate) fn document_put(real: &mut Document, shell: &Document) -> usize {
    put(&mut real.ext, &shell.ext);
    let mut lost = 0;
    if !shell.fonts.is_empty() {
        lost += faces_put(real, shell);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A face called `name_u` that holds an `.odg` declaration named `declared` (`style:name`), as the `.odg` reader makes one.
    fn declared_face(name_u: &str, declared: &str) -> FaceName {
        let mut face = FaceName { name_u: Arc::from(name_u), raw: RawAttrs::new(), ext: Ext::default() };
        face.ext.make_mut().odf = Kept { raw: RawAttrs::from_pairs([("style:name", declared)]), kept: Passthrough::new() };
        face
    }

    /// A face of the file's table: a name and nothing more, as the VSDX reader makes one.
    fn plain_face(name_u: &str) -> FaceName {
        FaceName { name_u: Arc::from(name_u), raw: RawAttrs::new(), ext: Ext::default() }
    }

    fn document_of(faces: Vec<FaceName>) -> Document {
        let mut doc = Document::empty();
        doc.fonts = Arc::new(faces);
        doc
    }

    /// The `style:name` of the declaration each face of the table holds, in the table's order (`-` for none).
    fn declarations(doc: &Document) -> Vec<String> {
        doc.fonts.iter().map(|f| f.ext.get().and_then(|d| d.odf.raw.get("style:name")).unwrap_or("-").to_owned()).collect()
    }

    /// Three declarations of one family, which a VSDX table names by the family: the faces an `.odg` of GATE1622-CROSS-FONTNAME's vector declares.
    fn model() -> Document {
        document_of(vec![
            declared_face("DejaVu Sans", "DV"),
            declared_face("Liberation Mono", "Mono"),
            declared_face("Liberation Mono", "Mono1"),
            declared_face("Liberation Mono", "Mono2"),
        ])
    }

    /// The table a VSDX file holds of the model's: the names of the faces and nothing of the `.odg`'s data.
    fn file_table(doc: &Document) -> Document {
        document_of(doc.fonts.iter().map(|f| plain_face(&f.name_u)).collect())
    }

    #[test]
    fn faces_of_one_name_are_each_put_on_a_face_of_their_own() {
        let model = model();
        let shell = document_shell(&model, &mut Sheets::default()).expect("the faces ride");
        let mut file = file_table(&model);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(
            declarations(&file),
            ["DV", "Mono", "Mono1", "Mono2"],
            "red on 1622f7c2: [\"DV\", \"Mono2\", \"-\", \"-\"], the last declaration on the first face"
        );
    }

    #[test]
    fn a_face_goes_where_it_stood_though_the_faces_before_it_hold_nothing_that_rides() {
        // The second face of the name is a VSDX face of its own; only the first and the third ride.
        let model =
            document_of(vec![declared_face("Liberation Mono", "Mono"), plain_face("Liberation Mono"), declared_face("Liberation Mono", "Mono2")]);
        let shell = document_shell(&model, &mut Sheets::default()).expect("two faces ride");
        assert_eq!(shell.fonts.len(), 2);
        let mut file = file_table(&model);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["Mono", "-", "Mono2"]);
    }

    #[test]
    fn a_shell_that_says_no_place_puts_its_faces_on_the_faces_of_the_name_in_order() {
        // A shell an earlier build wrote has no `at` on a face.
        let model = model();
        let mut shell = document_shell(&model, &mut Sheets::default()).expect("the faces ride");
        for face in Arc::make_mut(&mut shell.fonts).iter_mut() {
            face.raw = RawAttrs::new();
        }
        let mut file = file_table(&model);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["DV", "Mono", "Mono1", "Mono2"]);
    }

    #[test]
    fn a_place_that_is_not_the_face_of_that_name_is_no_place() {
        // Each `at` points at a face of another name, past the table, or at text: the faces are put by name, in order.
        let model = model();
        let mut shell = document_shell(&model, &mut Sheets::default()).expect("the faces ride");
        for (face, at) in Arc::make_mut(&mut shell.fonts).iter_mut().zip(["2", "0", "99999999999999999999", "not a number"]) {
            face.raw = RawAttrs::from_pairs([(AT, at)]);
        }
        let mut file = file_table(&model);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["DV", "Mono", "Mono1", "Mono2"]);
    }

    #[test]
    fn two_shell_faces_never_take_one_face_and_the_faces_the_table_lacks_are_added_to_it() {
        // The file's table lost two of the three faces of the name (another program rewrote it): the first declaration goes on the face left, and the others are faces of the table again, at its end.
        let model = model();
        let shell = document_shell(&model, &mut Sheets::default()).expect("the faces ride");
        let mut file = document_of(vec![plain_face("DejaVu Sans"), plain_face("Liberation Mono")]);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["DV", "Mono", "Mono1", "Mono2"], "no declaration is left out");
        assert_eq!(
            file.fonts.iter().map(|f| &*f.name_u).collect::<Vec<_>>(),
            ["DejaVu Sans", "Liberation Mono", "Liberation Mono", "Liberation Mono"]
        );
        // Two shell faces that name the same place: the second finds no face of the name left, and is added.
        let mut shell = document_shell(&model, &mut Sheets::default()).expect("the faces ride");
        for face in Arc::make_mut(&mut shell.fonts).iter_mut() {
            face.raw = RawAttrs::from_pairs([(AT, "1")]);
        }
        let mut file = document_of(vec![plain_face("DejaVu Sans"), plain_face("Liberation Mono")]);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["DV", "Mono", "Mono1", "Mono2"]);
        // A face of a name the table has none of at all is added too.
        let mut file = document_of(vec![plain_face("Arial")]);
        assert_eq!(document_put(&mut file, &shell), 0);
        assert_eq!(declarations(&file), ["-", "DV", "Mono", "Mono1", "Mono2"]);
    }

    #[test]
    fn a_face_that_carries_nothing_leaves_the_table_as_it_is() {
        let model = document_of(vec![plain_face("Arial"), plain_face("Arial")]);
        assert!(document_shell(&model, &mut Sheets::default()).is_none());
    }
}
