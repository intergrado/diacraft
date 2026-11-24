//! A theme part made in code, from the theme part's specification and the theme the THEMEVAL cases are built on:
//! scheme, effect, connector, font and primary numbers all 40; `dk1` `#111111`, `lt1` `#fefefe`, `accent1` `#204060`, `accent3` `#214365`,
//! background `#ffffff`. Every list is a field holding the XML of its items, so a test changes the one entry it is about.
//!
//! **The lists are composed for the tests, not taken from a file.** Each position holds a value that no neighbour holds, so that a reader one
//! entry off, or reading the wrong list, shows (`vt:lineStyles` runs one position ahead of the line list).

use std::sync::Arc;

use diacraft_doc::Theme;

/// The namespace of `a:` and of `vt:`, as a part declares them.
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_VT: &str = "http://schemas.microsoft.com/office/visio/2012/theme"; // brand-ok: a namespace the format defines

/// The XML of a theme part, in pieces a test can swap.
#[derive(Clone, Debug)]
pub struct ThemeXml {
    /// `schemeEnum` of the colour, font, effect, connector and primary schemes: all 40 by default.
    pub scheme: u32,
    /// `dk1`, `lt1`, `dk2`, `lt2`, `accent1` to `accent6`, `hlink`, `folHlink`, as `RRGGBB`.
    pub colours: [&'static str; 12],
    /// The `vt:bkgnd` colour, `RRGGBB`.
    pub background: &'static str,
    /// The latin typeface of the major and minor fonts.
    pub major_font: &'static str,
    pub minor_font: &'static str,
    /// Items of `a:fillStyleLst`, `a:lnStyleLst`, `a:effectStyleLst` of the effect scheme: XML, one string an item.
    pub fills: Vec<String>,
    pub lines: Vec<String>,
    pub effects: Vec<String>,
    /// The same three of `vt:fmtConnectorScheme`.
    pub connector_fills: Vec<String>,
    pub connector_lines: Vec<String>,
    pub connector_effects: Vec<String>,
    /// The `vt:lineStyle`s of `vt:fmtSchemeLineStyles` and `vt:fmtConnectorSchemeLineStyles`, one more than the lines.
    pub line_styles: Vec<String>,
    pub connector_line_styles: Vec<String>,
    /// `vt:fillProps` items.
    pub fill_props: Vec<String>,
    /// `vt:fontProps` items of `vt:fontStyles` and `vt:connectorFontStyles`.
    pub fonts: Vec<String>,
    pub connector_fonts: Vec<String>,
    /// `vt:variationClrScheme` items, or `None` for a part with no such list.
    pub colour_variants: Option<Vec<String>>,
    /// `vt:variationStyleScheme` items, or `None` for a part with no such list.
    pub style_variants: Option<Vec<String>>,
}

/// An `a:schemeClr` with these modifiers, as `(name, value)`.
pub fn scheme_clr(val: &str, mods: &[(&str, i32)]) -> String {
    let inner: String = mods.iter().map(|(n, v)| format!("<a:{n} val=\"{v}\"/>")).collect();
    format!("<a:schemeClr val=\"{val}\">{inner}</a:schemeClr>")
}

/// An `a:srgbClr` with these modifiers.
pub fn srgb_clr(hex: &str, mods: &[(&str, i32)]) -> String {
    let inner: String = mods.iter().map(|(n, v)| format!("<a:{n} val=\"{v}\"/>")).collect();
    format!("<a:srgbClr val=\"{hex}\">{inner}</a:srgbClr>")
}

/// A solid fill of a colour element.
pub fn solid(colour: &str) -> String {
    format!("<a:solidFill>{colour}</a:solidFill>")
}

/// A linear gradient of `(position over 100000, colour element)` stops at `ang` (60000ths of a degree).
pub fn gradient(stops: &[(u32, String)], ang: i32) -> String {
    let gs: String = stops.iter().map(|(p, c)| format!("<a:gs pos=\"{p}\">{c}</a:gs>")).collect();
    format!("<a:gradFill rotWithShape=\"1\"><a:gsLst>{gs}</a:gsLst><a:lin ang=\"{ang}\" scaled=\"0\"/></a:gradFill>")
}

/// An `a:ln` of `w` EMU with this fill element and dash.
pub fn ln(w: i64, cap: &str, fill: &str, dash: &str) -> String {
    format!("<a:ln w=\"{w}\" cap=\"{cap}\" cmpd=\"sng\" algn=\"ctr\">{fill}<a:prstDash val=\"{dash}\"/></a:ln>")
}

/// An `a:effectStyle` with an outer shadow, or with none.
pub fn effect(shadow: Option<(i64, i64, i64, &str)>) -> String {
    match shadow {
        Some((blur, dist, dir, colour)) => format!(
            "<a:effectStyle><a:effectLst><a:outerShdw blurRad=\"{blur}\" dist=\"{dist}\" dir=\"{dir}\" rotWithShape=\"0\">{colour}</a:outerShdw></a:effectLst></a:effectStyle>"
        ),
        None => "<a:effectStyle><a:effectLst/></a:effectStyle>".to_owned(),
    }
}

/// A `vt:lineStyle` of a `vt:lineEx`.
pub fn line_style(rndg: i64, start: u32, start_size: u32, end: u32, end_size: u32, pattern: u32) -> String {
    format!(
        "<vt:lineStyle><vt:lineEx rndg=\"{rndg}\" start=\"{start}\" startSize=\"{start_size}\" end=\"{end}\" endSize=\"{end_size}\" pattern=\"{pattern}\"/></vt:lineStyle>"
    )
}

/// A `vt:fontProps` of `style` with this colour element.
pub fn font_props(style: u32, colour: &str) -> String {
    format!("<vt:fontProps style=\"{style}\"><vt:color>{colour}</vt:color></vt:fontProps>")
}

/// A `vt:variationClrScheme` of seven colours, `monotone` stated or not.
pub fn colour_variant(monotone: Option<bool>, colours: [&str; 7]) -> String {
    let attr = monotone.map_or(String::new(), |m| format!(" monotone=\"{}\"", u8::from(m)));
    let vars: String =
        colours.iter().enumerate().map(|(i, c)| format!("<vt:varColor{n}><a:srgbClr val=\"{c}\"/></vt:varColor{n}>", n = i + 1)).collect();
    format!("<vt:variationClrScheme{attr}>{vars}</vt:variationClrScheme>")
}

/// A `vt:variationStyleScheme` of `embellishment` and these `(fillIdx, lineIdx, effectIdx, fontIdx)` styles.
pub fn style_variant(embellishment: u32, styles: &[(u32, u32, u32, u32)]) -> String {
    let items: String =
        styles.iter().map(|(f, l, e, t)| format!("<vt:varStyle fillIdx=\"{f}\" lineIdx=\"{l}\" effectIdx=\"{e}\" fontIdx=\"{t}\"/>")).collect();
    format!("<vt:variationStyleScheme embellishment=\"{embellishment}\">{items}</vt:variationStyleScheme>")
}

impl Default for ThemeXml {
    fn default() -> Self {
        let ph = |mods: &[(&str, i32)]| scheme_clr("phClr", mods);
        let black = |alpha: i32| srgb_clr("000000", &[("alpha", alpha)]);
        // Fills: 1 solid; 2 solid, shade 75000; 3 a gradient of two stops at 90 degrees; 4 a gradient of three stops with a tint
        // and a shade; 5 no fill; 6 a solid naming `lt1`.
        let fills = vec![
            solid(&ph(&[])),
            solid(&ph(&[("shade", 75000)])),
            gradient(&[(0, ph(&[("tint", 50000)])), (100_000, ph(&[("shade", 50000)]))], 5_400_000),
            gradient(&[(0, ph(&[("tint", 60000)])), (50_000, ph(&[("satMod", 150000)])), (100_000, ph(&[("shade", 80000)]))], 0),
            "<a:noFill/>".to_owned(),
            solid(&scheme_clr("lt1", &[])),
        ];
        // Lines: weights 0.75, 1, 1.5, 2, 3 and 4.5 points; 2 has a shade of 87000.
        let lines = vec![
            ln(9525, "flat", &solid(&ph(&[])), "solid"),
            ln(12700, "rnd", &solid(&ph(&[("shade", 87000)])), "solid"),
            ln(19050, "sq", &solid(&ph(&[("shade", 95000)])), "sysDash"),
            ln(25400, "flat", &solid(&ph(&[])), "sysDot"),
            ln(38100, "flat", &gradient(&[(0, ph(&[])), (100_000, ph(&[("shade", 50000)]))], 2_700_000), "solid"),
            ln(57150, "rnd", "<a:noFill/>", "solid"),
        ];
        // Effects: 1 none; 2 an outer shadow of 12700 EMU at 90 degrees; 3 a shadow of 38100 at 45 degrees, 40 percent black.
        let effects = vec![
            effect(None),
            effect(Some((50800, 12700, 5_400_000, &black(38000)))),
            effect(Some((76200, 38100, 2_700_000, &srgb_clr("303030", &[("alpha", 40000)])))),
            effect(None),
            effect(None),
            effect(None),
        ];
        // Seven positions: position n+1 is entry n, so position 1 holds a pattern no entry has.
        let line_styles = vec![
            line_style(0, 0, 2, 0, 2, 5),
            line_style(0, 0, 2, 4, 2, 1),
            line_style(114_300, 1, 3, 2, 4, 9),
            line_style(0, 0, 2, 13, 5, 10),
            line_style(0, 0, 2, 0, 2, 2),
            line_style(0, 0, 2, 0, 2, 23),
            line_style(57_150, 0, 2, 0, 2, 1),
        ];
        let fonts = vec![
            font_props(0, &ph(&[])),
            font_props(0, &ph(&[])),
            font_props(0, &ph(&[])),
            font_props(17, &scheme_clr("lt1", &[])),
            font_props(0, &scheme_clr("lt1", &[])),
            font_props(17, &scheme_clr("lt1", &[])),
        ];
        ThemeXml {
            scheme: 40,
            colours: ["111111", "fefefe", "202020", "dddddd", "204060", "506070", "214365", "708090", "a0b0c0", "c0d0e0", "0563c1", "954f72"],
            background: "ffffff",
            major_font: "Major Sans",
            minor_font: "Minor Sans",
            fills,
            lines,
            effects,
            connector_fills: vec![solid(&ph(&[])), solid(&ph(&[("shade", 50000)])), solid(&ph(&[("tint", 50000)]))],
            connector_lines: vec![
                ln(6350, "flat", &solid(&ph(&[])), "solid"),
                ln(31750, "flat", &solid(&ph(&[("shade", 60000)])), "solid"),
                ln(44450, "flat", &solid(&ph(&[])), "dash"),
            ],
            connector_effects: vec![effect(None), effect(None), effect(None)],
            line_styles,
            connector_line_styles: vec![
                line_style(0, 0, 2, 0, 2, 3),
                line_style(0, 0, 2, 5, 3, 1),
                line_style(0, 0, 2, 4, 2, 9),
                line_style(0, 0, 2, 0, 2, 2),
            ],
            fill_props: vec![
                "<vt:fillProps pattern=\"1\"/>".to_owned(),
                "<vt:fillProps pattern=\"1\"/>".to_owned(),
                "<vt:fillProps pattern=\"25\"/>".to_owned(),
                "<vt:fillProps pattern=\"28\"/>".to_owned(),
                "<vt:fillProps pattern=\"0\"/>".to_owned(),
                "<vt:fillProps pattern=\"1\"/>".to_owned(),
            ],
            fonts: fonts.clone(),
            connector_fonts: fonts.into_iter().take(3).collect(),
            colour_variants: Some(vec![
                colour_variant(None, ["112233", "223344", "334455", "445566", "556677", "667788", "778899"]),
                colour_variant(Some(true), ["a1a1a1", "b2b2b2", "c3c3c3", "d4d4d4", "e5e5e5", "f6f6f6", "070707"]),
                colour_variant(None, ["010203", "040506", "070809", "0a0b0c", "0d0e0f", "101112", "131415"]),
                colour_variant(Some(false), ["aa0000", "00aa00", "0000aa", "aaaa00", "00aaaa", "aa00aa", "aaaaaa"]),
            ]),
            style_variants: Some(vec![
                style_variant(2, &[(1, 1, 1, 1), (2, 2, 2, 2), (3, 3, 3, 3), (4, 4, 4, 4)]),
                style_variant(4, &[(3, 2, 1, 1), (4, 3, 2, 2), (1, 4, 3, 3), (2, 1, 1, 4)]),
                style_variant(3, &[(2, 3, 2, 2), (1, 2, 3, 3), (5, 4, 1, 4), (6, 5, 2, 1)]),
                style_variant(1, &[(1, 1, 1, 1), (1, 1, 1, 1), (1, 1, 1, 1), (1, 1, 1, 1)]),
            ]),
        }
    }
}

fn ext(uri: &str, body: &str) -> String {
    format!("<a:ext uri=\"{uri}\">{body}</a:ext>")
}

fn scheme_id(scheme: u32) -> String {
    format!("<vt:schemeID schemeEnum=\"{scheme}\" schemeGUID=\"{{11111111-1111-1111-1111-111111111111}}\"/>")
}

fn lists(fills: &[String], lines: &[String], effects: &[String]) -> String {
    format!(
        "<a:fillStyleLst>{}</a:fillStyleLst><a:lnStyleLst>{}</a:lnStyleLst><a:effectStyleLst>{}</a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst>",
        fills.concat(),
        lines.concat(),
        effects.concat()
    )
}

impl ThemeXml {
    /// The part's XML.
    pub fn xml(&self) -> String {
        let names = ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];
        let slots: String = names.iter().zip(self.colours).map(|(n, c)| format!("<a:{n}><a:srgbClr val=\"{c}\"/></a:{n}>")).collect();
        let mut clr_ext = ext("{093E89EA-6996-430E-BFF9-83A9FAAAAB73}", &format!("<vt:bkgnd><a:srgbClr val=\"{}\"/></vt:bkgnd>", self.background));
        clr_ext.push_str(&ext("{2703A3B3-D2E1-43D9-8057-6E9D74E0F44A}", &scheme_id(self.scheme)));
        if let Some(variants) = &self.colour_variants {
            clr_ext.push_str(&ext(
                "{DDD2D869-C2EF-471E-B8FA-914AFA308C9F}",
                &format!("<vt:variationClrSchemeLst>{}</vt:variationClrSchemeLst>", variants.concat()),
            ));
        }
        let mut el_ext = String::new();
        el_ext.push_str(&ext(
            "{1342405F-259F-4C95-8CDF-A9DCE2D418A2}",
            &format!(
                "<vt:fmtConnectorScheme name=\"Connector\">{}</vt:fmtConnectorScheme>",
                lists(&self.connector_fills, &self.connector_lines, &self.connector_effects)
            ),
        ));
        el_ext.push_str(&ext("{27CD58D4-7086-4B73-B2AB-7AEBE2148A8C}", &format!("<vt:fmtSchemeEx>{}</vt:fmtSchemeEx>", scheme_id(self.scheme))));
        el_ext.push_str(&ext(
            "{C6430689-8E98-42EC-ADBF-087148533A3F}",
            &format!("<vt:fmtConnectorSchemeEx>{}</vt:fmtConnectorSchemeEx>", scheme_id(self.scheme)),
        ));
        el_ext.push_str(&ext("{D75FF423-9257-4291-A4FE-1B2448832E17}", &format!("<vt:themeScheme>{}</vt:themeScheme>", scheme_id(self.scheme))));
        el_ext.push_str(&ext(
            "{6CAB99AB-0A78-4BAB-B597-526D62367CB4}",
            &format!(
                "<vt:lineStyles><vt:fmtConnectorSchemeLineStyles>{}</vt:fmtConnectorSchemeLineStyles><vt:fmtSchemeLineStyles>{}</vt:fmtSchemeLineStyles></vt:lineStyles>",
                self.connector_line_styles.concat(),
                self.line_styles.concat()
            ),
        ));
        el_ext.push_str(&ext("{56243398-1771-4C39-BF73-A5702A9C147F}", &format!("<vt:fillStyles>{}</vt:fillStyles>", self.fill_props.concat())));
        el_ext.push_str(&ext(
            "{EBE24D50-EC5C-4D6F-A1A3-C5F0A18B936A}",
            &format!(
                "<vt:fontStylesGroup><vt:connectorFontStyles>{}</vt:connectorFontStyles><vt:fontStyles>{}</vt:fontStyles></vt:fontStylesGroup>",
                self.connector_fonts.concat(),
                self.fonts.concat()
            ),
        ));
        if let Some(variants) = &self.style_variants {
            el_ext.push_str(&ext(
                "{494CE47F-D151-47DC-95E8-85652EA8A67E}",
                &format!("<vt:variationStyleSchemeLst>{}</vt:variationStyleSchemeLst>", variants.concat()),
            ));
        }
        format!(
            "<a:theme xmlns:a=\"{NS_A}\" xmlns:vt=\"{NS_VT}\" name=\"Composed\"><a:themeElements><a:clrScheme name=\"Composed\">{slots}<a:extLst>{clr_ext}</a:extLst></a:clrScheme>\
<a:fontScheme name=\"F\"><a:majorFont><a:latin typeface=\"{major}\"/></a:majorFont><a:minorFont><a:latin typeface=\"{minor}\"/></a:minorFont><a:extLst>{font_ext}</a:extLst></a:fontScheme>\
<a:fmtScheme name=\"E\">{fmt}</a:fmtScheme><a:extLst>{el_ext}</a:extLst></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>",
            major = self.major_font,
            minor = self.minor_font,
            font_ext = ext("{B3C0788F-89CC-4CDB-A57D-0664CFDA821B}", &scheme_id(self.scheme)),
            fmt = lists(&self.fills, &self.lines, &self.effects),
        )
    }

    /// The part read as the reader reads a theme part of a package, named the first theme part of a package.
    pub fn part(&self) -> Arc<Theme> {
        Arc::new(diacraft_vsdx::read_theme_part("/visio/theme/theme1.xml", self.xml().as_bytes())) // brand-ok: the part name a package gives a theme
    }
}

/// The composed theme as a part: schemes all 40.
pub fn composed() -> Arc<Theme> {
    ThemeXml::default().part()
}
