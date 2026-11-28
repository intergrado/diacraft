//! What makes two cells the same for the check: values by their kind (a number is a number in any unit, a colour is a colour in any spelling), formulas by their form, and the class a
//! cell that differs is told in.
//!
//! **The same content, by the rulings** (for a VSDX drawing through an `.odg`, for an `.odg`): a unit is a way a file states a length, so two cells that read as one number are
//! one value; a formula is written in a form the format holds (`Width*1` for `Width`), and two forms that differ only in blanks, case and a multiplication by one are one formula; a cell a
//! style sheet or a master gives the shape is the same value as the shape holding it. These are told once as a group, `as_values`, and never as a loss. **A formula that does not come
//! back is a loss, even when its value does**, and is counted.

use diacraft_doc::{Cell, CellClass, Document, Family, FormulaSlot, ValueKind, cell_info};

use super::class::Class;

/// How close two numbers must be to be one value: the writers write the shortest decimal that reads back to the value, so the numbers are the same to the last bits, and this is far wider
/// than a rounding and far narrower than any change a person makes.
const CLOSE: f64 = 1e-8;

/// What two cells' values are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Same {
    /// The same text.
    Exactly,
    /// The same value in another form: a unit, a spelling of a colour, case.
    InAnotherForm,
    /// Not the same.
    No,
}

/// The kind of value a cell holds: the catalogue's, and for a cell it does not list, what its unit and its value say.
pub(crate) fn kind_of(section: Option<&str>, name: &str, cell: &Cell) -> ValueKind {
    let info = cell_info(section, name);
    if info.listed {
        return info.kind;
    }
    if let Some(kind) = cell.unit.as_deref().and_then(ValueKind::from_unit) {
        return kind;
    }
    if cell.value.as_ref().is_some_and(|v| v.text().starts_with('#')) { ValueKind::Color } else { ValueKind::Scalar }
}

/// The number a text reads as, with the truth words read as 1 and 0.
fn number_of(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") {
        return Some(1.0);
    }
    if text.eq_ignore_ascii_case("false") {
        return Some(0.0);
    }
    diacraft_doc::Value::from_text(text).number()
}

/// Whether two numbers are one value.
pub(crate) fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= CLOSE * (1.0 + a.abs().max(b.abs()))
}

/// The index of the colour table a text names (a colour of the fixed palette, 0 to 23, or an `IX` of the document's table): a whole number from 0 to 100,000, or `None`.
fn colour_index(text: &str) -> Option<i64> {
    let index = diacraft_doc::Value::from_text(text.trim()).number()?;
    if index.fract() != 0.0 || !(0.0..=100_000.0).contains(&index) {
        return None;
    }
    Some(index as i64)
}

/// The colour a text names: `#RRGGBB`, or an index of the fixed palette (no table is read for one) or of the document's colour table (`table`, which a text past the palette needs: [`needs`]).
fn colour_of(table: Option<&ColourTable<'_>>, text: &str) -> Option<[u8; 4]> {
    let text = text.trim();
    if text.starts_with('#') {
        return diacraft_color::Rgba::from_hex(text).map(|c| [c.r, c.g, c.b, c.a]);
    }
    let index = colour_index(text)?;
    let colour = diacraft_color::fixed_color(index).or_else(|| table.and_then(|t| t.colour(index)))?;
    Some([colour.r, colour.g, colour.b, colour.a])
}

/// The colours of a document's table by their `IX`, made in one pass for a walk that looks many cells up: [`diacraft_doc::Document::color`]'s answer in one probe a cell, where it walks the whole table
/// for each (the neighbour of the security scan's finding on for the faces). The caller pays for the pass, a step for each colour.
pub(crate) struct ColourTable<'d> {
    /// The `RGB` of the first entry of each `IX`, the one [`diacraft_doc::Document::color`] finds.
    by_ix: std::collections::HashMap<u32, &'d str>,
}

impl<'d> ColourTable<'d> {
    pub(crate) fn of(doc: &'d Document) -> ColourTable<'d> {
        let mut by_ix = std::collections::HashMap::with_capacity(doc.colors.len());
        for entry in doc.colors.iter() {
            by_ix.entry(entry.ix).or_insert(entry.rgb.as_ref());
        }
        #[cfg(test)]
        probed_colours(doc.colors.len());
        ColourTable { by_ix }
    }

    /// What [`diacraft_doc::Document::color`] answers for an index past the fixed palette.
    fn colour(&self, index: i64) -> Option<diacraft_color::Rgba> {
        #[cfg(test)]
        probed_colours(1);
        let ix = u32::try_from(index).ok()?;
        self.by_ix.get(&ix).and_then(|rgb| diacraft_color::Rgba::from_hex(rgb))
    }
}

/// The number a `Font` cell's value names a face by: a whole number from 0 to 1,000,000, or `None`.
pub(crate) fn face_number(text: &str) -> Option<u64> {
    let n = diacraft_doc::Value::from_text(text.trim()).number()?;
    if n.fract() != 0.0 || !(0.0..=1_000_000.0).contains(&n) {
        return None;
    }
    Some(n as u64)
}

/// The faces of a table by their `ID`, made in one pass for a walk that looks many cells up: the face a `Font` cell's value names (the face whose `ID` the value is, else the face at that place of the
/// table) in one probe a cell (the security scan's findings: a walk of the table for each cell, unpaid). The caller pays for the pass, a step for each face.
pub(crate) struct FaceTable<'d> {
    doc: &'d Document,
    /// The place of the first face of each `ID`.
    by_id: std::collections::HashMap<&'d str, usize>,
}

impl<'d> FaceTable<'d> {
    pub(crate) fn of(doc: &'d Document) -> FaceTable<'d> {
        let mut by_id = std::collections::HashMap::with_capacity(doc.fonts.len());
        for (at, face) in doc.fonts.iter().enumerate() {
            if let Some(id) = face.raw.get("ID") {
                by_id.entry(id).or_insert(at);
            }
        }
        #[cfg(test)]
        probed(doc.fonts.len());
        FaceTable { doc, by_id }
    }

    /// The name in lower case of the face `text` names, or `None`.
    pub(crate) fn face_of(&self, text: &str) -> Option<String> {
        let n = face_number(text)?;
        #[cfg(test)]
        probed(1);
        let id = format!("{n}");
        let face = self.by_id.get(id.as_str()).and_then(|at| self.doc.fonts.get(*at)).or_else(|| self.doc.fonts.get(n as usize));
        face.map(|f| f.name_u.to_ascii_lowercase())
    }
}

#[cfg(test)]
thread_local! {
    /// The faces of a table a lookup of a face has looked at in this thread (test support: the security scan's finding, a lookup for each cell that walked the table unpaid).
    pub(crate) static FACE_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts `n` faces looked at ([`FACE_PROBES`]).
#[cfg(test)]
pub(crate) fn probed(n: usize) {
    FACE_PROBES.with(|probes| probes.set(probes.get().saturating_add(u64::try_from(n).unwrap_or(u64::MAX))));
}

#[cfg(test)]
thread_local! {
    /// The colours of a table a lookup of a colour has looked at in this thread (test support, as [`FACE_PROBES`]).
    pub(crate) static COLOUR_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts `n` colours looked at ([`COLOUR_PROBES`]).
#[cfg(test)]
pub(crate) fn probed_colours(n: usize) {
    COLOUR_PROBES.with(|probes| probes.set(probes.get().saturating_add(u64::try_from(n).unwrap_or(u64::MAX))));
}

/// Whether a text is what a cell of `kind` is when nothing gives it a value: the number 0, black, the empty string, the face `0`.
pub(crate) fn is_default(kind: ValueKind, text: &str) -> bool {
    let text = text.trim();
    match kind {
        ValueKind::Scalar | ValueKind::Length | ValueKind::Angle | ValueKind::Days => text.is_empty() || number_of(text).is_some_and(|n| n == 0.0),
        ValueKind::Color => text.is_empty() || text == "0" || text.eq_ignore_ascii_case("#000000"),
        ValueKind::Text => text.is_empty(),
        ValueKind::Font => text.is_empty() || text == "0",
        ValueKind::Point => text.is_empty() || text.replace(' ', "").eq_ignore_ascii_case("PNT(0,0)"),
    }
}

/// The formula a cell directly under a sheet reads when no sheet holds it (`diacraft_doc::CellInfo::default_formula`: the cells of the text block, and
/// a picture's size), or `None`: a file that holds no such cell gives the shape what a drawing that holds that formula gives it.
pub(crate) fn default_formula(section: Option<&str>, name: &str) -> Option<&'static str> {
    section.is_none().then(|| cell_info(None, name).default_formula).flatten()
}

/// The value a shape is **drawn** with for a cell directly under its sheet that nothing gives, where that is not the 0 of its kind: the size of a line end, 2 (medium:
/// the default, which the renderer takes). A file that writes it on the shape draws the shape as the drawing did.
pub(crate) fn drawn_default(section: Option<&str>, name: &str) -> Option<f64> {
    match (section, name) {
        (None, "BeginArrowSize" | "EndArrowSize") => Some(2.0),
        _ => None,
    }
}

/// Whether `text` is the value [`drawn_default`] gives the cell.
pub(crate) fn is_drawn_default(section: Option<&str>, name: &str, text: &str) -> bool {
    drawn_default(section, name).is_some_and(|d| number_of(text).is_some_and(|n| (n - d).abs() <= CLOSE))
}

/// The value the engine gives a `THEMEVAL()` cell of a style it made, where no number is known: it says only that a theme gives the cell its value.
const THEME_PLACEHOLDER: &str = "Themed";

/// Whether `text` is the placeholder of a `THEMEVAL()` cell, for a cell of `kind` (a text cell holds what a person typed).
fn is_theme_placeholder(kind: ValueKind, text: &str) -> bool {
    kind != ValueKind::Text && text.trim() == THEME_PLACEHOLDER
}

/// Which tables comparing two cell values reads (see [`needs`]): the faces and the colours, each for the drawing (first) and for the file read back (second).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    pub(crate) faces: (bool, bool),
    pub(crate) colours: (bool, bool),
}

/// The tables [`same_value`] looks a number up in, for the sides that are made. A side with none names nothing by a number: [`needs`] says which a pair of texts reads, and the caller makes and pays for
/// them before the comparison (the security scan's findings: the table walked for each cell, unpaid).
#[derive(Clone, Copy, Default)]
pub(crate) struct Tables<'t, 'd> {
    pub(crate) faces: (Option<&'t FaceTable<'d>>, Option<&'t FaceTable<'d>>),
    pub(crate) colours: (Option<&'t ColourTable<'d>>, Option<&'t ColourTable<'d>>),
}

/// The tables [`same_value`] reads to compare the value text `vb` of the drawing with `va` of the file read back, for a cell of `kind`: **none for two texts that are the same or both a default**
/// (decided before any table is read), a face table for each side of a `Font` cell whose text is a number, and a colour table for each side of a colour cell whose text is an index past the fixed
/// palette (a colour written out, and one of the palette, name their colours with no table).
pub(crate) fn needs(kind: ValueKind, vb: &str, va: &str) -> Needs {
    if vb == va || (is_default(kind, vb) && is_default(kind, va)) {
        return Needs::default();
    }
    let past_palette =
        |text: &str| !text.trim().starts_with('#') && colour_index(text).is_some_and(|index| diacraft_color::fixed_color(index).is_none());
    match kind {
        ValueKind::Font => Needs { faces: (face_number(vb).is_some(), face_number(va).is_some()), ..Needs::default() },
        ValueKind::Color => Needs { colours: (past_palette(vb), past_palette(va)), ..Needs::default() },
        _ => Needs::default(),
    }
}

/// Whether the value text `vb` of the drawing and `va` of the file read back are one value, for a cell of `kind`, **reading the tables `tables` that [`needs`] asked for**.
pub(crate) fn same_value(tables: Tables<'_, '_>, kind: ValueKind, vb: &str, va: &str) -> Same {
    if vb == va {
        return Same::Exactly;
    }
    let other_form = |same: bool| if same { Same::InAnotherForm } else { Same::No };
    if is_default(kind, vb) && is_default(kind, va) {
        return Same::InAnotherForm;
    }
    // **A cell a style gives through `THEMEVAL()` has no value of its own** (issue 10 of the demo video): the `Theme` style of the engine holds `THEMEVAL()` in the cells a theme gives
    // (a line end's size, a connector's rounding) with the placeholder `Themed` as their value, and a file writes the value such a cell draws with, as a number (as values, one flag). The placeholder is
    // no number to differ from, so what a file writes for it is the same content; a text cell that says `Themed` is a person's text and is judged as any text.
    if is_theme_placeholder(kind, vb) {
        return Same::InAnotherForm;
    }
    match kind {
        ValueKind::Color => match (colour_of(tables.colours.0, vb), colour_of(tables.colours.1, va)) {
            (Some(x), Some(y)) => other_form(x == y),
            _ => other_form(vb.trim().eq_ignore_ascii_case(va.trim())),
        },
        ValueKind::Font => match (tables.faces.0.and_then(|t| t.face_of(vb)), tables.faces.1.and_then(|t| t.face_of(va))) {
            (Some(x), Some(y)) => other_form(x == y),
            _ => other_form(vb.trim().eq_ignore_ascii_case(va.trim())),
        },
        ValueKind::Scalar | ValueKind::Length | ValueKind::Angle | ValueKind::Days => match (number_of(vb), number_of(va)) {
            (Some(x), Some(y)) => other_form(close(x, y)),
            _ => other_form(vb.trim().eq_ignore_ascii_case(va.trim())),
        },
        ValueKind::Text => other_form(vb.trim() == va.trim()),
        ValueKind::Point => other_form(vb.replace(' ', "").eq_ignore_ascii_case(&va.replace(' ', ""))),
    }
}

/// A formula in the form the check compares: no blanks, upper case, and no multiplication by one (`Width*1` is `Width`).
pub(crate) fn canonical(source: &str) -> String {
    let squashed: String = source.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_uppercase).collect();
    let bytes = squashed.as_bytes();
    let mut out = String::with_capacity(squashed.len());
    let mut at = 0;
    while at < bytes.len() {
        // `*1` that ends a factor: the next character does not continue the number (`*10`, `*1.5`) or start a name.
        let ends_factor = |next: Option<&u8>| next.is_none_or(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'!')));
        if bytes.get(at) == Some(&b'*') && bytes.get(at + 1) == Some(&b'1') && ends_factor(bytes.get(at + 2)) {
            at += 2;
            continue;
        }
        let Some(rest) = squashed.get(at..) else { break };
        let Some(c) = rest.chars().next() else { break };
        out.push(c);
        at += c.len_utf8();
    }
    out
}

/// What a before-cell's formula is, for the comparison: the text of a formula of its own, and nothing for the markers (`Inh`, `No Formula`), which are no formula of the cell.
pub(crate) fn own_formula(cell: &Cell) -> Option<&str> {
    match &cell.formula {
        FormulaSlot::Local(f) => Some(f.source.as_ref()),
        FormulaSlot::Absent | FormulaSlot::Inherit | FormulaSlot::NoFormula => None,
    }
}

/// Whether two formulas are one formula in two forms.
pub(crate) fn same_formula(before: &str, after: &str) -> Same {
    if before == after {
        Same::Exactly
    } else if canonical(before) == canonical(after) {
        Same::InAnotherForm
    } else {
        Same::No
    }
}

/// The cells directly under a sheet that give a shape its place, size and turn.
const FRAME: [&str; 11] = ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY", "BeginX", "BeginY"];
/// The cells of a one-dimensional shape's end and the rest of its frame.
const FRAME_MORE: [&str; 6] = ["EndX", "EndY", "ResizeMode", "ObjType", "DrawingResizeType", "NoAlignBox"];

/// The class a cell that differs is told in, by the section it is in (`None` for a cell directly under a sheet) and its name.
pub(crate) fn class_of(section: Option<&str>, name: &str) -> Class {
    match section {
        Some("Geometry") => Class::Geometry,
        Some("Connection") => Class::Connections,
        Some("Character" | "Paragraph" | "Tabs") => Class::TextFormatting,
        Some("Field") => Class::Fields,
        Some("Hyperlink") => Class::Hyperlinks,
        Some("User" | "Property") => Class::UserCells,
        Some("Layer") => Class::Layers,
        Some("FillGradient" | "LineGradient") => Class::Gradients,
        Some(_) => Class::ShapeAttributes,
        None => direct_class(name),
    }
}

fn direct_class(name: &str) -> Class {
    if FRAME.contains(&name) || FRAME_MORE.contains(&name) {
        return Class::Geometry;
    }
    match name {
        "BeginArrow" | "EndArrow" | "BeginArrowSize" | "EndArrowSize" => return Class::LineEnds,
        "FillGradientEnabled" | "FillGradientDir" | "FillGradientAngle" | "LineGradientEnabled" | "LineGradientDir" | "LineGradientAngle" => {
            return Class::Gradients;
        }
        "LayerMember" => return Class::Layers,
        "BegTrigger" | "EndTrigger" => return Class::Connections,
        "PageWidth" | "PageHeight" | "PageScale" | "DrawingScale" | "DrawingSizeType" | "DrawingScaleType" | "PageLockReplace" => {
            return Class::Pages;
        }
        _ => {}
    }
    let info = cell_info(None, name);
    match info.class {
        CellClass::LineFamily | CellClass::FillFamily => Class::StyleContent,
        CellClass::TextFamily => Class::TextFormatting,
        _ => match name {
            "TxtPinX" | "TxtPinY" | "TxtLocPinX" | "TxtLocPinY" | "TxtWidth" | "TxtHeight" | "TxtAngle" => Class::TextFormatting,
            _ if name.starts_with("Line")
                || name.starts_with("Fill")
                || name.starts_with("Shdw")
                || name.starts_with("ShapeShdw")
                || name.starts_with("Quick") =>
            {
                Class::StyleContent
            }
            _ if name.starts_with("Page") || name.starts_with("Print") => Class::Pages,
            _ => Class::ShapeAttributes,
        },
    }
}

/// The family of cells that a style link reaches, for the words of a link that differs.
#[allow(dead_code)]
pub(crate) fn family_of(name: &str) -> Option<Family> {
    cell_info(None, name).class.family()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multiplication_by_one_and_blanks_and_case_are_forms_of_one_formula() {
        assert_eq!(same_formula("Width*1", "Width"), Same::InAnotherForm);
        assert_eq!(same_formula("Width * 1", "WIDTH"), Same::InAnotherForm);
        assert_eq!(same_formula("Width*0.5", "Width*0.5"), Same::Exactly);
        // The one that is a factor of ten, of one and a half, or a name is not one.
        assert_eq!(same_formula("Width*10", "Width"), Same::No);
        assert_eq!(same_formula("Width*1.5", "Width"), Same::No);
        assert_eq!(same_formula("Width*1X", "Width"), Same::No);
        assert_eq!(same_formula("Width*0", "0"), Same::No, "a formula that is a constant in disguise is not the constant: it is lost");
        assert_eq!(same_formula("RGB(255,0,0)", "#ff0000"), Same::No);
        assert_eq!(canonical("Sheet.1!Width*1"), "SHEET.1!WIDTH");
        assert_eq!(canonical("Width*1+Height*1"), "WIDTH+HEIGHT");
    }

    #[test]
    fn a_number_is_one_value_in_any_form_and_a_colour_in_any_spelling() {
        assert_eq!(same_value(Tables::default(), ValueKind::Length, "2", "2.0"), Same::InAnotherForm);
        assert_eq!(same_value(Tables::default(), ValueKind::Length, "2", "2.0000000001"), Same::InAnotherForm);
        assert_eq!(same_value(Tables::default(), ValueKind::Length, "2", "2.001"), Same::No);
        assert_eq!(same_value(Tables::default(), ValueKind::Scalar, "TRUE", "1"), Same::InAnotherForm);
        assert_eq!(same_value(Tables::default(), ValueKind::Color, "#ff0000", "#FF0000"), Same::InAnotherForm);
        assert_eq!(same_value(Tables::default(), ValueKind::Color, "2", "#ff0000"), Same::InAnotherForm, "the palette's red is red");
        assert_eq!(same_value(Tables::default(), ValueKind::Color, "2", "#00ff00"), Same::No);
        assert_eq!(same_value(Tables::default(), ValueKind::Text, "a", "a"), Same::Exactly);
        assert_eq!(same_value(Tables::default(), ValueKind::Text, "a", "b"), Same::No);
        assert_eq!(same_value(Tables::default(), ValueKind::Length, "", "0"), Same::InAnotherForm, "what nothing gives a length is 0");
    }

    /// A document whose colour table holds `IX` 30 twice (the first is the one `Document::color` finds), `IX` 31 with an `RGB` that is no colour, and `IX` 32; and whose font table holds `Arial` with
    /// `ID` 5, `Noto Sans` with `ID` 5 again, and `Mono` with no `ID`.
    fn tabled() -> Document {
        let mut doc = Document::new_blank();
        doc.colors = std::sync::Arc::new(vec![
            diacraft_doc::ColorEntry { ix: 30, rgb: "#102030".into() },
            diacraft_doc::ColorEntry { ix: 30, rgb: "#ffffff".into() },
            diacraft_doc::ColorEntry { ix: 31, rgb: "not a colour".into() },
            diacraft_doc::ColorEntry { ix: 32, rgb: "#405060".into() },
        ]);
        let face = |name: &str, id: Option<&str>| diacraft_doc::FaceName {
            name_u: name.into(),
            raw: id.map_or_else(diacraft_doc::RawAttrs::new, |id| diacraft_doc::RawAttrs::from_pairs([("ID", id)])),
            ext: Default::default(),
        };
        std::sync::Arc::make_mut(&mut doc.fonts).extend([face("Arial", Some("5")), face("Noto Sans", Some("5")), face("Mono", None)]);
        doc
    }

    #[test]
    fn a_colour_table_answers_what_the_document_does_for_every_index() {
        let doc = tabled();
        let table = ColourTable::of(&doc);
        for index in [-1_i64, 0, 2, 23, 24, 29, 30, 31, 32, 33, 253, 100_000, 5_000_000_000] {
            let want = doc.color(index).map(|c| [c.r, c.g, c.b, c.a]);
            let got = diacraft_color::fixed_color(index).or_else(|| table.colour(index)).map(|c| [c.r, c.g, c.b, c.a]);
            assert_eq!(got, want, "index {index}");
        }
        assert_eq!(colour_of(Some(&table), " 30 "), Some([0x10, 0x20, 0x30, 0xff]), "the first entry of an IX is the colour");
        assert_eq!(colour_of(Some(&table), "31"), None, "an RGB that is no colour names none, as the document's answer");
        assert_eq!(colour_of(Some(&table), "#405060"), Some([0x40, 0x50, 0x60, 0xff]));
        assert_eq!(colour_of(None, "30"), None, "past the palette a text names a colour only through a table");
        assert_eq!(colour_of(None, "2"), colour_of(Some(&table), "2"), "the palette needs no table");
    }

    #[test]
    fn a_face_table_finds_the_first_face_of_an_id_and_else_the_face_at_that_place() {
        let doc = tabled();
        let table = FaceTable::of(&doc);
        assert_eq!(table.face_of("5").as_deref(), Some("arial"), "the first face of ID 5");
        assert_eq!(table.face_of("2").as_deref(), Some("mono"), "no face has ID 2: the face at place 2");
        assert_eq!(table.face_of("0").as_deref(), Some("arial"), "no face has ID 0: the face at place 0");
        assert_eq!(table.face_of("3"), None, "no face has ID 3 and no place 3 is there");
        assert_eq!(table.face_of("Arial"), None, "a name is no number");
        assert_eq!(table.face_of("1.5"), None);
        assert_eq!(table.face_of("-1"), None);
    }

    #[test]
    fn a_comparison_reads_a_table_only_for_a_text_that_is_a_number_the_table_could_answer() {
        let none = Needs::default();
        // The same text, and two defaults, are decided before any table is read.
        assert_eq!(needs(ValueKind::Font, "5", "5"), none);
        assert_eq!(needs(ValueKind::Font, "", "0"), none);
        assert_eq!(needs(ValueKind::Color, "30", "30"), none);
        // A face by its number: each side that is a number. A name needs none.
        assert_eq!(needs(ValueKind::Font, "5", "Arial"), Needs { faces: (true, false), ..none });
        assert_eq!(needs(ValueKind::Font, "Arial", "2"), Needs { faces: (false, true), ..none });
        assert_eq!(needs(ValueKind::Font, "Arial", "Mono"), none);
        // A colour by its index past the palette; the palette and a colour written out need none.
        assert_eq!(needs(ValueKind::Color, "30", "#ff0000"), Needs { colours: (true, false), ..none });
        assert_eq!(needs(ValueKind::Color, "#ff0000", " 31"), Needs { colours: (false, true), ..none });
        assert_eq!(needs(ValueKind::Color, "2", "23"), none);
        assert_eq!(needs(ValueKind::Color, "24", "23"), Needs { colours: (true, false), ..none });
        // No other kind reads one.
        assert_eq!(needs(ValueKind::Length, "30", "31"), none);
        assert_eq!(needs(ValueKind::Scalar, "5", "6"), none);
        // And the answer with the tables is the cell's: two numbers of one face, one colour in two spellings.
        let doc = tabled();
        let (faces, colours) = (FaceTable::of(&doc), ColourTable::of(&doc));
        let tables = Tables { faces: (Some(&faces), Some(&faces)), colours: (Some(&colours), Some(&colours)) };
        assert_eq!(same_value(tables, ValueKind::Font, "5", "0"), Same::InAnotherForm, "ID 5 and place 0 are one face");
        assert_eq!(same_value(tables, ValueKind::Font, "5", "2"), Same::No);
        assert_eq!(same_value(tables, ValueKind::Color, "30", "#102030"), Same::InAnotherForm);
        assert_eq!(same_value(tables, ValueKind::Color, "30", "32"), Same::No);
        assert_eq!(
            same_value(Tables::default(), ValueKind::Color, "30", "#102030"),
            Same::No,
            "with no table the index names no colour: the texts differ"
        );
    }

    #[test]
    fn a_cell_that_differs_is_told_in_the_class_of_its_place() {
        assert_eq!(class_of(None, "PinX"), Class::Geometry);
        assert_eq!(class_of(Some("Geometry"), "X"), Class::Geometry);
        assert_eq!(class_of(None, "FillForegnd"), Class::StyleContent);
        assert_eq!(class_of(None, "LinePattern"), Class::StyleContent);
        assert_eq!(class_of(None, "BeginArrow"), Class::LineEnds);
        assert_eq!(class_of(Some("FillGradient"), "GradientStopColor"), Class::Gradients);
        assert_eq!(class_of(Some("Character"), "Size"), Class::TextFormatting);
        assert_eq!(class_of(Some("User"), "Value"), Class::UserCells);
        assert_eq!(class_of(Some("Hyperlink"), "Address"), Class::Hyperlinks);
        assert_eq!(class_of(None, "PageWidth"), Class::Pages);
        assert_eq!(class_of(None, "NoSuchCell"), Class::ShapeAttributes);
    }
}
