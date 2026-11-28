//! The root style states what the renderer assumes: stating it changes nothing the renderer draws.
//!
//! `diacraft_doc::root_text::CELLS` are the text cells a root holds, each with the value the renderer assumes where nothing on a shape's way says. Two things must hold, and both are
//! here, where the renderer and the model meet:
//!
//! * **The cells are the renderer's own defaults** (`diacraft_text`'s `ParaFormat`, `BlockFormat` and `CharFormat`), so that a reader of a file that takes the root's value and a renderer that
//!   takes its own read one value.
//! * **A drawing with the cells and the same drawing without them are drawn alike**, pixel for pixel, for text that leans on the root for its alignment, its spacing, its tab stops and its
//!   size.
//!
//! The face is built in code (`quill`: every letter 0.6 of an em wide), so no machine font enters.

use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::root_text::{CELLS, RootCell, Scope, complete};
use diacraft_doc::{Document, RowKey, SharedVec};
use diacraft_render::Renderer;
use diacraft_testkit::build::{ShapeBuilder, StyleBuilder};
use diacraft_text::{BlockFormat, CharFormat, ParaFormat};

use crate::support::*;

fn quill() -> Renderer {
    let mut db = diacraft_fonts::FontDb::bundled();
    db.add_font_data(diacraft_fonts::synth::SynthFont::new("quill").ascii(600).build());
    Renderer::with_fonts(db)
}

// ----------------------------------------------------------------------------------------------------------------------- the same values

/// What the renderer assumes for `cell`, as a number: the default of its format.
fn assumed(cell: &RootCell) -> f64 {
    let (block, character, paragraph) = (BlockFormat::default(), CharFormat::default(), ParaFormat::default());
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    match (cell.section, cell.name) {
        (None, "VerticalAlign") => f64::from(block.vertical_align),
        (None, "DefaultTabStop") => block.default_tab_stop,
        (None, "TextBkgnd") => flag(block.background.is_some()),
        (None, "TextBkgndTrans") => 0.0,
        (None, "TextDirection") => f64::from(block.text_direction),
        (Some("Character"), "Size") => character.size,
        (Some("Character"), "Color") => flag(character.color != Rgba::BLACK),
        (Some("Character"), "Style") => f64::from(character.style),
        (Some("Character"), "Case") => f64::from(character.case),
        (Some("Character"), "Pos") => f64::from(character.pos),
        (Some("Character"), "FontScale") => character.scale,
        (Some("Character"), "Letterspace") => character.letterspace,
        (Some("Character"), "DblUnderline") => flag(character.double_underline),
        (Some("Character"), "Overline") => flag(character.overline),
        (Some("Character"), "Strikethru") => flag(character.strikethrough),
        (Some("Character"), "DoubleStrikethrough") => flag(character.double_strikethrough),
        (Some("Character"), "ComplexScriptSize") => character.complex_size,
        (Some("Paragraph"), "Bullet") => f64::from(paragraph.bullet),
        // A bullet face of 0 and a bullet string that is empty are what no cell gives (`face_name` reads 0 as none).
        (Some("Paragraph"), "BulletFont") => flag(!paragraph.bullet_font.is_empty()),
        (Some("Paragraph"), "BulletFontSize") => paragraph.bullet_size,
        (Some("Paragraph"), "BulletStr") => flag(!paragraph.bullet_string.is_empty()),
        (Some("Paragraph"), "Flags") => f64::from(paragraph.flags),
        (Some("Paragraph"), "HorzAlign") => f64::from(paragraph.halign),
        (Some("Paragraph"), "IndFirst") => paragraph.ind_first,
        (Some("Paragraph"), "IndLeft") => paragraph.ind_left,
        (Some("Paragraph"), "IndRight") => paragraph.ind_right,
        (Some("Paragraph"), "SpAfter") => paragraph.sp_after,
        (Some("Paragraph"), "SpBefore") => paragraph.sp_before,
        (Some("Paragraph"), "SpLine") => paragraph.sp_line,
        (section, name) => panic!("a cell of the root with no default of the renderer's to hold it to: {section:?} {name}"),
    }
}

#[test]
fn every_cell_the_root_states_is_the_value_the_renderer_assumes() {
    for cell in CELLS {
        // An empty value is the text of a cell that is empty (`BulletStr`): the renderer's empty string.
        let stated: f64 = if cell.value.is_empty() {
            0.0
        } else {
            cell.value.parse().unwrap_or_else(|_| panic!("{:?} {} is not a number", cell.section, cell.name))
        };
        let want = assumed(cell);
        assert!((stated - want).abs() < 1e-6, "{:?} {}: the root says {stated}, the renderer assumes {want}", cell.section, cell.name);
    }
}

#[test]
fn the_cells_a_new_drawings_root_holds_are_all_of_them() {
    let doc = Document::new_blank();
    let root = doc.style(0).expect("a root");
    assert!(diacraft_doc::root_text::missing(root, Scope::Whole).is_empty(), "the root of a new drawing states every cell of the table");
}

// ----------------------------------------------------------------------------------------------------------------------- drawn alike

/// Shape `id`, 2 inches by 1 at (1, `y`), with no fill and no line, showing `text` in `quill` at 0.2 inch. **It holds no cell for the alignment, the spacing or the stops**: they come from the root.
fn leaning(id: u32, y: f64, text: &str) -> ShapeBuilder {
    rect(id, 1.0, y, 2.0, 0.8)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.2")
        .row_cell("Character", None, RowKey::Ix(0), "Font", "quill")
}

/// A page of shapes that lean on the root in each way the cells say, and one that does not.
fn shapes() -> Vec<ShapeBuilder> {
    vec![
        // The alignment, the line spacing and the margins of the root.
        leaning(1, 0.1, "AAAA"),
        leaning(2, 1.0, "AAAA\nAA\nAAAAAA"),
        // A paragraph row that holds an indent and no alignment: the row merges with the root's, cell by cell.
        leaning(3, 1.9, "AAAA").row_cell("Paragraph", None, RowKey::Ix(0), "IndLeft", "0.3"),
        // A tab: the root's stops.
        leaning(4, 2.8, "A\tAA\tA"),
        // A shape that says its own alignment, and one that follows a style that does.
        leaning(5, 3.4, "AAAA").row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0"),
        leaning(6, 4.2, "AAAA").links(None, None, Some(1)),
    ]
}

/// A page, 4 inches wide and 5.2 tall, with those shapes, a text style that right-aligns, and `root` as style 0.
fn drawing(root: Option<&Arc<diacraft_doc::Style>>) -> Document {
    let mut p = page(4.0, 5.2);
    for s in shapes() {
        p = p.shape(s);
    }
    let right = StyleBuilder::new().name("Right").row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "2");
    let mut doc = base().style(1, right).page(1, p).build();
    if let Some(root) = root {
        let mut styles: Vec<Arc<diacraft_doc::Style>> = doc.styles.iter().cloned().collect();
        if let Some(first) = styles.iter_mut().find(|s| s.id == 0) {
            *first = Arc::clone(root);
        }
        doc.styles = SharedVec::from_vec(styles);
    }
    doc
}

fn pixels(doc: &Document) -> (Vec<u8>, usize) {
    let pic = draw_with(&mut quill(), doc);
    (pic.r.rgba_premul.clone(), pic.ink_pixels())
}

#[test]
fn a_root_that_states_the_cells_draws_the_text_as_a_root_that_does_not() {
    let bare = drawing(None);
    let root = bare.style(0).expect("a root").clone();
    assert!(!diacraft_doc::root_text::missing(&root, Scope::Whole).is_empty(), "the root of the support builder holds no Paragraph section");
    let stated = complete(&root, Scope::Whole).expect("cells are missing");
    let (before, ink_before) = pixels(&bare);
    let (after, ink_after) = pixels(&drawing(Some(&Arc::new(stated))));
    assert!(ink_before > 500, "text is drawn: {ink_before} pixels");
    assert_eq!(ink_before, ink_after);
    assert!(before == after, "the pictures differ though the root only states what the renderer assumed");
}

#[test]
fn the_cells_a_writer_adds_for_the_reading_change_no_pixel_either() {
    let bare = drawing(None);
    let root = bare.style(0).expect("a root").clone();
    let reading = complete(&root, Scope::Reading).expect("cells are missing");
    assert!(
        diacraft_doc::root_text::missing(&reading, Scope::Reading).is_empty() && !diacraft_doc::root_text::missing(&reading, Scope::Whole).is_empty(),
        "only the cells of the reading are added"
    );
    assert!(pixels(&bare).0 == pixels(&drawing(Some(&Arc::new(reading)))).0);
}

#[test]
fn text_with_no_alignment_anywhere_is_centred_with_and_without_the_cells() {
    // The reason for the cells: the first shape of the page, 1 to 3 inches across, draws AAAA (0.48 inch) about 2.0.
    let bare = drawing(None);
    let stated = complete(bare.style(0).expect("a root"), Scope::Whole).expect("cells are missing");
    for doc in [bare, drawing(Some(&Arc::new(stated)))] {
        let pic = draw_with(&mut quill(), &doc);
        let runs = pic.dark_runs_along_x(0.5, 0.0, 4.0);
        let (first, last) = (runs.first().expect("ink"), runs.last().expect("ink"));
        let (left, right) = (first.0 as f64 / PPI, (last.0 + last.1) as f64 / PPI);
        assert!(((left + right) / 2.0 - 2.0).abs() < 0.03, "centred about 2.0 inches: from {left} to {right}");
    }
}

// ----------------------------------------------------------------------------------------------------------------------- a row past the first

/// A text row's own cells: `(section, IX, [(cell, value)])`.
type OwnRow = (&'static str, u32, &'static [(&'static str, &'static str)]);

/// The cells each text row past the first of [`runs_and_paragraphs`] holds of its own.
const OWN_ROWS: [OwnRow; 4] = [
    ("Character", 1, &[("Size", "0.15")]),
    ("Character", 2, &[("Size", "0.25"), ("Font", "quill"), ("Pos", "1")]),
    ("Paragraph", 1, &[("IndLeft", "0.2")]),
    ("Paragraph", 2, &[("SpBefore", "0.05"), ("HorzAlign", "2")]),
];

/// A shape whose text has three runs and three paragraphs, each past the first with a row that holds a few cells of its own; with `whole`, each such row also holds every other cell of
/// `row_cells`, with the value the table gives.
fn runs_and_paragraphs(whole: bool) -> Document {
    use diacraft_doc::TextPart::{Chars, Cp, Pp};
    let chars = |s: &str| Chars(Arc::from(s));
    let parts = vec![
        Cp(0),
        Pp(0),
        chars("AAAA "),
        Cp(1),
        chars("AAA"),
        Cp(0),
        chars(" AA\n"),
        Pp(1),
        Cp(2),
        chars("AAAAAA\n"),
        Pp(2),
        Cp(1),
        chars("AA AAAA"),
    ];
    let mut shape = leaning(1, 0.2, "").text_parts(parts);
    for (section, ix, cells) in OWN_ROWS {
        for (name, value) in cells {
            shape = shape.row_cell(section, None, RowKey::Ix(ix), name, value);
        }
        if whole {
            for cell in diacraft_doc::root_text::row_cells(section).filter(|c| !cells.iter().any(|(n, _)| *n == c.name)) {
                shape = shape.row_cell(section, None, RowKey::Ix(ix), cell.name, cell.value);
            }
        }
    }
    let p = page(4.0, 2.0).shape(shape);
    base().page(1, p).build()
}

/// **A row past the first made whole is drawn as it was**: the renderer draws a cell such a row lacks with the value it assumes, which is the value the table gives, so stating
/// it changes no pixel. A reader of the file that reads a lacking cell as 0 draws what the renderer does only when it is stated.
#[test]
fn a_row_past_the_first_made_whole_draws_the_text_as_it_did() {
    let (before, ink_before) = pixels(&runs_and_paragraphs(false));
    let (after, ink_after) = pixels(&runs_and_paragraphs(true));
    assert!(ink_before > 500, "text is drawn: {ink_before} pixels");
    assert_eq!(ink_before, ink_after);
    assert!(before == after, "the pictures differ though the rows only state what the renderer assumed");
}
