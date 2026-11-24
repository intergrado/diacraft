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

/// The colour a text names in `doc`: `#RRGGBB`, or an index of the colour table.
fn colour_of(doc: &Document, text: &str) -> Option<[u8; 4]> {
    let text = text.trim();
    if text.starts_with('#') {
        return diacraft_color::Rgba::from_hex(text).map(|c| [c.r, c.g, c.b, c.a]);
    }
    let index = diacraft_doc::Value::from_text(text).number()?;
    if index.fract() != 0.0 || !(0.0..=100_000.0).contains(&index) {
        return None;
    }
    doc.color(index as i64).map(|c| [c.r, c.g, c.b, c.a])
}

/// The name of the font face a `Font` cell's value names in `doc`: the face whose `ID` the value is, else the face at that place of the table.
fn face_of(doc: &Document, text: &str) -> Option<String> {
    let n = diacraft_doc::Value::from_text(text.trim()).number()?;
    if n.fract() != 0.0 || !(0.0..=1_000_000.0).contains(&n) {
        return None;
    }
    let id = format!("{}", n as u64);
    let face = doc.fonts.iter().find(|f| f.raw.get("ID") == Some(id.as_str())).or_else(|| doc.fonts.get(n as usize));
    face.map(|f| f.name_u.to_ascii_lowercase())
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

/// Whether the value text `vb` of the drawing and `va` of the file read back are one value, for a cell of `kind`.
pub(crate) fn same_value(before: &Document, after: &Document, kind: ValueKind, vb: &str, va: &str) -> Same {
    if vb == va {
        return Same::Exactly;
    }
    let other_form = |same: bool| if same { Same::InAnotherForm } else { Same::No };
    if is_default(kind, vb) && is_default(kind, va) {
        return Same::InAnotherForm;
    }
    match kind {
        ValueKind::Color => match (colour_of(before, vb), colour_of(after, va)) {
            (Some(x), Some(y)) => other_form(x == y),
            _ => other_form(vb.trim().eq_ignore_ascii_case(va.trim())),
        },
        ValueKind::Font => match (face_of(before, vb), face_of(after, va)) {
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
        let doc = Document::new_blank();
        assert_eq!(same_value(&doc, &doc, ValueKind::Length, "2", "2.0"), Same::InAnotherForm);
        assert_eq!(same_value(&doc, &doc, ValueKind::Length, "2", "2.0000000001"), Same::InAnotherForm);
        assert_eq!(same_value(&doc, &doc, ValueKind::Length, "2", "2.001"), Same::No);
        assert_eq!(same_value(&doc, &doc, ValueKind::Scalar, "TRUE", "1"), Same::InAnotherForm);
        assert_eq!(same_value(&doc, &doc, ValueKind::Color, "#ff0000", "#FF0000"), Same::InAnotherForm);
        assert_eq!(same_value(&doc, &doc, ValueKind::Color, "2", "#ff0000"), Same::InAnotherForm, "the palette's red is red");
        assert_eq!(same_value(&doc, &doc, ValueKind::Color, "2", "#00ff00"), Same::No);
        assert_eq!(same_value(&doc, &doc, ValueKind::Text, "a", "a"), Same::Exactly);
        assert_eq!(same_value(&doc, &doc, ValueKind::Text, "a", "b"), Same::No);
        assert_eq!(same_value(&doc, &doc, ValueKind::Length, "", "0"), Same::InAnotherForm, "what nothing gives a length is 0");
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
