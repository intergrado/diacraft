//! The list of pages: delete a page and put pages in another order, copy a page, and the background pages.
//!
//! The rows of the page tabs: a foreground page may be taken out or moved among the others;
//! background pages have no order among themselves and "background tabs cannot be moved", and in a file they come
//! before every foreground page, so the order a person sees and moves is the order **of the foreground pages**, and a move leaves every
//! background page in the place it has in the list.
//!
//! * Our rule. A page is taken out with its shapes. A drawing keeps at least one foreground page: the last is refused, since a
//!   drawing with only backgrounds has no page to open. A page that named the deleted one as its background
//!   (`BackPage`) names none after it, unless another page of the same `ID` is still there. Nothing else is rewritten.
//! * Our rule. `to` is the place the page has among the foreground pages after the move, counted from 0. With `renumber`
//!   every foreground page that has a default name, `Page-` and a number,
//!   is named for the place it has now: `Page-1` for the first foreground page, `Page-2` for the second, and so on. A name a person gave is
//!   left alone. `NameU` is not touched, as `page.rename` leaves it.
//! * Our rule. A copy of a page ("a copy of the page on view, with its shapes, for separate
//!   editing") is the page whole: its page cells (so its layers, its size and its scale), its shapes with their ids, its connects and what it names as its
//!   background. It has an `ID` of its own, one past the highest, and the name of its source and `-2`, or the first number from 2 that no page has taken (the
//!   specifications give no name; a name that cannot take the number is cut short). It stands **right after its source** in the list, a background page's copy
//!   being a background page. What belongs to the place the page had in its package (its part, its relationship, the `Rel` its row held, the unknown markup of its
//!   ODF page element) is left behind, and the file the drawing is saved into gives the copy a part of its own. **A picture or an object that a file names by a relationship of its page's
//!   part is copied with the page**: the VSDX writers give the copy's new part a row for every picture, object and other part its shapes name, leading to the parts the
//!   source's rows lead to (one part, two rows), so both pages show it after a save and a reopen, through either writer. A picture with no relationship (one that came from another
//!   format) is copied as the source holds it, and a save gives it a part of its own. (The clipboard still pastes a shape with a picture of a file only on the page it was copied from.)
//! * Our rule. A page of Type Background is a page with `Background` on, and it may be given one background page of its own like any page
//!   A background page that is added is named `VBackground-N` for the first number that no page has taken, and is
//!   put **before the first page that is not a background**, as a file lists them. **A page names only a background page, never itself and never a page that
//!   names it back** (a `BackPage` that names a foreground page is forbidden by MS-VSDX, and a loop is one the renderer stops at). A page that goes back to the
//!   foreground is let go of by the pages that named it, as a deleted one is, and is put after every other page; a page that becomes a background
//!   goes where an added one does; and a drawing keeps one page that is not a background, so the last is not turned into one. The attributes of the page's row that
//!   hold what the model's fields hold (`Background`, `BackPage`, kept in `raw` when they were spelled in a way the model does not write back) are taken out of `raw`
//!   with the change, as a writer lets a kept spelling win.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{DocError, Document, Kept, MAX_PAGE_NAME_CHARS, MAX_PAGES, Page, RawAttrs, check_given_page_name, check_page_name, page_name_key};

use super::Problem;

/// What a move of a page did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    /// The place the page had among the foreground pages, and the place it has.
    pub from: usize,
    pub to: usize,
    /// How many pages were renamed (`renumber`).
    pub renamed: usize,
}

/// The number in a default page name, `Page-` and digits.
fn default_number(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("Page-")?;
    (!digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
}

/// What taking a page out did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deleted {
    /// The name the page had.
    pub name: String,
    /// How many pages named it as their background and name none now.
    pub unassigned: usize,
}

/// `attrs` without the attribute `name`: a field of the model that a change sets must not be masked by a spelling kept in `raw`.
fn without(attrs: &RawAttrs, name: &str) -> RawAttrs {
    RawAttrs::from_pairs(attrs.iter().filter(|(key, _)| *key != name))
}

/// The pages that name `id` as their background no longer do. → how many were let go of.
fn let_go_of(doc: &mut Document, id: u32) -> usize {
    let mut count = 0;
    for slot in 0..doc.pages.len() {
        if doc.pages.get(slot).is_some_and(|p| p.back_page == Some(id))
            && let Some(page) = doc.pages.get_mut(slot)
        {
            let page = Arc::make_mut(page);
            page.back_page = None;
            page.raw = without(&page.raw, "BackPage");
            count += 1;
        }
    }
    count
}

/// Takes page `id` out of the document.
pub fn delete_page(doc: &mut Document, id: u32) -> Result<Deleted, Problem> {
    let at = doc.pages.iter().position(|p| p.id == id).ok_or_else(|| Problem::bad(format!("there is no page {id}")))?;
    let page = doc.pages.get(at).ok_or_else(|| Problem::failed("the page went away"))?;
    let (name, background) = (page.name.to_string(), page.background);
    let others = doc.pages.iter().enumerate().filter(|(i, p)| *i != at && !p.background).count();
    if !background && others == 0 {
        return Err(Problem::failed("a drawing keeps at least one page that is not a background"));
    }
    doc.pages.remove(at);
    // The first page of an `ID` is the one a name finds, so a second page of the same `ID` that is still there keeps the pages that name it.
    let unassigned = if doc.pages.iter().any(|p| p.id == id) { 0 } else { let_go_of(doc, id) };
    // The comments of a page that is gone go with it, unless a page of the same `ID` is still there.
    if !doc.comments.is_empty() && !doc.pages.iter().any(|p| p.id == id) {
        doc.comments.remove_page(id);
    }
    Ok(Deleted { name, unassigned })
}

/// The error of a failure of the document model, as a command says it: a name that cannot be is the caller's.
fn from_doc(error: DocError) -> Problem {
    match error {
        DocError::EmptyName
        | DocError::NameTooLong
        | DocError::PageNameTooLong
        | DocError::PageNameReserved
        | DocError::PageNameTaken
        | DocError::ControlCharacter
        | DocError::NoSuchPage(_)
        | DocError::BadName(_) => Problem::bad(error.to_string()),
        other => Problem::failed(other.to_string()),
    }
}

/// What a copy of a page made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duplicated {
    /// The `ID` and the name of the new page.
    pub id: u32,
    pub name: String,
}

/// The name a copy of the page called `name` is given: `name-2`, or the first `name-N` after it that no page has. A name that fails the rule for names (an
/// imported one may) is replaced by `Page`, and one too long to take the number is cut short to leave room for it, so that **the copy's name has at most 31 characters**, as a name a
/// command gives a page has (the copy is named by a command, and the page it is a copy of keeps the name its file gave it, however long).
fn copy_name(doc: &Document, name: &str) -> Option<String> {
    let taken = taken_names(doc);
    let base = if check_page_name(name).is_ok() { name } else { "Page" };
    // A page more than the drawing holds has a name that is free.
    (2..=MAX_PAGES as u64 + 2).find_map(|n| {
        let tail = format!("-{n}");
        let room = MAX_PAGE_NAME_CHARS.saturating_sub(tail.chars().count());
        let candidate: String = base.chars().take(room).chain(tail.chars()).collect();
        (!taken.contains(&page_name_key(&candidate))).then_some(candidate)
    })
}

/// The names the pages have, `Name` and `NameU`, as the keys names are compared by (without regard to case): a name given to a new page is none of these.
fn taken_names(doc: &Document) -> HashSet<String> {
    doc.pages.iter().flat_map(|p| std::iter::once(&p.name).chain(p.name_u.as_ref())).map(|n| page_name_key(n)).collect()
}

/// The next `ID` a page added has: one past the highest.
fn next_id(doc: &Document) -> Result<u32, Problem> {
    if doc.pages.len() >= MAX_PAGES {
        return Err(from_doc(DocError::TooManyPages));
    }
    doc.pages.iter().map(|p| p.id).max().map_or(Some(1), |highest| highest.checked_add(1)).ok_or_else(|| from_doc(DocError::IdsExhausted))
}

/// Puts a copy of page `from` right after it. → the copy's `ID` and name.
pub fn duplicate_page(doc: &mut Document, from: u32) -> Result<Duplicated, Problem> {
    let at = doc.pages.iter().position(|p| p.id == from).ok_or_else(|| Problem::bad(format!("there is no page {from}")))?;
    let id = next_id(doc)?;
    let source = doc.pages.get(at).ok_or_else(|| Problem::failed("the page went away"))?;
    // A picture or an object of a file is copied with the shapes: a save gives the copy's part the rows its shapes name.
    let name = copy_name(doc, &source.name).ok_or_else(|| Problem::failed("no name was left for the copy"))?;
    check_given_page_name(&name).map_err(from_doc)?;
    let mut copy = Page::clone(source);
    copy.id = id;
    copy.name = Arc::from(name.as_str());
    copy.name_u = Some(Arc::from(name.as_str()));
    // What the page's part in its package held is not the copy's, and the unknown markup of an ODF page element names that element.
    crate::stencil::refs::leave_package_for_copy(&mut copy);
    if copy.ext.get().is_some_and(|data| !data.odf.kept.raw.is_empty() || !data.odf.kept.kept.is_empty()) {
        copy.ext.make_mut().odf.kept = Kept::default();
        copy.ext.tidy();
    }
    doc.pages.insert(at.saturating_add(1), Arc::new(copy)).map_err(|_| Problem::failed("the list of pages went away"))?;
    Ok(Duplicated { id, name })
}

/// The name a background page added without one is given: `VBackground-N` for the first number from 1 that no page has taken.
fn background_name(doc: &Document) -> Option<String> {
    let taken = taken_names(doc);
    (1..=MAX_PAGES as u64 + 1).map(|n| format!("VBackground-{n}")).find(|name| !taken.contains(&page_name_key(name)))
}

/// The place a background page is put at: before the first page that is not a background, or at the end.
fn background_place(doc: &Document) -> usize {
    doc.pages.iter().position(|p| !p.background).unwrap_or(doc.pages.len())
}

/// Adds a background page, named `name` or as [`background_name`] has it, before the first page that is not a background. → its `ID`.
pub fn add_background_page(doc: &mut Document, name: Option<&str>) -> Result<u32, Problem> {
    let name = match name {
        Some(name) => name.to_owned(),
        None => background_name(doc).ok_or_else(|| Problem::failed("no name was left for the page"))?,
    };
    // The document checks the name and the number of pages, and makes the page with the sheet a new page has; it is put in its place then.
    let id = doc.add_page(Some(&name)).map_err(from_doc)?;
    let last = doc.pages.len().saturating_sub(1);
    let mut page = doc.pages.remove(last).ok_or_else(|| Problem::failed("the page just added is missing"))?;
    Arc::make_mut(&mut page).background = true;
    let at = background_place(doc);
    doc.pages.insert(at, page).map_err(|_| Problem::failed("the list of pages went away"))?;
    Ok(id)
}

/// What a change of a page's type did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retyped {
    /// Whether the page was not of the type already.
    pub changed: bool,
    /// How many pages named it as their background and name none now.
    pub unassigned: usize,
}

/// Makes page `id` a background page or a page that is not.
pub fn set_page_type(doc: &mut Document, id: u32, background: bool) -> Result<Retyped, Problem> {
    let at = doc.pages.iter().position(|p| p.id == id).ok_or_else(|| Problem::bad(format!("there is no page {id}")))?;
    let page = doc.pages.get(at).ok_or_else(|| Problem::failed("the page went away"))?;
    if page.background == background {
        return Ok(Retyped { changed: false, unassigned: 0 });
    }
    if background && doc.pages.iter().enumerate().all(|(i, p)| i == at || p.background) {
        return Err(Problem::failed("a drawing keeps at least one page that is not a background"));
    }
    let mut taken = doc.pages.remove(at).ok_or_else(|| Problem::failed("the page went away"))?;
    {
        let page = Arc::make_mut(&mut taken);
        page.background = background;
        page.raw = without(&page.raw, "Background");
    }
    let place = if background { background_place(doc) } else { doc.pages.len() };
    doc.pages.insert(place, taken).map_err(|_| Problem::failed("the list of pages went away"))?;
    // A page that is not a background is no one's background.
    let unassigned = if background { 0 } else { let_go_of(doc, id) };
    Ok(Retyped { changed: true, unassigned })
}

/// Gives page `id` the background page `back` (`None`: no background). → whether the page's background changed.
pub fn set_background(doc: &mut Document, id: u32, back: Option<u32>) -> Result<bool, Problem> {
    let at = doc.pages.iter().position(|p| p.id == id).ok_or_else(|| Problem::bad(format!("there is no page {id}")))?;
    if let Some(back) = back {
        let named = doc.page(back).ok_or_else(|| Problem::bad(format!("there is no page {back}")))?;
        if !named.background {
            return Err(Problem::bad(format!("page {back} is not a background page: only a page of Type Background may be a page's background")));
        }
        if back == id {
            return Err(Problem::bad("a page cannot be its own background"));
        }
        // A loop: the background's own backgrounds, nearest first, would reach the page.
        if doc.background_chain(back).iter().any(|p| p.id == id) {
            return Err(Problem::bad(format!(
                "page {back} has page {id} for its background, so page {id} cannot have it for its own: that would be a loop"
            )));
        }
    }
    let page = doc.pages.get(at).ok_or_else(|| Problem::failed("the page went away"))?;
    if page.back_page == back {
        return Ok(false);
    }
    let page = doc.pages.get_mut(at).ok_or_else(|| Problem::failed("the page went away"))?;
    let page = Arc::make_mut(page);
    page.back_page = back;
    page.raw = without(&page.raw, "BackPage");
    Ok(true)
}

/// Moves page `id` to the place `to` among the foreground pages. → what changed; a move to the place it has, with nothing to
/// rename, changes nothing.
pub fn move_page(doc: &mut Document, id: u32, to: usize, renumber: bool) -> Result<Moved, Problem> {
    let slots: Vec<usize> = doc.pages.iter().enumerate().filter(|(_, p)| !p.background).map(|(i, _)| i).collect();
    let Some(from) = slots.iter().position(|slot| doc.pages.get(*slot).is_some_and(|p| p.id == id)) else {
        return if doc.pages.iter().any(|p| p.id == id) {
            Err(Problem::failed("a background page has no place in the order of the pages"))
        } else {
            Err(Problem::bad(format!("there is no page {id}")))
        };
    };
    if to >= slots.len() {
        return Err(Problem::bad(format!("`to` must be a place from 0 to {} among the {} pages", slots.len() - 1, slots.len())));
    }
    let mut order: Vec<Arc<Page>> = slots.iter().filter_map(|slot| doc.pages.get(*slot).cloned()).collect();
    if order.len() != slots.len() {
        return Err(Problem::failed("the list of pages went away"));
    }
    let page = order.remove(from);
    order.insert(to, page);
    let mut renamed = 0;
    if renumber {
        // A renumbering gives no page the name of another: the pages it does not name keep theirs, and a name it would give that one of them has is refused.
        let kept: HashSet<String> =
            doc.pages.iter().filter(|p| p.background || default_number(&p.name).is_none()).map(|p| page_name_key(&p.name)).collect();
        for (place, page) in order.iter_mut().enumerate() {
            if default_number(&page.name).is_none() {
                continue;
            }
            let name = format!("Page-{}", place + 1);
            if *page.name != *name {
                check_page_name(&name)?;
                if kept.contains(&page_name_key(&name)) {
                    return Err(Problem::bad(format!(
                        "renumbering would give a page the name \"{name}\", which another page has: rename that page first"
                    )));
                }
                Arc::make_mut(page).name = Arc::from(name);
                renamed += 1;
            }
        }
    }
    if from != to || renamed > 0 {
        for (slot, page) in slots.iter().zip(order) {
            doc.pages.set(*slot, page).map_err(|_| Problem::failed("the list of pages went away"))?;
        }
    }
    Ok(Moved { from, to, renamed })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn doc_of(pages: &[(u32, &str, bool, Option<u32>)]) -> Document {
        let mut doc = Document::empty();
        for (id, name, background, back) in pages {
            let mut page = Page::new(*id, name);
            page.background = *background;
            page.back_page = *back;
            doc.pages.push(Arc::new(page));
        }
        doc
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.pages.iter().map(|p| p.name.to_string()).collect()
    }

    #[test]
    fn a_default_name_is_page_and_a_number() {
        assert_eq!(default_number("Page-1"), Some(1));
        assert_eq!(default_number("Page-12"), Some(12));
        for not in ["Page-", "Page-x", "page-1", "Page-1a", "Page 1", "Overview", "Page-1234567890", "Page--1", "Page-+1"] {
            assert_eq!(default_number(not), None, "{not}");
        }
    }

    #[test]
    fn moving_a_page_reorders_the_foreground_pages_and_leaves_the_backgrounds_in_their_places() {
        // A file lists its backgrounds first.
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, None), (2, "B", false, None), (3, "C", false, None)]);
        let moved = move_page(&mut doc, 3, 0, false).unwrap();
        assert_eq!(moved, Moved { from: 2, to: 0, renamed: 0 });
        assert_eq!(names(&doc), ["Back", "C", "A", "B"]);
        move_page(&mut doc, 3, 2, false).unwrap();
        assert_eq!(names(&doc), ["Back", "A", "B", "C"]);
        // A background between foreground pages stays where it is in the list.
        let mut mixed = doc_of(&[(1, "A", false, None), (9, "Back", true, None), (2, "B", false, None)]);
        move_page(&mut mixed, 2, 0, false).unwrap();
        assert_eq!(names(&mixed), ["B", "Back", "A"]);
    }

    #[test]
    fn a_move_to_the_place_a_page_has_changes_nothing() {
        let mut doc = doc_of(&[(1, "A", false, None), (2, "B", false, None)]);
        let before = doc.clone();
        assert_eq!(move_page(&mut doc, 2, 1, false).unwrap(), Moved { from: 1, to: 1, renamed: 0 });
        assert_eq!(doc, before);
    }

    #[test]
    fn a_background_page_cannot_be_moved_and_a_place_past_the_list_is_refused() {
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, None), (2, "B", false, None)]);
        let before = doc.clone();
        let err = move_page(&mut doc, 9, 0, false).unwrap_err();
        assert!(!err.bad_params && err.message.contains("background"), "{err:?}");
        assert!(move_page(&mut doc, 77, 0, false).unwrap_err().bad_params, "no such page");
        let err = move_page(&mut doc, 1, 2, false).unwrap_err();
        assert!(err.bad_params && err.message.contains("0 to 1"), "{err:?}");
        assert_eq!(doc, before);
    }

    #[test]
    fn renumbering_names_the_default_named_pages_for_their_new_places() {
        // Page-1 and Page-2, Page-2 moved first, renumbering on: the first tab reads Page-1.
        let mut doc = doc_of(&[(1, "Page-1", false, None), (2, "Page-2", false, None)]);
        let moved = move_page(&mut doc, 2, 0, true).unwrap();
        assert_eq!(moved, Moved { from: 1, to: 0, renamed: 2 });
        assert_eq!(names(&doc), ["Page-1", "Page-2"]);
        assert_eq!(doc.pages.iter().map(|p| p.id).collect::<Vec<_>>(), [2, 1], "the pages moved, and the names followed the places");
        // A name a person gave stays, and the places count it.
        let mut mixed = doc_of(&[(1, "Page-1", false, None), (2, "Overview", false, None), (3, "Page-3", false, None)]);
        move_page(&mut mixed, 3, 0, true).unwrap();
        assert_eq!(names(&mixed), ["Page-1", "Page-2", "Overview"]);
        assert_eq!(mixed.pages.iter().map(|p| p.id).collect::<Vec<_>>(), [3, 1, 2]);
    }

    #[test]
    fn renumbering_alone_is_a_change_and_a_tidy_order_is_not() {
        let mut doc = doc_of(&[(1, "Page-1", false, None), (2, "Page-5", false, None)]);
        let moved = move_page(&mut doc, 2, 1, true).unwrap();
        assert_eq!(moved, Moved { from: 1, to: 1, renamed: 1 });
        assert_eq!(names(&doc), ["Page-1", "Page-2"]);
        let before = doc.clone();
        assert_eq!(move_page(&mut doc, 2, 1, true).unwrap().renamed, 0);
        assert_eq!(doc, before);
    }

    #[test]
    fn deleting_a_page_clears_the_backgrounds_that_named_it_and_keeps_the_last_page() {
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, Some(9)), (2, "B", false, Some(9))]);
        assert_eq!(delete_page(&mut doc, 9).unwrap(), Deleted { name: "Back".to_owned(), unassigned: 2 });
        assert_eq!(doc.pages.iter().map(|p| p.back_page).collect::<Vec<_>>(), [None, None]);
        assert_eq!(delete_page(&mut doc, 1).unwrap().name, "A");
        let before = doc.clone();
        let err = delete_page(&mut doc, 2).unwrap_err();
        assert!(!err.bad_params && err.message.contains("at least one"), "{err:?}");
        assert!(delete_page(&mut doc, 5).unwrap_err().bad_params);
        assert_eq!(doc, before);
    }

    #[test]
    fn a_drawing_may_lose_a_background_while_it_has_a_foreground_page() {
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, None)]);
        delete_page(&mut doc, 9).unwrap();
        assert_eq!(names(&doc), ["A"]);
    }

    #[test]
    fn a_page_whose_id_a_second_page_holds_keeps_the_backgrounds_that_name_it() {
        // A hostile file with two pages of one ID: the one that is left is the one a reference finds.
        let mut doc = doc_of(&[(1, "A", false, Some(2)), (2, "Back", true, None), (2, "Back again", true, None)]);
        delete_page(&mut doc, 2).unwrap();
        assert_eq!(doc.pages.iter().map(|p| p.back_page).collect::<Vec<_>>(), [Some(2), None]);
    }

    fn raw_of(doc: &Document, id: u32) -> Vec<(String, String)> {
        doc.page(id).unwrap().raw.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
    }

    #[test]
    fn a_spelling_a_file_kept_does_not_outlive_the_change_it_is_the_attribute_of() {
        // The writer lets a kept spelling win (`AttrBuilder::typed`), so a `BackPage="009"` left in `raw` would write the old background back.
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, Some(9)), (2, "B", false, None)]);
        Arc::make_mut(doc.pages.get_mut(1).unwrap()).raw.push("BackPage", "009");
        Arc::make_mut(doc.pages.get_mut(0).unwrap()).raw.push("Background", "true");
        Arc::make_mut(doc.pages.get_mut(1).unwrap()).raw.push("ViewScale", "1.5");
        assert!(set_background(&mut doc, 1, None).unwrap());
        assert_eq!(
            raw_of(&doc, 1),
            [("ViewScale".to_owned(), "1.5".to_owned())],
            "the spelling went with the assignment, the rest of the row stayed"
        );
        // A page that stops being a background takes its `Background` spelling with it, and lets go of the pages that named it.
        Arc::make_mut(doc.pages.get_mut(2).unwrap()).back_page = Some(9);
        let done = set_page_type(&mut doc, 9, false).unwrap();
        assert_eq!(done, Retyped { changed: true, unassigned: 1 });
        assert!(raw_of(&doc, 9).is_empty());
        assert_eq!(doc.page(2).unwrap().back_page, None);
        // A page let go of by a deletion loses its spelling too.
        let mut doc = doc_of(&[(9, "Back", true, None), (1, "A", false, Some(9))]);
        Arc::make_mut(doc.pages.get_mut(1).unwrap()).raw.push("BackPage", "9 ");
        delete_page(&mut doc, 9).unwrap();
        assert!(raw_of(&doc, 1).is_empty());
    }

    #[test]
    fn a_page_becomes_a_background_before_the_pages_that_are_not_and_goes_back_after_them() {
        let mut doc = doc_of(&[(7, "Old back", true, None), (1, "A", false, None), (2, "B", false, None), (3, "C", false, None)]);
        assert!(set_page_type(&mut doc, 2, true).unwrap().changed);
        assert_eq!(names(&doc), ["Old back", "B", "A", "C"], "after the backgrounds there were, before the first page that is not one");
        assert!(set_page_type(&mut doc, 7, false).unwrap().changed);
        assert_eq!(names(&doc), ["B", "A", "C", "Old back"], "the last of the pages that are not backgrounds");
        // A list that is not in the file's order (backgrounds first) is left as it is but for the page: it goes before the first page that is not a background.
        let mut mixed = doc_of(&[(1, "A", false, None), (9, "Back", true, None), (2, "B", false, None)]);
        add_background_page(&mut mixed, None).unwrap();
        assert_eq!(names(&mixed), ["VBackground-1", "A", "Back", "B"]);
        let none = set_page_type(&mut mixed, 2, true).unwrap();
        assert!(none.changed);
        assert_eq!(names(&mixed), ["VBackground-1", "B", "A", "Back"]);
    }

    #[test]
    fn a_background_page_is_named_for_the_first_number_no_page_has_and_a_copy_for_the_first_number_from_two() {
        let mut doc = doc_of(&[(1, "A", false, None), (2, "VBackground-1", true, None), (3, "VBackground-3", true, None)]);
        let id = add_background_page(&mut doc, None).unwrap();
        assert_eq!((id, doc.page(id).unwrap().name.to_string()), (4, "VBackground-2".to_owned()));
        let made = duplicate_page(&mut doc, 1).unwrap();
        assert_eq!(made, Duplicated { id: 5, name: "A-2".to_owned() });
        let again = duplicate_page(&mut doc, 1).unwrap();
        assert_eq!(again.name, "A-3");
        // A name that fails the rule for names (a file may hold one) is not carried to the copy.
        let mut hostile = doc_of(&[(1, "bad\nname", false, None)]);
        let made = duplicate_page(&mut hostile, 1).unwrap();
        assert_eq!(made.name, "Page-2");
        assert!(check_page_name(&made.name).is_ok());
    }

    #[test]
    fn a_copy_leaves_the_place_its_source_had_in_its_package_and_the_markup_that_names_its_element() {
        let mut doc = doc_of(&[(1, "A", false, None)]);
        {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            page.rel_id = Some(Arc::from("rId3"));
            page.part_name = Some(Arc::from("/visio/pages/page1.xml"));
            let data = page.ext.make_mut();
            data.odf.kept.raw.push("xml:id", "page-one");
            data.odf.master_page = Some(Arc::from("Default"));
            data.vsdx.0.push((Arc::from("Rel"), Kept { raw: RawAttrs::from_pairs([("Extra", "1")]), kept: Default::default() }));
            data.vsdx.0.push((
                Arc::from("PageContents"),
                Kept { raw: RawAttrs::from_pairs([("xmlns:r", "urn:x"), ("xml:space", "preserve")]), kept: Default::default() },
            ));
        }
        let made = duplicate_page(&mut doc, 1).unwrap();
        let copy = doc.page(made.id).unwrap();
        assert!(copy.rel_id.is_none() && copy.part_name.is_none());
        let data = copy.ext.get().unwrap();
        assert!(data.odf.kept.raw.is_empty(), "the element's own markup (an id) is not the copy's");
        assert_eq!(data.odf.master_page.as_deref(), Some("Default"), "the master page it is on is");
        assert!(data.vsdx.0.iter().all(|(key, _)| &**key != "Rel"));
        let root = data.vsdx.0.iter().find(|(key, _)| &**key == "PageContents").unwrap();
        assert_eq!(root.1.raw.iter().collect::<Vec<_>>(), [("xmlns:r", "urn:x")], "the prefixes of the root stay, for what is kept below it");
        // The source is as it was.
        let source = doc.page(1).unwrap();
        assert_eq!(source.rel_id.as_deref(), Some("rId3"));
        assert!(source.ext.get().unwrap().vsdx.0.iter().any(|(key, _)| &**key == "Rel"));
    }

    #[test]
    fn the_ids_of_pages_run_out_in_words() {
        let mut doc = doc_of(&[(u32::MAX, "A", false, None)]);
        let err = duplicate_page(&mut doc, u32::MAX).unwrap_err();
        assert!(!err.bad_params, "{err:?}");
        assert_eq!(names(&doc), ["A"]);
        assert!(add_background_page(&mut doc, None).is_err());
        assert_eq!(names(&doc), ["A"], "nothing changed");
    }
}
