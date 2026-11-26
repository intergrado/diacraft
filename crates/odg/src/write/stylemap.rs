//! The cells a restyle changes, and the attributes of a style they become:
//! which cell feeds which attribute of the shape's automatic style, and what an edit of the cell writes there.
//!
//! **What is written.** The fill (`FillPattern` 0 and 1, `FillForegnd`, `FillForegndTrans`), the line (`LinePattern` 0 and 1, `LineWeight`,
//! `LineColor`, `LineColorTrans`, `LineCap`), and the text block's vertical alignment and four margins. **What is refused, in words:** a pattern
//! of 2 or more (a hatch is written as a solid fill with the pattern number as tier 2; a dash needs a `draw:stroke-dash` in `office:styles`),
//! a gradient, a colour that is not `#rrggbb` (a palette index is written as its RGB, which needs the palette), a cell
//! that holds a formula (the formula is tier 2), and every cell not named here: a shadow, a line end, a run's or a paragraph's cells. A
//! change is never dropped.

use diacraft_doc::{Cell, FormulaSlot};

use crate::read::names::Ns;

/// The cells of a cell map: the number and text a cell holds, and its unit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CellVal {
    pub text: String,
    pub number: Option<f64>,
    pub unit: Option<String>,
}

/// Whether the cell `name` is a line end's number and says 254, the custom end (`USE` of a master): the writer draws number 4's marker for it and the
/// cell itself is tier 2.
pub(crate) fn is_custom_end(name: &str, cell: &Cell) -> bool {
    crate::ext::fingerprint::is_custom_end(name, cell)
}

impl CellVal {
    /// The number a custom end (254) is written as: 4, whose marker is what the writer draws for it.
    pub(crate) fn stand_in() -> CellVal {
        CellVal { text: "4".to_owned(), number: Some(4.0), unit: None }
    }

    /// **The value a cell with a formula of its own was last worked out to**: what a new shape's style says of a cell whose formula DiaCraft's data carries beside it, so
    /// that another program draws the shape with the value and DiaCraft reads the formula back (a VSDX shape's `RGB(255,0,0)` fill, a `GUARD` of its weight). `None` for a cell with no formula of its
    /// own, an inherited one, no value, or data of its own beside them (the cell is carried whole, as before).
    pub(crate) fn cached(cell: &Cell) -> Option<CellVal> {
        if !matches!(cell.formula, FormulaSlot::Local(_)) || cell.extra.is_some() || !cell.raw.is_empty() {
            return None;
        }
        let value = cell.value.as_ref()?;
        Some(CellVal { text: value.text().to_owned(), number: value.number(), unit: cell.unit.as_deref().map(str::to_owned) })
    }

    /// The value of a cell that holds no formula: `Err` for one that does (its formula is tier 2).
    pub(crate) fn of(cell: &Cell) -> Result<CellVal, &'static str> {
        if matches!(cell.formula, FormulaSlot::Local(_) | FormulaSlot::Inherit) {
            return Err("a style cell that holds a formula (the formula is tier 2, which a later version writes)");
        }
        let value = cell.value.as_ref().ok_or("a style cell with no value")?;
        Ok(CellVal { text: value.text().to_owned(), number: value.number(), unit: cell.unit.as_deref().map(str::to_owned) })
    }
}

/// What a restyle changes of a shape: each style cell that differs, with its value in the model (`None`: the cell is gone, so the property is
/// the style's parent's again).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StyleEdit {
    pub changes: Vec<(&'static str, Option<CellVal>)>,
    /// The numbers the model's line-end cells and `LineWeight` (inches) have, each the shape's own or what its line style gives it: what the attributes of a
    /// line end are worked out from (the width is the mark's spread times `S`, which is the size class's base plus the weight). Set by
    /// [`crate::write::own::decide_line_end`] for an edit that changes a line-end cell.
    pub line: Vec<(&'static str, f64)>,
    /// The line-end cells (`BeginArrow`, `EndArrow`) of the model whose end is DiaCraft's filled diamond (a custom end that names the master `Filled Diamond`): the writer draws
    /// the marker of its own for each ([`super::markers::DIAMOND`]) and not number 4's, the cell carried all the same. Set with `line` by [`crate::write::own::decide_line_end`].
    pub diamonds: Vec<&'static str>,
    /// The four margin cells' values in the model, for the paddings that are written together.
    pub margins: Vec<(&'static str, Option<CellVal>)>,
    /// The `draw:fill` the style must state when the edit sets a fill colour or turns a gradient off and says nothing of `FillPattern`: `solid`, when the
    /// model's fill pattern is 1 (`own::decide_fill` sets it). Without it the style's `draw:fill` stays as the file has it, so a colour set
    /// on a shape whose fill is a gradient, a hatch or a bitmap, or whose styles state no fill at all, was drawn by an ODF reader as it was before
    /// (milestone 1, class F4). A pattern of 0 is never stated by an edit that does not change it: the file's own `none`, bitmap or hatch stays.
    pub fill_kind: Option<&'static str>,
    /// Whether the model's shadow is visible (`ShdwPattern` 1): the reader gives `ShapeShdwType` 1 with a visible shadow and none with a hidden one, so a type of 1 is written by the
    /// visible shadow alone, and only then (`ops_for`). `None` when the edit does not know, and a type is then carried.
    pub shadow_visible: Option<bool>,
    /// The fill the edit writes as a display form (a gradient or a hatch of DiaCraft's own, `fx::fill_of`): its attributes are put after the cells' own ([`StyleEdit::fx_ops`]),
    /// and the cells that stand for it are written or carried as it says (`ops_for`).
    pub fx: Option<super::fx::Fill>,
    /// The fill kind the style must say again (`solid` or `none`) because the shape had a gradient of DiaCraft's own written as a display form and has none now: `draw:fill` then says
    /// `gradient` still, and a program that draws the style would draw the gradient the model no longer has.
    pub fx_off: Option<&'static str>,
    /// The style sheet the shape's three links name now, when they named another: written as the `style:parent-style-name` of its automatic style
    /// The root's ID (0) taking the parent out.
    pub parent: Option<u32>,
}

impl StyleEdit {
    /// The attributes that say the display form of the fill, which come after the cells' own and take the place of what they write for the same attributes.
    pub(crate) fn fx_ops(&self) -> Vec<Op> {
        match (self.fx, self.fx_off) {
            (Some(f), _) => f.ops(),
            (None, Some(kind)) => fill_ops(kind),
            (None, None) => Vec::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.parent.is_none() && self.fx.is_none() && self.fx_off.is_none()
    }
}

/// The style cells the writer writes, by name.
pub(crate) const CELLS: [&str; 24] = [
    "FillPattern",
    "FillGradientEnabled",
    "FillForegnd",
    "FillForegndTrans",
    "FillBkgndTrans",
    "ShdwPattern",
    "ShapeShdwType",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "LinePattern",
    "LineWeight",
    "LineColor",
    "LineColorTrans",
    "LineCap",
    "VerticalAlign",
    "TopMargin",
    "BottomMargin",
    "LeftMargin",
    "BeginArrow",
    "BeginArrowSize",
    "EndArrow",
    "EndArrowSize",
];

/// The line-end cells.
pub(crate) const ARROW_CELLS: [&str; 4] = ["BeginArrow", "BeginArrowSize", "EndArrow", "EndArrowSize"];

/// The cells a line end's attributes are worked out from: the four of the ends and the weight of the line.
pub(crate) const LINE_END_CELLS: [&str; 5] = ["BeginArrow", "BeginArrowSize", "EndArrow", "EndArrowSize", "LineWeight"];

/// The four margin cells.
pub(crate) const MARGINS: [(&str, &str); 4] =
    [("TopMargin", "padding-top"), ("BottomMargin", "padding-bottom"), ("LeftMargin", "padding-left"), ("RightMargin", "padding-right")];

/// The cells of a page sheet that a page layout holds: the page's size, margins and orientation.
pub(crate) const PAGE_CELLS: [&str; 7] =
    ["PageWidth", "PageHeight", "PageLeftMargin", "PageRightMargin", "PageTopMargin", "PageBottomMargin", "PrintPageOrientation"];

/// What a page's edit changes: the page cells that differ, with their values in the model, and the height the page had and has.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageEdit {
    pub changes: Vec<(&'static str, Option<CellVal>)>,
    pub height_was: f64,
    pub height_now: f64,
}

/// The attributes of a page layout's properties that an edit of the page cells writes.
pub(crate) fn page_ops(changes: &[(&'static str, Option<CellVal>)]) -> Result<Vec<Op>, &'static str> {
    let mut ops = Vec::new();
    for (cell, new) in changes {
        let (ns, local) = match *cell {
            "PageWidth" => (Ns::Fo, "page-width"),
            "PageHeight" => (Ns::Fo, "page-height"),
            "PageLeftMargin" => (Ns::Fo, "margin-left"),
            "PageRightMargin" => (Ns::Fo, "margin-right"),
            "PageTopMargin" => (Ns::Fo, "margin-top"),
            "PageBottomMargin" => (Ns::Fo, "margin-bottom"),
            _ => (Ns::Style, "print-orientation"),
        };
        let val = match (*cell, new) {
            ("PageWidth" | "PageHeight", None) => return Err("a page with no width or no height"),
            ("PrintPageOrientation", Some(v)) => match v.number {
                Some(1.0) => Val::Text("portrait".into()),
                Some(2.0) => Val::Text("landscape".into()),
                _ => return Err("a page orientation that is not 1 or 2"),
            },
            (_, Some(v)) => length(v)?,
            (_, None) => Val::Remove,
        };
        ops.push(Op { prop: 3, ns, local, val });
    }
    Ok(ops)
}

/// The static name of a style cell the writer writes, or `None`.
pub(crate) fn static_cell(name: &str) -> Option<&'static str> {
    CELLS.iter().copied().chain(std::iter::once("RightMargin")).find(|c| *c == name)
}

/// Which properties element holds an attribute: the graphic properties.
pub(crate) const GRAPHIC: usize = 0;

/// What is written for an attribute of a style.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    /// This text.
    Text(String),
    /// This length in inches, in the unit the attribute has, else the page's.
    Length(f64),
    /// The attribute is taken out.
    Remove,
}

/// One attribute of a style's properties to write.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Op {
    /// The properties element: 0 graphic, 1 paragraph, 2 text.
    pub prop: usize,
    pub ns: Ns,
    pub local: &'static str,
    pub val: Val,
}

fn op(ns: Ns, local: &'static str, val: Val) -> Op {
    Op { prop: GRAPHIC, ns, local, val }
}

/// The attributes of a bitmap fill that go with the picture: the `draw:fill-image-*` and the repeat mode (`style:repeat`) that only a picture uses.
const FILL_IMAGE: [(Ns, &str); 7] = [
    (Ns::Draw, "fill-image-name"),
    (Ns::Draw, "fill-image-width"),
    (Ns::Draw, "fill-image-height"),
    (Ns::Draw, "fill-image-ref-point"),
    (Ns::Draw, "fill-image-ref-point-x"),
    (Ns::Draw, "fill-image-ref-point-y"),
    (Ns::Style, "repeat"),
];

/// Whether the attribute `(ns, local)` is one of a bitmap fill's that go with the picture ([`FILL_IMAGE`]).
pub(crate) fn goes_with_a_picture(ns: Ns, local: &str) -> bool {
    FILL_IMAGE.iter().any(|(n, l)| *n == ns && *l == local)
}

/// `draw:fill` set to `kind`, and the names of a gradient and a hatch and every attribute of a bitmap taken out when the kind no longer uses them (the row:
/// `draw:fill-gradient-name`, `draw:fill-hatch-name`, `draw:fill-hatch-solid`, `draw:fill-image-*`).
pub(crate) fn fill_ops(kind: &str) -> Vec<Op> {
    let mut ops = vec![op(Ns::Draw, "fill", Val::Text(kind.to_owned()))];
    for local in ["fill-gradient-name", "fill-hatch-name", "fill-hatch-solid"] {
        ops.push(op(Ns::Draw, local, Val::Remove));
    }
    for (ns, local) in FILL_IMAGE {
        ops.push(op(ns, local, Val::Remove));
    }
    ops
}

/// The `draw:fill` of [`StyleEdit::fill_kind`], written once for the edit: by the first of the cells that call for it (`FillForegnd`, `FillGradientEnabled`).
fn fill_once(cell: &'static str, edit: &StyleEdit) -> Vec<Op> {
    let Some(kind) = edit.fill_kind else { return Vec::new() };
    let first = edit.changes.iter().map(|(n, _)| *n).find(|n| matches!(*n, "FillForegnd" | "FillGradientEnabled"));
    if first == Some(cell) { fill_ops(kind) } else { Vec::new() }
}

/// A colour as ODF writes it: `#rrggbb`, lower case. `Err` for any other text.
/// Whether `t` is a colour as the schema of the format writes one: `#` and six hexadecimal digits. Every colour the writer writes passes through this.
pub(crate) fn is_colour(t: &str) -> bool {
    t.len() == 7 && t.starts_with('#') && t.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

fn colour(v: &CellVal) -> Result<String, &'static str> {
    let t = v.text.as_str();
    if is_colour(t) {
        Ok(t.to_ascii_lowercase())
    } else {
        Err("a colour cell that is not #rrggbb (a palette index is written as its RGB, which needs the palette)")
    }
}

/// A share of 0 to 1 as the percentage ODF writes for its complement: `75%` for 0.25, to nine places and no more.
fn complement_percent(v: &CellVal) -> Result<String, &'static str> {
    let share = v.number.filter(|n| n.is_finite() && (0.0..=1.0).contains(n)).ok_or("a transparency that is not a number from 0 to 1")?;
    let percent = ((1.0 - share) * 100.0 * 1e9).round() / 1e9;
    Ok(format!("{percent}%"))
}

fn length(v: &CellVal) -> Result<Val, &'static str> {
    let inches = v.number.filter(|n| n.is_finite()).ok_or("a length cell that is not a number")?;
    Ok(Val::Length(inches))
}

/// The attributes written for the change of one style cell: `new` is the cell's value in the model, `None` where it is gone. `all` says what
/// the whole edit holds, for the cells that are written together (the four margins).
pub(crate) fn ops_for(cell: &'static str, new: Option<&CellVal>, edit: &StyleEdit) -> Result<Vec<Op>, &'static str> {
    Ok(match (cell, new) {
        // A gradient written as a display form says the fill's kind itself (`StyleEdit::fx`).
        ("FillPattern", Some(v)) if v.number == Some(1.0) && edit.fx.is_some_and(|f| f.is_gradient()) => Vec::new(),
        ("FillPattern", Some(v)) => match v.number {
            Some(0.0) => fill_ops("none"),
            Some(1.0) => fill_ops("solid"),
            _ => return Err("a fill pattern of 2 or more (a hatch is a solid fill with the pattern as tier 2, which a later version writes)"),
        },
        ("FillPattern", None) => vec![op(Ns::Draw, "fill", Val::Remove)],
        ("FillGradientEnabled", Some(v)) if v.number == Some(0.0) => fill_once(cell, edit),
        // Turned on beside a gradient of DiaCraft's own, which the display form says and the reader reads as `FillGradientEnabled` 1.
        ("FillGradientEnabled", Some(v)) if v.number == Some(1.0) && edit.fx.is_some_and(|f| f.is_gradient()) => Vec::new(),
        ("FillGradientEnabled", _) => return Err("a gradient fill (its mapping is not settled yet)"),
        // The marks of a hatch are the hatch's colour, and `draw:fill-color` is its ground: the cell is carried (`fx::display_of` tells the rebuild).
        ("FillForegnd", Some(_)) if matches!(edit.fx, Some(super::fx::Fill::Hatch(..) | super::fx::Fill::Stipple(..))) => {
            return Err("a fill colour that is the marks of a hatch (the ground takes the attribute)");
        }
        ("FillForegnd", Some(v)) => {
            let mut ops = vec![op(Ns::Draw, "fill-color", Val::Text(colour(v)?))];
            ops.extend(fill_once(cell, edit));
            ops
        }
        ("FillForegnd", None) => vec![op(Ns::Draw, "fill-color", Val::Remove)],
        ("FillForegndTrans", Some(v)) => {
            vec![op(Ns::Draw, "opacity", Val::Text(complement_percent(v)?)), op(Ns::Draw, "opacity-name", Val::Remove)]
        }
        ("FillForegndTrans", None) => vec![op(Ns::Draw, "opacity", Val::Remove), op(Ns::Draw, "opacity-name", Val::Remove)],
        // The reader sets it with `FillForegndTrans`, and no attribute is its own.
        ("FillBkgndTrans", _) => {
            let with = edit.changes.iter().any(|(name, v)| *name == "FillForegndTrans" && v.as_ref().map(|v| v.number) == new.map(|v| v.number));
            if !with {
                return Err("a change of FillBkgndTrans that FillForegndTrans does not share (no attribute is its own)");
            }
            Vec::new()
        }
        // The shadow: visible or hidden, its colour, its opacity (the complement of the cell's share), and its offset on the page's axes, y running down in the file and up in the
        // cell. **A type of 1 is what a visible shadow reads back as** (`shadow_visible`), so it is written by `ShdwPattern` alone; any other type, and the blur and the size of the
        // shadow, which ODF has no attribute for, are carried.
        ("ShdwPattern", Some(v)) => match v.number {
            Some(0.0) => vec![op(Ns::Draw, "shadow", Val::Text("hidden".into()))],
            Some(1.0) => vec![op(Ns::Draw, "shadow", Val::Text("visible".into()))],
            _ => return Err("a shadow pattern that is not 0 or 1"),
        },
        ("ShdwPattern", None) => vec![op(Ns::Draw, "shadow", Val::Remove)],
        ("ShapeShdwType", Some(v)) if v.number == Some(1.0) && edit.shadow_visible == Some(true) => Vec::new(),
        ("ShapeShdwType", _) => {
            return Err("a shadow type other than 1, or a type beside no visible shadow (the file says it only with a visible shadow)");
        }
        ("ShdwForegnd", Some(v)) => vec![op(Ns::Draw, "shadow-color", Val::Text(colour(v)?))],
        ("ShdwForegnd", None) => vec![op(Ns::Draw, "shadow-color", Val::Remove)],
        ("ShdwForegndTrans", Some(v)) => vec![op(Ns::Draw, "shadow-opacity", Val::Text(complement_percent(v)?))],
        ("ShdwForegndTrans", None) => vec![op(Ns::Draw, "shadow-opacity", Val::Remove)],
        ("ShapeShdwOffsetX", Some(v)) => vec![op(Ns::Draw, "shadow-offset-x", length(v)?)],
        ("ShapeShdwOffsetX", None) => vec![op(Ns::Draw, "shadow-offset-x", Val::Remove)],
        ("ShapeShdwOffsetY", Some(v)) => {
            let inches = v.number.filter(|n| n.is_finite()).ok_or("a length cell that is not a number")?;
            // ODF's y runs down the page and the cell's up; zero stays zero, never minus zero.
            vec![op(Ns::Draw, "shadow-offset-y", Val::Length(if inches == 0.0 { 0.0 } else { -inches }))]
        }
        ("ShapeShdwOffsetY", None) => vec![op(Ns::Draw, "shadow-offset-y", Val::Remove)],
        ("LinePattern", Some(v)) => match v.number {
            Some(0.0) => vec![op(Ns::Draw, "stroke", Val::Text("none".into()))],
            Some(1.0) => vec![op(Ns::Draw, "stroke", Val::Text("solid".into()))],
            _ => return Err("a dashed line (a dash needs its own draw:stroke-dash in office:styles)"),
        },
        ("LinePattern", None) => vec![op(Ns::Draw, "stroke", Val::Remove)],
        ("LineWeight", Some(v)) => vec![op(Ns::Svg, "stroke-width", length(v)?)],
        ("LineWeight", None) => vec![op(Ns::Svg, "stroke-width", Val::Remove)],
        ("LineColor", Some(v)) => vec![op(Ns::Svg, "stroke-color", Val::Text(colour(v)?))],
        ("LineColor", None) => vec![op(Ns::Svg, "stroke-color", Val::Remove)],
        ("LineColorTrans", Some(v)) => vec![op(Ns::Svg, "stroke-opacity", Val::Text(complement_percent(v)?))],
        ("LineColorTrans", None) => vec![op(Ns::Svg, "stroke-opacity", Val::Remove)],
        ("LineCap", Some(v)) => {
            let (cap, join) = match v.number {
                Some(0.0) => ("round", "round"),
                Some(1.0) => ("butt", "miter"),
                Some(2.0) => ("square", "miter"),
                _ => return Err("a line cap that is not 0, 1 or 2"),
            };
            vec![op(Ns::Svg, "stroke-linecap", Val::Text(cap.into())), op(Ns::Draw, "stroke-linejoin", Val::Text(join.into()))]
        }
        ("LineCap", None) => vec![op(Ns::Svg, "stroke-linecap", Val::Remove), op(Ns::Draw, "stroke-linejoin", Val::Remove)],
        ("VerticalAlign", Some(v)) => {
            let word = match v.number {
                Some(0.0) => "top",
                Some(1.0) => "middle",
                Some(2.0) => "bottom",
                _ => return Err("a vertical alignment that is not 0, 1 or 2"),
            };
            vec![op(Ns::Draw, "textarea-vertical-align", Val::Text(word.into()))]
        }
        ("VerticalAlign", None) => vec![op(Ns::Draw, "textarea-vertical-align", Val::Remove)],
        ("TopMargin" | "BottomMargin" | "LeftMargin" | "RightMargin", _) => {
            // The four paddings are written together, and `fo:padding` they replace is taken out (the row).
            let mut ops = Vec::new();
            for (name, local) in MARGINS {
                let held = edit.margins.iter().find(|(n, _)| *n == name).and_then(|(_, v)| v.as_ref());
                let val = match held {
                    Some(v) => length(v)?,
                    None => Val::Remove,
                };
                ops.push(op(Ns::Fo, local, val));
            }
            ops.push(op(Ns::Fo, "padding", Val::Remove));
            // Written once, by the first of the four cells that changed.
            let first = edit.changes.iter().map(|(n, _)| *n).find(|n| MARGINS.iter().any(|(m, _)| m == n));
            if first != Some(cell) {
                return Ok(Vec::new());
            }
            ops
        }
        ("BeginArrow" | "BeginArrowSize" | "EndArrow" | "EndArrowSize", _) => line_end_ops(cell, new, edit)?,
        _ => return Err("a style cell the writer does not write to an automatic style"),
    })
}

/// The attributes of a line end: `draw:marker-start` or `-end` naming DiaCraft's own marker for the number, its width and whether it is
/// centred on the vertex; written once for the end, by the first of its two cells that changed. A number of 0 writes the empty name (the style then draws no
/// end, whatever its parent's does) and takes the width and the centre out; a cell that is gone takes all three out. The marker's `draw:marker` element is
/// the save's to add to `office:styles` (`markers.rs`, and the editor notes which).
fn line_end_ops(cell: &'static str, new: Option<&CellVal>, edit: &StyleEdit) -> Result<Vec<Op>, &'static str> {
    let (arrow_cell, size_cell, end) =
        if cell.starts_with("Begin") { ("BeginArrow", "BeginArrowSize", "start") } else { ("EndArrow", "EndArrowSize", "end") };
    let first = edit.changes.iter().map(|(n, _)| *n).find(|n| *n == arrow_cell || *n == size_cell);
    if first != Some(cell) {
        return Ok(Vec::new());
    }
    let _ = new;
    let (local_marker, local_width, local_centre): (&'static str, &'static str, &'static str) = if end == "start" {
        ("marker-start", "marker-start-width", "marker-start-center")
    } else {
        ("marker-end", "marker-end-width", "marker-end-center")
    };
    let removed = || vec![op(Ns::Draw, local_marker, Val::Remove), op(Ns::Draw, local_width, Val::Remove), op(Ns::Draw, local_centre, Val::Remove)];
    let changed_arrow = edit.changes.iter().find(|(n, _)| *n == arrow_cell).map(|(_, v)| v);
    if matches!(changed_arrow, Some(None)) {
        return Ok(removed());
    }
    let line = |name: &str| edit.line.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
    let arrow = changed_arrow.and_then(|v| v.as_ref()).and_then(|v| v.number).or_else(|| line(arrow_cell));
    let Some(arrow) = arrow else { return Ok(Vec::new()) };
    if !(arrow.is_finite() && arrow >= 0.0 && arrow.fract() == 0.0 && arrow <= 1000.0) {
        return Err("a line end number that is not 0 to 45 (nor 254, the custom end)");
    }
    let number = arrow as u32;
    if number == 0 {
        // Only an arrow that was set to 0 says so; a size beside no end says nothing.
        return Ok(if changed_arrow.is_some() {
            vec![
                op(Ns::Draw, local_marker, Val::Text(String::new())),
                op(Ns::Draw, local_width, Val::Remove),
                op(Ns::Draw, local_centre, Val::Remove),
            ]
        } else {
            Vec::new()
        });
    }
    let class = super::markers::class_of_cell(
        line(size_cell).or_else(|| edit.changes.iter().find(|(n, _)| *n == size_cell).and_then(|(_, v)| v.as_ref()?.number)),
    )?;
    // The line's weight, in points: 0.75 pt where nothing says (the root style's), and a refusal in words past what a marker is written for.
    let weight_pt = super::markers::weight_pt(line("LineWeight"))?;
    // The custom end (254) is drawn as number 4's marker and the filled diamond as its own; the cell is carried by the extension, which the caller sees to.
    let drawn = if edit.diamonds.contains(&arrow_cell) { super::markers::DIAMOND } else { super::markers::drawn_as(number) };
    let plan = super::markers::plan(drawn, class, weight_pt)?;
    Ok(vec![
        op(Ns::Draw, local_marker, Val::Text(plan.key.name())),
        op(Ns::Draw, local_width, Val::Text(super::markers::points_text(plan.width))),
        op(Ns::Draw, local_centre, if super::marker_art::is_centred(drawn) { Val::Text("true".to_owned()) } else { Val::Remove }),
    ])
}

#[cfg(test)]
mod tests {
    use diacraft_doc::Value;

    use super::*;

    fn val(text: &str) -> CellVal {
        CellVal { text: text.to_owned(), number: text.parse().ok(), unit: None }
    }

    #[test]
    fn every_style_cell_is_its_attributes_when_it_is_set_and_when_it_is_gone() {
        use Ns::{Draw, Svg};
        let edit = StyleEdit::default();
        let text = |s: &str| Val::Text(s.into());
        let got = |cell: &'static str, v: Option<&str>| {
            let value = v.map(val);
            ops_for(cell, value.as_ref(), &edit).map(|ops| ops.into_iter().map(|o| (o.ns, o.local, o.val)).collect::<Vec<_>>())
        };
        assert_eq!(got("LinePattern", Some("0")).unwrap(), [(Draw, "stroke", text("none"))]);
        assert_eq!(got("LinePattern", Some("1")).unwrap(), [(Draw, "stroke", text("solid"))]);
        assert!(got("LinePattern", Some("2")).is_err());
        assert_eq!(got("LinePattern", None).unwrap(), [(Draw, "stroke", Val::Remove)]);
        assert_eq!(got("LineWeight", Some("0.01")).unwrap(), [(Svg, "stroke-width", Val::Length(0.01))]);
        assert_eq!(got("LineWeight", None).unwrap(), [(Svg, "stroke-width", Val::Remove)]);
        assert_eq!(got("LineColor", Some("#FF0000")).unwrap(), [(Svg, "stroke-color", text("#ff0000"))]);
        assert_eq!(got("LineColor", None).unwrap(), [(Svg, "stroke-color", Val::Remove)]);
        assert_eq!(got("LineColorTrans", Some("0.25")).unwrap(), [(Svg, "stroke-opacity", text("75%"))]);
        assert_eq!(got("LineColorTrans", None).unwrap(), [(Svg, "stroke-opacity", Val::Remove)]);
        for (cap, (word, join)) in [("0", ("round", "round")), ("1", ("butt", "miter")), ("2", ("square", "miter"))] {
            assert_eq!(got("LineCap", Some(cap)).unwrap(), [(Svg, "stroke-linecap", text(word)), (Draw, "stroke-linejoin", text(join))]);
        }
        assert!(got("LineCap", Some("3")).is_err());
        assert_eq!(got("LineCap", None).unwrap(), [(Svg, "stroke-linecap", Val::Remove), (Draw, "stroke-linejoin", Val::Remove)]);
        for (align, word) in [("0", "top"), ("1", "middle"), ("2", "bottom")] {
            assert_eq!(got("VerticalAlign", Some(align)).unwrap(), [(Draw, "textarea-vertical-align", text(word))]);
        }
        assert!(got("VerticalAlign", Some("3")).is_err());
        assert_eq!(got("VerticalAlign", None).unwrap(), [(Draw, "textarea-vertical-align", Val::Remove)]);
        assert_eq!(got("FillForegndTrans", Some("0.25")).unwrap(), [(Draw, "opacity", text("75%")), (Draw, "opacity-name", Val::Remove)]);
        assert_eq!(got("FillForegndTrans", None).unwrap(), [(Draw, "opacity", Val::Remove), (Draw, "opacity-name", Val::Remove)]);
        assert_eq!(got("FillForegnd", Some("#336699")).unwrap(), [(Draw, "fill-color", text("#336699"))]);
        // `FillBkgndTrans` has no attribute: it passes only beside a `FillForegndTrans` change to the same value.
        let beside = |name: &'static str, v: Option<&str>| StyleEdit {
            changes: vec![(name, v.map(val))],
            margins: Vec::new(),
            fill_kind: None,
            shadow_visible: None,
            fx: None,
            fx_off: None,
            line: Vec::new(),
            diamonds: Vec::new(),
            parent: None,
        };
        let bkgnd = |v: Option<&str>, edit: &StyleEdit| ops_for("FillBkgndTrans", v.map(val).as_ref(), edit);
        assert!(bkgnd(Some("0.5"), &beside("FillForegndTrans", Some("0.5"))).unwrap().is_empty());
        assert!(bkgnd(Some("0.25"), &beside("FillForegndTrans", Some("0.5"))).is_err(), "another value");
        assert!(bkgnd(Some("0.5"), &beside("LineWeight", Some("0.5"))).is_err(), "another cell of the same value");
        assert!(bkgnd(Some("0.5"), &StyleEdit::default()).is_err(), "no change beside it");
        assert!(bkgnd(None, &beside("FillForegndTrans", None)).unwrap().is_empty(), "gone with it");
        assert!(bkgnd(None, &beside("FillForegndTrans", Some("0.5"))).is_err(), "gone where it is not");
    }

    #[test]
    fn a_colour_is_seven_characters_with_a_hash_and_hex_digits_and_a_gone_fill_takes_its_attribute_out() {
        assert_eq!(colour(&val("#ABCdef")).as_deref(), Ok("#abcdef"));
        for bad in ["1234567", "#12345", "#12345g", "#1234567", "##12345", "", "#"] {
            assert!(colour(&val(bad)).is_err(), "{bad}");
        }
        let edit = StyleEdit::default();
        let removed = |cell| ops_for(cell, None, &edit).map(|ops| ops.into_iter().map(|o| (o.local, o.val)).collect::<Vec<_>>());
        assert_eq!(removed("FillPattern").unwrap(), [("fill", Val::Remove)]);
        assert_eq!(removed("FillForegnd").unwrap(), [("fill-color", Val::Remove)]);
        // A gradient that is on is refused in the words of a gradient, whether the cell is there or gone; one that is off writes nothing.
        assert!(removed("FillGradientEnabled").unwrap_err().contains("gradient fill"));
        assert!(ops_for("FillGradientEnabled", Some(&val("1")), &edit).unwrap_err().contains("gradient fill"));
        assert!(ops_for("FillGradientEnabled", Some(&val("0")), &edit).unwrap().is_empty());
    }

    #[test]
    fn each_page_cell_is_its_own_attribute_of_the_layout_and_an_orientation_is_one_of_two_words() {
        for (cell, ns, local) in [
            ("PageWidth", Ns::Fo, "page-width"),
            ("PageHeight", Ns::Fo, "page-height"),
            ("PageLeftMargin", Ns::Fo, "margin-left"),
            ("PageRightMargin", Ns::Fo, "margin-right"),
            ("PageTopMargin", Ns::Fo, "margin-top"),
            ("PageBottomMargin", Ns::Fo, "margin-bottom"),
            ("PrintPageOrientation", Ns::Style, "print-orientation"),
        ] {
            let value = if cell == "PrintPageOrientation" { val("1") } else { val("2") };
            let ops = page_ops(&[(cell, Some(value))]).unwrap();
            assert_eq!(ops.iter().map(|o| (o.ns, o.local, o.prop)).collect::<Vec<_>>(), [(ns, local, 3)], "{cell}");
        }
        let orientation = |v: &str| page_ops(&[("PrintPageOrientation", Some(val(v)))]).map(|ops| ops[0].val.clone());
        assert_eq!(orientation("1"), Ok(Val::Text("portrait".into())));
        assert_eq!(orientation("2"), Ok(Val::Text("landscape".into())));
        assert!(orientation("3").is_err() && orientation("0").is_err());
        // A margin that is gone is removed, and a page with no size is refused.
        assert_eq!(page_ops(&[("PageTopMargin", None)]).unwrap()[0].val, Val::Remove);
        assert!(page_ops(&[("PageWidth", None)]).is_err() && page_ops(&[("PageHeight", None)]).is_err());
    }

    #[test]
    fn a_cell_becomes_the_attribute_odg_styles_gives_it_and_what_has_no_attribute_is_refused_in_words() {
        let edit = StyleEdit::default();
        let one = |cell, v: &str| ops_for(cell, Some(&val(v)), &edit).map(|ops| ops.into_iter().map(|o| (o.local, o.val)).collect::<Vec<_>>());
        // The `draw:fill` row: the gradient's and the hatch's names, and every attribute of a bitmap (`draw:fill-image-*`, and the repeat mode only a picture uses).
        let names = [
            "fill-gradient-name",
            "fill-hatch-name",
            "fill-hatch-solid",
            "fill-image-name",
            "fill-image-width",
            "fill-image-height",
            "fill-image-ref-point",
            "fill-image-ref-point-x",
            "fill-image-ref-point-y",
            "repeat",
        ];
        let solid = one("FillPattern", "1").unwrap();
        assert_eq!(solid[0], ("fill", Val::Text("solid".into())));
        assert_eq!(
            solid[1..].iter().map(|(l, v)| (*l, v.clone())).collect::<Vec<_>>(),
            names.map(|n| (n, Val::Remove)),
            "the names a fill of this kind does not use go"
        );
        assert_eq!(one("FillPattern", "0").unwrap()[0], ("fill", Val::Text("none".into())));
        assert!(one("FillPattern", "5").is_err(), "a hatch pattern is tier 2");
        assert_eq!(one("FillForegnd", "#336699"), Ok(vec![("fill-color", Val::Text("#336699".into()))]));
        assert_eq!(one("FillForegnd", "#AA0000"), Ok(vec![("fill-color", Val::Text("#aa0000".into()))]), "ODF writes lower case");
        assert!(one("FillForegnd", "3").is_err(), "a palette index");
        assert_eq!(one("FillForegndTrans", "0.25").unwrap(), vec![("opacity", Val::Text("75%".into())), ("opacity-name", Val::Remove)], "V6");
        assert_eq!(one("FillForegndTrans", "0.4").unwrap()[0], ("opacity", Val::Text("60%".into())), "no 60.00000000000001");
        assert_eq!(one("LineColorTrans", "0.25"), Ok(vec![("stroke-opacity", Val::Text("75%".into()))]));
        assert_eq!(one("LinePattern", "0"), Ok(vec![("stroke", Val::Text("none".into()))]));
        assert!(one("LinePattern", "7").is_err(), "a dash needs a dash style");
        assert_eq!(one("LineWeight", "0.0118"), Ok(vec![("stroke-width", Val::Length(0.0118))]));
        assert_eq!(
            one("LineCap", "2"),
            Ok(vec![("stroke-linecap", Val::Text("square".into())), ("stroke-linejoin", Val::Text("miter".into()))]),
            "V7"
        );
        assert_eq!(one("LineCap", "0").map(|o| o.len()), Ok(2));
        assert_eq!(one("VerticalAlign", "2"), Ok(vec![("textarea-vertical-align", Val::Text("bottom".into()))]));
        assert!(one("Comment", "x").is_err(), "a cell no row of the table names");
        assert_eq!(ops_for("FillForegnd", None, &edit).map(|o| o.len()), Ok(1), "a cell that is gone takes its attribute out");
        assert!(ops_for("FillForegndTrans", None, &edit).is_ok_and(|o| o.iter().all(|op| op.val == Val::Remove)));
    }

    /// The shadow: visible or hidden, its colour, opacity and offset (y down in the file, up in the cell); a type of 1 only beside a visible shadow, any other is carried.
    #[test]
    fn the_cells_of_a_shadow_are_its_attributes() {
        let val = |text: &str| CellVal { text: text.to_owned(), number: text.parse().ok(), unit: None };
        let edit = StyleEdit::default();
        let one = |cell: &'static str, v: &str| {
            ops_for(cell, Some(&val(v)), &edit).map(|ops| ops.into_iter().map(|o| (o.local, o.val)).collect::<Vec<_>>())
        };
        assert_eq!(one("ShdwPattern", "1"), Ok(vec![("shadow", Val::Text("visible".into()))]));
        assert_eq!(one("ShdwPattern", "0"), Ok(vec![("shadow", Val::Text("hidden".into()))]));
        assert!(one("ShdwPattern", "2").is_err());
        assert_eq!(one("ShdwForegnd", "#AA0000"), Ok(vec![("shadow-color", Val::Text("#aa0000".into()))]));
        assert_eq!(one("ShdwForegndTrans", "0.45"), Ok(vec![("shadow-opacity", Val::Text("55%".into()))]));
        assert_eq!(one("ShapeShdwOffsetX", "0.1"), Ok(vec![("shadow-offset-x", Val::Length(0.1))]));
        assert_eq!(one("ShapeShdwOffsetY", "0.1"), Ok(vec![("shadow-offset-y", Val::Length(-0.1))]));
        assert_eq!(one("ShapeShdwOffsetY", "0"), Ok(vec![("shadow-offset-y", Val::Length(0.0))]), "never minus zero");
        assert!(one("ShapeShdwType", "1").is_err(), "no visible shadow beside it");
        let visible = StyleEdit { shadow_visible: Some(true), ..StyleEdit::default() };
        assert_eq!(ops_for("ShapeShdwType", Some(&val("1")), &visible), Ok(Vec::new()), "the visible shadow says it");
        assert!(ops_for("ShapeShdwType", Some(&val("2")), &visible).is_err(), "another type is carried");
        assert!(ops_for("ShapeShdwType", None, &visible).is_err(), "a type that is gone is carried as gone");
    }

    #[test]
    fn the_four_margins_are_written_together_once_and_take_fo_padding_out() {
        let margins =
            |t: &str| MARGINS.iter().map(|(name, _)| (*name, Some(val(t)))).chain(std::iter::once(("RightMargin", Some(val(t))))).collect::<Vec<_>>();
        let edit = StyleEdit {
            changes: vec![("TopMargin", Some(val("0.1"))), ("LeftMargin", Some(val("0.1")))],
            margins: margins("0.1"),
            fill_kind: None,
            shadow_visible: None,
            fx: None,
            fx_off: None,
            line: Vec::new(),
            diamonds: Vec::new(),
            parent: None,
        };
        let top = ops_for("TopMargin", edit.changes[0].1.as_ref(), &edit).unwrap();
        assert_eq!(top.iter().filter(|o| o.local.starts_with("padding-")).count(), 4);
        assert!(top.iter().any(|o| o.local == "padding" && o.val == Val::Remove), "`fo:padding` goes");
        assert!(ops_for("LeftMargin", edit.changes[1].1.as_ref(), &edit).unwrap().is_empty(), "the second cell adds nothing: it is written once");
    }

    #[test]
    fn a_line_end_cell_that_says_254_is_the_custom_end_with_a_formula_or_without_and_no_other_cell_is() {
        let custom = |name: &str, formula: Option<&str>, value: &str| {
            let cell = Cell::read(formula, Some(value), None, None, &mut 100);
            (is_custom_end(name, &cell), CellVal::of(&cell))
        };
        for name in ["BeginArrow", "EndArrow"] {
            // 254 by a formula or plain, on either end's cell: the custom end (a new shape writes number 4's marker for it and carries the cell as tier 2).
            for formula in [Some(r#"USE("Fancy end")"#), None] {
                let (is, _) = custom(name, formula, "254");
                assert!(is, "{name} {formula:?}");
            }
            // Any other value with a formula is a formula like any other, and any other number is a number.
            let (is, named) = custom(name, Some("THEMEVAL()"), "4");
            assert!(!is && named.unwrap_err().contains("holds a formula"), "{name}");
            assert_eq!(custom(name, None, "4").1.map(|v| v.number), Ok(Some(4.0)), "{name}");
            assert!(!custom(name, None, "253").0 && !custom(name, None, "255").0, "{name}: only 254");
        }
        // The size cell and any other cell that says 254 are no custom end: a formula is a formula like any other, a number is a number.
        for name in ["EndArrowSize", "LineWeight", "FillPattern"] {
            let (is, named) = custom(name, Some("THEMEVAL()"), "254");
            assert!(!is && named.unwrap_err().contains("holds a formula"), "{name}");
            assert_eq!(custom(name, None, "254").1.map(|v| v.number), Ok(Some(254.0)), "{name}");
        }
        // What the custom end is written as is number 4.
        assert_eq!(CellVal::stand_in().number, Some(4.0));
        assert_eq!(CellVal::stand_in().text, "4");
    }

    #[test]
    fn a_cell_with_a_formula_is_refused_and_a_transparency_beyond_its_range_too() {
        let with_formula = Cell::read(Some("THEMEVAL()"), Some("#336699"), None, None, &mut 100);
        assert!(CellVal::of(&with_formula).is_err());
        let plain = Cell::constant(Value::from_text("0.5"));
        assert_eq!(CellVal::of(&plain).map(|v| v.number), Ok(Some(0.5)));
        assert!(ops_for("FillForegndTrans", Some(&val("1.5")), &StyleEdit::default()).is_err());
        assert!(ops_for("FillForegndTrans", Some(&val("x")), &StyleEdit::default()).is_err());
    }

    #[test]
    fn a_colour_set_or_a_gradient_turned_off_states_the_fill_the_edit_decided_once() {
        use Ns::Draw;
        let text = |s: &str| Val::Text(s.into());
        let edit = |names: &[&'static str], kind: Option<&'static str>| StyleEdit {
            changes: names.iter().map(|n| (*n, Some(val(if *n == "FillForegnd" { "#123456" } else { "0" })))).collect(),
            margins: Vec::new(),
            fill_kind: kind,
            shadow_visible: None,
            fx: None,
            fx_off: None,
            line: Vec::new(),
            diamonds: Vec::new(),
            parent: None,
        };
        let ops =
            |cell: &'static str, e: &StyleEdit| ops_for(cell, e.changes.iter().find(|(n, _)| *n == cell).and_then(|(_, v)| v.as_ref()), e).unwrap();
        let fill = |kind: &str| {
            let mut ops = vec![
                (Draw, "fill", text(kind)),
                (Draw, "fill-gradient-name", Val::Remove),
                (Draw, "fill-hatch-name", Val::Remove),
                (Draw, "fill-hatch-solid", Val::Remove),
            ];
            ops.extend(FILL_IMAGE.map(|(ns, local)| (ns, local, Val::Remove)));
            ops
        };
        let shown = |ops: Vec<Op>| ops.into_iter().map(|o| (o.ns, o.local, o.val)).collect::<Vec<_>>();
        // A colour alone: the colour, and the fill the edit decided; without a decision, the colour alone as before.
        let solid = edit(&["FillForegnd"], Some("solid"));
        assert_eq!(shown(ops("FillForegnd", &solid)), [vec![(Draw, "fill-color", text("#123456"))], fill("solid")].concat());
        assert_eq!(shown(ops("FillForegnd", &edit(&["FillForegnd"], None))), [(Draw, "fill-color", text("#123456"))]);
        // A gradient turned off: the fill, with no colour.
        assert_eq!(shown(ops("FillGradientEnabled", &edit(&["FillGradientEnabled"], Some("solid")))), fill("solid"));
        assert!(ops("FillGradientEnabled", &edit(&["FillGradientEnabled"], None)).is_empty());
        // Both in one edit: the first of the two writes the fill, and the other does not write it again.
        let both = edit(&["FillForegnd", "FillGradientEnabled"], Some("solid"));
        assert_eq!(shown(ops("FillForegnd", &both)).len(), 1 + 1 + 3 + FILL_IMAGE.len());
        assert!(ops("FillGradientEnabled", &both).is_empty());
        let other_way = StyleEdit { changes: both.changes.iter().rev().cloned().collect(), ..both.clone() };
        assert_eq!(shown(ops("FillGradientEnabled", &other_way)).len(), 1 + 3 + FILL_IMAGE.len());
        assert_eq!(shown(ops("FillForegnd", &other_way)).len(), 1, "the colour alone, when the gradient is first");
    }

    #[test]
    fn a_line_end_number_or_size_class_that_is_not_a_whole_number_in_range_is_refused_in_words() {
        let run = |arrow: &str, class: f64| {
            let edit = StyleEdit {
                changes: vec![("EndArrow", Some(val(arrow)))],
                margins: Vec::new(),
                fill_kind: None,
                shadow_visible: None,
                fx: None,
                fx_off: None,
                line: vec![("EndArrowSize", class)],
                diamonds: Vec::new(),
                parent: None,
            };
            ops_for("EndArrow", Some(&val(arrow)), &edit)
        };
        for class in [0.0, 1.0, 2.0, 5.0, 6.0] {
            assert!(run("4", class).is_ok(), "{class}");
        }
        for class in [1.5, -1.0, 7.0, 100.0, 101.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let why = run("4", class).unwrap_err();
            assert!(why.contains("size"), "{class}: {why}");
        }
        for arrow in ["4.5", "-1", "46", "253", "255", "1001"] {
            let why = run(arrow, 2.0).unwrap_err();
            assert!(why.contains("0 to 45"), "{arrow}: {why}");
        }
        // 254 is the custom end: written as number 4's marker at the same class and weight, the cell being the caller's to carry.
        let custom = run("254", 2.0).expect("the custom end is drawn as number 4");
        assert_eq!(custom, run("4", 2.0).unwrap(), "the same marker, width and centre as number 4's");
        assert!(custom.iter().any(|o| o.local == "marker-end" && o.val == Val::Text("DiaCraft_20_end_20_4".to_owned())), "{custom:?}");
        assert!(custom.iter().any(|o| o.local == "marker-end-width" && o.val == Val::Text("6.54pt".to_owned())), "{custom:?}");
        // A size past 6 beside it is still refused, as for any number.
        assert!(run("254", 7.0).unwrap_err().contains("size"));
        // A number that is there beside a size that is gone: class 2; and a size beside no end writes nothing.
        let no_end = StyleEdit {
            changes: vec![("EndArrowSize", Some(val("3")))],
            margins: Vec::new(),
            fill_kind: None,
            shadow_visible: None,
            fx: None,
            fx_off: None,
            line: Vec::new(),
            diamonds: Vec::new(),
            parent: None,
        };
        assert!(ops_for("EndArrowSize", Some(&val("3")), &no_end).unwrap().is_empty());
        // The edge of the numbers: 45 is a number with art of its own, 0 is no end.
        assert!(run("45", 2.0).is_ok());
    }
}
