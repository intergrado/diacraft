//! The package data of DiaCraft's own skeleton, which an `.odg` does not carry.
//!
//! **A new drawing started from a built-in template holds data of a VSDX package** (`template::build` writes the template as the bytes of a `.vstx` and reads them back, so that one path
//! starts a drawing from a built-in template and from a template file: the model holds the package's own rows, the properties' elements, the solutions part, the attributes of the
//! document's root elements and of its settings, and the relationship, part name and attributes of its page, tagged VSDX). None of it is a person's: it is what DiaCraft's skeleton gives
//! every document it writes. The `.odg` model writer, which refuses in words the package data of a VSDX file it cannot say (an `.odg` carries it in DiaCraft's own folder,
//! which is not built), would refuse every drawing started from a template.
//!
//! So a drawing whose VSDX package data **is exactly what a built-in template holds** (its rows, its parts, the elements it has no slot for: the window settings and the Snap & Glue
//! words are the drawing's own and are compared with nothing) is given to the writer without it ([`without_skeleton`]), and so is a page's VSDX data that is the template's page's.
//! Anything else of VSDX stays, and the writer refuses it in its words: a drawing that came from a person's VSDX file is not an `.odg` until saving across formats is written
//! The model of the session is never changed: the writer is handed a copy.

use std::sync::{Arc, OnceLock};

use diacraft_doc::{Document, Passthrough, RawAttrs, RawElement, VsdxHeld, VsdxRest};

use crate::template;

/// What a built-in template's page holds of its VSDX part: the relationship that names it, its name in the package, and its element's attributes and unknown children.
struct PageData {
    id: u32,
    rel_id: Option<Arc<str>>,
    part_name: Option<Arc<str>>,
    raw: RawAttrs,
    passthrough: Passthrough,
}

/// What a built-in template's model holds of a VSDX package: its rows, its parts and the elements of the document it has no slot for.
struct Skeleton {
    rels: Vec<diacraft_doc::Rel>,
    content_types: Vec<diacraft_doc::ContentTypeRow>,
    props_rest: Vec<(Arc<str>, diacraft_doc::Kept)>,
    windows: Option<Arc<RawElement>>,
    parts: Vec<Arc<diacraft_doc::Part>>,
    rest: VsdxRest,
    /// The document's unknown elements but the Snap & Glue words (`DocumentSettings` holds the skeleton's own attributes beside them).
    unknown: Passthrough,
    pages: Vec<PageData>,
}

impl Skeleton {
    fn of(doc: &Document) -> Skeleton {
        let held = doc.vsdx.as_deref().cloned().unwrap_or_default();
        Skeleton {
            rels: held.rels,
            content_types: held.content_types,
            props_rest: held.props_rest,
            windows: held.windows,
            parts: doc.parts.iter().cloned().collect(),
            rest: doc.ext.get().map(|data| data.vsdx.clone()).unwrap_or_default(),
            unknown: doc.passthrough_but_snap_words(),
            pages: doc
                .pages
                .iter()
                .map(|page| PageData {
                    id: page.id,
                    rel_id: page.rel_id.clone(),
                    part_name: page.part_name.clone(),
                    raw: page.raw.clone(),
                    passthrough: page.passthrough.clone(),
                })
                .collect(),
        }
    }

    /// Whether `doc` holds this package data and no more: the rows, the parts and the document's elements, **not the window settings**, which a person may have turned.
    fn is_all_that(&self, doc: &Skeleton) -> bool {
        self.rels == doc.rels
            && self.content_types == doc.content_types
            && self.props_rest == doc.props_rest
            && self.parts == doc.parts
            && self.rest == doc.rest
            && self.unknown == doc.unknown
    }
}

/// The built-in templates' skeletons, made once: what a drawing started from each holds (`template::build`).
fn skeletons() -> &'static [Skeleton] {
    static SKELETONS: OnceLock<Vec<Skeleton>> = OnceLock::new();
    SKELETONS.get_or_init(|| {
        template::templates()
            .iter()
            .filter_map(|spec| template::build("file.save_as", spec).ok())
            .map(|built| Skeleton::of(&built.document))
            .collect()
    })
}

/// `doc` without the package data of a VSDX skeleton, when it holds exactly a built-in template's and nothing of a person's: `None` for a model that holds no such data and for one
/// that holds anything else (which the writer refuses in words). The window settings the drawing holds stay when they are not the template's own (a person turned one: the writer
/// writes them, or refuses them, as it does for any drawing).
pub(crate) fn without_skeleton(doc: &Document) -> Option<Document> {
    let own = Skeleton::of(doc);
    let holds_package = !(own.rels.is_empty()
        && own.content_types.is_empty()
        && own.props_rest.is_empty()
        && own.parts.is_empty()
        && own.rest.0.is_empty()
        && own.unknown.is_empty());
    if !holds_package {
        return None;
    }
    let skeleton = skeletons().iter().find(|skeleton| skeleton.is_all_that(&own))?;
    let mut copy = doc.clone();
    // The Snap & Glue words are the drawing's own and an `.odg` says them: they stay, in an element of their own.
    let words = doc.snap_words();
    copy.passthrough = Passthrough::default();
    copy.set_snap_words(&words);
    copy.parts = diacraft_doc::SharedVec::from_vec(Vec::new());
    let turned = own.windows.filter(|windows| skeleton.windows.as_ref() != Some(windows));
    copy.vsdx = turned.map(|windows| Arc::new(VsdxHeld { windows: Some(windows), ..VsdxHeld::default() }));
    if copy.ext.get().is_some() {
        copy.ext.make_mut().vsdx = VsdxRest::default();
        copy.ext.tidy();
    }
    // A page of the template that holds the template's page data and nothing more gives it up: the relationship that names its part in a package, the part's name, its element's attributes.
    for data in &skeleton.pages {
        let Some(at) = copy.pages.iter().position(|page| page.id == data.id) else { continue };
        let Some(page) = copy.pages.get_mut(at) else { continue };
        if page.rel_id == data.rel_id && page.part_name == data.part_name && page.raw == data.raw && page.passthrough == data.passthrough {
            let page = Arc::make_mut(page);
            page.rel_id = None;
            page.part_name = None;
            page.raw = RawAttrs::default();
            page.passthrough = Passthrough::default();
        }
    }
    Some(copy)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A drawing started from the built-in template `id`, as `file.new` makes it.
    fn started_from(id: &str) -> Document {
        let mut s = crate::Session::new();
        s.execute("file.new", &serde_json::json!({ "template": id })).unwrap();
        s.document().unwrap().clone()
    }

    fn holds_package_data(doc: &Document) -> bool {
        let own = Skeleton::of(doc);
        !(own.rels.is_empty()
            && own.content_types.is_empty()
            && own.props_rest.is_empty()
            && own.parts.is_empty()
            && own.rest.0.is_empty()
            && own.unknown.is_empty())
    }

    /// A model that holds no package data has nothing to leave out, and a built-in template's drawing holds exactly the skeleton's: the copy holds none of it and keeps the page, the
    /// styles and the theme.
    #[test]
    fn a_built_in_templates_data_is_left_out_and_a_drawing_with_none_has_nothing_to_leave_out() {
        assert!(without_skeleton(&Document::new_blank()).is_none(), "a blank drawing holds none");
        for spec in template::templates() {
            let doc = started_from(spec.id);
            assert!(holds_package_data(&doc), "{}: a template's drawing holds the skeleton's data", spec.id);
            let bare = without_skeleton(&doc).unwrap_or_else(|| panic!("{}: the skeleton's data is left out", spec.id));
            assert!(!holds_package_data(&bare), "{}", spec.id);
            assert!(bare.vsdx.is_none() && bare.parts.is_empty(), "{}", spec.id);
            assert_eq!(bare.pages.len(), doc.pages.len(), "{}", spec.id);
            assert_eq!(bare.styles.len(), doc.styles.len(), "{}", spec.id);
            assert_eq!(bare.themes.len(), doc.themes.len(), "{}", spec.id);
            assert!(bare.pages.iter().all(|p| p.rel_id.is_none() && p.part_name.is_none()), "{}: no page names a part of a package", spec.id);
            assert_eq!(bare.snap_words(), doc.snap_words(), "{}: the Snap & Glue words stay", spec.id);
        }
    }

    /// Anything of VSDX that is not exactly a built-in template's stays, whichever part of it: an extra row, an extra part, an extra element of the document's, or a row that is not the skeleton's.
    #[test]
    fn data_that_is_not_exactly_a_templates_is_not_left_out() {
        let doc = started_from("basic_diagram");
        let held = doc.vsdx.as_deref().cloned().unwrap();
        let mut extra_row = doc.clone();
        let mut more = held.clone();
        more.rels.push(
            held.rels
                .first()
                .cloned()
                .map(|mut r| {
                    r.id = Arc::from("rId99");
                    r
                })
                .unwrap(),
        );
        extra_row.vsdx = Some(Arc::new(more));
        assert!(without_skeleton(&extra_row).is_none(), "one row more");
        let mut extra_part = doc.clone();
        let mut parts: Vec<_> = doc.parts.iter().cloned().collect();
        parts.push(Arc::new(diacraft_doc::Part { name: Arc::from("/foo/bar.bin"), ..(**parts.first().unwrap()).clone() }));
        extra_part.parts = diacraft_doc::SharedVec::from_vec(parts);
        assert!(without_skeleton(&extra_part).is_none(), "one part more");
        let mut fewer = doc.clone();
        let mut less = held;
        less.rels.pop();
        fewer.vsdx = Some(Arc::new(less));
        assert!(without_skeleton(&fewer).is_none(), "one row fewer");
    }
}
