//! A cell box (design 5.6, 5.7): the formula at the left, the value at the right, the colour that says where the cell comes from, and the words that name it.
//!
//! * **Both texts**: with Formulas and Values on, a box shows the formula and the value it works out to; the value is left out when it reads as the formula does, and
//!   that is told **by the number and not by the text** (`2.5 in` and `2.5000 in` are one value). With one toggle on, the box shows that one: the formula at the left, the value
//!   at the right ("values sit at the right of their cells").
//! * **Decimals**: the engine's reading gives both texts, the formula's with at most four decimals and the zeros cut, the value's with four and
//!   the zeros kept; this module chooses between them.
//! * **Where it comes from**: a cell the sheet holds is drawn in the `sheet_local` token and has a small dot at its left edge, so that the difference does not rest on colour; an
//!   inherited cell is in the text colour; a cell found nowhere is in `sheet_default`; an error value is in `sheet_error`. **No colour is written here**: every one is a token.
//! * **Long text** is **cut at the box's edge with no ellipsis** (the reference cuts a formula too long for its cell at the cell's right edge, and a name too long for its
//!   column at the left, since names sit right-aligned) and whole in the tooltip; the bar holds the formula whole. A text is laid out in one line and the painter's clip cuts it, so no
//!   more characters are laid out than the box can show ([`room_chars`]).

use std::sync::Arc;

use diacraft_engine::sheetview::{CellValue, CellView, FromView, Origin};
use egui::text::LayoutJob;
use egui::{Color32, FontId, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, pos2};

use crate::theme::{self, Tokens, size};
use crate::widgets::text_width;

/// The room at each side of a cell box's text, in points.
pub const CELL_PAD: f32 = 6.0;

/// The room between the formula and the value in one box, in points.
const GAP: f32 = 8.0;

/// How much of a box the value may take when both texts are shown.
const VALUE_SHARE: f32 = 0.5;

/// The most characters of a text measured for the width of a column: a column is at most 240 points wide, which fewer characters than this fill.
const MEASURED_CHARS: usize = 80;

/// The font of the sheet's text.
pub fn font() -> FontId {
    theme::font(size::LABEL)
}

/// The colour a text of the sheet is drawn in, by the name of its token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// An inherited cell: the ordinary text.
    Text,
    /// A cell the sheet holds.
    Local,
    /// A cell found nowhere, which reads its default.
    Default,
    /// An error value.
    Error,
    /// A quiet text: a note, a column title's rule, a row of an inherited section.
    Dim,
}

impl Ink {
    /// The word a test and `ui.inspect` call it.
    pub fn word(self) -> &'static str {
        match self {
            Ink::Text => "text",
            Ink::Local => "local",
            Ink::Default => "default",
            Ink::Error => "error",
            Ink::Dim => "dim",
        }
    }

    /// The colour of the theme.
    pub fn colour(self, t: &Tokens) -> Color32 {
        match self {
            Ink::Text => t.text,
            Ink::Local => t.sheet_local,
            Ink::Default => t.sheet_default,
            Ink::Error => t.sheet_error,
            Ink::Dim => t.text_dim,
        }
    }
}

/// The ink of a cell's formula by where the cell comes from (design 5.6).
pub fn ink_of(origin: Origin) -> Ink {
    match origin {
        Origin::Local => Ink::Local,
        Origin::Inherited => Ink::Text,
        Origin::Default => Ink::Default,
    }
}

/// The two texts a box draws, each empty when it is not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Texts<'a> {
    pub formula: &'a str,
    pub value: &'a str,
}

/// How a box is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub formulas: bool,
    pub values: bool,
    pub selected: bool,
    /// The last commit changed the value (the `sheet_changed` background).
    pub changed: bool,
}

/// The texts a box draws for the two toggles.
pub fn texts(cell: &CellView, formulas: bool, values: bool) -> Texts<'_> {
    let formula = if formulas { cell.formula.as_str() } else { "" };
    let value = if !values || (formulas && reads_alike(cell)) { "" } else { cell.shown.as_str() };
    Texts { formula, value }
}

/// A number and the unit after it: `2.5 in` is `(2.5, "in")`, `25%` is `(25, "%")`, `-1e3` is `(-1000, "")`. `None` where the text does not begin with a number.
pub fn number_and_unit(text: &str) -> Option<(f64, String)> {
    let text = text.trim();
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end += 1;
    }
    let digits_from = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
    }
    if end == digits_from || (end == digits_from + 1 && bytes.get(digits_from) == Some(&b'.')) {
        return None;
    }
    // An exponent only when digits follow it: `1e3`, not the `e` of a unit.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut at = end + 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        if bytes.get(at).is_some_and(u8::is_ascii_digit) {
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            end = at;
        }
    }
    let number: f64 = text.get(..end)?.parse().ok()?;
    number.is_finite().then(|| (number, text.get(end..).unwrap_or("").trim().to_ascii_lowercase()))
}

/// Whether the value reads as the formula does: the two are one number in one unit, to the four decimals the value shows, or one text, or one word. A formula that is
/// no plain value (`Width*0.5`) never does, nor does a cell that reads an error, a formula that did not parse, or one the evaluator cannot work out.
pub fn reads_alike(cell: &CellView) -> bool {
    if cell.error.is_some() || !cell.parsed || !cell.worked_out {
        return false;
    }
    if cell.formula == cell.shown {
        return true;
    }
    match &cell.value {
        CellValue::Number(_) => match (number_and_unit(&cell.formula), number_and_unit(&cell.shown)) {
            (Some((a, unit_a)), Some((b, unit_b))) => unit_a == unit_b && (a - b).abs() < 5.0e-5,
            _ => false,
        },
        CellValue::Text(text) => cell.formula.strip_prefix('"').and_then(|f| f.strip_suffix('"')).is_some_and(|f| f.replace("\"\"", "\"") == *text),
        CellValue::Bool(_) => cell.formula.eq_ignore_ascii_case(&cell.shown),
        CellValue::None => false,
    }
}

/// What an error value means, in the words of the reference page.
pub fn error_meaning(code: &str) -> Option<&'static str> {
    Some(match code {
        "#DIV/0!" => "Division by zero",
        "#VALUE!" => "Wrong type of argument or operand",
        "#REF!" => "The referenced cell does not exist",
        "#NUM!" => "Invalid number",
        "#N/A" => "No value available",
        "#DIM!" => "Power outside the allowed range",
        _ => return None,
    })
}

/// Where a cell comes from, in a line for the tooltip and the footer: "Own", "From the master Process (ID 3)", "From the style Normal", "Default: not in the drawing".
pub fn where_from(cell: &CellView) -> String {
    match (cell.origin, &cell.from) {
        (Origin::Local, _) => "Own: the sheet holds this cell".to_owned(),
        (Origin::Default, _) => "Default: not in the drawing".to_owned(),
        (Origin::Inherited, Some(FromView { kind: "style", name, .. })) => match name {
            Some(name) => format!("From the style {name}"),
            None => "From a style".to_owned(),
        },
        (Origin::Inherited, Some(FromView { id, name, .. })) => match name {
            Some(name) => format!("From the master {name} (ID {id})"),
            None => format!("From a master (ID {id})"),
        },
        (Origin::Inherited, None) => "Inherited".to_owned(),
    }
}

/// The word of a cell's place in its name for assistive technology.
fn origin_word(origin: Origin) -> &'static str {
    match origin {
        Origin::Local => "own",
        Origin::Inherited => "inherited",
        Origin::Default => "default",
    }
}

/// The name of a cell for assistive technology: its place, its formula, its value and where it comes from ("Shape Transform, Width: formula 2 in, value 2.0000 in, inherited").
pub fn accessible_name(section: &str, cell: &CellView) -> String {
    let name = cell.reference.as_deref().unwrap_or(&cell.name);
    format!("{section}, {name}: formula {}, value {}, {}", cell.formula, cell.shown, origin_word(cell.origin))
}

/// The value a formula that did not parse was saved with, for its tooltip.
fn saved_value(cell: &CellView) -> String {
    match &cell.value {
        CellValue::Number(n) => format!("{n}{}", cell.unit.map_or(String::new(), |u| format!(" {}", u.to_ascii_lowercase()))),
        CellValue::Text(t) => t.clone(),
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        CellValue::None => "none".to_owned(),
    }
}

/// The tooltip of a cell: its formula whole as far as the reading gave it, its value, the meaning of its error, and where it comes from.
pub fn tooltip(cell: &CellView) -> String {
    let mut out = String::new();
    out.push_str(cell.reference.as_deref().unwrap_or(&cell.name));
    out.push_str("\nFormula: ");
    out.push_str(&cell.formula);
    if cell.truncated {
        out.push_str("\n(cut at 4,096 characters here: the formula bar holds it whole)");
    }
    out.push_str("\nValue: ");
    out.push_str(&cell.shown);
    if let Some(code) = &cell.error {
        match error_meaning(code) {
            Some(meaning) => out.push_str(&format!("\n{code}: {meaning}")),
            None => out.push_str(&format!("\n{code}")),
        }
    }
    if !cell.parsed {
        out.push_str(&format!("\nThis formula could not be read: it is kept as it is. The value saved with it is {}.", saved_value(cell)));
    } else if !cell.worked_out {
        out.push_str("\nDiaCraft cannot work this function out yet: the value is the one saved before.");
    }
    if cell.guarded {
        out.push_str("\nGuarded: GUARD keeps the drawing's own actions from changing it.");
    }
    out.push('\n');
    out.push_str(&where_from(cell));
    out
}

/// The most characters a box `width` points wide can show, however narrow the characters are, and a few more: no more than this are laid out.
pub fn room_chars(width: f32) -> usize {
    if width.is_finite() { ((width.max(0.0) / 2.5) as usize).saturating_add(8).min(4_104) } else { 8 }
}

/// A text of one line, laid out whole (as far as [`room_chars`] of `width`) and **not shortened**: the painter's clip cuts it at the box's edge, with no ellipsis.
pub fn cut(ui: &Ui, text: &str, colour: Color32, width: f32) -> Arc<Galley> {
    let few: String = text.chars().take(room_chars(width)).collect();
    ui.fonts_mut(|f| f.layout_job(LayoutJob::simple_singleline(few, font(), colour)))
}

/// A text of one line laid out from its **end** (as far as [`room_chars`] of `width`), for a name that sits right-aligned: a name too long for its room is cut at the left.
pub fn cut_tail(ui: &Ui, text: &str, colour: Color32, width: f32) -> Arc<Galley> {
    let wanted = room_chars(width);
    let skip = text.chars().count().saturating_sub(wanted);
    let few: String = text.chars().skip(skip).collect();
    ui.fonts_mut(|f| f.layout_job(LayoutJob::simple_singleline(few, font(), colour)))
}

/// A text of one line cut to `width` points with an ellipsis, laid out once and kept by egui's galley cache. For the footer and the like; a cell is cut at its edge ([`cut`]).
pub fn elided(ui: &Ui, text: &str, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font(), colour, width.max(1.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The width a box needs for its two texts: room at each side, the formula, the gap and the value. For the columns of a table, which are as wide as their widest box on view.
pub fn natural_width(ui: &Ui, cell: &CellView, formulas: bool, values: bool) -> f32 {
    let texts = texts(cell, formulas, values);
    let measure = |text: &str| if text.is_empty() { 0.0 } else { text_width(ui, &text.chars().take(MEASURED_CHARS).collect::<String>(), &font()) };
    let (f, v) = (measure(texts.formula), measure(texts.value));
    2.0 * CELL_PAD + f + v + if f > 0.0 && v > 0.0 { GAP } else { 0.0 }
}

/// What a box drew: for the tests and `ui.inspect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Painted {
    pub name: String,
    pub reference: Option<String>,
    pub origin: Origin,
    /// The ink of the formula.
    pub ink: Ink,
    /// The ink of the value: the error's when the cell reads one.
    pub value_ink: Ink,
    /// The dot at the left edge of a cell the sheet holds.
    pub dot: bool,
    /// The formula text drawn, empty when it is not; the value text drawn, empty when it is left out.
    pub text: String,
    pub shown: String,
    /// The last commit worked this cell out again: it has the `sheet_changed` background until the next commit.
    pub changed: bool,
}

/// Draws the box of `cell` in `rect`: the background of a changed cell, the dot of an own one, the formula at the left and the value at the right, the outline of the selected cell.
/// → what was drawn.
pub fn paint(ui: &Ui, painter: &Painter, t: &Tokens, rect: Rect, cell: &CellView, look: Look) -> Painted {
    if look.changed {
        painter.rect_filled(rect, 0.0, t.sheet_changed);
    }
    let ink = ink_of(cell.origin);
    let value_ink = if cell.error.is_some() { Ink::Error } else { ink };
    let dot = cell.origin == Origin::Local;
    let centre = rect.center().y;
    if dot {
        painter.circle_filled(pos2(rect.left() + 3.0, centre), 1.8, t.sheet_local);
    }
    let texts = texts(cell, look.formulas, look.values);
    let inner =
        Rect::from_min_max(pos2(rect.left() + CELL_PAD, rect.top()), pos2((rect.right() - CELL_PAD).max(rect.left() + CELL_PAD), rect.bottom()));
    let width = inner.width();
    // The value takes what it needs when both fit (a column is made wide enough for both); when they do not, it keeps its share and what the formula leaves, and the formula the
    // rest. Both are laid out in one line and cut at the edge of their room by the clip.
    let value = (!texts.value.is_empty()).then(|| {
        let natural = text_width(ui, &texts.value.chars().take(MEASURED_CHARS).collect::<String>(), &font());
        let room = if texts.formula.is_empty() {
            width
        } else {
            let formula = text_width(ui, &texts.formula.chars().take(MEASURED_CHARS).collect::<String>(), &font());
            (width - formula - GAP).max(width * VALUE_SHARE)
        };
        (cut(ui, texts.value, value_ink.colour(t), room), natural.min(room))
    });
    let value_w = value.as_ref().map_or(0.0, |(_, w)| *w);
    let formula = (!texts.formula.is_empty()).then(|| {
        let room = if value.is_some() { (width - value_w - GAP).max(1.0) } else { width };
        (cut(ui, texts.formula, ink.colour(t), room), room)
    });
    if let Some((g, room)) = formula {
        let at: Pos2 = pos2(inner.left(), centre - g.size().y / 2.0);
        let clip = Rect::from_min_size(pos2(inner.left(), rect.top()), egui::vec2(room, rect.height()));
        painter.with_clip_rect(clip.intersect(painter.clip_rect())).galley(at, g, ink.colour(t));
    }
    if let Some((g, room)) = value {
        let at = pos2(inner.right() - room, centre - g.size().y / 2.0);
        let clip = Rect::from_min_size(pos2(inner.right() - room, rect.top()), egui::vec2(room, rect.height()));
        painter.with_clip_rect(clip.intersect(painter.clip_rect())).galley(at, g, value_ink.colour(t));
    }
    if look.selected {
        painter.rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
    }
    let first = |s: &str| s.chars().take(120).collect::<String>();
    Painted {
        name: cell.name.clone(),
        reference: cell.reference.clone(),
        origin: cell.origin,
        ink,
        value_ink,
        dot,
        text: first(texts.formula),
        shown: first(texts.value),
        changed: look.changed,
    }
}

/// The size a cell's box has beyond its text: a line's height is the font's real row and some air, as the sheet measures it.
pub fn box_height(ui: &Ui) -> f32 {
    let ink = ui.visuals().text_color();
    let row = ui.fonts_mut(|f| f.layout_no_wrap("Hg".to_owned(), font(), ink).size().y);
    (row + 6.0).ceil().max(22.0)
}

#[cfg(test)]
mod tests {
    use diacraft_engine::sheetview::{CellValue, CellView, FromView, Origin};

    use super::*;

    /// A cell as the engine reads it, with the fields the texts depend on.
    fn cell(formula: &str, shown: &str, value: CellValue) -> CellView {
        CellView {
            name: "Width".to_owned(),
            col: None,
            reference: Some("Width".to_owned()),
            formula: formula.to_owned(),
            value,
            unit: Some("IN"),
            shown: shown.to_owned(),
            error: None,
            origin: Origin::Local,
            from: None,
            guarded: false,
            parsed: true,
            worked_out: true,
            truncated: false,
        }
    }

    #[test]
    fn a_number_is_read_with_its_unit_and_an_exponent_only_when_digits_follow_it() {
        assert_eq!(number_and_unit("2.5 in"), Some((2.5, "in".to_owned())));
        assert_eq!(number_and_unit("  -0.25 MM "), Some((-0.25, "mm".to_owned())));
        assert_eq!(number_and_unit("25%"), Some((25.0, "%".to_owned())));
        assert_eq!(number_and_unit("1e3"), Some((1000.0, String::new())));
        assert_eq!(number_and_unit("2 ea"), Some((2.0, "ea".to_owned())), "the e of a unit is no exponent");
        assert_eq!(number_and_unit("3.0000 deg"), Some((3.0, "deg".to_owned())));
        assert_eq!(number_and_unit("Width*0.5"), None);
        assert_eq!(number_and_unit(".5"), Some((0.5, String::new())));
        assert_eq!(number_and_unit("."), None);
        assert_eq!(number_and_unit("-"), None);
        assert_eq!(number_and_unit(""), None);
        assert_eq!(number_and_unit("1e999"), None, "a number that is not finite is none");
    }

    #[test]
    fn a_value_that_reads_as_its_formula_is_left_out_by_number_and_not_by_text() {
        let plain = cell("2.5 in", "2.5000 in", CellValue::Number(2.5));
        assert!(reads_alike(&plain), "`2.5 in` and `2.5000 in` are one number");
        assert_eq!(texts(&plain, true, true), Texts { formula: "2.5 in", value: "" });
        let worked = cell("Width*0.5", "1.2500 in", CellValue::Number(1.25));
        assert!(!reads_alike(&worked), "a formula that is no plain value is shown with its value");
        assert_eq!(texts(&worked, true, true), Texts { formula: "Width*0.5", value: "1.2500 in" });
        // Another unit is another value.
        assert!(!reads_alike(&cell("2.5 mm", "2.5000 in", CellValue::Number(2.5))));
        // A number that differs in the fifth decimal is the same to four.
        assert!(reads_alike(&cell("0.6667", "0.6667", CellValue::Number(0.666_666))));
        assert!(!reads_alike(&cell("0.6", "0.6667", CellValue::Number(0.666_666))));
        // A text in quotation marks reads as the text.
        assert!(reads_alike(&cell("\"say \"\"hi\"\"\"", "say \"hi\"", CellValue::Text("say \"hi\"".to_owned()))));
        assert!(!reads_alike(&cell("\"a\"&\"b\"", "ab", CellValue::Text("ab".to_owned()))));
        assert!(reads_alike(&cell("TRUE", "TRUE", CellValue::Bool(true))));
        assert!(!reads_alike(&cell("#FF0000", "RGB(255, 0, 0)", CellValue::Text("#FF0000".to_owned()))), "the colour's words tell more than its hex");
    }

    #[test]
    fn the_toggles_choose_the_texts_and_an_error_or_an_unread_formula_is_never_left_out() {
        let plain = cell("2.5 in", "2.5000 in", CellValue::Number(2.5));
        assert_eq!(texts(&plain, true, false), Texts { formula: "2.5 in", value: "" }, "Formulas alone");
        assert_eq!(texts(&plain, false, true), Texts { formula: "", value: "2.5000 in" }, "Values alone: four decimals, zeros kept");
        let mut broken = cell("1/0", "#DIV/0!", CellValue::None);
        broken.error = Some("#DIV/0!".to_owned());
        assert_eq!(texts(&broken, true, true), Texts { formula: "1/0", value: "#DIV/0!" });
        let mut same = cell("#REF!", "#REF!", CellValue::None);
        same.error = Some("#REF!".to_owned());
        assert!(!reads_alike(&same), "an error is shown as the value it is");
        let mut unread = cell("1 +* 2", "not read", CellValue::Number(3.0));
        unread.parsed = false;
        assert_eq!(texts(&unread, true, true), Texts { formula: "1 +* 2", value: "not read" });
        let mut unknown = cell("FOO(2)", "2.0000 in?", CellValue::Number(2.0));
        unknown.worked_out = false;
        assert_eq!(texts(&unknown, true, true).value, "2.0000 in?");
    }

    #[test]
    fn each_ink_is_a_token_and_a_cell_is_inked_by_where_it_comes_from() {
        let t = Tokens::light();
        assert_eq!(ink_of(Origin::Local).colour(&t), t.sheet_local);
        assert_eq!(ink_of(Origin::Inherited).colour(&t), t.text);
        assert_eq!(ink_of(Origin::Default).colour(&t), t.sheet_default);
        assert_eq!(Ink::Error.colour(&t), t.sheet_error);
        assert_eq!(Ink::Dim.colour(&t), t.text_dim);
        let words: Vec<&str> = [Ink::Text, Ink::Local, Ink::Default, Ink::Error, Ink::Dim].iter().map(|i| i.word()).collect();
        assert_eq!(words, ["text", "local", "default", "error", "dim"]);
    }

    #[test]
    fn a_cell_says_where_it_comes_from_and_what_its_error_means() {
        let mut c = cell("2 in", "2.0000 in", CellValue::Number(2.0));
        assert_eq!(where_from(&c), "Own: the sheet holds this cell");
        c.origin = Origin::Inherited;
        c.from = Some(FromView { kind: "master", id: 3, name: Some("Process".to_owned()), shape: Some(1), family: None });
        assert_eq!(where_from(&c), "From the master Process (ID 3)");
        c.from = Some(FromView { kind: "style", id: 0, name: Some("Normal".to_owned()), shape: None, family: Some("line") });
        assert_eq!(where_from(&c), "From the style Normal");
        c.origin = Origin::Default;
        c.from = None;
        assert_eq!(where_from(&c), "Default: not in the drawing");
        assert_eq!(error_meaning("#DIV/0!"), Some("Division by zero"));
        assert_eq!(error_meaning("#REF!"), Some("The referenced cell does not exist"));
        assert_eq!(error_meaning("#SOMETHING!"), None);
        c.error = Some("#DIV/0!".to_owned());
        c.shown = "#DIV/0!".to_owned();
        let tip = tooltip(&c);
        assert!(tip.contains("#DIV/0!: Division by zero") && tip.contains("Default: not in the drawing"), "{tip}");
        c.parsed = false;
        assert!(tooltip(&c).contains("could not be read"), "an unread formula says so, with the value saved with it");
        c.parsed = true;
        c.worked_out = false;
        assert!(tooltip(&c).contains("cannot work this function out yet"));
    }

    #[test]
    fn a_cells_name_for_assistive_technology_says_its_place_its_texts_and_its_origin() {
        let c = cell("2 in", "2.0000 in", CellValue::Number(2.0));
        assert_eq!(accessible_name("Shape Transform", &c), "Shape Transform, Width: formula 2 in, value 2.0000 in, own");
        let mut inherited = c.clone();
        inherited.origin = Origin::Inherited;
        inherited.reference = None;
        assert_eq!(accessible_name("Shape Transform", &inherited), "Shape Transform, Width: formula 2 in, value 2.0000 in, inherited");
    }
}
