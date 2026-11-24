//! The face a shape's text is shown in (gap U21): `shape.formatting` names the face the renderer draws the text in, as the
//! document or the theme gives its name, and the same function reads it for both. Each test asks the query and then draws the page with a renderer
//! given the same faces, and checks that the face the query names is the face the renderer drew with.
//!
//! The faces are built in code (`diacraft_fonts::synth`), under names invented here, so that no face of the machine is relied on. The drawing
//! target records the family of each run of text it is handed, which is the renderer's own choice of face.

use std::sync::Arc;

use diacraft_doc::{Document, FaceName, RowKey, SectionKey, TextPart};
use diacraft_engine::formatting::{look_of_in, look_of_range_in};
use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, FontStyle};
use diacraft_render::{Paint, Pixmap, Renderer, SkipKind, Target, TextRun};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use kurbo::{Affine, BezPath, Stroke};
use serde_json::json;

use crate::{blank_session, run};

const REGULAR: FontStyle = FontStyle { bold: false, italic: false };

/// The default face the commands name: the database the application shares with the engine if one is in hand, else the bundled faces
/// (`formatting/face.rs`). Nothing is shared in the tests of the engine but those of `measure_fonts.rs`.
pub(crate) fn shown_default() -> Option<String> {
    diacraft_engine::measure::measure_fonts_now().unwrap_or_else(|| Arc::new(FontDb::bundled())).default_family()
}

// ---- the renderer's choice -------------------------------------------------------------------------------------------------------------

/// A target that keeps the family of each run of letters it is handed.
#[derive(Default)]
struct Families {
    drawn: Vec<String>,
}

impl Target for Families {
    fn to_px(&self) -> Affine {
        Affine::new([100.0, 0.0, 0.0, -100.0, 0.0, 600.0])
    }
    fn size(&self) -> (f64, f64) {
        (800.0, 600.0)
    }
    fn fill(&mut self, _: &BezPath, _: &Paint, _: bool) {}
    fn fill_with_paint_transform(&mut self, _: &BezPath, _: &Paint, _: bool, _: Affine) {}
    fn stroke(&mut self, _: &BezPath, _: &Stroke, _: &Paint) {}
    fn blurred<F: FnOnce(&mut Self)>(&mut self, _: f64, draw: F) {
        draw(self);
    }
    fn push_clip(&mut self, _: &BezPath) {}
    fn pop_clip(&mut self) {}
    fn image(&mut self, _: &Arc<Pixmap>, _: Affine) {}
    fn mark(&self) -> usize {
        0
    }
    fn rewind(&mut self, _: usize) {}
    fn wants_text(&self) -> bool {
        true
    }
    fn text_run(&mut self, run: &TextRun<'_>) -> bool {
        self.drawn.push(run.face.family().to_owned());
        true
    }
}

/// A database of these faces and no others: each covers the ASCII letters, so that no cluster is handed to a face that was not asked for.
fn fonts_of(families: &[&str]) -> FontDb {
    let mut db = FontDb::empty();
    for family in families {
        assert_eq!(db.add_font_data(SynthFont::new(family).ascii(500).build()).added, 1, "{family}");
    }
    db
}

/// The family the renderer draws each run of text of page 1 in, in the order the runs are drawn, with a renderer that has these faces.
fn drawn(doc: &Document, families: &[&str]) -> Vec<String> {
    let mut target = Families::default();
    let skipped = Renderer::with_fonts(fonts_of(families)).draw_onto(doc, 1, &mut target);
    assert!(skipped.iter().all(|s| s.kind != SkipKind::NoSuchPage), "{skipped:?}");
    target.drawn
}

/// The family a face name is drawn with in a database of these faces: what `FontDb::resolve` gives, which is what the renderer's layout asks.
fn family_for(families: &[&str], name: &str) -> String {
    fonts_of(families).resolve(name, REGULAR).face.family().to_owned()
}

// ---- documents -------------------------------------------------------------------------------------------------------------------------

fn page() -> PageBuilder {
    PageBuilder::new().cell("PageWidth", "8").cell("PageHeight", "6").cell("PageScale", "1").cell("DrawingScale", "1")
}

/// A 4 by 1 inch box at (1, 1), the pin at its lower-left corner, showing `Hello` in the run `Cp(0)`.
fn label(id: u32) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", "1")
        .cell("PinY", "1")
        .cell("Width", "4")
        .cell("Height", "1")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .text_parts(vec![TextPart::Cp(0), TextPart::Chars("Hello".into())])
}

fn font_row(shape: ShapeBuilder, ix: u32, font: &str) -> ShapeBuilder {
    shape.row_cell("Character", None, RowKey::Ix(ix), "Font", font)
}

/// A document of one page holding this shape; its root style is `root`.
fn doc_with(shape: ShapeBuilder, root: impl FnOnce(StyleBuilder) -> StyleBuilder) -> Document {
    DocBuilder::new().root(root).page(1, page().shape(shape)).build()
}

/// What the query names for the first run of shape 1, and what the renderer drew the text in: the two must be the one face.
fn agree(doc: &Document, families: &[&str], named: &str) {
    let fonts = fonts_of(families);
    let said = look_of_in(doc, 1, &[1], &fonts).unwrap().remove(0).font;
    assert_eq!(said.as_deref(), Some(named), "the name the query gives");
    let faces = drawn(doc, families);
    assert!(!faces.is_empty(), "the text is drawn");
    let wanted = family_for(families, named);
    assert!(faces.iter().all(|f| *f == wanted), "the query names {named}, which is drawn in {wanted}; the renderer drew {faces:?}");
}

// ---- no cell of the shape names a face ----------------------------------------------------------------------------------------------------

#[test]
fn a_shape_whose_cells_and_styles_name_no_face_is_shown_in_the_face_the_renderer_draws_the_default_in() {
    let doc = doc_with(label(1), |r| r);
    // The default of the database: Liberation Sans over the other faces, which the renderer's layout asks for the unnamed run (`resolve("")`).
    let families = ["Ashlar", "Liberation Sans"];
    assert_eq!(fonts_of(&families).default_family().as_deref(), Some("Liberation Sans"));
    agree(&doc, &families, "Liberation Sans");
    // The machine decides it: with only Ashlar there is no better face to draw it in, and that is the name.
    agree(&doc, &["Ashlar"], "Ashlar");
    // The name is not a constant: it follows the faces.
    agree(&doc, &["Carlito", "Liberation Sans"], "Carlito");
}

#[test]
fn a_font_of_0_or_empty_takes_the_root_styles_face_and_not_the_first_name_of_the_font_table() {
    // The root style says `Other Face`, the table lists `Some Face` first.
    let families = ["Some Face", "Other Face"];
    for own in ["0", ""] {
        let mut doc = doc_with(font_row(label(1), 0, own), |r| r.row_cell("Character", None, RowKey::Ix(0), "Font", "Other Face"));
        Arc::make_mut(&mut doc.fonts).push(FaceName { name_u: Arc::from("Some Face"), raw: Default::default(), ext: Default::default() });
        agree(&doc, &families, "Other Face");
    }
    // No `Font` cell on the shape at all reads the same.
    let doc = doc_with(label(1), |r| r.row_cell("Character", None, RowKey::Ix(0), "Font", "Other Face"));
    agree(&doc, &families, "Other Face");
}

#[test]
fn the_marker_with_no_theme_reads_the_root_styles_face() {
    // The cell is `THEMEVAL()` whose value is the marker, the page selects no theme, and the root style says `Other Face`.
    let shape = label(1).row_formula("Character", None, RowKey::Ix(0), "Font", "THEMEVAL()", "Themed");
    let doc = doc_with(shape, |r| r.row_cell("Character", None, RowKey::Ix(0), "Font", "Other Face"));
    agree(&doc, &["Some Face", "Other Face"], "Other Face");
}

#[test]
fn a_shape_with_no_face_of_its_own_under_a_theme_is_shown_in_the_themes_font() {
    // A theme applied to a shape with text: the shape follows the `Theme` style, whose `Font` is the theme's call, and holds no face itself.
    let mut s = blank_session();
    let id = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 4.0);
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Hello"}));
    run(&mut s, "theme.apply", json!({"theme": "tidepool"}));
    let theme_font = diacraft_engine::theme::catalogue().iter().find(|t| t.id == "tidepool").unwrap().def.minor_font.clone();
    let doc = s.document().unwrap().clone();
    let shape = doc.shape(&doc.find_shape(diacraft_doc::Owner::Page(1), id).unwrap()).unwrap();
    assert!(shape.sheet.section(&SectionKey::new("Character", None)).is_none(), "the shape holds no face of its own");
    // The renderer's theme answer, and the query's, are the theme part's minor font; nobody wrote it into the shape.
    // Carlito is among the faces so that the default (the face with no name) is another face than the theme's.
    let families = [theme_font.as_str(), "Ashlar", "Carlito"];
    let fonts = fonts_of(&families);
    assert_ne!(fonts.default_family().as_deref(), Some(theme_font.as_str()));
    let said = look_of_in(&doc, 1, &[id], &fonts).unwrap().remove(0).font;
    assert_eq!(said.as_deref(), Some(theme_font.as_str()));
    let faces = drawn(&doc, &families);
    assert!(!faces.is_empty());
    assert!(faces.iter().all(|f| *f == family_for(&families, &theme_font)), "{faces:?}");
    // With no theme again the cells name no face, and the default is shown.
    run(&mut s, "theme.apply", json!({"theme": "none"}));
    let doc = s.document().unwrap().clone();
    let said = look_of_in(&doc, 1, &[id], &fonts).unwrap().remove(0).font;
    assert_eq!(said, fonts.default_family());
    let faces = drawn(&doc, &families);
    assert!(faces.iter().all(|f| Some(f) == said.as_ref()), "{faces:?} against {said:?}");
}

// ---- a style, a master, the shape's own cell ----------------------------------------------------------------------------------------------

/// Shape 1 follows style 2 for its text, which names `Style Face`; the root names `Root Face`; the shape's own row 0 names `own` if it is given.
fn styled(own: Option<&str>) -> Document {
    let shape = label(1).links(None, None, Some(2));
    let shape = match own {
        Some(face) => font_row(shape, 0, face),
        None => shape,
    };
    DocBuilder::new()
        .root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Font", "Root Face"))
        .style(2, StyleBuilder::new().name("Quill").links(None, None, Some(0)).row_cell("Character", None, RowKey::Ix(0), "Font", "Style Face"))
        .page(1, page().shape(shape))
        .build()
}

#[test]
fn a_style_that_names_a_face_gives_it_and_the_shapes_own_cell_wins_over_the_style() {
    let families = ["Root Face", "Style Face", "Own Face"];
    agree(&styled(None), &families, "Style Face");
    agree(&styled(Some("Own Face")), &families, "Own Face");
}

#[test]
fn an_instance_whose_master_names_a_face_is_shown_in_it() {
    // The instance holds its place and size; the master's shape holds the text and the face.
    let master = MasterBuilder::new().shape(font_row(label(5), 0, "Master Face"));
    let instance = ShapeBuilder::new(1).master(1).cell("PinX", "1").cell("PinY", "1").cell("Width", "4").cell("Height", "1");
    let doc = DocBuilder::new().master(1, master).page(1, page().shape(instance)).build();
    agree(&doc, &["Master Face", "Other Face"], "Master Face");
    // The instance's own text, in the master's row 0, reads the master's face as well.
    let instance = label(1).master(1);
    let master = MasterBuilder::new().shape(font_row(ShapeBuilder::new(5).cell("Width", "4").cell("Height", "1"), 0, "Master Face"));
    let doc = DocBuilder::new().master(1, master).page(1, page().shape(instance)).build();
    agree(&doc, &["Master Face", "Other Face"], "Master Face");
}

#[test]
fn a_name_is_the_documents_not_the_stand_ins() {
    // Calibri is drawn in Carlito where the machine has no Calibri, and is still Calibri in the Font box (keeps the cell and the table).
    let doc = doc_with(font_row(label(1), 0, "Calibri"), |r| r);
    let families = ["Carlito", "Ashlar"];
    let fonts = fonts_of(&families);
    let said = look_of_in(&doc, 1, &[1], &fonts).unwrap().remove(0).font;
    assert_eq!(said.as_deref(), Some("Calibri"));
    assert_eq!(drawn(&doc, &families), ["Carlito"], "the renderer draws the stand-in");
    assert_eq!(family_for(&families, "Calibri"), "Carlito");
    // And a name no face has, nor a stand-in, is the name as well.
    let doc = doc_with(font_row(label(1), 0, "Zephyr Display"), |r| r);
    let said = look_of_in(&doc, 1, &[1], &fonts).unwrap().remove(0).font;
    assert_eq!(said.as_deref(), Some("Zephyr Display"));
    assert_eq!(drawn(&doc, &families).len(), 1);
}

// ---- runs that differ -------------------------------------------------------------------------------------------------------------------

/// `Aaa Bbb Ccc`: three runs, in rows 0, 1 and 2, whose faces are those given (`None`: no `Font` cell for the row).
fn three_runs(faces: [Option<&str>; 3]) -> Document {
    let parts = vec![
        TextPart::Cp(0),
        TextPart::Chars("Aaa ".into()),
        TextPart::Cp(1),
        TextPart::Chars("Bbb ".into()),
        TextPart::Cp(2),
        TextPart::Chars("Ccc".into()),
    ];
    let mut shape = label(1).text_parts(parts);
    for (ix, face) in (0_u32..).zip(faces) {
        shape = match face {
            Some(face) => font_row(shape, ix, face),
            None => shape.row_cell("Character", None, RowKey::Ix(ix), "Size", "0.1666666666666667"),
        };
    }
    doc_with(shape, |r| r)
}

#[test]
fn runs_that_differ_in_face_are_mixed_and_runs_that_draw_alike_are_not() {
    let families = ["Quill Sans", "Ferro Text", "Liberation Sans"];
    let fonts = fonts_of(&families);
    let seen = |doc: &Document, from: usize, to: usize| look_of_range_in(doc, 1, &[1], from, to, &fonts).unwrap();
    let dedup = |mut faces: Vec<String>| {
        faces.dedup();
        faces
    };
    // The second run names the first run's face in other capitals (matches faces so): the two draw alike, so they are one value.
    let doc = three_runs([Some("Quill Sans"), Some("quill sans"), Some("Ferro Text")]);
    let alike = seen(&doc, 0, 8);
    assert_eq!((alike.look.font.as_deref(), alike.mixed.font), (Some("Quill Sans"), false));
    assert_eq!(dedup(drawn(&doc, &families)), ["Quill Sans", "Ferro Text"], "the renderer draws the first two runs in one face");
    // The third differs from them.
    assert!(seen(&doc, 0, 11).mixed.font);
    assert!(seen(&doc, 4, 11).mixed.font);
    let third = seen(&doc, 8, 11);
    assert_eq!((third.look.font.as_deref(), third.mixed.font), (Some("Ferro Text"), false));
    // A run that names no face and one that names the default draw alike, and are not mixed: both are shown as the default.
    let doc = three_runs([None, Some("Liberation Sans"), None]);
    let together = seen(&doc, 0, 8);
    assert_eq!(together.look.font.as_deref(), Some("Liberation Sans"));
    assert!(!together.mixed.font, "the default is the face the second run names");
    assert_eq!(dedup(drawn(&doc, &families)), ["Liberation Sans"]);
    // One that names another face is mixed with an unnamed one.
    let doc = three_runs([None, Some("Ferro Text"), None]);
    assert!(seen(&doc, 0, 8).mixed.font);
    assert_eq!(seen(&doc, 0, 4).look.font.as_deref(), Some("Liberation Sans"));
    assert_eq!(dedup(drawn(&doc, &families)), ["Liberation Sans", "Ferro Text", "Liberation Sans"]);
}

// ---- through the command ----------------------------------------------------------------------------------------------------------------

#[test]
fn the_command_answers_a_name_for_every_shape_and_never_null() {
    let mut s = blank_session();
    let id = crate::edit::drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let said = run(&mut s, "shape.formatting", json!({"ids": [id]}))["shapes"][0]["font"].clone();
    // The shape has no text and names no face; the default's name is a string.
    assert_eq!(said, json!(shown_default()));
    assert!(said.is_string(), "{said}");
    run(&mut s, "text.format", json!({"ids": [id], "font": "Arial"}));
    assert_eq!(run(&mut s, "shape.formatting", json!({"ids": [id]}))["shapes"][0]["font"], "Arial");
    assert_eq!(run(&mut s, "shape.formatting", json!({"ids": [id], "start": 0, "end": 0}))["shapes"][0]["font"], "Arial");
}
