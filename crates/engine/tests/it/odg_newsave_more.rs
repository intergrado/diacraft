//! 2026-11-16: three follow-ups. Each case opens an `.odg` (a hand-made package), applies the commands, saves, **opens the file in a
//! session of its own** and reads what came back, and saves again, which must be no edit.
//!
//! 1. The `Theme` style takes the file's default graphic style as its parent (`standard`, in a file that has one).
//! 2. The theme part a drawing's `theme.apply` makes has a neutral name, so that no saved `.odg` names the proprietary product.
//! 3. A shape dropped after `theme.apply` keeps its link to the `Theme` style, so it follows `theme.variant` after a reopen.

use diacraft_doc::Document;
use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{DRAWING, Dir, Drawing, names_of, open_bytes_in, package, part_bytes, part_of, read_back};
use crate::odg_file::{four, look, save};
use crate::run;

/// A session of its own on the file `name`: on **a copy of it** (a file a session saves is changed by the save, whatever the bytes, and a session that opened it before
/// would find the file changed by something else).
fn reopened(dir: &Dir, name: &str) -> Session {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COPIES: AtomicU32 = AtomicU32::new(0);
    let copy = format!("copy{}.odg", COPIES.fetch_add(1, Ordering::Relaxed));
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.write(&copy, &dir.read(name)) }));
    again
}

/// What `odg_file::assert_saves_again_as_no_edit` asserts, of the file the session itself saved (not one called `a.odg`): the second save and the third are no edit, give
/// the same bytes and say nothing.
#[track_caller]
fn saves_again_as_no_edit(s: &mut Session) {
    let path = s.source().unwrap().path().to_path_buf();
    let before = std::fs::read(&path).unwrap();
    for time in ["second", "third"] {
        let again = save(s);
        assert_eq!(again["edited"], false, "the {time} save of the same drawing is no edit: {again}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "and the file is the same bytes");
        assert_eq!(again["warning_count"], 0, "and says nothing: {again}");
    }
}

/// The bytes of the file `s` is open on.
fn file_of(s: &Session) -> Vec<u8> {
    std::fs::read(s.source().unwrap().path()).unwrap()
}

/// A session of its own on the file `s` saved.
fn reopened_of(dir: &Dir, s: &Session) -> Session {
    let name = "of_a_session.odg";
    let mut again = Session::new();
    run(&mut again, "file.open", json!({ "path": dir.write(name, &file_of(s)) }));
    again
}

// ---- item 2: the name of a theme part -----------------------------------------------------------------------------------------------

/// Where `needle` (lower case) is found, case-insensitively, in `hay`.
fn found_in(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Every place in a saved package that names `word`, as text: the entries' names, the manifest's rows (its entries are parts like the rest), every part's bytes
/// decompressed, and the strings of DiaCraft's document file as the reader gives them back (the themes' part names, the masters' names and the page names).
fn places_that_name(bytes: &[u8], word: &str) -> Vec<String> {
    let package = diacraft_odg::Package::read(bytes, &diacraft_odg::Limits::default()).unwrap();
    let mut places = Vec::new();
    for name in package.parts() {
        if found_in(name.as_bytes(), word) {
            places.push(format!("the name of the entry {name}"));
        }
        if found_in(package.part(name).unwrap(), word) {
            places.push(format!("the bytes of the entry {name}"));
        }
    }
    for name in package.folders() {
        if found_in(name.as_bytes(), word) {
            places.push(format!("the name of the folder {name}"));
        }
    }
    let (doc, _) = read_back(bytes);
    for theme in doc.themes.iter() {
        if found_in(theme.part.as_bytes(), word) {
            places.push(format!("the part name of a theme: {}", theme.part));
        }
    }
    for master in doc.masters.iter() {
        for text in [&master.name, &master.name_u].into_iter().flatten() {
            if found_in(text.as_bytes(), word) {
                places.push(format!("the name of a master: {text}"));
            }
        }
    }
    places
}

/// What a person does to a new drawing in one sitting, saved: a theme, a shape dropped, the Snap and Glue options and a guide.
fn rich_session(dir: &Dir) -> Session {
    let mut s = open_bytes_in(dir, "a.odg", &four().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    run(&mut s, "snap.set", json!({ "grid": false, "glue_vertices": true }));
    run(&mut s, "guide.add", json!({ "page": 0, "orientation": "vertical", "at": 2.0 }));
    s
}

/// Item 2: the part name `theme.apply` gives the theme it adds was the one a VSDX package holds a theme under, `/visio/theme/theme1.xml`, and the document file of an `.odg`
/// wrote it as the theme's `part`: a saved `.odg` named the product it is not (found the same when a VSDX drawing was saved across). No entry's name, no part's text,
/// and no string DiaCraft's document file holds says it (a part name, a master's name), whatever the drawing holds.
#[test]
fn no_part_entry_or_document_file_string_of_a_saved_odg_names_the_product() {
    let dir = Dir::new();
    let mut s = rich_session(&dir);
    assert_eq!(save(&mut s)["edited"], true);
    let saved = dir.read("a.odg");
    assert!(part_of(&saved, "DiaCraft/document.xml").contains("<diacraft:theme "), "the theme is carried in the document file");
    assert_eq!(places_that_name(&saved, "visio"), Vec::<String>::new());
    // A second save is no edit, and the drawing a session of its own opens saves as none either.
    saves_again_as_no_edit(&mut s);
    let mut again = reopened(&dir, "a.odg");
    saves_again_as_no_edit(&mut again);
    assert_eq!(places_that_name(&dir.read("a.odg"), "visio"), Vec::<String>::new());
}

/// The part is named `theme<n>.xml`, the least `n` no theme of the drawing has, and the document file says it so; after a reopen the same name is what the model has (a
/// second save is no edit), and a second theme is `theme2.xml`.
#[test]
fn a_theme_part_is_named_theme_n_and_a_second_theme_takes_the_next_number() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood" }));
    assert_eq!(parts_of(s.document().unwrap()), ["theme1.xml"]);
    save(&mut s);
    assert!(part_of(&dir.read("a.odg"), "DiaCraft/document.xml").contains("<diacraft:theme part=\"theme1.xml\">"));
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(parts_of(again.document().unwrap()), ["theme1.xml"], "the reopened model has the name the file gave");
    run(&mut again, "theme.apply", json!({ "theme": "tidepool" }));
    assert_eq!(parts_of(again.document().unwrap()), ["theme1.xml", "theme2.xml"]);
    save(&mut again);
    assert_eq!(parts_of(reopened_of(&dir, &again).document().unwrap()), ["theme1.xml", "theme2.xml"]);
    saves_again_as_no_edit(&mut again);
    // The bytes of each are the catalogue's, whatever the name.
    let (doc, _) = read_back(&file_of(&again));
    let bytes: Vec<&[u8]> = doc.themes.iter().map(|t| &t.bytes[..]).collect();
    assert_eq!(bytes, [&diacraft_engine::theme::find("fernwood").unwrap().bytes[..], &diacraft_engine::theme::find("tidepool").unwrap().bytes[..]]);
}

fn parts_of(doc: &Document) -> Vec<String> {
    doc.themes.iter().map(|t| t.part.to_string()).collect()
}

/// What the measure shows and the report says: the bytes of a theme are DrawingML, and DiaCraft's own theme writer gives each extension element the namespace the format gives it,
/// a URI that holds the product's name (`.../office/<name>/2012/theme`, declared as `xmlns:vt` on each of them). It is inside the base64 text of the document file, so the
/// search of the other test cannot see it. **This pins that the word stands in the decoded bytes of a theme only in that declaration**, so that another place would be found;
/// whether a theme's own bytes should name the product is a question for the owner (the same one asked about a theme crossing from a VSDX drawing).
#[test]
fn the_decoded_bytes_of_a_theme_name_the_product_only_in_the_namespace_declaration_of_its_extension_elements() {
    let dir = Dir::new();
    let mut s = rich_session(&dir);
    save(&mut s);
    let (doc, _) = read_back(&dir.read("a.odg"));
    assert_eq!(doc.themes.len(), 1);
    let text = String::from_utf8(doc.themes.get(0).unwrap().bytes.to_vec()).unwrap();
    let declaration = "xmlns:vt=\"http://schemas.microsoft.com/office/visio/2012/theme\"";
    let declared = text.matches(declaration).count();
    assert!(declared > 0, "the extension elements declare the namespace");
    let rest = text.replace(declaration, "");
    assert!(!rest.to_ascii_lowercase().contains("visio"), "the word stands nowhere else in the theme's bytes");
}

// ---- item 1: the parent of the Theme style ------------------------------------------------------------------------------------------

/// The common styles of a file a drawing program made: the default graphic style (which another program cannot name as a parent) and `standard`, the style every shape's own style
/// has as its parent, with the face a shape of the file is drawn in. LibreOffice 26.8.1.1 writes this form (a black-box probe of 2026-11-16: the default style holds the program's
/// pool font, a serif, and `standard` a sans, and a shape style with no parent is written with `standard` as its parent).
const STANDARD: &str = concat!(
    r##"<office:styles><style:default-style style:family="graphic"><style:text-properties style:font-name="Serif Face" fo:font-size="24pt"/></style:default-style>"##,
    r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-color="#112233"/>"##,
    r##"<style:text-properties style:font-name="Sans Face" fo:font-size="18pt"/></style:style></office:styles>"##
);

/// The package of `drawing` whose `styles.xml` holds [`STANDARD`] and the two faces it names.
fn with_standard(drawing: &Drawing) -> Vec<u8> {
    let styles = drawing
        .styles_xml()
        .replace("<office:styles/>", STANDARD)
        .replace(
            "<office:font-face-decls/>",
            r#"<office:font-face-decls><style:font-face style:name="Serif Face" svg:font-family="'Serif Face'"/><style:font-face style:name="Sans Face" svg:font-family="'Sans Face'"/></office:font-face-decls>"#,
        );
    assert!(styles.contains("Sans Face"), "the styles part holds the faces");
    package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), drawing.content_xml().into_bytes()),
            ("styles.xml".to_owned(), styles.into_bytes()),
            ("meta.xml".to_owned(), drawing.meta_xml().into_bytes()),
        ],
    )
}

/// `four()` and a fifth shape, `T`, whose automatic style sets no fill and no line.
fn drawing_with_plain_shape() -> Drawing {
    let mut drawing = four();
    drawing.auto.push_str(r#"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties/></style:style>"#);
    let shapes = crate::odg_build::rect_on("gr3", "T", (2.0, 13.0, 4.0, 2.0), "");
    drawing.pages = drawing.pages.replace("</draw:page>", &format!("{shapes}</draw:page>"));
    drawing
}

fn style_named<'a>(doc: &'a Document, name: &str) -> &'a diacraft_doc::Style {
    doc.styles.iter().find(|s| s.name_u.as_deref() == Some(name)).unwrap_or_else(|| panic!("no style {name}")).as_ref()
}

fn links_of(style: &diacraft_doc::Style) -> (Option<u32>, Option<u32>, Option<u32>) {
    (style.sheet.links.line, style.sheet.links.fill, style.sheet.links.text)
}

/// Item 1: a common style that has no parent is drawn by LibreOffice with its own pool defaults and not with `standard`, which is where a shape's face comes from: a shape that
/// followed the new `Theme` style came out in a serif where the same shape followed nothing and came out in a sans. In a file that has `standard`, the `Theme` style takes it as its
/// parent: the model has it so (the links of the style name `standard`), the file says it (`style:parent-style-name`), the shapes that follow the theme name `Theme`, a session of its
/// own reads the same style sheets, and a second save is no edit.
#[test]
fn the_theme_style_takes_the_files_default_graphic_style_as_its_parent() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &with_standard(&drawing_with_plain_shape()));
    let standard = style_named(s.document().unwrap(), "standard").id;
    assert_ne!(standard, 0, "`standard` is a style sheet of the file, and the default styles are the root");
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    let doc = s.document().unwrap().clone();
    let theme = style_named(&doc, "Theme");
    assert_eq!(links_of(theme), (Some(standard), Some(standard), Some(standard)), "the Theme style is based on `standard`");
    assert_eq!(save(&mut s)["edited"], true);
    let styles = part_of(&dir.read("a.odg"), "styles.xml");
    assert!(styles.contains(r#"style:name="Theme" style:family="graphic" style:parent-style-name="standard""#), "the file says the parent: {styles}");
    assert!(part_of(&dir.read("a.odg"), "content.xml").contains("style:parent-style-name=\"Theme\""), "the shapes name the Theme style");
    let mut again = reopened(&dir, "a.odg");
    let theirs = again.document().unwrap().clone();
    assert_eq!(doc.styles.len(), theirs.styles.len());
    // A style sheet as read holds the `.odg` data of its slot, which the model's has not: the rest is the same.
    let bare = |s: &diacraft_doc::Style| diacraft_doc::Style { ext: Default::default(), ..s.clone() };
    for (a, b) in doc.styles.iter().zip(theirs.styles.iter()) {
        assert_eq!(bare(a), bare(b), "style sheet {} after a reopen", a.id);
    }
    assert_eq!(links_of(style_named(&theirs, "Theme")), (Some(standard), Some(standard), Some(standard)));
    saves_again_as_no_edit(&mut s);
    saves_again_as_no_edit(&mut again);
    // The theme still follows its variants in the file a session of its own opened.
    let before = look_of(&mut again, "T");
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    assert_ne!(look_of(&mut again, "T"), before);
}

/// The default graphic style the `Theme` style is based on is the one that is named `standard`, **by the name as the file writes it** (the reader's `NameU`): a style that is
/// called another thing, or `Standard`, is not it.
#[test]
fn only_a_style_named_standard_is_the_default_graphic_style() {
    for name in ["Standard", "default", "standard2"] {
        let dir = Dir::new();
        let renamed = with_standard(&drawing_with_plain_shape());
        // The same package with the style called `name`: the package is built again from the parts.
        let styles = part_of(&renamed, "styles.xml").replace("style:name=\"standard\"", &format!("style:name=\"{name}\""));
        let content = part_of(&renamed, "content.xml");
        let meta = part_of(&renamed, "meta.xml");
        let bytes = package(
            DRAWING,
            vec![
                ("content.xml".to_owned(), content.into_bytes()),
                ("styles.xml".to_owned(), styles.into_bytes()),
                ("meta.xml".to_owned(), meta.into_bytes()),
            ],
        );
        let mut s = open_bytes_in(&dir, "a.odg", &bytes);
        run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
        assert_eq!(links_of(style_named(s.document().unwrap(), "Theme")), (Some(0), Some(0), Some(0)), "{name}");
    }
}

/// A file with no default graphic style (a hand-made one, or a new drawing, whose blank package has an empty `office:styles`): the `Theme` style is based on the root, as before,
/// and the file says no parent for it. What another program draws for a shape that follows it is its own defaults (LibreOffice: a serif face) and not the drawing's. A second
/// save is no edit.
#[test]
fn a_file_with_no_default_graphic_style_gets_a_theme_style_with_no_parent() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &drawing_with_plain_shape().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    assert_eq!(links_of(style_named(s.document().unwrap(), "Theme")), (Some(0), Some(0), Some(0)));
    save(&mut s);
    let styles = part_of(&dir.read("a.odg"), "styles.xml");
    assert!(styles.contains(r#"<style:style style:name="Theme" style:family="graphic"/>"#), "no parent: {styles}");
    saves_again_as_no_edit(&mut s);
}

/// Only a drawing that came from an `.odg` takes the file's default graphic style: a drawing that has none of that file, saved as a VSDX one, keeps the style based on the root
/// even if a style of its own is called `standard`.
#[test]
fn a_vsdx_drawing_keeps_its_theme_style_on_the_root() {
    let mut s = crate::blank_session();
    s.edit("Test setup", |doc| {
        let id = doc.styles.iter().map(|x| x.id).max().unwrap() + 1;
        doc.add_style(diacraft_doc::Style {
            id,
            name: Some(std::sync::Arc::from("standard")),
            name_u: Some(std::sync::Arc::from("standard")),
            ..diacraft_doc::Style::default()
        })
        .unwrap();
        Ok(())
    })
    .unwrap();
    run(&mut s, "theme.apply", json!({ "theme": "fernwood" }));
    assert_eq!(links_of(style_named(s.document().unwrap(), "Theme")), (Some(0), Some(0), Some(0)));
}

fn look_of(s: &mut Session, name: &str) -> serde_json::Value {
    let at = names_of(s.document().unwrap(), 0).iter().position(|n| n == name).unwrap();
    look(s, 0, at)
}

// ---- item 3: a new shape keeps its link to the Theme style ----------------------------------------------------------------------------

/// The shape ids of the stencil that the `.odg` save writes (`odgfile::WRITABLE_MASTERS`, the thirty of the original masters).
const WRITABLE_MASTERS: [&str; 30] = [
    "rectangle",
    "square",
    "ellipse",
    "circle",
    "triangle",
    "right_triangle",
    "diamond",
    "pentagon",
    "hexagon",
    "octagon",
    "parallelogram",
    "trapezoid",
    "process",
    "decision",
    "terminator",
    "data",
    "document",
    "multiple_documents",
    "predefined_process",
    "manual_input",
    "manual_operation",
    "preparation",
    "on_page_reference",
    "off_page_reference",
    "database",
    "internal_storage",
    "delay",
    "display",
    "summing_junction",
    "or",
];

type Links = (Option<u32>, Option<u32>, Option<u32>);

fn last_look(s: &mut Session) -> serde_json::Value {
    let at = s.document().unwrap().pages.get(0).unwrap().shapes.len() - 1;
    look(s, 0, at)
}

fn last_links(doc: &Document) -> Links {
    let page = doc.pages.get(0).unwrap();
    let shape = page.shapes.iter().next_back().unwrap();
    // Its own links, or those of the master's shape it stands for (a theme links the masters' shapes, and an instance keeps no link of its own).
    crate::theme_master_links::follows(doc, page.id, shape.id)
}

/// What a person does after a theme: `act` puts a new shape last on page 0 of `bytes`' drawing. Two sessions do it, one that saves and one that never does. The shape is saved,
/// and a session of its own opens the file: **the shape still follows the `Theme` style** (its links), looks as it did, and follows `theme.variant` as the session that never saved
/// does (`changes`: the variant changes its look); a second save is no edit, in either session; and a variant given in the session that saved, then saved and read again, is the
/// look the other session has (the copy of the shape the save before made is not used again when the page's variant is another).
#[track_caller]
fn follows_after_a_reopen(bytes: &[u8], changes: bool, act: &dyn Fn(&mut Session)) {
    let dir = Dir::new();
    let mut saved = open_bytes_in(&dir, "a.odg", bytes);
    let mut never = open_bytes_in(&dir, "n.odg", bytes);
    for s in [&mut saved, &mut never] {
        run(s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
        act(s);
    }
    let style = diacraft_engine::theme::theme_style_id(saved.document().unwrap()).unwrap();
    let wanted = (Some(style), Some(style), Some(style));
    assert_eq!(last_links(saved.document().unwrap()), wanted, "the page's theme is given to the new shape");
    let before = last_look(&mut saved);
    assert_eq!(last_look(&mut never), before);
    assert_eq!(save(&mut saved)["edited"], true);
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(last_links(again.document().unwrap()), wanted, "the new shape follows the Theme style after the file is read again");
    assert_eq!(last_look(&mut again), before, "and looks as it did");
    saves_again_as_no_edit(&mut saved);
    saves_again_as_no_edit(&mut again);
    for s in [&mut never, &mut again] {
        run(s, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    }
    let after = last_look(&mut again);
    assert_eq!(
        after,
        last_look(&mut never),
        "the shape of the file a session of its own read follows the variant as the shape of the session that never saved"
    );
    assert_eq!(after != before, changes, "the variant changes the look of the shape: {before} then {after}");
    // The other variants too (a style variant may give the shape a gradient, a shadow or rounded corners): the same look in both.
    for variant in [2, 3, 0] {
        for s in [&mut never, &mut again] {
            run(s, "theme.variant", json!({ "variant": variant, "scope": "all" }));
        }
        assert_eq!(last_look(&mut again), last_look(&mut never), "variant {variant}");
    }
    run(&mut never, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    // The session that saved gives the variant, saves, and a third session reads it.
    run(&mut saved, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    assert_eq!(save(&mut saved)["edited"], true);
    let mut third = reopened(&dir, "a.odg");
    assert_eq!(last_look(&mut third), after, "variant 1, saved and read again");
    assert_eq!(last_links(third.document().unwrap()), wanted);
    saves_again_as_no_edit(&mut saved);
    saves_again_as_no_edit(&mut third);
}

/// The brief's case: a theme is applied, a master is dropped, the drawing is saved and opened by a session of its own, and the new shape follows `theme.variant` (it was a
/// shape of constants on the root style, which no variant moved), and a second save is no edit.
#[test]
fn a_shape_dropped_after_theme_apply_follows_theme_variant_after_a_reopen() {
    follows_after_a_reopen(&drawing_with_plain_shape().bytes(), true, &|s| {
        run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    });
}

/// The same in a file a drawing program made, which has `standard` and so a `Theme` style based on it.
#[test]
fn a_shape_dropped_after_theme_apply_follows_theme_variant_in_a_file_that_has_a_default_graphic_style() {
    follows_after_a_reopen(&with_standard(&drawing_with_plain_shape()), true, &|s| {
        run(s, "shape.drop", json!({ "master": "decision", "page": 0, "x": 4.0, "y": 2.0 }));
    });
}

/// Every master the `.odg` save writes (the thirty of the original masters), dropped after the theme.
#[test]
fn each_of_the_thirty_writable_masters_dropped_after_theme_apply_follows_the_variant_after_a_reopen() {
    for master in WRITABLE_MASTERS {
        follows_after_a_reopen(&drawing_with_plain_shape().bytes(), true, &|s| {
            run(s, "shape.drop", json!({ "master": master, "page": 0, "x": 4.0, "y": 2.0 }));
        });
    }
}

/// A text box has no fill and no line of its own, and takes the theme's text colour and face: its links are carried too (its look is the same in a variant).
#[test]
fn a_text_box_made_after_theme_apply_keeps_its_theme_link_after_a_reopen() {
    follows_after_a_reopen(&drawing_with_plain_shape().bytes(), false, &|s| {
        run(s, "text.box", json!({ "page": 0, "x": 4.0, "y": 4.0, "text": "Hi" }));
    });
}

/// A connector drawn after the theme (`connector.add` between two shapes dropped after it) follows it too: a line.
#[test]
fn a_connector_added_after_theme_apply_follows_the_variant_after_a_reopen() {
    follows_after_a_reopen(&drawing_with_plain_shape().bytes(), true, &|s| {
        let one = run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
        let two = run(s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 8.0, "y": 2.0 }));
        run(s, "connector.add", json!({ "from": { "id": one["id"] }, "to": { "id": two["id"] } }));
    });
}

/// A person's own fill, set after the drop, stays what the person set after the reopen and the variant; the line, which they did not set, follows the theme.
#[test]
fn a_fill_set_after_the_drop_stays_set_after_a_reopen_and_a_variant() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &drawing_with_plain_shape().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    run(&mut s, "shape.format", json!({ "ids": [made["id"]], "fill": "#00ff00" }));
    let before = last_look(&mut s);
    save(&mut s);
    let mut again = reopened(&dir, "a.odg");
    assert_eq!(last_look(&mut again), before);
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    let after = last_look(&mut again);
    assert_eq!(after["fill"], before["fill"], "the green fill is the person's: {before} then {after}");
    assert_ne!(after["line"], before["line"], "the line is the theme's, and the variant moved it: {before} then {after}");
    saves_again_as_no_edit(&mut s);
}

// ---- item 2, VSDX: what a VSDX save names its theme parts ----------------------------------------------------------------------------------

fn vsdx_theme_parts(path: &str) -> Vec<(String, Vec<u8>)> {
    let pkg = diacraft_opc::Package::read(&std::fs::read(path).unwrap(), &diacraft_opc::Limits::default()).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> =
        pkg.parts().filter(|p| p.contains("/theme/")).map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect();
    parts.sort();
    parts
}

/// A VSDX save names the theme parts it writes itself (`vsdx::write::themes`: `theme<n>.xml` in the folder beside the document part, the least `n` no part has), whatever the
/// model names them, so the neutral name `theme.apply` gives a part (item 2) is no change to a VSDX package: a theme made by `theme.apply` is `theme1.xml` in the package; a
/// file that holds a theme in a part of that name and is given another keeps the first part, byte for byte, and the second is `theme2.xml`; and a model that holds two themes
/// made one after the other writes both, in the order they were made.
#[test]
fn a_vsdx_save_names_its_theme_parts_as_before_whatever_the_model_names_them() {
    let dir = Dir::new();
    let mut s = crate::blank_session();
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3.0, "y": 4.0 }));
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "variant": 1 }));
    let one = dir.path("one.vsdx");
    run(&mut s, "file.save_as", json!({ "path": one }));
    let fern = diacraft_engine::theme::find("fernwood").unwrap();
    let tide = diacraft_engine::theme::find("tidepool").unwrap();
    let cinder = diacraft_engine::theme::find("cinderglow").unwrap();
    assert_eq!(
        vsdx_theme_parts(&one),
        [("/visio/theme/theme1.xml".to_owned(), fern.bytes.to_vec())],
        "the part is the package's own name for the first theme"
    );
    // The file holds a theme in `/visio/theme/theme1.xml` (the name it was read under), and another theme is applied: two parts, the first as it was.
    let mut t = Session::new();
    run(&mut t, "file.open", json!({ "path": one }));
    assert_eq!(parts_of(t.document().unwrap()), ["/visio/theme/theme1.xml"], "a theme read from a package keeps the name the package gave it");
    run(&mut t, "theme.apply", json!({ "theme": "tidepool" }));
    assert_eq!(
        parts_of(t.document().unwrap()),
        ["/visio/theme/theme1.xml", "theme1.xml"],
        "the part made is named neutrally, and the one name the model holds twice is no clash"
    );
    let two = dir.path("two.vsdx");
    run(&mut t, "file.save_as", json!({ "path": two }));
    assert_eq!(
        vsdx_theme_parts(&two),
        [("/visio/theme/theme1.xml".to_owned(), fern.bytes.to_vec()), ("/visio/theme/theme2.xml".to_owned(), tide.bytes.to_vec())]
    );
    // Two themes made one after the other in a drawing never saved: both are written, in the order they were made.
    let mut u = crate::blank_session();
    run(&mut u, "shape.drop", json!({ "master": "rectangle", "page": 1, "x": 3.0, "y": 4.0 }));
    run(&mut u, "theme.apply", json!({ "theme": "tidepool" }));
    run(&mut u, "theme.apply", json!({ "theme": "cinderglow", "variant": 3 }));
    let three = dir.path("three.vsdx");
    run(&mut u, "file.save_as", json!({ "path": three }));
    assert_eq!(
        vsdx_theme_parts(&three),
        [("/visio/theme/theme1.xml".to_owned(), tide.bytes.to_vec()), ("/visio/theme/theme2.xml".to_owned(), cinder.bytes.to_vec())]
    );
}

/// The start tag of the automatic style the shape called `name` is drawn with, from `content.xml`.
fn style_tag_of(content: &str, name: &str) -> String {
    let at = content.find(&format!("draw:name=\"{name}\"")).unwrap_or_else(|| panic!("no shape {name}: {content}"));
    let before = &content[..at];
    let tag_at = before.rfind("<draw:").unwrap();
    let tag = &content[tag_at..at];
    let style = tag.split("draw:style-name=\"").nth(1).unwrap().split('"').next().unwrap();
    let start = content.find(&format!("<style:style style:name=\"{style}\"")).unwrap_or_else(|| panic!("no style {style}"));
    content[start..].split('>').next().unwrap().to_owned()
}

/// A new shape whose three links are not all the `Theme` style (a master that links a style of its own for one family gives the shape the other two) cannot keep them: the
/// file says one parent. It is written as before, on the root with the look as constants, **and the save says the link is not written** (it was dropped with no
/// word), whichever of the three families holds the link; a shape whose three links are the style, beside it, is written with the parent and says nothing of it.
#[test]
fn a_new_shape_whose_links_are_not_all_the_theme_style_is_written_on_the_root_and_the_save_says_the_link_is_lost() {
    // Each way the three links can name the style and not all of them (0 is the root, and a link that is none is the root's too).
    for (line, fill, text) in
        [(true, false, false), (false, true, false), (false, false, true), (true, true, false), (true, false, true), (false, true, true)]
    {
        let dir = Dir::new();
        let mut s = open_bytes_in(&dir, "a.odg", &four().bytes());
        run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
        let whole = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
        let partial = run(&mut s, "shape.drop", json!({ "master": "square", "page": 0, "x": 8.0, "y": 2.0 }));
        let style = diacraft_engine::theme::theme_style_id(s.document().unwrap()).unwrap();
        s.edit("Test setup", |doc| {
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let last = page.shapes.len() - 1;
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(last).unwrap());
            let one = |on: bool| Some(if on { style } else { 0 });
            std::sync::Arc::make_mut(&mut shape.sheet).links = diacraft_doc::Links { line: one(line), fill: one(fill), text: one(text) };
            Ok(())
        })
        .unwrap();
        let saved = save(&mut s);
        let said: Vec<String> = saved["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_owned()).collect();
        let why = format!("links {line} {fill} {text}: {said:?}");
        assert_eq!(said.iter().filter(|w| w.contains("the link to the Theme style")).count(), 1, "{why}");
        assert!(said.iter().any(|w| w.contains("the link to the Theme style") && w.contains("1 new shape")), "{why}");
        let content = part_of(&dir.read("a.odg"), "content.xml");
        let name = |made: &serde_json::Value| {
            let id = made["id"].as_u64().unwrap() as u32;
            let doc = s.document().unwrap();
            doc.pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().name_u.as_deref().unwrap().to_owned()
        };
        assert!(
            style_tag_of(&content, &name(&whole)).contains("style:parent-style-name=\"Theme\""),
            "the shape that follows the style names it: {why}"
        );
        assert!(
            !style_tag_of(&content, &name(&partial)).contains("parent-style-name"),
            "the other is on the root, with its look as constants: {why}"
        );
        saves_again_as_no_edit(&mut s);
    }
}

/// A cell the shape holds a value of its own for, whose formula is the style's (`F="Inh"`: a file of another program holds such cells, a recalculation of a copy leaves
/// them), is the shape's own as far as the value goes: it is written as a constant and stays after the reopen and a variant, where the style's own cell would follow the theme.
#[test]
fn a_value_of_the_shapes_own_under_a_formula_the_style_gives_is_written_and_stays_after_a_variant() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &four().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    s.edit("Test setup", |doc| {
        let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let last = page.shapes.len() - 1;
        let shape = std::sync::Arc::make_mut(page.shapes.get_mut(last).unwrap());
        let cell = diacraft_doc::Cell::read(Some("Inh"), Some("#00ff00"), None, None, &mut 8);
        std::sync::Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new("FillForegnd"), cell);
        Ok(())
    })
    .unwrap();
    let before = last_look(&mut s);
    assert_eq!(before["fill"], "#00ff00", "the value of the shape's own is what it shows: {before}");
    save(&mut s);
    let mut again = reopened(&dir, "a.odg");
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    let after = last_look(&mut again);
    assert_eq!(after["fill"], "#00ff00", "and it stays after a reopen and a variant: {after}");
    assert_ne!(after["line"], before["line"], "where the line, which is the style's, follows the theme: {after}");
}

/// The default graphic style is the first style sheet but the root that is called `standard`: the root is none, even when it is called so.
#[test]
fn the_default_graphic_style_is_found_by_its_name_and_the_root_is_none() {
    let dir = Dir::new();
    let s = open_bytes_in(&dir, "a.odg", &with_standard(&drawing_with_plain_shape()));
    let doc = s.document().unwrap();
    let standard = diacraft_engine::theme::default_graphic_style_id(doc).unwrap();
    assert_eq!(style_named(doc, "standard").id, standard);
    assert_ne!(standard, 0);
    let mut renamed = doc.clone();
    let root = std::sync::Arc::make_mut(renamed.styles.get_mut(0).unwrap());
    root.name_u = Some(std::sync::Arc::from("standard"));
    let found = diacraft_engine::theme::default_graphic_style_id(&renamed);
    assert_eq!(found, Some(standard), "the root is not the default graphic style though it is called so: the first style after it is");
    let none = open_bytes_in(&dir, "b.odg", &drawing_with_plain_shape().bytes());
    assert_eq!(diacraft_engine::theme::default_graphic_style_id(none.document().unwrap()), None, "a file with none has none");
    let mut only_root = none.document().unwrap().clone();
    std::sync::Arc::make_mut(only_root.styles.get_mut(0).unwrap()).name_u = Some(std::sync::Arc::from("standard"));
    assert_eq!(diacraft_engine::theme::default_graphic_style_id(&only_root), None, "and the root, called so, is not it");
}

// ---- the review (task-ODG.NEWSAVE-review.md), ---------------------------------------------------

/// The bytes of the file `s` saved to.
fn saved_bytes(s: &Session) -> Vec<u8> {
    std::fs::read(s.source().unwrap().path()).unwrap()
}

/// After `theme.apply` and a save, `edit.undo` took the style sheet back and the save refused ("Undo the change to save", with nothing left to undo: only a redo
/// saved). The save takes the empty common style the first save added out of the file, with its entry of the document file and the theme part, and the shapes' parents. The
/// drawing a session of its own opens is the one before the theme; the redo saves again as the first did; every second save is no edit.
#[test]
fn undo_after_a_saved_theme_apply_saves_by_taking_the_style_sheet_the_theme_part_and_the_parents_out_of_the_file() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &drawing_with_plain_shape().bytes());
    let plain = look_of(&mut s, "T");
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    let themed = look_of(&mut s, "T");
    assert_ne!(plain, themed);
    assert_eq!(save(&mut s)["edited"], true);
    assert!(part_of(&saved_bytes(&s), "styles.xml").contains("style:name=\"Theme\""));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(look_of(&mut s, "T"), plain, "the undo gave the drawing back as it was");
    let undone = save(&mut s);
    assert_eq!(undone["edited"], true, "the save after the undo is made: {undone}");
    let bytes = saved_bytes(&s);
    assert!(!part_of(&bytes, "styles.xml").contains("Theme"), "the empty common style is out of the file");
    assert!(!part_of(&bytes, "content.xml").contains("Theme"), "and no shape names it as its parent");
    assert!(
        part_bytes(&bytes, "DiaCraft/document.xml")
            .is_none_or(|f| !String::from_utf8_lossy(&f).contains("Theme") && !String::from_utf8_lossy(&f).contains("theme")),
        "nor the document file"
    );
    let mut again = reopened_of(&dir, &s);
    assert_eq!(look_of(&mut again, "T"), plain, "a session of its own opens the drawing as it was before the theme");
    assert!(again.document().unwrap().themes.is_empty());
    saves_again_as_no_edit(&mut s);
    // The redo: saved, and the file has the style sheet again.
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(look_of(&mut s, "T"), themed);
    assert_eq!(save(&mut s)["edited"], true);
    assert!(part_of(&saved_bytes(&s), "styles.xml").contains("style:name=\"Theme\""));
    let mut third = reopened_of(&dir, &s);
    assert_eq!(look_of(&mut third, "T"), themed);
    saves_again_as_no_edit(&mut s);
}

/// What the save cannot take out of the file (a style sheet the file's markup gives a property) is refused in words that say to **redo**: the undo is the change.
#[test]
fn an_undo_the_save_cannot_take_out_of_the_file_is_refused_with_a_redo_as_the_way_out() {
    let dir = Dir::new();
    let drawing = four().common(r##"<style:style style:name="Heavy" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##);
    let mut s = open_bytes_in(&dir, "a.odg", &drawing.bytes());
    s.edit("Test setup: a style sheet taken back", |doc| {
        let kept: Vec<_> = doc.styles.iter().filter(|x| x.id == 0).cloned().collect();
        doc.styles = diacraft_doc::SharedVec::from_vec(kept);
        Ok(())
    })
    .unwrap();
    let words = crate::odg_file::refused(&mut s);
    assert!(words.contains("taken back by an undo"), "{words}");
    assert!(words.contains("Redo the change you took back (edit.redo)") && !words.contains("Undo the change"), "{words}");
}

/// A file another program saved keeps the empty common style `Theme` and drops the document file that held its cells. `theme.apply` found the style by its name and
/// answered that it changed nothing of the look (the style had no cell for the theme to answer). The style is filled in; the shapes that name it follow the theme; the save
/// carries the cells in the document file; a session of its own opens the same look; `theme.variant` moves it; every second save is no edit.
#[test]
fn theme_apply_fills_in_the_empty_theme_style_a_save_of_another_program_leaves_and_the_file_carries_it() {
    let dir = Dir::new();
    let mut drawing = four();
    drawing.common.push_str(r#"<style:style style:name="Theme" style:family="graphic"/>"#);
    drawing.auto.push_str(
        r#"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="Theme"><style:graphic-properties/></style:style>"#,
    );
    let shapes = crate::odg_build::rect_on("gr3", "T", (2.0, 13.0, 4.0, 2.0), "");
    drawing.pages = drawing.pages.replace("</draw:page>", &format!("{shapes}</draw:page>"));
    let mut s = open_bytes_in(&dir, "a.odg", &drawing.bytes());
    let theme = diacraft_engine::theme::theme_style_id(s.document().unwrap()).unwrap();
    assert!(s.document().unwrap().styles.get(theme as usize).unwrap().sheet.cells.is_empty(), "the file's Theme style holds no cell");
    let before = look_of(&mut s, "T");
    let applied = run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    assert_eq!(applied["style_added"], true, "the style was filled in: {applied}");
    assert!(!s.document().unwrap().styles.get(theme as usize).unwrap().sheet.cells.is_empty());
    let after = look_of(&mut s, "T");
    assert_ne!(before, after, "the theme changes what T, which names the Theme style, looks like");
    assert_eq!(save(&mut s)["edited"], true);
    assert!(part_of(&saved_bytes(&s), "DiaCraft/document.xml").contains("THEMEVAL"), "the document file carries the cells");
    let mut again = reopened_of(&dir, &s);
    assert_eq!(look_of(&mut again, "T"), after, "a session of its own opens the same look");
    run(&mut again, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    run(&mut s, "theme.variant", json!({ "variant": 1, "scope": "all" }));
    assert_ne!(look_of(&mut again, "T"), after, "and the variant moves it");
    assert_eq!(look_of(&mut again, "T"), look_of(&mut s, "T"));
    // The variant is saved (page cells), and then the second save is no edit.
    assert_eq!(save(&mut s)["edited"], true);
    saves_again_as_no_edit(&mut s);
    // The same theme again is no change.
    let mut third = reopened_of(&dir, &s);
    let once_more = run(&mut third, "theme.apply", json!({ "theme": "fernwood", "variant": 1, "scope": "all" }));
    assert_eq!((once_more["style_added"].clone(), once_more["changed_pages"].clone()), (json!(false), json!([])), "{once_more}");
}

/// The one way an `.odg` is opened. The reader of the `odg` crate keeps a theme as bytes; `read_odg` parses it, as `file.open` does, so the command line's export draws the
/// colours the window does.
#[test]
fn read_odg_parses_the_themes_a_file_carries_as_file_open_does() {
    let bytes = four_with_theme("fernwood");
    let (raw, _) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).unwrap();
    assert!(raw.themes.get(0).unwrap().colors.is_none(), "the odg crate keeps the bytes of a theme");
    let (read, report) = diacraft_engine::odgfile::read_odg(&bytes, &diacraft_odg::Limits::default()).unwrap();
    assert!(read.themes.get(0).unwrap().colors.is_some() && read.themes.get(0).unwrap().theme_scheme_id.is_some());
    assert_eq!(report.warnings, Vec::<String>::new());
    let dir = Dir::new();
    let s = open_bytes_in(&dir, "a.odg", &bytes);
    assert_eq!(read.themes.get(0).unwrap().colors, s.document().unwrap().themes.get(0).unwrap().colors, "`file.open` reads the same");
}

/// A malformed theme in an `.odg` opens with a warning of the report, as a VSDX file's does (`theme_unreadable`), and its bytes are kept; one that reads whole says
/// nothing.
#[test]
fn a_theme_of_an_odg_that_cannot_be_read_is_a_warning_of_the_open_and_its_bytes_are_kept() {
    let dir = Dir::new();
    let bytes = four_with_theme_bytes(b"this is not a theme");
    let (doc, report) = diacraft_engine::odgfile::read_odg(&bytes, &diacraft_odg::Limits::default()).unwrap();
    assert_eq!(report.warned("theme_unreadable"), 1, "{:?}", report.warnings);
    assert!(
        report.warnings.iter().any(|w| w.contains("nothing could be read from the theme part") && w.contains("theme/theme1.xml")),
        "{:?}",
        report.warnings
    );
    assert_eq!(&*doc.themes.get(0).unwrap().bytes, b"this is not a theme", "the bytes are kept");
    // `file.open` tells it too.
    let mut s = Session::new();
    let opened = run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &bytes) }));
    assert_eq!(opened["file"]["warning_count"].as_u64().unwrap(), report.warning_counts.values().sum::<u64>(), "{opened}");
    assert!(
        opened["file"]["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("nothing could be read from the theme part")),
        "{opened}"
    );
    // A theme that reads whole says nothing, and one with a part past the root says its entries.
    let fine = diacraft_engine::odgfile::read_odg(&four_with_theme("tidepool"), &diacraft_odg::Limits::default()).unwrap().1;
    assert_eq!(fine.warnings, Vec::<String>::new());
    let wrong_root =
        diacraft_engine::odgfile::read_odg(&four_with_theme_bytes(b"<x:other xmlns:x=\"u\"/>"), &diacraft_odg::Limits::default()).unwrap().1;
    assert_eq!(wrong_root.warned("theme_unreadable"), 1, "{:?}", wrong_root.warnings);
}

/// `four()` saved by the writer with a theme of these bytes in its model.
fn four_with_theme_bytes(theme_bytes: &[u8]) -> Vec<u8> {
    let bytes = four().bytes();
    let (mut doc, report) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).expect("the package reads");
    doc.themes.push(std::sync::Arc::new(diacraft_doc::Theme {
        part: std::sync::Arc::from("theme/theme1.xml"),
        bytes: std::sync::Arc::from(theme_bytes),
        ..diacraft_doc::Theme::default()
    }));
    let original = diacraft_odg::OriginalPackage { bytes: &bytes, index: &report.index };
    diacraft_odg::write_with(&doc, Some(&original), &diacraft_odg::WriteOptions::default()).expect("the writer saves the theme").bytes
}

/// `four()` saved by the writer with the theme `id` of the catalogue in its model.
fn four_with_theme(id: &str) -> Vec<u8> {
    four_with_theme_bytes(&diacraft_engine::theme::find(id).expect("a theme of the catalogue").bytes)
}

/// The links: the `Theme` style a program left empty may be based on another style (`standard`, the parent a drawing program gives every style): filling it in keeps its
/// links, and the file a session of its own opens holds the same.
#[test]
fn an_empty_theme_style_that_has_a_parent_is_filled_in_under_the_links_it_has() {
    let dir = Dir::new();
    let mut drawing = drawing_with_plain_shape();
    drawing.common.push_str(concat!(
        r#"<style:style style:name="standard" style:family="graphic"><style:text-properties fo:font-size="18pt"/></style:style>"#,
        r#"<style:style style:name="Theme" style:family="graphic" style:parent-style-name="standard"/>"#
    ));
    let mut s = open_bytes_in(&dir, "a.odg", &drawing.bytes());
    let (standard, theme) = (style_named(s.document().unwrap(), "standard").id, style_named(s.document().unwrap(), "Theme").id);
    assert_eq!(links_of(style_named(s.document().unwrap(), "Theme")), (Some(standard), Some(standard), Some(standard)));
    let applied = run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    assert_eq!(applied["style_added"], true, "{applied}");
    let filled = style_named(s.document().unwrap(), "Theme");
    assert_eq!(filled.id, theme);
    assert!(!filled.sheet.cells.is_empty());
    assert_eq!(links_of(filled), (Some(standard), Some(standard), Some(standard)), "the links the style had are kept");
    assert_eq!(save(&mut s)["edited"], true);
    let again = reopened_of(&dir, &s);
    assert_eq!(links_of(style_named(again.document().unwrap(), "Theme")), (Some(standard), Some(standard), Some(standard)));
    assert_eq!(style_named(again.document().unwrap(), "Theme"), style_named(s.document().unwrap(), "Theme"));
    saves_again_as_no_edit(&mut s);
}

// ---- the survivors of the mutation run: what the read of the themes of an `.odg` says, and when the Theme style is filled in -----------------------------------

/// `four()` saved by the writer with themes of these bytes in its model, in this order.
fn four_with_themes(themes: &[&[u8]]) -> Vec<u8> {
    let bytes = four().bytes();
    let (mut doc, report) = diacraft_odg::read(&bytes, &diacraft_odg::Limits::default()).expect("the package reads");
    for (at, theme_bytes) in themes.iter().enumerate() {
        doc.themes.push(std::sync::Arc::new(diacraft_doc::Theme {
            part: std::sync::Arc::from(format!("theme/theme{}.xml", at + 1).as_str()),
            bytes: std::sync::Arc::from(*theme_bytes),
            ..diacraft_doc::Theme::default()
        }));
    }
    let original = diacraft_odg::OriginalPackage { bytes: &bytes, index: &report.index };
    diacraft_odg::write_with(&doc, Some(&original), &diacraft_odg::WriteOptions::default()).expect("the writer saves the themes").bytes
}

/// The counts: a theme whose entries cannot be read says how many (`theme_entry_unreadable`), one whose lists are past their cap says how many lists
/// (`theme_list_capped`), each by itself and both at once, and one that reads whole says nothing; the warnings are held to the report's room and counted past it.
#[test]
fn a_theme_with_entries_it_cannot_read_or_lists_past_the_cap_says_how_many_and_the_report_has_room_for_so_many() {
    use diacraft_testkit::theme::{ThemeXml, solid, srgb_clr};
    let read = |bytes: &[u8], limits: &diacraft_odg::Limits| diacraft_engine::odgfile::read_odg(&four_with_themes(&[bytes]), limits).unwrap().1;
    let limits = diacraft_odg::Limits::default();
    // Three entries that cannot be read: a fill named by a colour the reader has no name for, a line whose width is no number, and a variant that is not monotone or not.
    let mut unread = ThemeXml::default();
    unread.fills[1] = "<a:solidFill><a:prstClr val=\"black\"/></a:solidFill>".to_owned();
    unread.lines[2] = unread.lines[2].replace("w=\"19050\"", "w=\"wide\"");
    let mut variants = unread.colour_variants.clone().unwrap();
    variants[1] = variants[1].replace("monotone=\"1\"", "monotone=\"maybe\"");
    unread.colour_variants = Some(variants);
    let report = read(unread.xml().as_bytes(), &limits);
    assert_eq!(
        (report.warned("theme_entry_unreadable"), report.warned("theme_list_capped"), report.warned("theme_unreadable")),
        (1, 0, 0),
        "{:?}",
        report.warnings
    );
    assert!(report.warnings.iter().any(|w| w.contains("3 entries of the theme part theme/theme1.xml could not be read")), "{:?}", report.warnings);
    // A list past its cap.
    let capped =
        ThemeXml { fills: (0..diacraft_doc::MAX_THEME_ENTRIES + 5).map(|_| solid(&srgb_clr("102030", &[]))).collect(), ..ThemeXml::default() };
    let report = read(capped.xml().as_bytes(), &limits);
    assert_eq!(
        (report.warned("theme_entry_unreadable"), report.warned("theme_list_capped"), report.warned("theme_unreadable")),
        (0, 1, 0),
        "{:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("1 lists of the theme part theme/theme1.xml are longer than the reader keeps")),
        "{:?}",
        report.warnings
    );
    // Both at once, in one theme: two warnings of two classes.
    let both = ThemeXml { fills: capped.fills.clone(), lines: unread.lines.clone(), ..ThemeXml::default() };
    let report = read(both.xml().as_bytes(), &limits);
    assert_eq!((report.warned("theme_entry_unreadable"), report.warned("theme_list_capped")), (1, 1), "{:?}", report.warnings);
    // A theme that reads whole says nothing.
    assert_eq!(read(ThemeXml::default().xml().as_bytes(), &limits).warnings, Vec::<String>::new());
    // The report has room for `max_report_entries` of them and counts the rest: three themes that cannot be read, room for one.
    let three = four_with_themes(&[b"not a theme", b"nor this", b"nor that"]);
    let narrow = diacraft_odg::Limits { max_report_entries: 1, ..diacraft_odg::Limits::default() };
    let report = diacraft_engine::odgfile::read_odg(&three, &narrow).unwrap().1;
    assert_eq!((report.warnings.len(), report.warned("theme_unreadable")), (1, 3), "{:?}", report.warnings);
    let wide = diacraft_engine::odgfile::read_odg(&three, &limits).unwrap().1;
    assert_eq!((wide.warnings.len(), wide.warned("theme_unreadable")), (3, 3));
}

/// What is not filled in: a `Theme` style that holds a cell, or a section, is the file's own and is used as it is. Only one that holds neither is filled in.
#[test]
fn a_theme_style_that_holds_a_cell_or_a_section_is_used_as_it_is_and_only_an_empty_one_is_filled_in() {
    let dir = Dir::new();
    let mut drawing = four();
    drawing.common.push_str(r#"<style:style style:name="Theme" style:family="graphic"/>"#);
    for what in ["a cell", "a section"] {
        let mut s = open_bytes_in(&dir, "a.odg", &drawing.bytes());
        s.edit("Test setup", |doc| {
            let at = doc.styles.iter().position(|x| x.name_u.as_deref() == Some("Theme")).unwrap();
            let style = std::sync::Arc::make_mut(doc.styles.get_mut(at).unwrap());
            let sheet = std::sync::Arc::make_mut(&mut style.sheet);
            if what == "a cell" {
                sheet.cells.insert(diacraft_doc::CellName::new("Tag"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("t")));
            } else {
                sheet.sections.push(std::sync::Arc::new(diacraft_doc::Section::new("User", None)));
            }
            Ok(())
        })
        .unwrap();
        let before = style_named(s.document().unwrap(), "Theme").clone();
        let applied = run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
        assert_eq!(applied["style_added"], false, "{what}: {applied}");
        assert_eq!(style_named(s.document().unwrap(), "Theme"), &before, "{what}: the style is as it was");
    }
    // The same file with the style empty: filled in.
    let mut s = open_bytes_in(&dir, "b.odg", &drawing.bytes());
    assert_eq!(run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }))["style_added"], true);
}

/// The whole `style:style` element (to its end) of the automatic style the shape called `name` is drawn with, from `content.xml`.
fn style_element_of(content: &str, name: &str) -> String {
    let tag = style_tag_of(content, name);
    let style = tag.split("style:name=\"").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let start = content.find(&format!("<style:style style:name=\"{style}\"")).unwrap();
    let end = content[start..].find("</style:style>").map_or(content[start..].find("/>").unwrap() + 2, |e| e + "</style:style>".len());
    content[start..start + end].to_owned()
}

/// A shape that follows the `Theme` style holds, in its automatic style, the parent and what it has of its own, **and none of the constants the style gives it** (the fill, the line,
/// the text's vertical alignment and the four margins would be the look of the theme as it was, and the shape's own once the file is read again, which no variant moves).
#[test]
fn the_automatic_style_of_a_shape_that_follows_the_theme_holds_the_parent_and_no_constant_the_style_gives() {
    let dir = Dir::new();
    let mut s = open_bytes_in(&dir, "a.odg", &drawing_with_plain_shape().bytes());
    run(&mut s, "theme.apply", json!({ "theme": "fernwood", "scope": "all" }));
    let made = run(&mut s, "shape.drop", json!({ "master": "rectangle", "page": 0, "x": 4.0, "y": 2.0 }));
    save(&mut s);
    let id = made["id"].as_u64().unwrap() as u32;
    let name = s.document().unwrap().pages.get(0).unwrap().shapes.iter().find(|x| x.id == id).unwrap().name_u.as_deref().unwrap().to_owned();
    let element = style_element_of(&part_of(&saved_bytes(&s), "content.xml"), &name);
    assert!(element.contains("style:parent-style-name=\"Theme\""), "{element}");
    for constant in ["draw:fill-color", "svg:stroke-color", "svg:stroke-width", "fo:padding", "draw:textarea-vertical-align"] {
        assert!(!element.contains(constant), "{constant} is the style's: {element}");
    }
}
