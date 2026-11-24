//! Pages: add, copy and rename. Deleting and reordering pages, a page's type and background, and the page setup commands are in `page_setup.rs`.

use serde_json::{Value, json};

use super::params::optional_whole;
use super::{CommandSpec, from_doc, has_document, optional_text, required_text, required_u32};
use crate::error::{bad, internal};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "page.add",
            "Add Page",
            ["Page"],
            None,
            "name: text, the new page's name (optional, and without it the page is called \"Page-N\" for its id N, or for the next N no page is called by, and a background page \"VBackground-N\" for the first N no page is called by); \
             like: whole number, optional, the id of a page whose setup (size, orientation, scale, units, shadow, ruler and grid) the new page copies; \
             background: true or false, optional (default false), the new page is a background page, kept with the background pages before the pages that are not backgrounds, and the pages that are not backgrounds name it in Page Setup",
            has_document,
            add
        ),
        cmd!(
            "page.rename",
            "Rename Page",
            ["Page"],
            None,
            "id: whole number, the id of the page to rename; name: text, the new name",
            has_document,
            rename
        ),
        cmd!(
            "page.duplicate",
            "Duplicate Page",
            ["Page"],
            None,
            "id: whole number, optional, the id of the page to copy (default: the first page that is not a background): the copy has its shapes, connections, layers and page cells, is named for the page and a number (\"Page-1-2\"), is put right after it, and is a background page when it is",
            has_document,
            duplicate
        ),
    ]
}

/// Adds a page after the others → `{id, name}` of the new page. With `like` the page has the page setup of that page,
/// "Add a page from the page tabs" has it (the new page copies size, orientation, scale, units, shadow offset, ruler and grid from the page on view). With `background` it is
/// a background page (`draw/pages.rs`), put before the pages that are not backgrounds.
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.add";
    let name = optional_text(p, "name", ID)?;
    let like = optional_whole(p, "like", ID)?;
    let background = match p.get("background") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err(bad(ID, "`background` must be true or false")),
    };
    s.edit(if background { "Add background page" } else { "Add page" }, |doc| {
        if let Some(like) = like
            && doc.page(like).is_none()
        {
            return Err(bad(ID, format!("there is no page {like} to copy the setup of")));
        }
        let id = if background {
            crate::draw::add_background_page(doc, name).map_err(|e| e.into_engine(ID))?
        } else {
            // Without a name the page is called as `Document::add_page` calls it, unless a page has that name: a drawing read from an `.odg` numbers its pages from 0, so
            // the second page's `ID` 1 would name it `Page-1` beside the first, and two pages of one name cannot be saved in an `.odg` (`draw:name` is unique among pages).
            let default = if name.is_none() { default_page_name(doc) } else { None };
            doc.add_page(default.as_deref().or(name)).map_err(|e| from_doc(ID, e))?
        };
        if let Some(like) = like {
            crate::draw::copy_setup(doc, like, id).map_err(|e| e.into_engine(ID))?;
        }
        // A field that shows a page's name, its number or the count of pages is worked out again.
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        let page = doc.page(id).ok_or_else(|| internal(ID, "the page just added is missing"))?;
        Ok(json!({ "id": page.id, "name": page.name }))
    })
}

/// The name a page added without one is given: `Page-N` for the `ID` N it will have, or for the next number whose name no page has. `None` when no `ID` is left (the
/// document refuses the page then).
fn default_page_name(doc: &diacraft_doc::Document) -> Option<String> {
    let id = doc.pages.iter().map(|p| p.id).max().map_or(Some(1), |highest| highest.checked_add(1))?;
    let taken: std::collections::HashSet<&str> = doc.pages.iter().map(|p| &*p.name).collect();
    (u64::from(id)..).map(|n| format!("Page-{n}")).find(|name| !taken.contains(name.as_str()))
}

/// Puts a copy of a page right after it → `{id, name, from}`: the new page's `ID` and name, and the `ID` of the page it is a copy of. One undo step.
fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.duplicate";
    let named = optional_whole(p, "id", ID)?;
    let from = match named {
        Some(id) => id,
        None => {
            let doc = s.document().ok_or_else(|| bad(ID, "no document open"))?;
            doc.pages
                .iter()
                .find(|page| !page.background)
                .or_else(|| doc.pages.iter().next())
                .map(|page| page.id)
                .ok_or_else(|| bad(ID, "the document has no pages"))?
        }
    };
    s.edit("Duplicate page", |doc| {
        let made = crate::draw::duplicate_page(doc, from).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok(json!({ "id": made.id, "name": made.name, "from": from }))
    })
}

/// Renames a page → `{id, name}`.
fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.rename";
    let id = required_u32(p, "id", ID)?;
    let name = required_text(p, "name", ID)?;
    s.edit("Rename page", |doc| {
        doc.rename_page(id, name).map_err(|e| from_doc(ID, e))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok(json!({ "id": id, "name": name }))
    })
}
