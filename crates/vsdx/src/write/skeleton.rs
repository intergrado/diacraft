//! The package of a new drawing, before its pages: the starting point a save of a new drawing patches.
//!
//! A drawing that was never saved has no file to patch, and a saved one is patched from its file. So that there is one way to write and not
//! two, a new drawing is written by patching **this**: the eleven parts V15 lists, with no page and no master, a document part that holds what
//! R54 asks (`xml:space="preserve"` on the root, `DocumentSettings`, `FaceNames`, `StyleSheets`, `DocumentSheet`), and the property parts
//! already saying the drawing's title, its pages and its writer. The document is read from these bytes as the baseline model, and the save
//! then brings it to the document: the styles, the sheet, the pages and the masters are added exactly as they are added to a file that was
//! read.
//!
//! **The window.** A new drawing's `windows.xml` holds one drawing `Window` for the first page, with `ShowRulers` and `ShowGrid` on
//! So that it reopens with the grid
//! and the rulers showing. A drawing whose window settings nothing set holds no windows part, so it takes the new drawing's, on. A file that was
//! read keeps its windows part byte for byte but for the flags a save sets in it (`sync_windows` of the splice): this module is only the starting point of
//! a drawing that has no file. **A drawing of no file whose window settings were set** holds a windows part of its own (the model holds the
//! settings as this part holds them, made from [`new_windows_part`] and then set), and the model writer writes it (`held.rs`), with that part.
//!
//! **The font table.** `FaceNames` holds the model's own table (`Document::fonts`) as it is, then a row for every other name that a `Font`,
//! `AsianFont`, `ComplexScriptFont` or `BulletFont` cell of a style or a shape names (a cell names a face by a `NameU` of the table),
//! and, when that leaves it empty, one default face, since MS-VSDX asks for at least one row and R54 asks for the element.
//!
//! What the choices here are, none of them settled by a spec and all of them listed in the report as assumed about the reference app: the content of
//! `DocumentSettings` (every default style is the root style, 0), a client area of 1024 by 768, which of the `Window` attributes and
//! children a new drawing holds, and the default face.

use std::collections::BTreeSet;
use std::sync::Arc;

use diacraft_doc::{Document, FaceName, RawAttrs, Reading, Sheet};
use diacraft_opc::PackageWriter;
use diacraft_opc::xml::{El, Node};

use super::build::{element, fonts_el};
use super::parts::{CT_RELS, CT_XML, OD, PK, RelsTree, Tree, VIS, rel_type};
use super::props::{new_app, new_core, new_custom};
use super::types::TypesTree;
use super::{WriteError, WriteOptions};
use crate::ns::{NS_2012, NS_REL};

const PART_DOCUMENT: &str = "/visio/document.xml"; // brand-ok: a part name the format defines
const PART_DOCUMENT_RELS: &str = "/visio/_rels/document.xml.rels"; // brand-ok: a part name the format defines
const PART_PAGES: &str = "/visio/pages/pages.xml"; // brand-ok: a part name the format defines
const PART_PAGES_RELS: &str = "/visio/pages/_rels/pages.xml.rels"; // brand-ok: a part name the format defines
const PART_WINDOWS: &str = "/visio/windows.xml"; // brand-ok: a part name the format defines
const CT_DOCUMENT: &str = "application/vnd.ms-visio.drawing.main+xml"; // brand-ok: a content type the format defines
const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml"; // brand-ok: a content type the format defines
const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml"; // brand-ok: a content type the format defines
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_CUSTOM: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";

/// The face a new drawing's font table holds when the drawing names none: the table must have a row, and the model's
/// root style has no `Font`, so this row is listed and used by nothing.
const DEFAULT_FACE: &str = "Arial";
/// The most faces beyond the model's own table that the cells of a drawing add. A name past this is left off the table, which a cell may do
/// (a `Font` that matches no `NameU` is kept, and the lookup falls back).
const MAX_FACES_ADDED: usize = 1_024;
/// The cells that name a face, and the sections they sit in.
const FACE_CELLS: [&str; 4] = ["Font", "AsianFont", "ComplexScriptFont", "BulletFont"];
const FACE_SECTIONS: [&str; 2] = ["Character", "Paragraph"];

/// The root of a VSDX part: `<name xmlns=".." xmlns:r=".." xml:space="preserve">`.
pub(super) fn part_root(name: &str, children: Vec<diacraft_opc::xml::Node>) -> diacraft_opc::xml::El {
    element(
        name,
        vec![("xmlns".to_owned(), NS_2012.to_owned()), ("xmlns:r".to_owned(), NS_REL.to_owned()), ("xml:space".to_owned(), "preserve".to_owned())],
        children,
    )
}

fn node(name: &str, attrs: &[(&str, &str)]) -> diacraft_opc::xml::Node {
    diacraft_opc::xml::Node::El(element(name, attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), vec![]))
}

fn text_node(name: &str, text: &str) -> Node {
    Node::El(element(name, Vec::new(), vec![Node::Text(text.to_owned())]))
}

/// The drawing `Window` of a new drawing, for its first page; `None` for a drawing with no page, whose `Windows` is then empty.
///
/// The window opens the page (`ContainerType` `Page`, `Page`) whole (`ViewScale` -1) around the page's middle (`PageWidth` and
/// `PageHeight` are in drawing units), and shows the rulers and the grid. **The page breaks** show (`ShowPageBreaks` 1, after the grid in the
/// order) when DiaCraft's own data says the drawing's Page Breaks box is on (a drawing of no VSDX file keeps the box in that data,
/// and its VSDX file says it where the format has a place for it); a box that is off writes nothing, which a reader takes as off. The other attributes and
/// children that R27 lists are left out: no spec gives a value for them, and one written here would stand for a choice nobody made.
pub(super) fn drawing_window(doc: &Document) -> Option<Node> {
    let page = doc.pages.iter().next()?;
    let half = |name: &str| match page.sheet.cells.get(name)?.value.as_ref()?.reading() {
        Reading::Number(n) => Some(n / 2.0),
        _ => None,
    };
    let mut attrs: Vec<(String, String)> =
        [("ID", "0"), ("WindowType", "Drawing"), ("ContainerType", "Page")].iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    attrs.push(("Page".to_owned(), page.id.to_string()));
    attrs.push(("ViewScale".to_owned(), "-1".to_owned()));
    if let (Some(x), Some(y)) = (half("PageWidth"), half("PageHeight")) {
        attrs.push(("ViewCenterX".to_owned(), x.to_string()));
        attrs.push(("ViewCenterY".to_owned(), y.to_string()));
    }
    let mut flags = vec![text_node("ShowRulers", "1"), text_node("ShowGrid", "1")];
    if doc.page_breaks_in_data() == Some(true) {
        flags.push(text_node("ShowPageBreaks", "1"));
    }
    Some(Node::El(element("Window", attrs, flags)))
}

/// The `FaceNames` of a new drawing: the model's table, then the faces its cells name that the table lacks (in name order), and the default
/// face if that is nothing.
pub(super) fn face_names(doc: &Document) -> El {
    let mut table: Vec<FaceName> = doc.fonts.iter().cloned().collect();
    let have: BTreeSet<&str> = doc.fonts.iter().map(|f| f.name_u.as_ref()).collect();
    let used = faces_used(doc);
    table.extend(used.iter().filter(|name| !have.contains(name.as_str())).map(|name| FaceName {
        name_u: Arc::from(name.as_str()),
        raw: RawAttrs::new(),
        ext: Default::default(),
    }));
    if table.is_empty() {
        table.push(FaceName { name_u: Arc::from(DEFAULT_FACE), raw: RawAttrs::new(), ext: Default::default() });
    }
    fonts_el(&table)
}

/// The names of the faces that cells of styles and shapes (of pages and masters, members of groups too) hold: a text that is a name, so not a
/// number such as `0`, not empty and not the theme marker. At most [`MAX_FACES_ADDED`].
fn faces_used(doc: &Document) -> BTreeSet<String> {
    fn scan(sheet: &Sheet, found: &mut BTreeSet<String>) {
        for section in sheet.sections.iter().filter(|s| FACE_SECTIONS.iter().any(|n| s.name == *n)) {
            for row in section.rows.iter() {
                for cell in FACE_CELLS {
                    let Some(value) = row.cells.get(cell).and_then(|c| c.value.as_ref()) else { continue };
                    if matches!(value.reading(), Reading::Text) && found.len() < MAX_FACES_ADDED {
                        found.insert(value.text().to_owned());
                    }
                }
            }
        }
    }
    let mut found = BTreeSet::new();
    for style in doc.styles.iter() {
        scan(&style.sheet, &mut found);
    }
    // The shapes, by a stack of lists and not by recursion: a group nests as deep as a hostile file made it.
    let mut lists = Vec::new();
    lists.extend(doc.pages.iter().map(|p| &p.shapes));
    lists.extend(doc.masters.iter().map(|m| &m.shapes));
    while let Some(list) = lists.pop() {
        for shape in list.iter() {
            scan(&shape.sheet, &mut found);
            lists.push(&shape.children);
        }
    }
    found
}

fn rels(name: &str, rows: &[(&str, String, &str)]) -> RelsTree {
    let mut tree = RelsTree::empty(name);
    for (id, ty, target) in rows {
        tree.add(id, ty, target);
    }
    tree
}

/// The `DocumentSettings` of a new drawing: the attributes are the skeleton's; the children are the model's Snap & Glue words, when it has any (`settings.rs`).
pub(super) fn default_settings(doc: &Document) -> El {
    let first_page = doc.pages.iter().next().map_or(0, |p| p.id);
    let settings_attrs = [
        ("TopPage".to_owned(), first_page.to_string()),
        ("DefaultTextStyle".to_owned(), "0".to_owned()),
        ("DefaultLineStyle".to_owned(), "0".to_owned()),
        ("DefaultFillStyle".to_owned(), "0".to_owned()),
        ("DefaultGuideStyle".to_owned(), "0".to_owned()),
    ];
    element("DocumentSettings", settings_attrs.to_vec(), super::settings::words_el(&doc.snap_words()))
}

/// The windows part of a new drawing, as the model holds one ([`diacraft_doc::VsdxHeld::windows`]): what [`windows_root`] makes, for a command that sets the window settings
/// of a drawing that holds no windows part yet. Its flags are the ones a new drawing shows (the rulers and the grid on, and no guide or connection point
/// flag, which a reader takes as on and off), so making it changes nothing that is read of the drawing.
pub fn new_windows_part(doc: &Document) -> diacraft_doc::RawElement {
    diacraft_sheetxml::passthrough::raw_element(&windows_root(doc))
}

/// The `Windows` of a new drawing: one drawing window for its first page.
pub(super) fn windows_root(doc: &Document) -> El {
    element(
        "Windows",
        vec![
            ("xmlns".to_owned(), NS_2012.to_owned()),
            ("xmlns:r".to_owned(), NS_REL.to_owned()),
            ("xml:space".to_owned(), "preserve".to_owned()),
            ("ClientWidth".to_owned(), "1024".to_owned()),
            ("ClientHeight".to_owned(), "768".to_owned()),
        ],
        drawing_window(doc).into_iter().collect(),
    )
}

/// The bytes of the package of a new drawing with no page, for `doc`'s title and page names.
pub(super) fn build(doc: &Document, options: &WriteOptions) -> Result<Vec<u8>, WriteError> {
    let now = options.now.and_then(super::props::format_time);
    let variant = options.variant.map_or(CT_DOCUMENT, |v| v.content_type());

    let mut types = TypesTree::new(&[("rels", CT_RELS), ("xml", CT_XML)]);
    for (name, ct) in [
        (PART_DOCUMENT, variant),
        (PART_PAGES, CT_PAGES),
        (PART_WINDOWS, CT_WINDOWS),
        ("/docProps/app.xml", CT_APP),
        ("/docProps/core.xml", CT_CORE),
        ("/docProps/custom.xml", CT_CUSTOM),
    ] {
        types.set_override(name, ct);
    }

    let settings = Node::El(default_settings(doc));
    let document = part_root("VisioDocument", vec![settings, Node::El(face_names(doc)), node("StyleSheets", &[]), node("DocumentSheet", &[])]);
    let pages = part_root("Pages", vec![]);
    let windows = windows_root(doc);
    let page_names: Vec<String> = doc.pages.iter().map(|p| p.name.to_string()).collect();
    let master_names: Vec<String> = doc.masters.iter().map(|m| m.name.as_deref().or(m.name_u.as_deref()).unwrap_or("").to_owned()).collect();

    let package_rels = rels(
        "/_rels/.rels",
        &[
            ("rId1", rel_type(PK, "core-properties"), "docProps/core.xml"),
            ("rId2", rel_type(OD, "extended-properties"), "docProps/app.xml"),
            ("rId3", rel_type(OD, "custom-properties"), "docProps/custom.xml"),
            ("rId4", rel_type(VIS, "document"), PART_DOCUMENT.trim_start_matches('/')),
        ],
    );
    let document_rels =
        rels(PART_DOCUMENT_RELS, &[("rId1", rel_type(VIS, "pages"), "pages/pages.xml"), ("rId2", rel_type(VIS, "windows"), "windows.xml")]);
    let pages_rels = RelsTree::empty(PART_PAGES_RELS);

    let types_bytes = types.bytes()?;
    let mut writer = PackageWriter::new(&types_bytes).map_err(WriteError::Package)?;
    let mut add = |name: &str, tree: &Tree| -> Result<(), WriteError> {
        let bytes = tree.bytes(name)?;
        writer.add(name, &bytes).map_err(WriteError::Package)
    };
    add("/_rels/.rels", &package_rels.tree)?;
    add("/docProps/app.xml", &new_app(&doc.props, page_names, master_names))?;
    // **The drawing's creation date is its own**: a drawing written as VSDX for the first time from another format (an `.odg`, or a `.diacraft` drawing of one)
    // says when it was made, as the file it came from did, and only a new document (one made new here, or a copy made from a template) is made now.
    let created = if options.new_copy { now.as_deref() } else { doc.props.created.as_deref().or(now.as_deref()) };
    add("/docProps/core.xml", &new_core(&doc.title, &doc.props, created, now.as_deref()))?;
    add("/docProps/custom.xml", &new_custom(false))?;
    add(PART_DOCUMENT, &Tree::new(document))?;
    add(PART_DOCUMENT_RELS, &document_rels.tree)?;
    add(PART_PAGES, &Tree::new(pages))?;
    add(PART_PAGES_RELS, &pages_rels.tree)?;
    add(PART_WINDOWS, &Tree::new(windows))?;
    writer.finish().map_err(WriteError::Package)
}
