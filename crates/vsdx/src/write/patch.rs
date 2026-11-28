//! Patching a part's tree: the edit is made at the edited nodes and nothing else is touched.
//!
//! A part that an edit touched is parsed to the tree of `opc` (which keeps attribute order, prefixes and `xmlns` declarations, text,
//! comments and instructions), and this walks the tree with **two models: the one the part was read into, and the one the edit left**. A
//! node is changed only where the two differ:
//!
//! * an **attribute** is set or removed only when the two models give a different one (the lists of `build.rs`, which are the reader's
//!   inverse), so `V="1.50"` of a cell nobody wrote stays `1.50`, and an attribute the model has no field for stays as it is;
//! * a **cell, section, row, shape, page, master or style** is matched to its original by the very allocation (the model shares what an edit
//!   did not copy) and else by its key (an `ID`, a name, an `IX`), and an item the two models hold equal is not looked at again;
//! * an **item the edit added** is made from the model (`build.rs`) and put after the item it follows; an item it took away is taken out
//!   with the white space before it;
//! * **everything else in the tree stays where it is**: unknown elements and attributes, comments and instructions, text between elements,
//!   namespace declarations and the order of all of it. A node that is not one the reader modelled is a trailer of the item before it
//!   (`patch_list`), and goes with it.
//!
//! **What this does not do is said, not done wrong.** An edit to something the model keeps and this does not yet write (the unknown nodes
//! of a sheet, a shape's pictures, `Data1`) is `WriteError::Unsupported`, and nothing is written: the model and the file would otherwise
//! disagree after a save that said it worked. A part whose tree does not have the items the baseline model says (the file is not what the
//! model was read from) is `WriteError::Mismatch`.
//!
//! **The sheet's half is `diacraft-sheetxml`'s** (`patch.rs` there): the patch of the cells, sections and rows of a sheet, and the list
//! patcher (`patch_list`, `match_items`) that every list below uses. What is this crate's is the patch of what holds a sheet: shapes, pages,
//! masters and styles. An error of the sheet's half is a `WriteError` of the same words.
//!
//! The functions know which elements the reader modelled from the same rules it uses (`ns.rs`: an element is the reference app's in either the reference app
//! namespace or none; `sheet.rs`: a `Cell` with a name, once; and so on), so the item `k` of the model is the `k`th modelled element of the
//! tree.

use diacraft_doc::{Connect, Master, Page, Shape, Sheet, Style, Text};
use diacraft_opc::xml::{El, Node};
use diacraft_sheetxml::patch::{Items, after_last, at_end, is_ours, patch_list, same_unknown_nodes, sync_attrs};
pub(super) use diacraft_sheetxml::patch::{match_items, set_attr};

use super::WriteError;
use super::build::{
    connect_attrs, connect_el, master_attrs, master_row_el, page_attrs, page_row_el, shape_attrs, shape_el, style_attrs, style_el, text_children,
    text_el, with_prefix,
};
use crate::ns::{Scope, VsdxVocabulary};
use crate::passthrough::{is_blank, read_u32};

type R<T> = Result<T, WriteError>;

impl From<diacraft_sheetxml::PatchError> for WriteError {
    fn from(e: diacraft_sheetxml::PatchError) -> WriteError {
        match e {
            diacraft_sheetxml::PatchError::Unsupported(what) => WriteError::Unsupported(what),
            diacraft_sheetxml::PatchError::Mismatch(what) => WriteError::Mismatch(what),
        }
    }
}

fn unsupported_unless(same: bool, what: &'static str) -> R<()> {
    if same { Ok(()) } else { Err(WriteError::Unsupported(what)) }
}

/// The cells and sections of a sheet, among the children of `el` (a `Shape`, a `StyleSheet`, a `PageSheet`, a `DocumentSheet`).
pub(super) fn patch_sheet_children(el: &mut El, sc: &Scope, base: &Sheet, now: &Sheet) -> R<()> {
    diacraft_sheetxml::patch::patch_sheet_children(el, sc, base, now)
}

/// An element that is only a sheet (`PageSheet`, `DocumentSheet`), found among the children of `parent` by its name, made if the new sheet has
/// anything and the old one had no element, and left alone if neither holds anything. `before` names the child a new one is put before.
pub(super) fn patch_sheet_only(parent: &mut El, sc: &Scope, name: &str, before: &[&str], base: &Sheet, now: &Sheet) -> R<()> {
    diacraft_sheetxml::patch::patch_sheet_only(parent, sc, name, before, base, now)
}

// ----------------------------------------------------------------------------------------------------------------- shapes

/// Patches the `Shapes` element `el` (inside which `sc` is the scope) from the `base` list of shapes to the `now` one.
pub(super) fn patch_shapes(el: &mut El, sc: &Scope, base: &diacraft_doc::ShapeList, now: &diacraft_doc::ShapeList) -> R<()> {
    let b: Vec<&Shape> = base.iter().map(|s| &**s).collect();
    let n: Vec<&Shape> = now.iter().map(|s| &**s).collect();
    // A shape is told by its `ID`, its `Type` and its master together (of the M3 review): a shape that has the `ID` of one that was
    // taken out, but is another thing, is not that shape edited; it is one removed and one added, so that the writer does not have to turn a
    // picture or a `Data1` into something else, which it cannot write.
    type ShapeKey = (u32, Option<String>, Option<u32>);
    let kind: Items<'_, VsdxVocabulary, Shape, ShapeKey> = Items {
        modelled: &|c: &El, csc: &Scope| is_ours(c, csc, "Shape"),
        key: &|s: &Shape| Some((s.id, s.kind.text().map(str::to_owned), s.master)),
        first_at: &at_end,
        // A shape with no readable `ID` was read as ID 0.
        is_it: &|e: &El, s: &Shape| e.attr("ID").map_or(0, |v| read_u32(v).0.unwrap_or(0)) == s.id,
    };
    let prefix = el.prefix().to_owned();
    patch_list(el, sc, &kind, &b, &n, &mut |e, esc, b, n| patch_shape(e, esc, b, n), &mut |s| {
        refuse_a_new_picture(s)?;
        let mut made = shape_el(s, 0)?;
        with_prefix(&mut made, &prefix);
        Ok(made)
    })
}

/// A shape the edit added, with the shapes inside it, that holds a picture no relationship of the page names (a picture inserted from a file): this writer makes the shape's own element, and
/// the part for the picture, its relationship and its content type are not made here for a page the file already has, so the picture would be lost without a word. **It is refused instead**
/// The model writer (a `.diacraft` drawing, a new drawing) writes it. **Every place that makes the element of a shape new to a page part the file has asks this** (a page
/// part with no `Shapes` element, a blank page, made its first shapes without asking, and a picture inserted on it was saved as a `ForeignData` with no `Rel`, lost with no word).
pub(super) fn refuse_a_new_picture(shape: &Shape) -> R<()> {
    let mut stack: Vec<&Shape> = vec![shape];
    while let Some(s) = stack.pop() {
        if s.foreign.as_ref().is_some_and(|f| f.picture.is_some() && f.rel_id.is_none()) {
            return Err(WriteError::Unsupported(
                "a picture new to a page the file already has (save the drawing as a .diacraft drawing, which holds it; a new .vsdx drawing does too)",
            ));
        }
        stack.extend(s.children.iter().map(|c| &**c));
    }
    Ok(())
}

fn patch_shape(el: &mut El, sc: &Scope, base: &Shape, now: &Shape) -> R<()> {
    sync_attrs(el, &shape_attrs(base), &shape_attrs(now));
    patch_sheet_children(el, sc, &base.sheet, &now.sheet)?;
    unsupported_unless(same_unknown_nodes(&base.passthrough, &now.passthrough), "a change to the unknown elements of a shape")?;
    unsupported_unless(
        base.data1 == now.data1 && base.data2 == now.data2 && base.data3 == now.data3,
        "a change to Data1, Data2 or Data3 of a shape",
    )?;
    unsupported_unless(shows_the_same(base.foreign.as_deref(), now.foreign.as_deref()), "a change to the picture of a shape")?;
    if base.text != now.text {
        patch_text(el, sc, now.text.as_deref())?;
    }
    // The members of a group.
    let found = el.children.iter().position(|n| matches!(n, Node::El(e) if is_ours(e, &sc.enter(e), "Shapes")));
    match found {
        Some(at) => {
            if let Some(Node::El(shapes)) = el.children.get_mut(at) {
                let ssc = sc.enter(shapes);
                patch_shapes(shapes, &ssc, &base.children, &now.children)?;
            }
        }
        None => {
            unsupported_unless(base.children.is_empty(), "a shape that lacks the members the model was read from")?;
            if !now.children.is_empty() {
                for member in now.children.iter() {
                    refuse_a_new_picture(member)?;
                }
                let mut made = super::build::shapes_el(&now.children, 1)?;
                with_prefix(&mut made, el.prefix().to_owned().as_str());
                el.children.push(Node::El(made));
            }
        }
    }
    Ok(())
}

/// **Whether the drawing's `ForeignData` shows what the file's does**, so that the file's element stands as it is: the two are the same, or they hold the same text, unknown nodes and
/// data of other formats, and either
///
/// * **show one picture**, where the file's names a relationship: the same bytes, of the same type, with the same attributes once a writer gives a picture of its type its `CompressionType`
///   (`model::with_compression`). **The relationship and the part's name are the file's**, as a page's `Rel` is (`patch_pages`): a picture the drawing holds by its bytes (one inserted from a file,
///   or a new drawing's) names none, and one the drawing read from an earlier file names that file's, while the file a model writer made gives each the relationship and the part it numbers, and the
///   `CompressionType`;
/// * or are **one embedded object**: the same attributes and the same relationship, which names the object, and a fallback picture of the same bytes and type, whatever the name of its part (the
///   `.odg` a VSDX object crossed holds its picture as the object's replacement, and the file the model writer made from that drawing as a picture of the package's).
///
/// A picture of other bytes or of another type, an object that names another relationship, and a `ForeignData` that shows no picture and is not the same are a change, which this writer does not make.
fn shows_the_same(base: Option<&diacraft_doc::ForeignData>, now: Option<&diacraft_doc::ForeignData>) -> bool {
    let (base, now) = match (base, now) {
        (None, None) => return true,
        (Some(base), Some(now)) => (base, now),
        _ => return false,
    };
    if base == now {
        return true;
    }
    let (Some(shown), Some(held), Some(file_rel)) = (&now.picture, &base.picture, &base.rel_id) else { return false };
    let kind = super::model::picture_content_type(shown);
    let same_rest = shown.bytes == held.bytes
        && kind == super::model::picture_content_type(held)
        && now.text == base.text
        && now.passthrough == base.passthrough
        && now.ext == base.ext;
    if matches!(now.attr("ForeignType"), Some("Object" | "Ink")) {
        return same_rest && now.rel_id.as_ref() == Some(file_rel) && now.attrs == base.attrs;
    }
    same_rest && super::model::with_compression(&now.attrs, &kind) == base.attrs
}

/// A shape's `Text`: its content replaced when the text changed, the element made when the shape had none, taken out when the shape has none now.
fn patch_text(el: &mut El, sc: &Scope, now: Option<&Text>) -> R<()> {
    let found = el.children.iter().position(|n| matches!(n, Node::El(e) if is_ours(e, &sc.enter(e), "Text")));
    match (found, now) {
        (Some(at), Some(t)) => {
            if let Some(Node::El(e)) = el.children.get_mut(at) {
                e.children = text_children(t);
            }
        }
        (Some(at), None) => {
            el.children.remove(at);
            if at > 0 && matches!(el.children.get(at - 1), Some(Node::Text(t)) if is_blank(t)) {
                el.children.remove(at - 1);
            }
        }
        (None, Some(t)) => {
            let mut made = text_el(t);
            with_prefix(&mut made, el.prefix().to_owned().as_str());
            let at = after_last(&el.children, &["Cell", "Section"]);
            el.children.insert(at, Node::El(made));
        }
        (None, None) => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------- connects

/// Patches a `Connects` element: rows are matched by what they say, so a changed row is one taken out and one put in.
pub(super) fn patch_connects(el: &mut El, sc: &Scope, base: &diacraft_doc::Connects, now: &diacraft_doc::Connects) -> R<()> {
    let b: Vec<&Connect> = base.iter().collect();
    let n: Vec<&Connect> = now.iter().collect();
    type Key = (u32, u32, Option<u32>, Option<u32>, Option<String>, Option<String>);
    let kind: Items<'_, VsdxVocabulary, Connect, Key> = Items {
        modelled: &|c: &El, csc: &Scope| {
            // A row with no readable `FromSheet` and `ToSheet` was skipped by the reader and is not an item.
            is_ours(c, csc, "Connect")
                && c.attr("FromSheet").is_some_and(|v| read_u32(v).0.is_some())
                && c.attr("ToSheet").is_some_and(|v| read_u32(v).0.is_some())
        },
        key: &|c: &Connect| {
            Some((
                c.from_sheet,
                c.to_sheet,
                c.from_part,
                c.to_part,
                c.from_cell.as_deref().map(str::to_owned),
                c.to_cell.as_deref().map(str::to_owned),
            ))
        },
        first_at: &at_end,
        is_it: &|e: &El, c: &Connect| e.attr("FromSheet").is_some_and(|v| read_u32(v).0 == Some(c.from_sheet)),
    };
    let prefix = el.prefix().to_owned();
    patch_list(
        el,
        sc,
        &kind,
        &b,
        &n,
        &mut |e, _, b, n| {
            sync_attrs(e, &connect_attrs(b), &connect_attrs(n));
            Ok(())
        },
        &mut |c| {
            let mut made = connect_el(c);
            with_prefix(&mut made, &prefix);
            Ok(made)
        },
    )
}

// -------------------------------------------------------------------------------------------------------------------- rows

/// Whether `el` is a `Page` row the reader modelled: it has an `ID` that is a number.
fn has_id(el: &El) -> bool {
    el.attr("ID").is_some_and(|v| read_u32(v).0.is_some())
}

/// Patches the `Pages` element from the pages of the baseline model to the pages of the new one. `new_rel` gives the `r:id` of the `Rel` of a
/// page the edit added (or of a page that had no part and has one now), and `None` for a page with no part. A page's `Rel` is the writer's: the
/// `rel_id` of a page is not compared.
pub(super) fn patch_pages(el: &mut El, sc: &Scope, base: &[&Page], now: &[&Page], new_rel: &mut dyn FnMut(&Page, bool) -> Option<String>) -> R<()> {
    let kind = Items {
        modelled: &|c: &El, csc: &Scope| is_ours(c, csc, "Page") && has_id(c),
        key: &|p: &Page| Some(p.id),
        first_at: &at_end,
        is_it: &|e: &El, p: &Page| e.attr("ID").is_some_and(|v| read_u32(v).0 == Some(p.id)),
    };
    let prefix = el.prefix().to_owned();
    // A page is patched when it differs but for `rel_id`; the patcher and the maker are both given the page.
    let new_rel = std::cell::RefCell::new(new_rel);
    patch_list(el, sc, &kind, base, now, &mut |e, esc, b, n| patch_page_row(e, esc, b, n, &mut |p| (new_rel.borrow_mut())(p, false)), &mut |p| {
        let rel = (new_rel.borrow_mut())(p, true);
        let mut made = page_row_el(p, rel.as_deref())?;
        with_prefix(&mut made, &prefix);
        Ok(made)
    })
}

/// Gives the `Page` rows of the `Pages` element at the places given (the `k`th page the reader modelled) a `Rel` each: `wanted` is the place and
/// the `r:id`. A row that has a `Rel` already is left as it is. A place the element does not have is `Mismatch`: the part is not the file the model
/// was read from.
pub(super) fn add_page_rels(el: &mut El, sc: &Scope, wanted: &[(usize, String)]) -> R<()> {
    let mut place = 0usize;
    let mut reached = 0usize;
    for node in &mut el.children {
        let Node::El(child) = node else { continue };
        let csc = sc.enter(child);
        if !(is_ours(child, &csc, "Page") && has_id(child)) {
            continue;
        }
        if let Some((_, id)) = wanted.iter().find(|(at, _)| *at == place) {
            reached += 1;
            if !child.children.iter().any(|n| matches!(n, Node::El(e) if e.is("Rel"))) {
                let mut made = super::build::rel_el(id);
                with_prefix(&mut made, child.prefix().to_owned().as_str());
                child.children.push(Node::El(made));
            }
        }
        place += 1;
    }
    if reached == wanted.len() { Ok(()) } else { Err(WriteError::Mismatch("a part does not hold the items the model was read from")) }
}

fn patch_page_row(el: &mut El, sc: &Scope, base: &Page, now: &Page, new_rel: &mut dyn FnMut(&Page) -> Option<String>) -> R<()> {
    sync_attrs(el, &page_attrs(base), &page_attrs(now));
    unsupported_unless(same_unknown_nodes(&base.passthrough, &now.passthrough), "a change to the unknown elements of a page row")?;
    patch_sheet_only(el, sc, "PageSheet", &["Rel"], &base.sheet, &now.sheet)?;
    // A page that had no part and has one now gets its `Rel`.
    if base.rel_id.is_none()
        && let Some(id) = new_rel(now)
        && !el.children.iter().any(|n| matches!(n, Node::El(e) if e.is("Rel")))
    {
        let mut made = super::build::rel_el(&id);
        with_prefix(&mut made, el.prefix().to_owned().as_str());
        el.children.push(Node::El(made));
    }
    Ok(())
}

/// The same for the `Masters` element and the masters.
pub(super) fn patch_masters(
    el: &mut El,
    sc: &Scope,
    base: &[&Master],
    now: &[&Master],
    new_rel: &mut dyn FnMut(&Master, bool) -> Option<String>,
) -> R<()> {
    let kind = Items {
        modelled: &|c: &El, csc: &Scope| is_ours(c, csc, "Master") && has_id(c),
        key: &|m: &Master| Some(m.id),
        first_at: &|lead: &[Node]| after_last(lead, &["Master"]),
        is_it: &|e: &El, m: &Master| e.attr("ID").is_some_and(|v| read_u32(v).0 == Some(m.id)),
    };
    let prefix = el.prefix().to_owned();
    let new_rel = std::cell::RefCell::new(new_rel);
    patch_list(el, sc, &kind, base, now, &mut |e, esc, b, n| patch_master_row(e, esc, b, n, &mut |m| (new_rel.borrow_mut())(m, false)), &mut |m| {
        let rel = (new_rel.borrow_mut())(m, true);
        let mut made = master_row_el(m, rel.as_deref())?;
        with_prefix(&mut made, &prefix);
        Ok(made)
    })
}

fn patch_master_row(el: &mut El, sc: &Scope, base: &Master, now: &Master, new_rel: &mut dyn FnMut(&Master) -> Option<String>) -> R<()> {
    sync_attrs(el, &master_attrs(base), &master_attrs(now));
    unsupported_unless(same_unknown_nodes(&base.passthrough, &now.passthrough), "a change to the unknown elements of a master row")?;
    unsupported_unless(base.icon == now.icon, "a change to the icon of a master")?;
    patch_sheet_only(el, sc, "PageSheet", &["Icon", "Rel"], &base.sheet, &now.sheet)?;
    if base.rel_id.is_none()
        && let Some(id) = new_rel(now)
        && !el.children.iter().any(|n| matches!(n, Node::El(e) if e.is("Rel")))
    {
        let mut made = super::build::rel_el(&id);
        with_prefix(&mut made, el.prefix().to_owned().as_str());
        el.children.push(Node::El(made));
    }
    Ok(())
}

/// The `StyleSheets` element, from the baseline styles to the new ones.
pub(super) fn patch_styles(el: &mut El, sc: &Scope, base: &[&Style], now: &[&Style]) -> R<()> {
    let kind = Items {
        modelled: &|c: &El, csc: &Scope| is_ours(c, csc, "StyleSheet") && c.attr("ID").is_some_and(|v| read_u32(v).0.is_some()),
        key: &|s: &Style| Some(s.id),
        first_at: &at_end,
        is_it: &|e: &El, s: &Style| e.attr("ID").is_some_and(|v| read_u32(v).0 == Some(s.id)),
    };
    let prefix = el.prefix().to_owned();
    patch_list(el, sc, &kind, base, now, &mut |e, esc, b, n| patch_style(e, esc, b, n), &mut |s| {
        let mut made = style_el(s)?;
        with_prefix(&mut made, &prefix);
        Ok(made)
    })
}

fn patch_style(el: &mut El, sc: &Scope, base: &Style, now: &Style) -> R<()> {
    sync_attrs(el, &style_attrs(base), &style_attrs(now));
    unsupported_unless(same_unknown_nodes(&base.passthrough, &now.passthrough), "a change to the unknown elements of a style")?;
    patch_sheet_children(el, sc, &base.sheet, &now.sheet)
}
