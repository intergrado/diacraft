//! The properties `THEMEVAL` answers from a theme part, by name and by the cell that holds the call.
//!
//! Names match without regard to case, and both sets of D4 are taken: Learn's (`LineBegin`, `LineColorTrans`, `ShadowType`) and MS-VSDX's
//! (`ConnectorBegin`, `LineTransparency`, `ShadowStyle`). A `Connector...` and a `Line...` name read the same property: which list answers,
//! the connector scheme's or the effect scheme's, is the shape's kind, not the name's.

use diacraft_doc::{CellAddr, RowKey};

/// What a stop cell of a theme gradient is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StopWhat {
    Colour,
    Transparency,
    Position,
}

/// A property, in the sense the evaluator answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum P {
    /// `Dark`, `Light`, `AccentColor` to `AccentColor6`, `BackgroundColor`: 0 to 8 (MS-VSDX's numbering).
    Slot(u8),
    /// `VariantColor1` to `VariantColor7`: 0 to 6.
    VariantColour(u8),
    TextColour,
    FillColour,
    FillColour2,
    LineColour,
    ShadowColour,
    /// `FillStop<n>Color` and its kin: `n` counts from 0 here, from 1 in the name.
    Stop {
        line: bool,
        n: u8,
        what: StopWhat,
    },
    LineWeight,
    LineCap,
    LineCompound,
    LinePattern,
    LineRounding,
    LineBegin,
    LineBeginSize,
    LineEnd,
    LineEndSize,
    LineTrans,
    FillPattern,
    FillTrans,
    FillTransBkgnd,
    FillGradEnabled,
    FillGradDir,
    FillGradAngle,
    RotateGradient,
    LineGradEnabled,
    LineGradDir,
    LineGradAngle,
    ShadowPattern,
    ShadowStyle,
    ShadowTrans,
    ShadowX,
    ShadowY,
    ShadowMag,
    ShadowDir,
    ShadowBlur,
    LatinFont,
    TextStyle,
    /// A name of the table that this evaluator does not compute from a theme (bevel, glow, reflection, soft edges, sketch, the other fonts).
    Other,
}

/// Names with no number in them, and what they are. **The names that are not here give `#VALUE!`**, and some of them are in
/// MS-VSDX's table or in Learn's: `ShadowStyle` and `LineTransparency` (MS-VSDX lists them and every one of 386 calls in files holds `#VALUE!`),
/// `AccentColor1`, `FillGradStop1Position` to `FillGradStop3Position`, and a stop past the tenth.
const NAMES: &[(&str, P)] = &[
    ("Dark", P::Slot(0)),
    ("Light", P::Slot(1)),
    ("AccentColor", P::Slot(2)),
    ("AccentColor2", P::Slot(3)),
    ("AccentColor3", P::Slot(4)),
    ("AccentColor4", P::Slot(5)),
    ("AccentColor5", P::Slot(6)),
    ("AccentColor6", P::Slot(7)),
    ("BackgroundColor", P::Slot(8)),
    ("TextColor", P::TextColour),
    ("FillColor", P::FillColour),
    ("FillColor2", P::FillColour2),
    ("LineColor", P::LineColour),
    ("ConnectorColor", P::LineColour),
    ("ShadowColor", P::ShadowColour),
    ("LineWeight", P::LineWeight),
    ("ConnectorWeight", P::LineWeight),
    ("LineCap", P::LineCap),
    ("LineCompoundType", P::LineCompound),
    ("LinePattern", P::LinePattern),
    ("ConnectorPattern", P::LinePattern),
    ("LineRounding", P::LineRounding),
    ("ConnectorRounding", P::LineRounding),
    ("LineBegin", P::LineBegin),
    ("ConnectorBegin", P::LineBegin),
    ("LineBeginSize", P::LineBeginSize),
    ("ConnectorBeginSize", P::LineBeginSize),
    ("LineEnd", P::LineEnd),
    ("ConnectorEnd", P::LineEnd),
    // `ConnectorEnd2` is MS-VSDX's name with no meaning given. With no theme every sample reads it as 4 (`fn_color.rs`); under a theme our rule
    // reads it as `ConnectorEnd`.
    ("ConnectorEnd2", P::LineEnd),
    ("LineEndSize", P::LineEndSize),
    ("ConnectorEndSize", P::LineEndSize),
    ("ConnectorTransparency", P::LineTrans),
    ("LineColorTrans", P::LineTrans),
    ("FillPattern", P::FillPattern),
    ("FillTransparency", P::FillTrans),
    ("FillTransparencyBkgnd", P::FillTransBkgnd),
    ("FillGradientEnabled", P::FillGradEnabled),
    ("FillGradientDir", P::FillGradDir),
    ("FillGradientAngle", P::FillGradAngle),
    ("RotateGradientWithShape", P::RotateGradient),
    ("LineGradientEnabled", P::LineGradEnabled),
    ("LineGradientDir", P::LineGradDir),
    ("LineGradientAngle", P::LineGradAngle),
    ("ShadowPattern", P::ShadowPattern),
    ("ShadowType", P::ShadowStyle),
    ("ShadowTransparency", P::ShadowTrans),
    ("ShadowXOffset", P::ShadowX),
    ("ShadowYOffset", P::ShadowY),
    ("ShadowMagnification", P::ShadowMag),
    ("ShadowDirection", P::ShadowDir),
    ("ShadowBlur", P::ShadowBlur),
    ("LatinFont", P::LatinFont),
    ("TextStyle", P::TextStyle),
    // In the table, and not worked out from a theme here.
    ("AsianFont", P::Other),
    ("ComplexFont", P::Other),
    ("UseGroupGradient", P::Other),
    ("BevelTopType", P::Other),
    ("BevelTopWidth", P::Other),
    ("BevelTopHeight", P::Other),
    ("BevelContourColor", P::Other),
    ("BevelContourSize", P::Other),
    ("BevelLightingAngle", P::Other),
    ("BevelMaterial", P::Other),
    ("BevelLighting", P::Other),
    ("BevelLightingType", P::Other),
    ("GlowSize", P::Other),
    ("GlowColor", P::Other),
    ("GlowColorTrans", P::Other),
    ("GlowTransparency", P::Other),
    ("ReflectionBlur", P::Other),
    ("ReflectionDist", P::Other),
    ("ReflectionSize", P::Other),
    ("ReflectionTrans", P::Other),
    ("SoftEdgesSize", P::Other),
    ("SketchAmount", P::Other),
    ("SketchEnabled", P::Other),
    ("SketchFillChange", P::Other),
    ("SketchLineChange", P::Other),
    ("SketchLineWeight", P::Other),
];

/// The property a name is, or `None` for a name that is not in the table (`#VALUE!`).
pub(crate) fn canonical(name: &str) -> Option<P> {
    if let Some((_, p)) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*p);
    }
    // `LineCompoundtype`, Learn's spelling, is `LineCompoundType` read without regard to case, so the table above holds it.
    if let Some(n) = number_in(name, "VariantColor", "") {
        return (1..=7).contains(&n).then(|| P::VariantColour((n - 1) as u8));
    }
    for (line, prefix) in [(false, "FillStop"), (true, "LineStop")] {
        for (suffix, what) in [("Color", StopWhat::Colour), ("Transparency", StopWhat::Transparency), ("Position", StopWhat::Position)] {
            if let Some(n) = number_in(name, prefix, suffix) {
                // Stops 1 to 10 are in the table; `LineStop11Color` and the rest are in files, in no table, and give `#VALUE!`.
                return (1..=10).contains(&n).then(|| P::Stop { line, n: (n - 1) as u8, what });
            }
        }
    }
    None
}

/// The whole number between `prefix` and `suffix` in `name`, ignoring case: the digits and nothing else.
fn number_in(name: &str, prefix: &str, suffix: &str) -> Option<u32> {
    let head = name.get(..prefix.len()).filter(|h| h.eq_ignore_ascii_case(prefix))?;
    let rest = name.get(head.len()..)?;
    let digits = rest.get(..rest.len().checked_sub(suffix.len())?)?;
    let tail = rest.get(digits.len()..)?;
    if !tail.eq_ignore_ascii_case(suffix) || digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The cells that hold each property: directly under the sheet, or in a row of a section.
const CELLS: &[(Option<&str>, &str, P)] = &[
    (None, "FillForegnd", P::FillColour),
    (None, "FillBkgnd", P::FillColour2),
    (None, "FillForegndTrans", P::FillTrans),
    (None, "FillBkgndTrans", P::FillTransBkgnd),
    (None, "FillPattern", P::FillPattern),
    (None, "FillGradientEnabled", P::FillGradEnabled),
    (None, "FillGradientDir", P::FillGradDir),
    (None, "FillGradientAngle", P::FillGradAngle),
    (None, "RotateGradientWithShape", P::RotateGradient),
    (None, "LineGradientEnabled", P::LineGradEnabled),
    (None, "LineGradientDir", P::LineGradDir),
    (None, "LineGradientAngle", P::LineGradAngle),
    (None, "UseGroupGradient", P::Other),
    (None, "ShdwForegnd", P::ShadowColour),
    (None, "ShdwForegndTrans", P::ShadowTrans),
    (None, "ShdwPattern", P::ShadowPattern),
    (None, "ShapeShdwType", P::ShadowStyle),
    (None, "ShapeShdwOffsetX", P::ShadowX),
    (None, "ShapeShdwOffsetY", P::ShadowY),
    (None, "ShapeShdwScaleFactor", P::ShadowMag),
    (None, "ShapeShdwObliqueAngle", P::ShadowDir),
    (None, "ShapeShdwBlur", P::ShadowBlur),
    (None, "LineColor", P::LineColour),
    (None, "LineWeight", P::LineWeight),
    (None, "LineCap", P::LineCap),
    (None, "CompoundType", P::LineCompound),
    (None, "LinePattern", P::LinePattern),
    (None, "Rounding", P::LineRounding),
    (None, "BeginArrow", P::LineBegin),
    (None, "BeginArrowSize", P::LineBeginSize),
    (None, "EndArrow", P::LineEnd),
    (None, "EndArrowSize", P::LineEndSize),
    (None, "LineColorTrans", P::LineTrans),
    (Some("Character"), "Color", P::TextColour),
    (Some("Character"), "Font", P::LatinFont),
    (Some("Character"), "Style", P::TextStyle),
    (Some("Character"), "AsianFont", P::Other),
    (Some("Character"), "ComplexScriptFont", P::Other),
];

/// What a cell reads when its call names no property: by the cell it sits in. `None` for a cell the table does not cover.
pub(crate) fn host_property(addr: &CellAddr) -> Option<P> {
    let section = addr.section.as_ref().map(|s| s.name.as_str());
    if let (Some(sec @ ("FillGradient" | "LineGradient")), Some(RowKey::Ix(ix))) = (section, addr.row.as_ref()) {
        let what = match addr.cell.as_str() {
            "GradientStopColor" => StopWhat::Colour,
            "GradientStopColorTrans" => StopWhat::Transparency,
            "GradientStopPosition" => StopWhat::Position,
            _ => return None,
        };
        // IX counts from 0 and the theme's stops from 1.
        return u8::try_from(*ix).ok().filter(|n| *n < 10).map(|n| P::Stop { line: sec == "LineGradient", n, what });
    }
    CELLS.iter().find(|(s, c, _)| *s == section && *c == addr.cell.as_str()).map(|(_, _, p)| *p)
}

/// The cell of the root style that a property falls back to: the first cell of the table that holds it.
pub(crate) fn root_cell(p: P) -> Option<CellAddr> {
    if let P::Stop { line, n, what } = p {
        let cell = match what {
            StopWhat::Colour => "GradientStopColor",
            StopWhat::Transparency => "GradientStopColorTrans",
            StopWhat::Position => "GradientStopPosition",
        };
        return Some(CellAddr::in_row(if line { "LineGradient" } else { "FillGradient" }, None, RowKey::Ix(u32::from(n)), cell));
    }
    CELLS.iter().find(|(_, _, q)| *q == p).map(|(s, c, _)| match s {
        None => CellAddr::direct(c),
        Some(section) => CellAddr::in_row(section, None, RowKey::Ix(0), c),
    })
}

/// The root style's cell that a property of a given name reads with no theme: the cell the name maps to, so that
/// `ConnectorBeginSize` reads `BeginArrowSize`, `ConnectorWeight` reads `LineWeight`, `LatinFont` reads the `Font` of the first `Character` row
/// and `FillStop2Position` the `GradientStopPosition` of the second `FillGradient` row. `None` for a name that is no cell's property: the
/// colours (they have the no-theme table, and the variant colours the accents) and a name the table lacks.
pub(crate) fn mapped_root_cell(name: &str) -> Option<CellAddr> {
    match canonical(name)? {
        P::Slot(_) | P::VariantColour(_) => None,
        P::Other => other_root_cell(name),
        p if table_name(p).is_some() => None,
        p => root_cell(p),
    }
}

/// The root style's cell of a name the table holds and this evaluator does not work out from a theme (the other two fonts, `UseGroupGradient`,
/// and the effects): the cell of the same name, but for `BevelMaterial` (`BevelMaterialType`), `BevelLighting` (`BevelLightingType`),
/// `GlowTransparency` (`GlowColorTrans`), `ComplexFont` (`ComplexScriptFont`) and `AsianFont`, which are cells of the first `Character` row.
fn other_root_cell(name: &str) -> Option<CellAddr> {
    const SAME: &[&str] = &[
        "UseGroupGradient",
        "BevelTopType",
        "BevelTopWidth",
        "BevelTopHeight",
        "BevelContourColor",
        "BevelContourSize",
        "BevelLightingAngle",
        "BevelLightingType",
        "GlowSize",
        "GlowColor",
        "GlowColorTrans",
        "ReflectionBlur",
        "ReflectionDist",
        "ReflectionSize",
        "ReflectionTrans",
        "SoftEdgesSize",
        "SketchAmount",
        "SketchEnabled",
        "SketchFillChange",
        "SketchLineChange",
        "SketchLineWeight",
    ];
    let character = |cell: &str| CellAddr::in_row("Character", None, RowKey::Ix(0), cell);
    // Compared in place and in any case: a name is not copied to be lower-cased.
    let is = |spelled: &str| name.eq_ignore_ascii_case(spelled);
    if is("AsianFont") {
        return Some(character("AsianFont"));
    }
    if is("ComplexFont") {
        return Some(character("ComplexScriptFont"));
    }
    if is("BevelMaterial") {
        return Some(CellAddr::direct("BevelMaterialType"));
    }
    if is("BevelLighting") {
        return Some(CellAddr::direct("BevelLightingType"));
    }
    if is("GlowTransparency") {
        return Some(CellAddr::direct("GlowColorTrans"));
    }
    SAME.iter().find(|n| is(n)).map(|n| CellAddr::direct(n))
}

/// The root style's cell that a call naming no property reads, in a cell the table holds and this evaluator does not work out from a theme
/// (`Character`'s `AsianFont` and `ComplexScriptFont`, `UseGroupGradient`): the cell of the same name, in the first `Character` row for a row cell.
pub(crate) fn other_host_cell(host: &CellAddr) -> CellAddr {
    match host.section.as_ref().map(|s| s.name.as_str()) {
        Some("Character") => CellAddr::in_row("Character", None, RowKey::Ix(0), host.cell.as_str()),
        _ => CellAddr::direct(host.cell.as_str()),
    }
}

/// The name of a colour property in the no-theme table: what a number past a list takes.
pub(crate) fn table_name(p: P) -> Option<&'static str> {
    Some(match p {
        P::TextColour => "TextColor",
        P::FillColour => "FillColor",
        P::FillColour2 => "FillColor2",
        P::LineColour => "LineColor",
        P::ShadowColour => "ShadowColor",
        _ => return None,
    })
}
