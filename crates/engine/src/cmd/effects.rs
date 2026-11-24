//! Gradients, shadows and Quick Styles: `shape.gradient`, `shape.shadow`, `shape.quick_style` and `shape.quick_styles`.
//!
//! The commands check their parameters and turn them into the types of [`crate::formatting`], which writes the cells (`formatting/effects.rs`, `quick.rs`). The
//! fill's and line's transparency, the pattern fill, the line cap and the compound line are parameters of `shape.format`, in `cmd/format.rs`. A shadow's lengths are
//! points on the paper and its angle degrees clockwise from the right, as the cells' words and the Format Shape pane's fields are.

use serde_json::{Value, json};

use super::params::{optional_whole, page_for, required_ids};
use super::{CommandSpec, has_document, optional_text};
use crate::Session;
use crate::error::{Result, bad, failed, shown};
use crate::formatting::{
    FxLook, GradientFormat, GradientKind, GradientLook, GradientTarget, MAX_GRADIENT_STOPS, QuickStyle, QuickTile, Rgb, ShadowFormat, ShadowLook,
    StopFormat, StopLook, format_gradient, format_quick_style, format_shadow, quick_tiles, shadow_preset,
};

/// The words of `LineCap` (what files draw, whatever MS-VSDX names them), in the order of the cell's numbers.
pub(super) const CAPS: [&str; 3] = ["round", "flat", "square"];
/// The words of `CompoundType`, in the order of the cell's numbers.
pub(super) const COMPOUNDS: [&str; 5] = ["single", "double", "thick_thin", "thin_thick", "triple"];

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.gradient",
            "Gradient Fill",
            ["Home", "Shape Styles"],
            None,
            "ids: array of whole numbers, the shapes to give the gradient (a group is formatted through the shapes inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             target: text, optional, fill (the default) or line: which of the two gradients is set; \
             on: boolean, optional, false to turn the gradient off (its stops stay) and anything else given turns it on; \
             kind: text, optional, linear, radial, rectangular or path: the layout, and with it origin; \
             origin: text, optional, where a radial or rectangular gradient starts: center (the default), top_left, top_right, bottom_left or bottom_right, and for a radial one also top or bottom; \
             direction: whole number, optional, from 0 to 13: the layout and its origin as the cell FillGradientDir holds them (0 linear, 1 to 7 radial, 8 to 12 rectangular, 13 along the path), which kind and origin name; \
             angle: number, optional, degrees clockwise from the right as seen on the page, for a linear gradient (90 runs from the top to the bottom); \
             rotate_with_shape: boolean, optional, whether a fill's gradient turns with the shape (the default of a shape is true); \
             stops: array, optional, from 2 to 10 objects {position: number from 0 to 1, color: text #RRGGBB, transparency: number from 0 to 100, optional, 0 by default}, which replace the stops the shape has \
             (turned on with none given the shape keeps its own, or gets two: its colour and a lighter one)",
            has_document,
            shape_gradient
        ),
        cmd!(
            "shape.shadow",
            "Shadow",
            ["Home", "Shape Styles"],
            None,
            "ids: array of whole numbers, the shapes to give the shadow (a group is formatted through the shapes inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             on: boolean, optional, false to take the shadow away and anything else given turns it on; \
             preset: text, optional, a shadow to start from: none, bottom_right, bottom, bottom_left, right, center, left, top_right, top or top_left (the settings below replace the preset's); \
             color: text, optional, the colour of the shadow as #RRGGBB (black for a shadow that was off); \
             transparency: number, optional, from 0 (opaque) to 100 (clear), 60 for a shadow that was off; \
             size: number, optional, the size of the shadow in percent of the shape, from 1 to 1000, 100 for a shadow that was off; \
             blur: number, optional, in points on the paper from 0 to 1000, 4 for a shadow that was off; \
             angle: number, optional, degrees clockwise from the right as seen on the page (90 is straight down), 45 for a shadow that was off; \
             distance: number, optional, in points on the paper from 0 to 1000, 4 for a shadow that was off",
            has_document,
            shape_shadow
        ),
        cmd!(
            "shape.quick_style",
            "Quick Styles",
            ["Home", "Shape Styles"],
            None,
            "ids: array of whole numbers, the shapes to put the style on (a group is formatted through the shapes inside it); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes); \
             row: whole number, optional, from 1 to 6: the row of the Theme Styles in the gallery, with column; \
             column: whole number, optional, from 1 to 7: the column of the Theme Styles, with row; \
             variant: whole number, optional, from 0 to 3: one of the four Variant Styles instead of row and column",
            has_document,
            shape_quick_style
        ),
        cmd!(
            query "shape.quick_styles",
            "Show Quick Styles",
            ["Home", "Shape Styles"],
            None,
            "ids: array of whole numbers, the shapes the styles are for (the first shape inside the first one named gives the look of every tile); \
             page: whole number, optional, the id of the page (default: the first page holding all the shapes)",
            has_document,
            shape_quick_styles
        ),
    ]
}

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

/// A number in `from` to `to` (both included), if `p` gives one.
fn optional_in(p: &Value, key: &str, from: f64, to: f64, unit: &str, cmd: &str) -> Result<Option<f64>> {
    match optional_number(p, key, cmd)? {
        Some(n) if (from..=to).contains(&n) => Ok(Some(n)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be from {from} to {to}{unit}"))),
        None => Ok(None),
    }
}

/// The truth `p` gives for `key`, if it gives one.
fn optional_bool(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// A colour as `#RRGGBB`, if `p` gives one.
fn optional_colour(p: &Value, key: &str, cmd: &str) -> Result<Option<Rgb>> {
    let Some(text) = optional_text(p, key, cmd)? else { return Ok(None) };
    Rgb::parse(text).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a colour like #3C78D8; not `{}`", shown(text))))
}

fn hex(c: Rgb) -> String {
    c.hex()
}

fn target_page(s: &Session, p: &Value, ids: &[u32], cmd: &str) -> Result<u32> {
    let named = optional_whole(p, "page", cmd)?;
    let doc = s.document().ok_or_else(|| failed(cmd, "no document open"))?;
    page_for(doc, named, ids, cmd)
}

// ---- shape.gradient ----------------------------------------------------------------------------------------------------------------

/// The stops `p` gives: two to ten objects.
fn stops_of(p: &Value, cmd: &str) -> Result<Option<Vec<StopFormat>>> {
    let Some(value) = p.get("stops").filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(list) = value.as_array() else { return Err(bad(cmd, "`stops` must be an array of {position, color, transparency}")) };
    if !(2..=MAX_GRADIENT_STOPS).contains(&list.len()) {
        return Err(bad(cmd, format!("`stops` must hold from 2 to {MAX_GRADIENT_STOPS} stops, not {}", list.len())));
    }
    let mut out = Vec::with_capacity(list.len());
    for (n, item) in list.iter().enumerate() {
        let at = |what: &str| format!("stop {n}: `{what}`");
        let position = item.get("position").and_then(Value::as_f64).filter(|v| (0.0..=1.0).contains(v));
        let position = position.ok_or_else(|| bad(cmd, format!("{} must be a number from 0 to 1", at("position"))))?;
        let colour = item.get("color").and_then(Value::as_str).and_then(Rgb::parse);
        let colour = colour.ok_or_else(|| bad(cmd, format!("{} must be a colour like #3C78D8", at("color"))))?;
        let transparency = match item.get("transparency") {
            None | Some(Value::Null) => 0.0,
            Some(v) => v
                .as_f64()
                .filter(|v| (0.0..=100.0).contains(v))
                .ok_or_else(|| bad(cmd, format!("{} must be a number from 0 to 100", at("transparency"))))?,
        };
        out.push(StopFormat { position, colour, transparency });
    }
    Ok(Some(out))
}

/// Sets a fill's or a line's gradient → `{page, target, formatted}`.
fn shape_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.gradient";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let target = match optional_text(p, "target", ID)? {
        None => GradientTarget::Fill,
        Some(word) => GradientTarget::from_word(word).ok_or_else(|| bad(ID, format!("`target` must be fill or line; not `{}`", shown(word))))?,
    };
    let kind = match optional_text(p, "kind", ID)? {
        None => None,
        Some(word) => Some(
            GradientKind::from_word(word)
                .ok_or_else(|| bad(ID, format!("`kind` must be linear, radial, rectangular or path; not `{}`", shown(word))))?,
        ),
    };
    let origin = optional_text(p, "origin", ID)?;
    let direction = match optional_whole(p, "direction", ID)? {
        None => None,
        Some(n) if n <= 13 => Some(n as u8),
        Some(_) => return Err(bad(ID, "`direction` must be a whole number from 0 to 13")),
    };
    if direction.is_some() && (kind.is_some() || origin.is_some()) {
        return Err(bad(ID, "give `direction`, or `kind` with `origin`, and not both"));
    }
    let dir = match (direction, kind, origin) {
        (Some(d), _, _) => Some(d),
        (None, Some(kind), origin) => Some(kind.dir_at(origin.unwrap_or("center")).ok_or_else(|| {
            bad(ID, format!("a {} gradient has no origin `{}`: see `origin` for the places it has", kind.word(), shown(origin.unwrap_or(""))))
        })?),
        (None, None, Some(_)) => return Err(bad(ID, "`origin` needs a `kind`: linear, radial, rectangular or path")),
        (None, None, None) => None,
    };
    let format = GradientFormat {
        on: optional_bool(p, "on", ID)?,
        dir,
        angle: optional_number(p, "angle", ID)?,
        rotate_with_shape: optional_bool(p, "rotate_with_shape", ID)?,
        stops: stops_of(p, ID)?,
    };
    if format.is_empty() {
        return Err(bad(ID, "nothing to set: give at least one of on, kind, direction, angle, rotate_with_shape or stops"));
    }
    if let Some(angle) = format.angle
        && angle.abs() > 360_000.0
    {
        return Err(bad(ID, "`angle` must be from -360000 to 360000 degrees"));
    }
    s.edit_if("Gradient", |doc| {
        let formatted = format_gradient(doc, page, &ids, target, &format).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "target": target.word(), "formatted": formatted }), formatted > 0))
    })
}

// ---- shape.shadow ------------------------------------------------------------------------------------------------------------------

/// Sets a shadow → `{page, formatted}`.
fn shape_shadow(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.shadow";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let mut format = match optional_text(p, "preset", ID)? {
        None => ShadowFormat::default(),
        Some(word) => shadow_preset(word).ok_or_else(|| {
            bad(
                ID,
                format!(
                    "`preset` must be none, bottom_right, bottom, bottom_left, right, center, left, top_right, top or top_left; not `{}`",
                    shown(word)
                ),
            )
        })?,
    };
    let (size_from, size_to) = (crate::formatting::SHADOW_SIZE_RANGE.0, crate::formatting::SHADOW_SIZE_RANGE.1);
    let max = crate::formatting::SHADOW_MAX_POINTS;
    if let Some(on) = optional_bool(p, "on", ID)? {
        format.on = Some(on);
    }
    if let Some(c) = optional_colour(p, "color", ID)? {
        format.colour = Some(c);
    }
    if let Some(t) = optional_in(p, "transparency", 0.0, 100.0, " percent", ID)? {
        format.transparency = Some(t);
    }
    if let Some(n) = optional_in(p, "size", size_from, size_to, " percent", ID)? {
        format.size = Some(n);
    }
    if let Some(n) = optional_in(p, "blur", 0.0, max, " points", ID)? {
        format.blur = Some(n);
    }
    if let Some(n) = optional_in(p, "angle", -360_000.0, 360_000.0, " degrees", ID)? {
        format.angle = Some(n);
    }
    if let Some(n) = optional_in(p, "distance", 0.0, max, " points", ID)? {
        format.distance = Some(n);
    }
    if format.is_empty() {
        return Err(bad(ID, "nothing to set: give at least one of on, preset, color, transparency, size, blur, angle or distance"));
    }
    s.edit_if("Shadow", |doc| {
        let formatted = format_shadow(doc, page, &ids, &format).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "formatted": formatted }), formatted > 0))
    })
}

// ---- Quick Styles ------------------------------------------------------------------------------------------------------------------

/// Puts a Quick Style on shapes → `{page, formatted, matrix, colour}`.
fn shape_quick_style(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.quick_style";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let (row, column, variant) = (optional_whole(p, "row", ID)?, optional_whole(p, "column", ID)?, optional_whole(p, "variant", ID)?);
    let style = match (row, column, variant) {
        (Some(row), Some(column), None) => {
            QuickStyle::at(row, column).ok_or_else(|| bad(ID, "`row` must be from 1 to 6 and `column` from 1 to 7"))?
        }
        (None, None, Some(variant)) => QuickStyle::variant(variant).ok_or_else(|| bad(ID, "`variant` must be from 0 to 3"))?,
        (None, None, None) => return Err(bad(ID, "give `row` and `column` (a Theme Style), or `variant` (a Variant Style)")),
        (_, _, Some(_)) => return Err(bad(ID, "give `row` and `column`, or `variant`, and not both")),
        _ => return Err(bad(ID, "`row` and `column` go together: give both or neither")),
    };
    s.edit_if("Quick Style", |doc| {
        let formatted = format_quick_style(doc, page, &ids, style).map_err(|problem| problem.into_engine(ID))?;
        Ok((json!({ "page": page, "formatted": formatted, "matrix": style.matrix, "colour": style.colour }), formatted > 0))
    })
}

fn stop_json(stop: &StopLook) -> Value {
    json!({ "position": stop.position, "color": hex(stop.colour), "transparency": stop.transparency })
}

/// A gradient as the replies hold it.
pub(super) fn gradient_json(g: &GradientLook) -> Value {
    let kind = GradientKind::of_dir(g.dir);
    json!({
        "direction": g.dir,
        "kind": kind.word(),
        "origin": GradientKind::origin_of(g.dir),
        "angle": g.angle,
        "rotate_with_shape": g.rotate_with_shape,
        "stops": g.stops.iter().map(stop_json).collect::<Vec<_>>(),
    })
}

/// A shadow as the replies hold it.
pub(super) fn shadow_json(s: &ShadowLook) -> Value {
    json!({
        "color": hex(s.colour),
        "transparency": s.transparency,
        "size": s.size,
        "blur": s.blur,
        "angle": s.angle,
        "distance": s.distance,
        "kind": s.kind,
    })
}

/// The fill's and line's transparency, pattern, gradients, cap, compound line and shadow, the keys `shape.formatting` adds to a shape's entry.
pub(super) fn fx_json(fx: &FxLook) -> Vec<(&'static str, Value)> {
    vec![
        ("fill_pattern", json!(fx.fill_pattern)),
        ("fill_transparency", json!(fx.fill_transparency)),
        ("fill_background", fx.fill_background.map_or(Value::Null, |c| json!(hex(c)))),
        ("line_transparency", json!(fx.line_transparency)),
        ("line_cap", json!(CAPS.get(usize::from(fx.line_cap)).copied().unwrap_or("round"))),
        ("compound", json!(COMPOUNDS.get(usize::from(fx.compound)).copied().unwrap_or("single"))),
        ("gradient", fx.gradient.as_ref().map_or(Value::Null, gradient_json)),
        ("line_gradient", fx.line_gradient.as_ref().map_or(Value::Null, gradient_json)),
        ("shadow", fx.shadow.as_ref().map_or(Value::Null, shadow_json)),
    ]
}

fn tile_json(t: &QuickTile, place: Value) -> Value {
    let paint = |c: Option<Rgb>| c.map_or_else(|| "none".to_owned(), Rgb::hex);
    let mut out = json!({
        "matrix": t.style.matrix,
        "colour": t.style.colour,
        "fill": paint(t.fill),
        "line": paint(t.line),
        "weight": (t.weight * 1.0e4).round() / 1.0e4,
        "fill_transparency": t.fx.fill_transparency,
        "gradient": t.fx.gradient.as_ref().map_or(Value::Null, gradient_json),
        "shadow": t.fx.shadow.as_ref().map_or(Value::Null, shadow_json),
    });
    if let (Some(map), Some(extra)) = (out.as_object_mut(), place.as_object()) {
        for (k, v) in extra {
            map.insert(k.clone(), v.clone());
        }
    }
    out
}

/// The gallery of Quick Styles for a shape → `{page, current: {matrix, colour}, variants: [tile], styles: [tile]}`: the four Variant Styles and the Theme Styles row by
/// row (six rows of seven), each tile `{variant | row and column, matrix, colour, fill, line, weight, fill_transparency, gradient, shadow}` as the shape looks in it.
fn shape_quick_styles(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "shape.quick_styles";
    let ids = required_ids(p, "ids", ID)?;
    let page = target_page(s, p, &ids, ID)?;
    let doc = s.document().ok_or_else(|| failed(ID, "no document open"))?;
    let look = quick_tiles(doc, page, &ids).map_err(|problem| problem.into_engine(ID))?;
    let variants: Vec<Value> = look.variants.iter().enumerate().map(|(n, t)| tile_json(t, json!({ "variant": n }))).collect();
    let styles: Vec<Value> = look.styles.iter().enumerate().map(|(n, t)| tile_json(t, json!({ "row": n / 7 + 1, "column": n % 7 + 1 }))).collect();
    Ok(json!({ "page": page, "current": { "matrix": look.current.matrix, "colour": look.current.colour }, "variants": variants, "styles": styles }))
}
