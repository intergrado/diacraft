//! The shells: what of an object an `.odg` carries of the VSDX file's data, and how it is put back (the native format's design, 5.1 and 5.3).
//!
//! **A shell is a copy of an object with nothing in it but what an `.odg` cannot say**: the attributes and elements the VSDX reader kept (`raw`, `passthrough`), the text of `Data1` to `Data3`, a deleted
//! flag, the kind of the shape when the markup gives another, the `vsdx` and `native` sides of the object's extension slots (the `odf` side is the `.odg`'s own and is written as the file says it), and the
//! same of the shape's sheet and text. No cell with a value, a formula or a unit is in it: the cells are written natively, or carried as tier 2 does (`payload`). A shell is encoded by the codec of the
//! model (`diacraft_sheetxml::model`, the fragment of one shape) and read back by it, so that whatever the codec can say, the carrier can carry; **foreign data beyond the picture is a shell of its own**
//! ([`foreign_shell`]), judged by the picture it sits beside.
//!
//! A shell is put back **over what the reader rebuilt from the markup** ([`apply`]): the fields it holds replace the rebuilt ones, and the slots take its `vsdx` and `native` sides.

use std::sync::Arc;

use diacraft_doc::{Ext, ForeignData, Kept, Shape, ShapeType, Sheet, Text};

/// What of a slot rides: its `vsdx` and `native` sides, and nothing of its `odf` side (the `.odg`'s own).
pub(crate) fn carried<T: Default + Clone + PartialEq>(slot: &Ext<T>) -> Ext<T> {
    let mut out: Ext<T> = Ext::default();
    if let Some(data) = slot.get() {
        let side = out.make_mut();
        side.vsdx = data.vsdx.clone();
        side.native = data.native.clone();
        out.tidy();
    }
    out
}

/// Whether a slot holds something that rides.
pub(crate) fn rides<T: Default + Clone + PartialEq>(slot: &Ext<T>) -> bool {
    slot.get().is_some_and(|data| !data.vsdx.0.is_empty() || data.native != Kept::default())
}

/// Puts what rides in `shell` into `slot`: the `vsdx` and `native` sides, replaced; the `odf` side, which the file gave, as it is.
pub(crate) fn put<T: Default + Clone + PartialEq>(slot: &mut Ext<T>, shell: &Ext<T>) {
    let Some(data) = shell.get() else { return };
    let side = slot.make_mut();
    side.vsdx = data.vsdx.clone();
    side.native = data.native.clone();
    slot.tidy();
}

/// The shell of a sheet's own data (its attributes, its elements and its slot), or `None` when it holds none.
fn sheet_shell(sheet: &Sheet) -> Option<Sheet> {
    if sheet.raw.is_empty() && sheet.passthrough.is_empty() && !rides(&sheet.ext) {
        return None;
    }
    Some(Sheet { raw: sheet.raw.clone(), passthrough: sheet.passthrough.clone(), ext: carried(&sheet.ext), ..Sheet::default() })
}

/// The shell of a shape: its attributes and elements, `Data1` to `Data3`, the deleted flag, the kind when it is not `expected` (the kind the reader gives the element the shape is written as),
/// the slot, and the same of its sheet and its text's slot. **Foreign data is not in it** ([`foreign_shell`]), and neither are its children (each is written as a shape of its own). `None` when the
/// shape holds nothing that rides.
pub(crate) fn shape_shell(shape: &Shape, expected: &ShapeType) -> Option<Shape> {
    let sheet = sheet_shell(&shape.sheet);
    let text = shape.text.as_deref().filter(|t| rides(&t.ext)).map(|t| Arc::new(Text { parts: Vec::new(), ext: carried(&t.ext) }));
    let data = [&shape.data1, &shape.data2, &shape.data3].iter().any(|d| d.is_some());
    // A guide is carried as a guide (`Tier2::kind`).
    let kind_differs = shape.kind != *expected && shape.kind != ShapeType::Guide;
    if shape.raw.is_empty()
        && shape.passthrough.is_empty()
        && !data
        && !shape.del
        && !kind_differs
        && !rides(&shape.ext)
        && sheet.is_none()
        && text.is_none()
    {
        return None;
    }
    Some(Shape {
        id: shape.id,
        kind: if kind_differs { shape.kind.clone() } else { expected.clone() },
        del: shape.del,
        data1: shape.data1.clone(),
        data2: shape.data2.clone(),
        data3: shape.data3.clone(),
        raw: shape.raw.clone(),
        passthrough: shape.passthrough.clone(),
        sheet: Arc::new(sheet.unwrap_or_default()),
        text,
        ext: carried(&shape.ext),
        ..Shape::default()
    })
}

/// Puts a shape's shell over the shape the reader made and its sheet (`sheet` is the sheet the reader gave it, which the caller puts back in the shape).
pub(crate) fn apply(shape: &mut Shape, sheet: &mut Sheet, shell: &Shape) {
    shape.kind = shell.kind.clone();
    shape.del = shell.del;
    shape.data1.clone_from(&shell.data1);
    shape.data2.clone_from(&shell.data2);
    shape.data3.clone_from(&shell.data3);
    shape.raw = shell.raw.clone();
    shape.passthrough = shell.passthrough.clone();
    put(&mut shape.ext, &shell.ext);
    sheet.raw = shell.sheet.raw.clone();
    sheet.passthrough = shell.sheet.passthrough.clone();
    put(&mut sheet.ext, &shell.sheet.ext);
    if let Some(text) = &shell.text
        && let Some(target) = shape.text.as_mut()
    {
        put(&mut Arc::make_mut(target).ext, &text.ext);
    }
}

/// The shell of foreign data: everything of it but the picture, which the markup holds. `None` for foreign data that holds nothing the markup does not say. `reads` says what the reader gives
/// the shape's frame: `Some(type)` for a picture frame, whose element says the foreign data's type (`ForeignType`, by the picture's signature: `Some(None)` for a picture of a kind the reader gives none)
/// and nothing else of it; `None` for an element that gives no foreign data. The data is carried whenever it is more than that.
pub(crate) fn foreign_shell(foreign: &ForeignData, reads: Option<Option<&str>>) -> Option<ForeignData> {
    if let Some(given) = reads {
        let attrs_as_read = match given {
            Some(t) => foreign.attrs.len() == 1 && foreign.attrs.get("ForeignType") == Some(t),
            None => foreign.attrs.is_empty(),
        };
        if attrs_as_read && foreign.rel_id.is_none() && foreign.text.is_none() && foreign.passthrough.is_empty() && !rides(&foreign.ext) {
            return None;
        }
    }
    Some(ForeignData {
        attrs: foreign.attrs.clone(),
        rel_id: foreign.rel_id.clone(),
        picture: None,
        text: foreign.text.clone(),
        passthrough: foreign.passthrough.clone(),
        ext: carried(&foreign.ext),
    })
}

/// Puts foreign data's shell over the foreign data the reader made, which keeps its picture: the shell's attributes replace the reader's (`ForeignType` and the attributes that rode, in the order the
/// file had them), and the rest is the shell's.
pub(crate) fn apply_foreign(shape: &mut Shape, shell: &ForeignData) {
    let mut target = shape.foreign.as_deref().cloned().unwrap_or_default();
    // The type of a picture is told by its own bytes: where the reader knows it, it stands whatever the data says (a picture saved under another type reads as the type it is).
    // (An object frame's is `Object`, which is no type of a picture: it is the data's.)
    let by_bytes = target
        .attrs
        .get("ForeignType")
        .map(str::to_owned)
        .filter(|_| shell.attrs.get("ForeignType").is_some_and(|t| matches!(t, "Bitmap" | "EnhMetaFile" | "MetaFile")));
    target.attrs = match by_bytes {
        Some(kind) if shell.attrs.get("ForeignType").is_some() => diacraft_doc::RawAttrs::from_pairs(
            shell.attrs.iter().map(|(k, v)| (k.to_owned(), if k == "ForeignType" { kind.clone() } else { v.to_owned() })),
        ),
        _ => shell.attrs.clone(),
    };
    target.rel_id.clone_from(&shell.rel_id);
    target.text.clone_from(&shell.text);
    target.passthrough = shell.passthrough.clone();
    put(&mut target.ext, &shell.ext);
    shape.foreign = Some(Arc::new(target));
}

// ------------------------------------------------------------------------------------------------------------------ pages, styles, the document

/// The shell of a page: the attributes and elements of its part and of its row of the pages index that no ODF markup says (`raw`, `passthrough`, the relationship and the part it was read from), its slot, and the
/// same of its sheet. **A page's name, its size, its background and its shapes are what the file's markup and DiaCraft's document file say**; a page that has a background, which an `.odg` writes no page
/// for, is refused by the writer, as it was. `None` when the page holds nothing that rides.
pub(crate) fn page_shell(page: &diacraft_doc::Page) -> Option<diacraft_doc::Page> {
    let sheet = sheet_shell(&page.sheet);
    if page.raw.is_empty() && page.passthrough.is_empty() && page.rel_id.is_none() && page.part_name.is_none() && !rides(&page.ext) && sheet.is_none()
    {
        return None;
    }
    let mut shell = diacraft_doc::Page::new(page.id, "");
    shell.name_u = None;
    shell.raw = page.raw.clone();
    shell.passthrough = page.passthrough.clone();
    shell.rel_id.clone_from(&page.rel_id);
    shell.part_name.clone_from(&page.part_name);
    shell.ext = carried(&page.ext);
    shell.sheet = Arc::new(sheet.unwrap_or_default());
    Some(shell)
}

/// Puts a page's shell over the page the reader made.
pub(crate) fn page_put(page: &mut diacraft_doc::Page, shell: &diacraft_doc::Page) {
    page.raw = shell.raw.clone();
    page.passthrough = shell.passthrough.clone();
    page.rel_id.clone_from(&shell.rel_id);
    page.part_name.clone_from(&shell.part_name);
    put(&mut page.ext, &shell.ext);
    let sheet = Arc::make_mut(&mut page.sheet);
    sheet.raw = shell.sheet.raw.clone();
    sheet.passthrough = shell.sheet.passthrough.clone();
    put(&mut sheet.ext, &shell.sheet.ext);
}

/// The shell of a style sheet: its attributes and elements, its slot, and the same of its sheet. `None` when it holds nothing that rides.
pub(crate) fn style_shell(style: &diacraft_doc::Style) -> Option<diacraft_doc::Style> {
    let sheet = sheet_shell(&style.sheet);
    if style.raw.is_empty() && style.passthrough.is_empty() && !rides(&style.ext) && sheet.is_none() {
        return None;
    }
    Some(diacraft_doc::Style {
        id: style.id,
        name: style.name.clone(),
        raw: style.raw.clone(),
        passthrough: style.passthrough.clone(),
        ext: carried(&style.ext),
        sheet: Arc::new(sheet.unwrap_or_default()),
        ..diacraft_doc::Style::default()
    })
}

/// Puts a style's shell over the style the reader made.
pub(crate) fn style_put(style: &mut diacraft_doc::Style, shell: &diacraft_doc::Style) {
    style.raw = shell.raw.clone();
    style.passthrough = shell.passthrough.clone();
    put(&mut style.ext, &shell.ext);
    let sheet = Arc::make_mut(&mut style.sheet);
    sheet.raw = shell.sheet.raw.clone();
    sheet.passthrough = shell.sheet.passthrough.clone();
    put(&mut sheet.ext, &shell.sheet.ext);
}

/// The properties `meta.xml` has a field for, in one order: the subject, the creator, the last modifier, the keywords, the description, the times of creation, last save and last print, and the language.
/// A field of `meta.xml` that holds no text is no field to the reader (it keeps the element as markup), so the writer writes none for an empty text and the reader gives `None` for it.
pub(crate) fn typed_texts(p: &diacraft_doc::DocProps) -> [&Option<Arc<str>>; 9] {
    [&p.subject, &p.creator, &p.last_modified_by, &p.keywords, &p.description, &p.created, &p.modified, &p.last_printed, &p.language]
}

/// [`typed_texts`], to write to.
pub(crate) fn typed_texts_mut(p: &mut diacraft_doc::DocProps) -> [&mut Option<Arc<str>>; 9] {
    [
        &mut p.subject,
        &mut p.creator,
        &mut p.last_modified_by,
        &mut p.keywords,
        &mut p.description,
        &mut p.created,
        &mut p.modified,
        &mut p.last_printed,
        &mut p.language,
    ]
}

/// The shell of the document: its slot and elements, the faces that hold something (found again by their names), the properties an `.odg` has no field for, the package's relationships and rows and the
/// rest of its properties parts (`windows.xml` is DiaCraft's document file's own), and its sheet's own data. `None` when it holds nothing that rides. **The parts and the custom properties are no part of it**: they
/// are an item of their own, with no stamp.
pub(crate) fn document_shell(doc: &diacraft_doc::Document) -> Option<diacraft_doc::Document> {
    // The faces a VSDX file declares (they hold attributes of their own: its face numbers among them), in their order: an `.odg` holds the faces its runs name, so a face the file lacks is put back in its
    // place ([`faces_put`]). A face that
    // is a name and nothing more is the engine's, made when a run is given a font, and the save declares it where it is used.
    let fonts: Vec<diacraft_doc::FaceName> = doc
        .fonts
        .iter()
        .filter(|f| !f.raw.is_empty() || rides(&f.ext))
        .map(|f| diacraft_doc::FaceName { name_u: Arc::clone(&f.name_u), raw: f.raw.clone(), ext: carried(&f.ext) })
        .collect();
    let mut props = diacraft_doc::DocProps {
        category: doc.props.category.clone(),
        company: doc.props.company.clone(),
        manager: doc.props.manager.clone(),
        hyperlink_base: doc.props.hyperlink_base.clone(),
        ..diacraft_doc::DocProps::default()
    };
    // A field `meta.xml` has that the drawing holds as an empty text (a VSDX file's empty element) is one the reader of `meta.xml` takes for no field: it rides here, as empty.
    for (shell, mine) in typed_texts_mut(&mut props).into_iter().zip(typed_texts(&doc.props)) {
        if mine.as_deref() == Some("") {
            shell.clone_from(mine);
        }
    }
    let has_props = props != diacraft_doc::DocProps::default();
    let held = doc.vsdx.as_ref().filter(|v| !v.rels.is_empty() || !v.content_types.is_empty() || !v.props_rest.is_empty()).map(|v| {
        // The windows part is DiaCraft's document file's own (the window settings).
        Arc::new(diacraft_doc::VsdxHeld { windows: None, ..(**v).clone() })
    });
    let sheet = sheet_shell(&doc.sheet);
    // The Snap & Glue words of `DocumentSettings` are DiaCraft's document file's own (`words`).
    let passthrough = doc.passthrough_but_snap_words();
    if !rides(&doc.ext) && passthrough.is_empty() && fonts.is_empty() && !has_props && held.is_none() && sheet.is_none() {
        return None;
    }
    let mut shell = diacraft_doc::Document::empty();
    shell.ext = carried(&doc.ext);
    shell.passthrough = passthrough;
    shell.fonts = Arc::new(fonts);
    shell.props = Arc::new(props);
    shell.vsdx = held;
    shell.sheet = Arc::new(sheet.unwrap_or_default());
    Some(shell)
}

/// **The face table the item carries, in its order and with every face it holds** (found the VSDX written after an `.odg` trip dropping a face in 10 files): `file` is the
/// table the reader made of the `.odg` (a face for each font declaration, by its name), `carried` the faces the item holds (a VSDX file's table, each with its attributes, its number among them). Each
/// carried face, in the carried order, takes the first face of the file of its name that no earlier one took, with the carried attributes and slot over it; **two carried faces of one name are two faces**
/// (one name was the one key, so the second took the first's place, and a face and its number were lost); a carried face the file has no declaration for (no run names it) is the table's own, in its
/// place. The faces of the file that no carried face takes (another program declared them) follow, in the file's order. The work is a pass over each list and a map of the file's names.
fn faces_put(file: &[diacraft_doc::FaceName], carried: &[diacraft_doc::FaceName]) -> Vec<diacraft_doc::FaceName> {
    use std::collections::{HashMap, VecDeque};
    let mut by_name: HashMap<&str, VecDeque<usize>> = HashMap::with_capacity(file.len());
    for (at, face) in file.iter().enumerate() {
        by_name.entry(face.name_u.as_ref()).or_default().push_back(at);
    }
    let mut taken = vec![false; file.len()];
    let mut out: Vec<diacraft_doc::FaceName> = Vec::with_capacity(carried.len().saturating_add(file.len()));
    for face in carried {
        let at = by_name.get_mut(face.name_u.as_ref()).and_then(VecDeque::pop_front);
        match at.and_then(|at| file.get(at).map(|f| (at, f))) {
            Some((at, mine)) => {
                if let Some(slot) = taken.get_mut(at) {
                    *slot = true;
                }
                let mut merged = mine.clone();
                merged.raw = face.raw.clone();
                put(&mut merged.ext, &face.ext);
                out.push(merged);
            }
            None => out.push(face.clone()),
        }
    }
    out.extend(file.iter().zip(taken).filter(|(_, taken)| !taken).map(|(face, _)| face.clone()));
    out
}

/// Puts the document's shell over the document the reader made. → how many faces the file does not have (their data is left out).
pub(crate) fn document_put(doc: &mut diacraft_doc::Document, shell: &diacraft_doc::Document) -> usize {
    put(&mut doc.ext, &shell.ext);
    if !shell.passthrough.is_empty() {
        let words = doc.snap_words();
        doc.passthrough = shell.passthrough.clone();
        doc.set_snap_words(&words);
    }
    let props = Arc::make_mut(&mut doc.props);
    props.category.clone_from(&shell.props.category);
    props.company.clone_from(&shell.props.company);
    props.manager.clone_from(&shell.props.manager);
    props.hyperlink_base.clone_from(&shell.props.hyperlink_base);
    for (mine, held) in typed_texts_mut(props).into_iter().zip(typed_texts(&shell.props)) {
        if mine.is_none() && held.as_deref() == Some("") {
            mine.clone_from(held);
        }
    }
    if let Some(held) = &shell.vsdx {
        let mine = Arc::make_mut(doc.vsdx.get_or_insert_with(Default::default));
        mine.rels = held.rels.clone();
        mine.content_types = held.content_types.clone();
        mine.props_rest = held.props_rest.clone();
    }
    let lost = 0usize;
    if !shell.fonts.is_empty() {
        doc.fonts = Arc::new(faces_put(&doc.fonts, &shell.fonts));
    }
    let sheet = Arc::make_mut(&mut doc.sheet);
    sheet.raw = shell.sheet.raw.clone();
    sheet.passthrough = shell.sheet.passthrough.clone();
    put(&mut sheet.ext, &shell.sheet.ext);
    lost
}
