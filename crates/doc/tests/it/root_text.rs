//! The root style of a new drawing states every text cell the program assumes when no cell says.
//! A file whose root holds no `Paragraph` section is read by the reference application with the
//! default of the cell's kind, 0 (left), where this program centres: the root says what is assumed, so that both read one value.
//!
//! The inputs are the model's own: a new document, and styles made here by hand.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, RowKey, Section, SectionKey, Sheet, Style, Value, root_text};

/// The text of the cell `name` in row 0 of the root's section `section`, or directly under the root when `section` is `None`.
fn root_cell(doc: &Document, section: Option<&str>, name: &str) -> Option<String> {
    style_cell(doc.style(0)?, section, name)
}

fn style_cell(style: &Style, section: Option<&str>, name: &str) -> Option<String> {
    let cell = match section {
        None => style.sheet.cell(name)?,
        Some(section) => style.sheet.section(&SectionKey::new(section, None))?.row(&RowKey::Ix(0))?.cells.get(name)?,
    };
    cell.value.as_ref().map(|v| v.text().to_owned())
}

/// The cells of the root's `Paragraph` row 0 that lists, with the value the root holds in every file of its census, but for `TextPosAfterBullet`
/// (0), which is left out: a reader takes a cell that is missing as 0, and an OpenDocument style has no attribute for it.
const PARAGRAPH: [(&str, &str); 12] = [
    ("Bullet", "0"),
    ("BulletFont", "0"),
    ("BulletFontSize", "-1"),
    ("BulletStr", ""),
    ("Flags", "0"),
    ("HorzAlign", "1"),
    ("IndFirst", "0"),
    ("IndLeft", "0"),
    ("IndRight", "0"),
    ("SpAfter", "0"),
    ("SpBefore", "0"),
    ("SpLine", "-1.2"),
];

#[test]
fn a_new_documents_root_holds_a_paragraph_row_that_centres_and_spaces_lines_as_the_renderer_assumes() {
    let doc = Document::new_blank();
    for (name, value) in PARAGRAPH {
        assert_eq!(root_cell(&doc, Some("Paragraph"), name).as_deref(), Some(value), "Paragraph row 0, {name}");
    }
}

#[test]
fn a_new_documents_root_holds_all_nine_text_block_cells() {
    let doc = Document::new_blank();
    // The four margins and the vertical alignment are the ones the root always held (4 points, the middle).
    for (name, value) in [
        ("LeftMargin", "0.05555556"),
        ("RightMargin", "0.05555556"),
        ("TopMargin", "0.05555556"),
        ("BottomMargin", "0.05555556"),
        ("VerticalAlign", "1"),
        ("DefaultTabStop", "0.5"),
        ("TextBkgnd", "0"),
        ("TextBkgndTrans", "0"),
        ("TextDirection", "0"),
    ] {
        assert_eq!(root_cell(&doc, None, name).as_deref(), Some(value), "{name}");
    }
}

#[test]
fn a_new_documents_root_holds_the_character_cells_whose_absence_would_read_as_0() {
    let doc = Document::new_blank();
    for (name, value) in [
        ("Size", "0.1666667"),
        ("Color", "0"),
        ("Style", "0"),
        ("Case", "0"),
        ("Pos", "0"),
        ("FontScale", "1"),
        ("Letterspace", "0"),
        ("DblUnderline", "0"),
        ("Overline", "0"),
        ("Strikethru", "0"),
        ("DoubleStrikethrough", "0"),
        ("ComplexScriptSize", "-1"),
    ] {
        assert_eq!(root_cell(&doc, Some("Character"), name).as_deref(), Some(value), "Character row 0, {name}");
    }
    // A face and a language are the person's, not the program's: the root names none.
    for name in ["Font", "AsianFont", "ComplexScriptFont", "LangID"] {
        assert_eq!(root_cell(&doc, Some("Character"), name), None, "{name} is left to the editing language and the font database");
    }
}

// ---- completing a root that lacks them ----------------------------------------------------------------------------------------

/// A root with the old content a new drawing held before the `Paragraph` row: a `Character` row with a size and a colour, a vertical alignment and the margins.
fn old_root() -> Style {
    let mut sheet = Sheet::default();
    for (name, value) in [("VerticalAlign", "1"), ("LeftMargin", "0.05555556"), ("FillPattern", "1")] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    let mut row = Row { ix: Some(0), ..Row::default() };
    for (name, value) in [("Size", "0.1666667"), ("Color", "0")] {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    let mut character = Section::new("Character", None);
    character.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(character));
    Style { id: 0, name: Some(Arc::from("No Style")), name_u: Some(Arc::from("No Style")), sheet: Arc::new(sheet), ..Style::default() }
}

#[test]
fn completing_for_the_reading_adds_only_the_cells_whose_absence_reads_otherwise() {
    let old = old_root();
    let done = root_text::complete(&old, root_text::Scope::Reading).expect("the root lacks cells");
    // What a file's reader would read as 0 where this program assumes another value (against): added with the assumed value.
    for (section, name, value) in [
        (Some("Paragraph"), "HorzAlign", "1"),
        (Some("Paragraph"), "SpLine", "-1.2"),
        (Some("Paragraph"), "BulletFontSize", "-1"),
        (Some("Character"), "FontScale", "1"),
        (Some("Character"), "ComplexScriptSize", "-1"),
        (None, "DefaultTabStop", "0.5"),
    ] {
        assert_eq!(style_cell(&done, section, name).as_deref(), Some(value), "{section:?} {name}");
    }
    // A cell whose absence reads as it is assumed (0, none) is not added: the file would say what it did not need to.
    for (section, name) in [
        (Some("Paragraph"), "IndLeft"),
        (Some("Paragraph"), "Flags"),
        (Some("Paragraph"), "Bullet"),
        (Some("Character"), "Style"),
        (Some("Character"), "Strikethru"),
        (None, "TextBkgnd"),
        (None, "TextDirection"),
    ] {
        assert_eq!(style_cell(&done, section, name), None, "{section:?} {name} is not added for the reading");
    }
    // What the root held is as it was.
    assert_eq!(style_cell(&done, Some("Character"), "Size").as_deref(), Some("0.1666667"));
    assert_eq!(style_cell(&done, None, "LeftMargin").as_deref(), Some("0.05555556"));
}

#[test]
fn the_root_a_new_drawing_held_before_is_told_from_a_root_a_file_holds() {
    // DiaCraft's own earlier root: no `Paragraph` section and a `Character` row 0 of a size and a colour.
    assert!(root_text::is_an_earlier_root(&old_root()));
    // Not the root of a new drawing as it is, which holds the whole of it.
    assert!(!root_text::is_an_earlier_root(Document::new_blank().style(0).expect("a root")));
    // Not a root with a `Paragraph` section, whatever else it holds.
    let mut with_paragraph = old_root();
    let mut sheet = Sheet::clone(&with_paragraph.sheet);
    sheet.sections.push(Arc::new(Section::new("Paragraph", None)));
    with_paragraph.sheet = Arc::new(sheet);
    assert!(!root_text::is_an_earlier_root(&with_paragraph));
    // Not a sparse root that holds no `Character` row (a file of another program, a hand-made one): its lack is the file's.
    assert!(!root_text::is_an_earlier_root(&Style { sheet: Arc::new(Sheet::default()), ..old_root() }));
    // Not a root whose `Character` row 0 holds a cell the earlier root never did: that root is somebody's own.
    let mut row = Row { ix: Some(0), ..Row::default() };
    for name in ["Size", "Color", "Font"] {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text("0")));
    }
    let mut character = Section::new("Character", None);
    character.rows.push(Arc::new(row));
    let mut sheet = Sheet::default();
    sheet.sections.push(Arc::new(character));
    assert!(!root_text::is_an_earlier_root(&Style { sheet: Arc::new(sheet), ..old_root() }));
    // And not a root of another name.
    assert!(!root_text::is_an_earlier_root(&Style { name_u: Some(Arc::from("Normal")), ..old_root() }));
}

#[test]
fn completing_for_the_whole_adds_every_cell_and_a_root_that_has_them_all_is_left_alone() {
    let old = old_root();
    let done = root_text::complete(&old, root_text::Scope::Whole).expect("the root lacks cells");
    for (name, value) in PARAGRAPH {
        assert_eq!(style_cell(&done, Some("Paragraph"), name).as_deref(), Some(value), "{name}");
    }
    assert!(root_text::complete(&done, root_text::Scope::Whole).is_none(), "nothing is missing: no new style");
    assert!(root_text::complete(&done, root_text::Scope::Reading).is_none());
}

#[test]
fn a_cell_the_root_holds_is_never_changed_and_a_formula_stays_a_formula() {
    let mut style = old_root();
    let mut sheet = Sheet::clone(&style.sheet);
    let mut row = Row { ix: Some(0), ..Row::default() };
    row.cells.insert(CellName::new("HorzAlign"), Cell::with_formula("0", Some(Value::from_text("0"))));
    row.cells.insert(CellName::new("SpLine"), Cell::constant(Value::from_text("-1")));
    let mut paragraph = Section::new("Paragraph", None);
    paragraph.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(paragraph));
    style.sheet = Arc::new(sheet);
    let done = root_text::complete(&style, root_text::Scope::Reading).expect("BulletFontSize and others are missing");
    assert_eq!(style_cell(&done, Some("Paragraph"), "HorzAlign").as_deref(), Some("0"), "the file's own left alignment stands");
    assert_eq!(style_cell(&done, Some("Paragraph"), "SpLine").as_deref(), Some("-1"));
    assert_eq!(style_cell(&done, Some("Paragraph"), "BulletFontSize").as_deref(), Some("-1"), "a missing cell of the same row is added");
    let held = done.sheet.section(&SectionKey::new("Paragraph", None)).and_then(|s| s.row(&RowKey::Ix(0))).and_then(|r| r.cells.get("HorzAlign"));
    assert!(held.is_some_and(Cell::owns_formula), "the formula is kept verbatim");
    assert_eq!(done.sheet.sections.iter().filter(|s| s.name.as_str() == "Paragraph").count(), 1, "no second section");
}

// ----------------------------------------------------------------------------------------------------------------------- a row past the first

/// The 12 cells of a `Character` row that name no face and no language, with the values the root states.
const CHARACTER: [(&str, &str); 12] = [
    ("Size", "0.1666667"),
    ("Color", "0"),
    ("Style", "0"),
    ("Case", "0"),
    ("Pos", "0"),
    ("FontScale", "1"),
    ("Letterspace", "0"),
    ("DblUnderline", "0"),
    ("Overline", "0"),
    ("Strikethru", "0"),
    ("DoubleStrikethrough", "0"),
    ("ComplexScriptSize", "-1"),
];

fn sorted(cells: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = cells.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())).collect();
    out.sort();
    out
}

fn texts(cells: &diacraft_doc::Cells) -> Vec<(String, String)> {
    cells.iter().map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())).collect()
}

/// A row past the first states the cells the root states in its section, and no others: no face, no language, no `ColorTrans` and no `TextPosAfterBullet`.
#[test]
fn a_row_past_the_first_states_the_cells_the_root_states_in_its_section() {
    let names = |section: &str| root_text::row_cells(section).map(|c| (c.name.to_owned(), c.value.to_owned())).collect::<Vec<_>>();
    let mut character = names("Character");
    character.sort();
    assert_eq!(character, sorted(&CHARACTER));
    let mut paragraph = names("Paragraph");
    paragraph.sort();
    assert_eq!(paragraph, sorted(&PARAGRAPH));
    assert!(names("Tabs").is_empty() && names("Geometry").is_empty() && names("character").is_empty(), "a text section only, by its exact name");
}

/// A title run's row (the org chart's: its size and weight alone) is given the 9 cells of the table it lacks, each with the value the renderer assumes; what it held stays, a formula as a formula.
#[test]
fn a_row_past_the_first_is_given_the_cells_it_lacks_and_keeps_its_own() {
    let mut cells = diacraft_doc::Cells::new();
    cells.insert(CellName::new("Size"), Cell::constant(Value::from_text("0.111111111")).with_unit("IN"));
    cells.insert(CellName::new("Style"), Cell::constant(Value::from_text("0")));
    cells.insert(CellName::new("Color"), Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed"))));
    cells.insert(CellName::new("LangID"), Cell::constant(Value::from_text("fr-FR")));
    assert_eq!(root_text::complete_row_cells("Character", &mut cells), 9);
    let mut want = sorted(&CHARACTER);
    for (name, value) in [("Size", "0.111111111"), ("Style", "0"), ("Color", "Themed")] {
        want.iter_mut().find(|(n, _)| n == name).expect("a cell of the table").1 = value.to_owned();
    }
    want.push(("LangID".to_owned(), "fr-FR".to_owned()));
    want.sort();
    assert_eq!(texts(&cells), want);
    assert!(cells.get("Color").is_some_and(Cell::owns_formula), "the formula is kept");
    assert_eq!(cells.get("Size").and_then(|c| c.unit.as_deref()), Some("IN"), "the unit is kept");
    assert_eq!(root_text::complete_row_cells("Character", &mut cells), 0, "a whole row is given nothing");
    let mut tabs = diacraft_doc::Cells::new();
    assert_eq!(root_text::complete_row_cells("Tabs", &mut tabs), 0, "a section of no table is given nothing");
}

/// A row of the model is given the cells through `Row::put_cell`, so a node kept after its last cell stays after the cells, and a cell the caller says no to (one a master's row of the same `IX`
/// gives) is not put in.
#[test]
fn a_row_of_the_model_is_given_the_cells_wanted_with_its_nodes_in_place() {
    use diacraft_doc::{Passthrough, Placed, RawNode};
    let mut row = Row { ix: Some(1), ..Row::default() };
    row.cells.insert(CellName::new("HorzAlign"), Cell::constant(Value::from_text("2")));
    row.passthrough = Passthrough::from_nodes(vec![Placed { after: 1, node: RawNode::Comment(Arc::from("after the cell")) }]);
    assert!(root_text::row_lacks("Paragraph", &row));
    let added = root_text::complete_row_with("Paragraph", &mut row, |cell| cell.name != "SpLine");
    assert_eq!(added, 10, "11 lacking, one left to the master");
    assert!(!row.cells.contains_key("SpLine"));
    assert_eq!(row.cells.get("HorzAlign").and_then(|c| c.value.as_ref()).map(Value::text), Some("2"));
    assert_eq!(row.passthrough.iter().map(|p| p.after).collect::<Vec<_>>(), [11], "the node stays after the row's last cell");
    assert_eq!(root_text::complete_row_with("Paragraph", &mut row, |_| true), 1);
    assert!(!root_text::row_lacks("Paragraph", &row));
}

/// **Whose rows a writer may state**. A drawing that came from no VSDX package holds the rows this program and the `.odg` reader made; a drawing that came from a package holds
/// them only with the root an earlier build made, and with any other root it holds the file's own rows.
#[test]
fn the_rows_of_a_drawing_are_ours_to_state_unless_a_vsdx_package_with_another_root_gave_them() {
    use diacraft_doc::{CellAddr, Format, Part, PartRole, RawElement, VsdxHeld};
    let part = |format: Format| Part {
        format,
        name: Arc::from("part.bin"),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Thumbnail,
        manifest: Default::default(),
        rels: Arc::default(),
        bytes: Arc::from(&b"x"[..]),
    };
    // A new drawing, and a drawing with a package of the `.odg` format, came from no VSDX package.
    let mut doc = Document::new_blank();
    assert!(!root_text::came_from_a_vsdx_package(&doc));
    assert!(root_text::rows_are_ours_to_state(&doc));
    doc.parts.push(Arc::new(part(Format::Odf)));
    assert!(root_text::rows_are_ours_to_state(&doc), "an .odg's package is no VSDX package");
    // A package of the VSDX format with the root of a new drawing: the file's own root, the file's own rows.
    doc.parts.push(Arc::new(part(Format::Vsdx)));
    assert!(root_text::came_from_a_vsdx_package(&doc));
    assert!(!root_text::rows_are_ours_to_state(&doc), "a VSDX package with a root that states the text cells");
    // The root an earlier build made: `No Style`, no `Paragraph` section, and a `Character` row 0 of a size and a colour. It is DiaCraft's own, and so are the rows of the file.
    let mut earlier = Document::new_blank();
    earlier.parts.push(Arc::new(part(Format::Vsdx)));
    let at = earlier.styles.iter().position(|s| s.id == 0).expect("the root");
    let mut root = Style::clone(earlier.style(0).expect("the root"));
    root.name_u = Some(Arc::from("No Style"));
    let mut sheet = Sheet::default();
    for (name, value) in [("Size", "0.1666667"), ("Color", "0")] {
        sheet.set_cell(&CellAddr::in_row("Character", None, RowKey::Ix(0), name), Cell::constant(Value::from_text(value))).unwrap();
    }
    root.sheet = Arc::new(sheet);
    assert!(root_text::is_an_earlier_root(&root));
    earlier.styles.set(at, Arc::new(root)).unwrap();
    assert!(root_text::rows_are_ours_to_state(&earlier), "a VSDX package with the root of an earlier build");
    // Window settings alone are no package.
    let mut windowed = Document::new_blank();
    windowed.vsdx = Some(Arc::new(VsdxHeld {
        windows: Some(Arc::new(RawElement { name: Arc::from("Windows"), attrs: Default::default(), children: Vec::new() })),
        ..VsdxHeld::default()
    }));
    assert!(!root_text::came_from_a_vsdx_package(&windowed));
    assert!(root_text::rows_are_ours_to_state(&windowed));
}

/// A row an `.odg` gave is completed with the value the format reads: a paragraph that states no alignment is at the left, 0, and every other cell is the table's.
#[test]
fn a_row_of_an_odg_is_given_the_alignment_the_format_reads_and_the_rest_of_the_table() {
    for cell in root_text::row_cells("Paragraph") {
        let want = if cell.name == "HorzAlign" { "0" } else { cell.value };
        assert_eq!(root_text::odf_row_value(cell), want, "{}", cell.name);
    }
    for cell in root_text::row_cells("Character") {
        assert_eq!(root_text::odf_row_value(cell), cell.value, "{}", cell.name);
    }
    let mut row = Row { ix: Some(1), ..Row::default() };
    row.cells.insert(CellName::new("IndLeft"), Cell::constant(Value::from_text("0.4")));
    let added = root_text::complete_row_valued("Paragraph", &mut row, |cell| Some(root_text::odf_row_value(cell)));
    assert_eq!(added, 11);
    assert_eq!(row.cells.get("HorzAlign").and_then(|c| c.value.as_ref()).map(Value::text), Some("0"));
    assert_eq!(row.cells.get("SpLine").and_then(|c| c.value.as_ref()).map(Value::text), Some("-1.2"));
    assert_eq!(row.cells.get("IndLeft").and_then(|c| c.value.as_ref()).map(Value::text), Some("0.4"), "the cell the row held is never changed");
    // A cell the caller gives no value for is left out.
    let mut other = Row { ix: Some(1), ..Row::default() };
    assert_eq!(root_text::complete_row_valued("Paragraph", &mut other, |cell| (cell.name == "SpLine").then_some("-1")), 1);
    assert_eq!(other.cells.len(), 1);
}
