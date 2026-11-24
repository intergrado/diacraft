//! Formatting: `shape.format`, `text.format` and `shape.formatting`.
//!
//! The commands check their parameters and turn them into the types of [`crate::formatting`], which writes the cells. Lengths on the command
//! follow the other editing commands: paper inches, turned into the page's drawing units at the edge (only `rounding` is one; a weight and a size are
//! in points on paper, and the cells hold them as inches on paper whatever the page's scale).

use serde_json::{Value, json};

use super::effects::{CAPS, COMPOUNDS};
use super::params::{optional_range, optional_whole, page_for, required_ids, units_per_inch};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::error::{Result, bad, failed, shown};
use crate::formatting::{
    Align, Case, Direction, Indent, Paint, RangeLook, Rgb, ShapeFormat, ShapeLook, TextFormat, TextPaint, VAlign, format_shapes, format_text,
    look_of, look_of_range, parse_paint,
};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.format",
            "Format Shapes",
            ["Home", "Shape"],
            None,
            "ids: array of whole numbers, the shapes to format (a group is formatted through the shapes inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             fill: text, optional, the fill as a colour #RRGGBB, none, a colour of the page's theme as theme:accent2 (with a shade as theme:accent2:40 lighter or theme:accent2:-25 darker, from -100 to 100, and the names light, dark, background, accent1 to accent6 and variant1 to variant7), which follows the theme when it changes, or default, which hands the fill to the theme (see theme.colors); \
             line: text, optional, the line as a colour #RRGGBB, none, a theme colour as for fill, or default; \
             weight: number, optional, the thickness of the line in points on the paper, more than 0 and at most 1000; \
             pattern: whole number, optional, the line pattern from 0 (no line) to 23; \
             begin_arrow: whole number, optional, the line end at the start of the line from 0 (none) to 45; \
             end_arrow: whole number, optional, the line end at the end of the line from 0 (none) to 45; \
             begin_arrow_size: whole number, optional, how big the line end at the start is, from 0 (very small) to 6 (colossal), 2 being medium; \
             end_arrow_size: whole number, optional, how big the line end at the end is, from 0 (very small) to 6 (colossal), 2 being medium; \
             rounding: number, optional, the radius that rounds the corners in inches, from 0 to 100; \
             fill_transparency: number, optional, how see-through the fill is, from 0 (opaque) to 100 (clear) percent, on half-percent steps (with a gradient on, the same on every stop); \
             line_transparency: number, optional, how see-through the line is, from 0 to 100 percent; \
             fill_pattern: whole number, optional, the fill pattern from 1 (solid) to 24 (hatches and stipples, the gradients are set by shape.gradient), which turns a gradient off; \
             fill_background: text, optional, the colour between the marks of a pattern as #RRGGBB; \
             line_cap: text, optional, how the ends of the line finish: round, flat or square; \
             compound: text, optional, the compound line: single, double, thick_thin, thin_thick or triple",
            has_document,
            shape_format
        ),
        cmd!(
            "text.format",
            "Format Text",
            ["Home", "Font"],
            None,
            "ids: array of whole numbers, the shapes whose text to format (a group is formatted through the shapes inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             start: whole number, optional, and with end: format only the characters from start up to end of the one shape named, counted in \
             characters from 0 and not counting its fields, and keep its runs and fields (start equal to end is a point, which formats no character); \
             end: whole number, optional, see start; \
             font: text, optional, the name of the face; \
             size: number, optional, the size in points, more than 0 and at most 1000; \
             bold: boolean, optional; \
             italic: boolean, optional; \
             underline: boolean, optional; \
             strike: boolean, optional; \
             color: text, optional, the colour of the text as #RRGGBB, a colour of the page's theme as theme:accent2 (with a shade as theme:accent2:40, as for shape.format's fill), or default, which hands the colour to the theme; \
             align: text, optional, how the lines of the paragraphs sit: left, center, right, justify (every line but the last of a paragraph fills the margins) or distributed (every line fills them, the last too); \
             valign: text, optional, where the text sits in its block: top, middle or bottom; \
             bullet: whole number, optional, the bullet of the paragraphs: 0 for none and 1 to 7 for the marks of the table (a paragraph with no indent is also given the hanging form a bulleted paragraph has, `IndLeft` 0.25 and `IndFirst` -0.25 inch, and a bullet taken off takes that away again); \
             indent: text, optional, increase or decrease: the left indent of each paragraph moves to the next multiple of a quarter inch, up or down, and not below 0; \
             case: text, optional, how the letters show (Change Case): normal, all_caps, initial_caps or small_caps (the text itself is not changed, only how it shows); \
             direction: text, optional, the reading direction of the paragraphs: ltr or rtl; \
             vertical: boolean, optional, true to run the text down the block (`TextDirection` 1) and false to run it across",
            has_document,
            text_format
        ),
        cmd!(
            query "shape.formatting",
            "Show Formatting",
            ["Home", "Font"],
            None,
            "ids: array of whole numbers, the shapes to describe (for a group, the first shape inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             start: whole number, optional, and with end: describe the text of the one shape named for the characters from start up to end, counted in \
             characters from 0 and not counting its fields, as text.format counts them: each of font, size, bold, italic, underline, strike, color, align, bullet, case and direction is \
             then one value or the word mixed when the runs or paragraphs the range touches differ, and mixed lists those (start equal to end is a point: the \
             run before it, the one typing there joins); \
             end: whole number, optional, see start",
            has_document,
            shape_formatting
        ),
    ]
}

/// The most a weight may be, in points: past any line a page holds.
const MAX_WEIGHT_POINTS: f64 = 1_000.0;
/// The most a corner radius may be, in paper inches.
const MAX_ROUNDING_INCHES: f64 = 100.0;
/// The highest line pattern and line end numbers that are drawn by a number alone (254 names a master).
const MAX_LINE_PATTERN: u32 = 23;
const MAX_LINE_END: u32 = 45;
/// The largest size class of a line end (`BeginArrowSize`, `EndArrowSize`: 0 to 6).
const MAX_ARROW_SIZE: u32 = 6;

/// The number `p` gives for `key`, if it gives one: finite.
fn optional_number(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(bad(cmd, format!("`{key}` must be a number"))),
        },
    }
}

/// The truth `p` gives for `key`, if it gives one: `true` or `false`, not a number.
fn optional_bool(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// A whole number from 0 to `most`, if `p` gives one.
fn optional_up_to(p: &Value, key: &str, most: u32, cmd: &str) -> Result<Option<u8>> {
    let Some(n) = optional_whole(p, key, cmd)? else { return Ok(None) };
    match u8::try_from(n) {
        Ok(n) if u32::from(n) <= most => Ok(Some(n)),
        _ => Err(bad(cmd, format!("`{key}` must be a whole number from 0 to {most}"))),
    }
}

/// A colour as `#RRGGBB`, if `p` gives one.
fn optional_colour(p: &Value, key: &str, cmd: &str) -> Result<Option<Rgb>> {
    let Some(text) = optional_text(p, key, cmd)? else { return Ok(None) };
    Rgb::parse(text).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a colour like #3C78D8; not `{}`", shown(text))))
}

/// A percentage from 0 to 100, if `p` gives one.
fn optional_percent(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    match optional_number(p, key, cmd)? {
        Some(n) if (0.0..=100.0).contains(&n) => Ok(Some(n)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be from 0 to 100 percent"))),
        None => Ok(None),
    }
}

/// The fill pattern, a whole number from 1 to 24: the solid fill, and the hatches and stipples (the gradients 25 to 40 are `shape.gradient`'s).
fn optional_pattern(p: &Value, cmd: &str) -> Result<Option<u8>> {
    let Some(n) = optional_whole(p, "fill_pattern", cmd)? else { return Ok(None) };
    match u8::try_from(n) {
        Ok(n) if (1..=24).contains(&n) => Ok(Some(n)),
        _ => Err(bad(cmd, "`fill_pattern` must be a whole number from 1 (solid) to 24; the gradients are set with shape.gradient")),
    }
}

/// The number of the word `p` gives for `key` in `words` (the place in the list), if it gives one.
fn optional_word(p: &Value, key: &str, words: &[&str], list: &str, cmd: &str) -> Result<Option<u8>> {
    let Some(word) = optional_text(p, key, cmd)? else { return Ok(None) };
    words
        .iter()
        .position(|w| w.eq_ignore_ascii_case(word))
        .and_then(|at| u8::try_from(at).ok())
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("`{key}` must be {list}; not `{}`", shown(word))))
}

/// A paint: `none`, a colour as `#RRGGBB`, a colour of the page's theme (`theme:accent2:40`), or `default`.
fn optional_paint(p: &Value, key: &str, cmd: &str) -> Result<Option<Paint>> {
    match optional_text(p, key, cmd)? {
        Some(text) => parse_paint(text, key, true).map(Some).map_err(|problem| problem.into_engine(cmd)),
        None => Ok(None),
    }
}

/// The colour of text: `#RRGGBB`, a colour of the page's theme, or `default`.
fn optional_text_paint(p: &Value, key: &str, cmd: &str) -> Result<Option<TextPaint>> {
    match optional_text(p, key, cmd)? {
        Some(text) => TextPaint::parse(text).map(Some).map_err(|problem| problem.into_engine(cmd)),
        None => Ok(None),
    }
}

/// The page the command works on (see [`page_for`]).
fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

/// Sets the fill and line cells of shapes → `{page, formatted}`: how many shapes were written to (a group counts the shapes inside it).
fn shape_format(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.format";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let mut format = ShapeFormat {
        fill: optional_paint(p, "fill", ID)?,
        line: optional_paint(p, "line", ID)?,
        pattern: optional_up_to(p, "pattern", MAX_LINE_PATTERN, ID)?,
        begin_arrow: optional_up_to(p, "begin_arrow", MAX_LINE_END, ID)?,
        end_arrow: optional_up_to(p, "end_arrow", MAX_LINE_END, ID)?,
        begin_arrow_size: optional_up_to(p, "begin_arrow_size", MAX_ARROW_SIZE, ID)?,
        end_arrow_size: optional_up_to(p, "end_arrow_size", MAX_ARROW_SIZE, ID)?,
        fill_transparency: optional_percent(p, "fill_transparency", ID)?,
        line_transparency: optional_percent(p, "line_transparency", ID)?,
        fill_pattern: optional_pattern(p, ID)?,
        fill_background: optional_colour(p, "fill_background", ID)?,
        line_cap: optional_word(p, "line_cap", &CAPS, "round, flat or square", ID)?,
        compound: optional_word(p, "compound", &COMPOUNDS, "single, double, thick_thin, thin_thick or triple", ID)?,
        ..ShapeFormat::default()
    };
    if let Some(weight) = optional_number(p, "weight", ID)? {
        if weight <= 0.0 || weight > MAX_WEIGHT_POINTS {
            return Err(bad(ID, format!("`weight` must be more than 0 and at most {MAX_WEIGHT_POINTS} points")));
        }
        format.weight = Some(weight);
    }
    if let Some(rounding) = optional_number(p, "rounding", ID)? {
        if !(0.0..=MAX_ROUNDING_INCHES).contains(&rounding) {
            return Err(bad(ID, format!("`rounding` must be from 0 to {MAX_ROUNDING_INCHES} inches")));
        }
        // Rounding is a length of the drawing, in the page's units.
        let k = s.document().map_or(1.0, |doc| units_per_inch(doc, page));
        format.rounding = Some(rounding * k);
    }
    if format == ShapeFormat::default() {
        return Err(bad(
            ID,
            "nothing to format: give at least one of fill, line, weight, pattern, begin_arrow, end_arrow, begin_arrow_size, end_arrow_size, rounding, fill_transparency, line_transparency, fill_pattern, fill_background, line_cap or compound",
        ));
    }
    s.edit_if("Format shapes", |doc| {
        let formatted = format_shapes(doc, page, &ids, &format).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "formatted": formatted }), formatted > 0))
    })
}

/// Sets the character, paragraph and block cells of the text of shapes, or of a range of the characters of one → `{page, formatted}`.
fn text_format(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "text.format";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let range = optional_range(p, ID)?;
    let align = match optional_text(p, "align", ID)? {
        None => None,
        Some(word) => Some(
            Align::from_word(word)
                .ok_or_else(|| bad(ID, format!("`align` must be left, center, right, justify or distributed; not `{}`", shown(word))))?,
        ),
    };
    let valign = match optional_text(p, "valign", ID)? {
        None => None,
        Some(word) => Some(VAlign::from_word(word).ok_or_else(|| bad(ID, format!("`valign` must be top, middle or bottom; not `{}`", shown(word))))?),
    };
    let bullet = optional_up_to(p, "bullet", u32::from(crate::formatting::MAX_BULLET), ID)?;
    let indent = match optional_text(p, "indent", ID)? {
        None => None,
        Some(word) => Some(Indent::from_word(word).ok_or_else(|| bad(ID, format!("`indent` must be increase or decrease; not `{}`", shown(word))))?),
    };
    let case = match optional_text(p, "case", ID)? {
        None => None,
        Some(word) => Some(
            Case::from_word(word)
                .ok_or_else(|| bad(ID, format!("`case` must be normal, all_caps, initial_caps or small_caps; not `{}`", shown(word))))?,
        ),
    };
    let direction = match optional_text(p, "direction", ID)? {
        None => None,
        Some(word) => Some(Direction::from_word(word).ok_or_else(|| bad(ID, format!("`direction` must be ltr or rtl; not `{}`", shown(word))))?),
    };
    let format = TextFormat {
        font: optional_text(p, "font", ID)?.map(str::to_owned),
        size: optional_number(p, "size", ID)?,
        bold: optional_bool(p, "bold", ID)?,
        italic: optional_bool(p, "italic", ID)?,
        underline: optional_bool(p, "underline", ID)?,
        strike: optional_bool(p, "strike", ID)?,
        color: optional_text_paint(p, "color", ID)?,
        align,
        valign,
        bullet,
        indent,
        case,
        direction,
        vertical: optional_bool(p, "vertical", ID)?,
    };
    if format.is_empty() {
        return Err(bad(
            ID,
            "nothing to format: give at least one of font, size, bold, italic, underline, strike, color, align, valign, bullet, indent, case, direction or vertical",
        ));
    }
    s.edit_if("Format text", |doc| {
        let formatted = format_text(doc, page, &ids, range, &format).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "formatted": formatted }), formatted > 0))
    })
}

/// A number for a reply: rounded to a millionth, so that arithmetic noise (`0.7500000000000001` points) is not shown.
fn shown_number(n: f64) -> f64 {
    let rounded = (n * 1.0e6).round() / 1.0e6;
    if rounded.is_finite() { rounded } else { 0.0 }
}

/// A shape's resolved formatting as the reply holds it.
fn look_json(look: &ShapeLook, k: f64) -> Value {
    let paint = |c: Option<Rgb>| c.map_or_else(|| "none".to_owned(), Rgb::hex);
    let mut out = json!({
        "id": look.id,
        "fill": paint(look.fill),
        "line": paint(look.line),
        "weight": shown_number(look.weight),
        "pattern": look.pattern,
        "begin_arrow": look.begin_arrow,
        "end_arrow": look.end_arrow,
        "begin_arrow_size": look.begin_arrow_size,
        "end_arrow_size": look.end_arrow_size,
        "rounding": shown_number(look.rounding / k),
        "font": look.font,
        "size": shown_number(look.size),
        "bold": look.bold,
        "italic": look.italic,
        "underline": look.underline,
        "strike": look.strike,
        "color": look.color.hex(),
        "align": look.align.word(),
        "valign": look.valign.word(),
        "bullet": look.bullet,
        "ind_left": shown_number(look.ind_left),
        "case": look.case.word(),
        "direction": look.direction.word(),
        "vertical": look.vertical,
    });
    if let Some(map) = out.as_object_mut() {
        for (key, value) in super::effects::fx_json(&look.fx) {
            map.insert(key.to_owned(), value);
        }
    }
    out
}

/// The reply for a range: the shape's look, with the word `mixed` for each character property the range's runs do not share, and `mixed` listing them
/// (a face may be called `mixed`, so a program tells a mixed font by the list and not by the word).
fn range_json(seen: &RangeLook, k: f64) -> Value {
    let mut out = look_json(&seen.look, k);
    let names = seen.mixed.names();
    if let Some(map) = out.as_object_mut() {
        for name in &names {
            map.insert((*name).to_owned(), json!(MIXED));
        }
        map.insert("mixed".to_owned(), json!(names));
    }
    out
}

/// What a property holds in a reply when the runs of the range differ in it.
const MIXED: &str = "mixed";

/// What the cells of shapes resolve to, for the controls that show it → `{page, shapes: [{id, fill, line, weight, ...}]}`, in the order of `ids`:
/// the colours as `#rrggbb` or `none`; the weight and the size in points; `rounding` in inches; the face, size, style, colour and alignment of the
/// first run of the text; `bullet`, `ind_left` (inches), `case`, `direction` and `vertical` of its first paragraph and block. With `start` and `end` (one
/// shape that is not a group) the face, size, bold, italic, underline, strike, colour, alignment, bullet, case and direction
/// are those of the characters in the range, each the one value the runs share or `"mixed"`, and the shape's entry holds `mixed`, the list of the
/// properties that are. Nothing is changed and nothing is recorded.
fn shape_formatting(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.formatting";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let range = optional_range(p, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let k = units_per_inch(doc, page);
    if let Some((start, end)) = range {
        let seen = look_of_range(doc, page, &ids, start, end).map_err(|problem| problem.into_engine(ID))?;
        return Ok(json!({ "page": page, "shapes": [range_json(&seen, k)] }));
    }
    let looks = look_of(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
    Ok(json!({ "page": page, "shapes": looks.iter().map(|look| look_json(look, k)).collect::<Vec<_>>() }))
}
