//! Pages: add, copy and rename. Deleting and reordering pages, a page's type and background, and the page setup commands are in `page_setup.rs`.

use diacraft_doc::check_given_page_name;
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
            "name: text, the new page's name (optional, at most 31 characters, not \"page\" and digits in any case, and not the name of another page, compared without regard to case, and without it the page is called \"Page-N\" for its id N, or for the next N no page is called by, and a background page \"VBackground-N\" for the first N no page is called by); \
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
            "id: whole number, the id of the page to rename; name: text, the new name, at most 31 characters, not \"page\" and digits in any case, and not the name of another page, compared without regard to case",
            has_document,
            rename
        ),
        cmd!(
            "page.duplicate",
            "Duplicate Page",
            ["Page"],
            None,
            "id: whole number, optional, the id of the page to copy (default: the first page that is not a background): the copy has its shapes, connections, layers and page cells, is named for the page and a number (\"Page-1-2\", the page's name cut short where the whole would pass 31 characters), is put right after it, and is a background page when it is",
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
    // A name a command gives a page has at most 31 characters (the page setup dialog's Page Properties tab), the background page's too.
    if let Some(name) = name {
        check_given_page_name(name).map_err(|e| from_doc(ID, e))?;
    }
    let like = optional_whole(p, "like", ID)?;
    let background = match p.get("background") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err(bad(ID, "`background` must be true or false")),
    };
    s.edit(if background { "Add background page" } else { "Add page" }, |doc| {
        let before = doc.clone();
        if let Some(like) = like
            && doc.page(like).is_none()
        {
            return Err(bad(ID, format!("there is no page {like} to copy the setup of")));
        }
        // **No two pages have one name**, compared without regard to case: the page is refused a name another has, before anything is changed.
        if let Some(name) = name {
            doc.check_free_page_name(name, None).map_err(|e| from_doc(ID, e))?;
        }
        let id = if background {
            crate::draw::add_background_page(doc, name).map_err(|e| e.into_engine(ID))?
        } else {
            // Without a name the page is called as `Document::add_page` calls it: `Page-N` for its `ID` N, or for the next number no page has the name of. A drawing read from an `.odg`
            // numbers its pages from 0, so the second page's `ID` 1 would name it `Page-1` beside the first, and two pages of one name cannot be saved in an `.odg` (`draw:name` is unique among pages).
            doc.add_page(name).map_err(|e| from_doc(ID, e))?
        };
        if let Some(like) = like {
            crate::draw::copy_setup(doc, like, id).map_err(|e| e.into_engine(ID))?;
        }
        // The cells that name the new page, or read the list of pages, are worked out, and a field that shows a page's name, its number or the count of pages is
        // worked out again.
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        let page = doc.page(id).ok_or_else(|| internal(ID, "the page just added is missing"))?;
        Ok(json!({ "id": page.id, "name": page.name }))
    })
}

/// Puts a copy of a page right after it → `{id, name, from}`: the new page's `ID` and name, and the `ID` of the page it is a copy of. One undo step.
fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.duplicate";
    let named = optional_whole(p, "id", ID)?;
    let from = match named {
        Some(id) => id,
        None => {
            let doc = s.document().ok_or_else(|| bad(ID, "no document open"))?;
            doc.first_foreground_page().map(|page| page.id).ok_or_else(|| bad(ID, "the document has no pages"))?
        }
    };
    s.edit("Duplicate page", |doc| {
        let before = doc.clone();
        let made = crate::draw::duplicate_page(doc, from).map_err(|e| e.into_engine(ID))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok(json!({ "id": made.id, "name": made.name, "from": from }))
    })
}

/// Renames a page → `{id, name}`.
fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "page.rename";
    let id = required_u32(p, "id", ID)?;
    let name = required_text(p, "name", ID)?;
    check_given_page_name(name).map_err(|e| from_doc(ID, e))?;
    s.edit("Rename page", |doc| {
        let before = doc.clone();
        // A page that is not there is the first thing to say; then **no other page has the name**, and the page may be given its own name again, in another case too.
        if doc.page(id).is_none() {
            return Err(from_doc(ID, diacraft_doc::DocError::NoSuchPage(id)));
        }
        doc.check_free_page_name(name, Some(id)).map_err(|e| from_doc(ID, e))?;
        doc.rename_page(id, name).map_err(|e| from_doc(ID, e))?;
        crate::draw::settle_pages(doc, &before).map_err(|e| e.into_engine(ID))?;
        crate::draw::fields::refresh_all_after_edit(doc, &crate::draw::fields::Env::none());
        Ok(json!({ "id": id, "name": name }))
    })
}
