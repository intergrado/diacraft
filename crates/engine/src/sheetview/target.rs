//! Naming a sheet (design 7.1): the params every `sheet.*` command takes, and the object a window's tab holds.
//!
//! | Params | The sheet |
//! |---|---|
//! | `id`, and `page` optional | The shape with that `ID`: on `page`, else on the first page that holds it (a group's member too) |
//! | `page` alone | The page's sheet |
//! | `master` and `id` | The shape with that `ID` in the master |
//! | `master` alone | The master's page sheet |
//! | `style` | The style's sheet |
//! | `document: true` | The document's sheet |
//!
//! Any other mix is refused in the words of [`ONE_SHEET`]. A [`Place`] holds the sheet **by owner and `ID`, never by a path**: a structural edit changes paths, so a window that
//! keeps a place asks [`Place::sheet`] again at each revision, and an object that has gone is `None`.

use diacraft_doc::{Document, Owner, ShapeRef, SheetKind, SheetRef};
use serde_json::{Map, Value, json};

/// The words of a refusal of a bad mix of sheet params (design 7.1).
pub const ONE_SHEET: &str = "give one sheet: id (with page), page, master (with id), style or document";

/// The most characters of a name in a title: a name is the file's and may be as long as the model allows.
const TITLE_CHARS: usize = 200;

/// Which object a sheet is of, by what names it in a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// A shape of a page or a master part, by the part and the shape's `ID`.
    Shape { owner: Owner, id: u32 },
    /// A page's sheet.
    Page(u32),
    /// A master's page sheet.
    Master(u32),
    /// A style.
    Style(u32),
    /// The document.
    Document,
}

/// A whole number `p` gives for `key`, if it gives one.
fn whole(p: &Value, key: &str) -> Result<Option<u32>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| format!("`{key}` must be a whole number from 0 to {}", u32::MAX))
        }
    }
}

/// The sheet a call names, **before the document is asked**: what the five keys of say and nothing more. A batch of writes parses each write's keys with
/// [`Target::from_params`] and finds all its shapes in one walk of each page, where [`Place::from_params`] asks the document for one sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A shape: on `page`, in the master `master`, or on the first page that holds it.
    Shape {
        id: u32,
        page: Option<u32>,
        master: Option<u32>,
    },
    Page(u32),
    Master(u32),
    Style(u32),
    Document,
}

impl Target {
    /// The sheet `p` names, by the five keys `id`, `page`, `master`, `style` and `document`: the words of a refusal for a key of the wrong kind and for a bad mix.
    pub fn from_params(p: &Value) -> Result<Target, String> {
        let (id, page, master, style) = (whole(p, "id")?, whole(p, "page")?, whole(p, "master")?, whole(p, "style")?);
        let document = match p.get("document") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            Some(_) => return Err("`document` must be true or false".to_owned()),
        };
        match (id, page, master, style, document) {
            (Some(id), page, None, None, false) => Ok(Target::Shape { id, page, master: None }),
            (None, Some(page), None, None, false) => Ok(Target::Page(page)),
            (Some(id), None, Some(master), None, false) => Ok(Target::Shape { id, page: None, master: Some(master) }),
            (None, None, Some(master), None, false) => Ok(Target::Master(master)),
            (None, None, None, Some(style), false) => Ok(Target::Style(style)),
            (None, None, None, None, true) => Ok(Target::Document),
            _ => Err(ONE_SHEET.to_owned()),
        }
    }

    /// The place this target is in `doc`, by a search for each shape: the words of a refusal when the object is not there.
    pub fn resolve(&self, doc: &Document) -> Result<Place, String> {
        match *self {
            Target::Shape { id, page, master: None } => on_a_page(doc, id, page),
            Target::Shape { id, master: Some(master), .. } => {
                doc.master(master).ok_or_else(|| format!("the drawing has no master {master}"))?;
                match live(doc, Owner::Master(master), id) {
                    Some(_) => Ok(Place::Shape { owner: Owner::Master(master), id }),
                    None => Err(format!("master {master} has no shape {id}")),
                }
            }
            Target::Page(page) => doc.page(page).map(|_| Place::Page(page)).ok_or_else(|| format!("the drawing has no page {page}")),
            Target::Master(master) => doc.master(master).map(|_| Place::Master(master)).ok_or_else(|| format!("the drawing has no master {master}")),
            Target::Style(style) => doc.style(style).map(|_| Place::Style(style)).ok_or_else(|| format!("the drawing has no style {style}")),
            Target::Document => Ok(Place::Document),
        }
    }
}

impl Place {
    /// The sheet `p` names, which the document must have: the words of a refusal otherwise. The keys it reads are `id`, `page`, `master`, `style` and `document`.
    pub fn from_params(doc: &Document, p: &Value) -> Result<Place, String> {
        Target::from_params(p)?.resolve(doc)
    }

    /// The place of the sheet `sheet` of `doc`: a shape by its owner and `ID`, never by its path. `None` for a sheet that is not there.
    pub fn of(doc: &Document, sheet: &SheetRef) -> Option<Place> {
        match sheet {
            SheetRef::Shape(at) => doc.shape(at).map(|shape| Place::Shape { owner: at.owner, id: shape.id }),
            SheetRef::Page(id) => doc.page(*id).map(|_| Place::Page(*id)),
            SheetRef::Master(id) => doc.master(*id).map(|_| Place::Master(*id)),
            SheetRef::Style(id) => doc.style(*id).map(|_| Place::Style(*id)),
            SheetRef::Document => Some(Place::Document),
        }
    }

    /// The sheet this place is in `doc` now, if the object is still there: a shape is found by its `ID` (the first with it in the part, a deleted one not counting).
    pub fn sheet(&self, doc: &Document) -> Option<SheetRef> {
        match self {
            Place::Shape { owner, id } => live(doc, *owner, *id).map(SheetRef::Shape),
            Place::Page(id) => doc.page(*id).map(|_| SheetRef::Page(*id)),
            Place::Master(id) => doc.master(*id).map(|_| SheetRef::Master(*id)),
            Place::Style(id) => doc.style(*id).map(|_| SheetRef::Style(*id)),
            Place::Document => Some(SheetRef::Document),
        }
    }

    /// The kind of sheet it is.
    pub fn kind(&self) -> SheetKind {
        match self {
            Place::Shape { .. } => SheetKind::Shape,
            Place::Page(_) | Place::Master(_) => SheetKind::Page,
            Place::Style(_) => SheetKind::Style,
            Place::Document => SheetKind::Document,
        }
    }

    /// The place as an answer names the sheet back: `{kind, page, master, style, id}` with the keys that apply.
    pub fn json(&self) -> Value {
        let mut out = Map::new();
        match self {
            Place::Shape { owner: Owner::Page(page), id } => {
                out.insert("kind".into(), json!("shape"));
                out.insert("page".into(), json!(page));
                out.insert("id".into(), json!(id));
            }
            Place::Shape { owner: Owner::Master(master), id } => {
                out.insert("kind".into(), json!("shape"));
                out.insert("master".into(), json!(master));
                out.insert("id".into(), json!(id));
            }
            Place::Page(page) => {
                out.insert("kind".into(), json!("page"));
                out.insert("page".into(), json!(page));
            }
            Place::Master(master) => {
                out.insert("kind".into(), json!("master"));
                out.insert("master".into(), json!(master));
            }
            Place::Style(style) => {
                out.insert("kind".into(), json!("style"));
                out.insert("style".into(), json!(style));
            }
            Place::Document => {
                out.insert("kind".into(), json!("document"));
            }
        }
        Value::Object(out)
    }

    /// The title of a tab (design 5.3): the shape's `Name`, else `NameU`, else `Sheet.<id>`, and the page (`Process.4, Page-1`); the page's name; `Document`; the style's name
    /// with "(style)"; the master's name with "(master)". A name longer than 200 characters is cut.
    pub fn title(&self, doc: &Document) -> String {
        let clip = |text: &str| super::text::cut(text, TITLE_CHARS).0;
        let master_name =
            |id: u32| doc.master(id).and_then(|m| m.name.clone().or_else(|| m.name_u.clone())).map_or_else(|| format!("Master {id}"), |n| clip(&n));
        match self {
            Place::Shape { owner, id } => {
                let name = live(doc, *owner, *id)
                    .and_then(|at| doc.shape(&at).and_then(|s| s.name.clone().or_else(|| s.name_u.clone())))
                    .map_or_else(|| format!("Sheet.{id}"), |n| clip(&n));
                match owner {
                    Owner::Page(page) => format!("{name}, {}", doc.page(*page).map_or_else(|| format!("Page {page}"), |p| clip(&p.name))),
                    Owner::Master(master) => format!("{name}, {} (master)", master_name(*master)),
                }
            }
            Place::Page(id) => doc.page(*id).map_or_else(|| format!("Page {id}"), |p| clip(&p.name)),
            Place::Master(id) => format!("{} (master)", master_name(*id)),
            Place::Style(id) => {
                format!(
                    "{} (style)",
                    doc.style(*id).and_then(|s| s.name.clone().or_else(|| s.name_u.clone())).map_or_else(|| format!("Style {id}"), |n| clip(&n))
                )
            }
            Place::Document => "Document".to_owned(),
        }
    }
}

/// The first shape with `id` in a page or a master part that is not deleted (`Del`).
fn live(doc: &Document, owner: Owner, id: u32) -> Option<ShapeRef> {
    let at = doc.find_shape(owner, id)?;
    doc.shape(&at).filter(|shape| !shape.del).map(|_| at)
}

/// A shape on `page`, or on the first page that holds it.
fn on_a_page(doc: &Document, id: u32, page: Option<u32>) -> Result<Place, String> {
    match page {
        Some(page) => {
            doc.page(page).ok_or_else(|| format!("the drawing has no page {page}"))?;
            live(doc, Owner::Page(page), id)
                .map(|_| Place::Shape { owner: Owner::Page(page), id })
                .ok_or_else(|| format!("page {page} has no shape {id}"))
        }
        None => doc
            .pages
            .iter()
            .find(|p| live(doc, Owner::Page(p.id), id).is_some())
            .map(|p| Place::Shape { owner: Owner::Page(p.id), id })
            .ok_or_else(|| format!("no page has a shape {id}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_place_names_the_sheet_back_with_only_the_keys_that_apply() {
        assert_eq!(Place::Document.json(), json!({"kind": "document"}));
        assert_eq!(Place::Shape { owner: Owner::Master(2), id: 5 }.json(), json!({"kind": "shape", "master": 2, "id": 5}));
        assert_eq!(Place::Page(1).json(), json!({"kind": "page", "page": 1}));
    }

    #[test]
    fn a_place_on_a_document_without_it_is_not_there() {
        let doc = Document::new_blank();
        assert_eq!(Place::Page(9).sheet(&doc), None);
        assert_eq!(Place::Shape { owner: Owner::Page(1), id: 3 }.sheet(&doc), None);
        assert_eq!(Place::Document.sheet(&doc), Some(SheetRef::Document));
        assert_eq!(Place::Page(1).kind(), SheetKind::Page);
    }
}
