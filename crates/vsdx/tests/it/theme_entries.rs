//! What the theme functions need from a theme part: the variants, the lists of both
//! schemes and the VSDX extensions, read as a view of the bytes, with an unreadable slot left unread and warned of, and every list capped.
//!
//! The part is the composed one of `diacraft_testkit::theme` (the THEMEVAL vectors' theme), with a value at each position that no
//! neighbour holds, so a reader one entry off shows.

use std::time::{Duration, Instant};

use diacraft_color::{ColorTransform, Rgba, SchemeSlot};
use diacraft_doc::{FillEntry, GradientKind, MAX_THEME_ENTRIES, MAX_THEME_MODIFIERS, MAX_THEME_STOPS, ThemeBase};
use diacraft_testkit::package::VsdxBuilder;
use diacraft_testkit::theme::{ThemeXml, colour_variant, solid, srgb_clr};
use diacraft_vsdx::Limits;

use crate::{read_ok, read_with};

fn rgb(hex: &str) -> Rgba {
    Rgba::from_hex(hex).unwrap()
}

#[test]
fn every_list_of_the_composed_theme_is_read_in_order() {
    let theme = ThemeXml::default().part();
    let e = &theme.entries;
    assert_eq!(e.background, Some(rgb("ffffff")), "vt:bkgnd");
    // Four variants, the second `monotone`, the first and third silent about it.
    assert_eq!(e.colour_variants.len(), 4);
    let v: Vec<_> = e.colour_variants.iter().map(|v| v.as_ref().unwrap()).collect();
    assert_eq!(v.iter().map(|v| v.monotone).collect::<Vec<_>>(), [false, true, false, false]);
    assert_eq!(v[0].colours[1], Some(rgb("223344")));
    assert_eq!(v[1].colours[3], Some(rgb("d4d4d4")));
    // Embellishment and the four numbers of each style.
    assert_eq!(e.style_variants.iter().map(|v| v.as_ref().unwrap().embellishment).collect::<Vec<_>>(), [Some(2), Some(4), Some(3), Some(1)]);
    let s2 = e.style_variants[2].as_ref().unwrap();
    assert_eq!((s2.styles[2].fill_idx, s2.styles[2].line_idx, s2.styles[2].effect_idx, s2.styles[2].font_idx), (Some(5), Some(4), Some(1), Some(4)));
    // What the reader took before is as it was.
    assert_eq!(theme.color_scheme_id.as_ref().unwrap().scheme_enum, 40);
    assert_eq!(theme.slot_colour(SchemeSlot::Accent1), Some(rgb("204060")));
}

#[test]
fn the_fill_line_and_effect_lists_of_both_schemes_are_read_in_order() {
    let theme = ThemeXml::default().part();
    let e = &theme.entries;
    // Six entries each in the effect scheme, three in the connector scheme.
    assert_eq!((e.effect.fills.len(), e.effect.lines.len(), e.effect.effects.len()), (6, 6, 6));
    assert_eq!((e.connector.fills.len(), e.connector.lines.len(), e.connector.effects.len()), (3, 3, 3));
    // The fills: the second is a solid `phClr` with a shade, the third a gradient of two stops at 90 degrees.
    let Some(FillEntry::Solid(c)) = &e.effect.fills[1] else { panic!("{:?}", e.effect.fills[1]) };
    assert_eq!(c.base, ThemeBase::Slot(SchemeSlot::PhClr));
    assert_eq!(c.modifiers, vec![ColorTransform::Shade(75000)]);
    let Some(FillEntry::Gradient(g)) = &e.effect.fills[2] else { panic!("{:?}", e.effect.fills[2]) };
    assert_eq!(g.kind, GradientKind::Linear { degrees: 90.0 });
    assert_eq!((g.stops.len(), g.stops[0].pos, g.stops[1].pos), (2, 0.0, 1.0));
    assert_eq!(g.stops[1].colour.modifiers, vec![ColorTransform::Shade(50000)]);
    assert_eq!(g.rotate_with_shape, Some(true));
    assert!(matches!(e.effect.fills[4], Some(FillEntry::NoFill)));
    // The lines: width in EMU, cap, dash and the fill of the `a:ln`.
    let l = e.effect.lines[1].as_ref().unwrap();
    assert_eq!((l.width_emu, l.cap.as_deref(), l.dash.as_deref()), (Some(12700), Some("rnd"), Some("solid")));
    assert!(matches!(&l.fill, Some(FillEntry::Solid(c)) if c.modifiers == vec![ColorTransform::Shade(87000)]));
    // The effects: the second holds an outer shadow.
    assert!(e.effect.effects[0].as_ref().unwrap().outer_shadow.is_none());
    let sh = e.effect.effects[1].as_ref().unwrap().outer_shadow.as_ref().unwrap();
    assert_eq!((sh.blur_emu, sh.dist_emu, sh.dir, sh.scale_x), (50800, 12700, 5_400_000, 1.0));
    assert_eq!(sh.colour.base, ThemeBase::Rgb(rgb("000000")));
    assert_eq!(sh.colour.modifiers, vec![ColorTransform::Alpha(38000)]);
}

#[test]
fn the_extensions_of_the_line_fill_and_font_lists_are_read_in_order() {
    let theme = ThemeXml::default().part();
    let e = &theme.entries;
    // `vt:lineStyles` runs one position ahead of the lines: seven and four.
    assert_eq!((e.line_ex.len(), e.connector_line_ex.len()), (7, 4));
    let ex = e.line_ex[1].unwrap();
    assert_eq!((ex.rounding_emu, ex.start, ex.start_size, ex.end, ex.end_size, ex.pattern), (Some(0), Some(0), Some(2), Some(4), Some(2), Some(1)));
    assert_eq!(e.line_ex[2].unwrap().pattern, Some(9));
    assert_eq!(e.line_ex[2].unwrap().rounding_emu, Some(114_300));
    assert_eq!(e.fill_props.len(), 6);
    assert_eq!(e.fill_props[2].unwrap().pattern, Some(25));
    assert_eq!((e.fonts.len(), e.connector_fonts.len()), (6, 3));
    let f = e.fonts[3].as_ref().unwrap();
    assert_eq!(f.style, Some(17));
    assert_eq!(f.colour.as_ref().unwrap().base, ThemeBase::Slot(SchemeSlot::Lt1));
}

#[test]
fn a_part_with_no_extensions_reads_no_entries_and_says_nothing() {
    let bytes = br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements/></a:theme>"#;
    let (d, report) = read_ok(&VsdxBuilder::new().theme(bytes).build());
    assert_eq!(d.themes.get(0).unwrap().entries, Default::default());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_part_with_no_variant_lists_has_none_and_no_warning() {
    let xml = ThemeXml { colour_variants: None, style_variants: None, ..ThemeXml::default() }.xml();
    let (d, report) = read_ok(&VsdxBuilder::new().theme(xml.as_bytes()).build());
    let e = &d.themes.get(0).unwrap().entries;
    assert!(e.colour_variants.is_empty() && e.style_variants.is_empty());
    assert_eq!(e.effect.fills.len(), 6, "the rest is read");
    assert_eq!(report.warning_count("theme_entry_unreadable"), 0);
}

#[test]
fn a_variant_with_no_monotone_is_not_monotone_and_monotone_one_is() {
    let xml = ThemeXml {
        colour_variants: Some(vec![
            colour_variant(None, ["010101"; 7]),
            colour_variant(Some(true), ["020202"; 7]),
            colour_variant(Some(false), ["030303"; 7]),
        ]),
        ..ThemeXml::default()
    }
    .part();
    let monotone: Vec<_> = xml.entries.colour_variants.iter().map(|v| v.as_ref().unwrap().monotone).collect();
    assert_eq!(monotone, [false, true, false]);
}

#[test]
fn a_slot_that_cannot_be_read_is_left_unread_in_its_place_and_warned_of() {
    let mut x = ThemeXml::default();
    // A fill whose colour is a named one (`a:prstClr`), a line with a width that is no number, a variant with `monotone="maybe"`.
    x.fills[1] = "<a:solidFill><a:prstClr val=\"black\"/></a:solidFill>".to_owned();
    x.lines[2] = x.lines[2].replace("w=\"19050\"", "w=\"wide\"");
    let mut variants = x.colour_variants.clone().unwrap();
    variants[1] = variants[1].replace("monotone=\"1\"", "monotone=\"maybe\"");
    x.colour_variants = Some(variants);
    let bytes = VsdxBuilder::new().theme(x.xml().as_bytes()).build();
    let (d, report) = read_ok(&bytes);
    let e = &d.themes.get(0).unwrap().entries;
    assert!(e.effect.fills[1].is_none(), "the entry is unread");
    assert!(e.effect.fills[2].is_some() && e.effect.fills[0].is_some(), "and the others, and their places, stay");
    assert_eq!(e.effect.fills.len(), 6);
    assert!(e.effect.lines[2].is_none() && e.effect.lines[1].is_some());
    assert!(e.colour_variants[1].is_none() && e.colour_variants[2].is_some());
    // One warning for the part, counting what was left unread.
    assert_eq!(report.warning_count("theme_entry_unreadable"), 1, "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("3 entries")), "{:?}", report.warnings);
    assert_eq!(&*d.themes.get(0).unwrap().bytes, x.xml().as_bytes(), "the bytes are as they were");
}

#[test]
fn a_list_past_its_cap_is_cut_and_warned_of() {
    let x = ThemeXml { fills: (0..MAX_THEME_ENTRIES + 5).map(|_| solid(&srgb_clr("102030", &[]))).collect(), ..ThemeXml::default() };
    let (d, report) = read_ok(&VsdxBuilder::new().theme(x.xml().as_bytes()).build());
    assert_eq!(d.themes.get(0).unwrap().entries.effect.fills.len(), MAX_THEME_ENTRIES);
    assert_eq!(report.warning_count("theme_list_capped"), 1);
    // Stops and modifiers have caps of their own.
    let stops: Vec<_> = (0..MAX_THEME_STOPS + 3).map(|i| (i as u32, srgb_clr("102030", &[]))).collect();
    let mods: Vec<_> = (0..MAX_THEME_MODIFIERS + 3).map(|_| ("lumMod", 90000)).collect();
    let x = ThemeXml { fills: vec![diacraft_testkit::theme::gradient(&stops, 0), solid(&srgb_clr("102030", &mods))], ..ThemeXml::default() };
    let theme = x.part();
    let Some(FillEntry::Gradient(g)) = &theme.entries.effect.fills[0] else { panic!() };
    assert_eq!(g.stops.len(), MAX_THEME_STOPS);
    let Some(FillEntry::Solid(c)) = &theme.entries.effect.fills[1] else { panic!() };
    assert_eq!(c.modifiers.len(), MAX_THEME_MODIFIERS);
}

#[test]
fn several_parts_are_each_read_for_themselves() {
    let a = ThemeXml::default();
    let b = ThemeXml { scheme: 41, background: "000000", ..ThemeXml::default() };
    let (d, _) = read_ok(&VsdxBuilder::new().theme(a.xml().as_bytes()).theme(b.xml().as_bytes()).build());
    assert_eq!(d.themes.get(0).unwrap().entries.background, Some(rgb("ffffff")));
    assert_eq!(d.themes.get(1).unwrap().entries.background, Some(rgb("000000")));
    assert_eq!(d.themes.get(1).unwrap().theme_scheme_id.as_ref().unwrap().scheme_enum, 41);
}

// ------------------------------------------------------------------------------------------------------------ hostile and timed

/// A hang guard and no measure of cost (a cost is bounded by the reader's budgets and caps, which these tests assert): the slowest read here
/// takes a few seconds at load 50, so a minute tells a hang from a slow machine.
const BOUND: Duration = Duration::from_secs(60);

/// The part of a theme near the 4 MiB budget whose `variationClrSchemeLst` holds as many variants as fit: the list is read to the cap and
/// no further, in a time that the size of the file alone sets.
#[test]
fn a_theme_part_of_the_whole_byte_budget_with_a_huge_variant_list_is_read_to_the_cap_in_time() {
    let one = colour_variant(Some(true), ["010203"; 7]);
    let n = (3_900_000 / one.len()).max(1);
    let x = ThemeXml { colour_variants: Some(vec![one; n]), ..ThemeXml::default() };
    let xml = x.xml();
    assert!(xml.len() > 3_000_000 && xml.len() < 4 * 1024 * 1024, "{} bytes of {n} variants", xml.len());
    let bytes = VsdxBuilder::new().theme(xml.as_bytes()).build();
    let started = Instant::now();
    let (d, report) = read_with(&bytes, &Limits::default());
    let took = started.elapsed();
    eprintln!("[timing] a theme part of {} bytes and {n} variants: {took:?}", xml.len());
    assert!(took < BOUND, "took {took:?}");
    let e = &d.themes.get(0).unwrap().entries;
    assert_eq!(e.colour_variants.len(), MAX_THEME_ENTRIES);
    assert_eq!(e.effect.fills.len(), 6, "the rest of the part is read");
    assert_eq!(report.warning_count("theme_list_capped"), 1);
    assert_eq!(&*d.themes.get(0).unwrap().bytes, xml.as_bytes());
}

/// A part that is huge in every way a list can be: fills, lines, effects, stops of one gradient, modifiers of one colour, and the extension list.
#[test]
fn a_theme_part_with_every_list_huge_is_read_to_the_caps_in_time() {
    let mut x = ThemeXml::default();
    let mods: Vec<_> = (0..50_000).map(|_| ("lumMod", 99000)).collect();
    let stops: Vec<_> = (0..8_000).map(|i| (i, srgb_clr("102030", &[]))).collect();
    x.fills =
        std::iter::once(diacraft_testkit::theme::gradient(&stops, 0)).chain((0..4_000).map(|_| solid(&srgb_clr("102030", &mods[..4])))).collect();
    x.lines = (0..3_000).map(|_| diacraft_testkit::theme::ln(9525, "flat", &solid(&srgb_clr("102030", &mods[..8])), "solid")).collect();
    x.effects = (0..4_000).map(|_| diacraft_testkit::theme::effect(None)).collect();
    x.line_styles = (0..3_000).map(|_| diacraft_testkit::theme::line_style(0, 0, 2, 0, 2, 1)).collect();
    x.fonts = (0..3_000).map(|_| diacraft_testkit::theme::font_props(0, &srgb_clr("102030", &[]))).collect();
    let mut xml = x.xml();
    // 4,000 further extensions on `a:themeElements`, none of them the reference app's.
    let junk: String = (0..4_000).map(|i| format!("<a:ext uri=\"{{{i}}}\"><x:y xmlns:x=\"urn:z\"/></a:ext>")).collect();
    xml = xml.replacen("</a:extLst></a:themeElements>", &format!("{junk}</a:extLst></a:themeElements>"), 1);
    assert!(xml.len() < 4 * 1024 * 1024, "{} bytes", xml.len());
    let bytes = VsdxBuilder::new().theme(xml.as_bytes()).build();
    let started = Instant::now();
    let (d, report) = read_with(&bytes, &Limits::default());
    let took = started.elapsed();
    eprintln!("[timing] a theme part of {} bytes with every list huge: {took:?}", xml.len());
    assert!(took < BOUND, "took {took:?}");
    let e = &d.themes.get(0).unwrap().entries;
    assert_eq!((e.effect.fills.len(), e.effect.lines.len(), e.effect.effects.len()), (MAX_THEME_ENTRIES, MAX_THEME_ENTRIES, MAX_THEME_ENTRIES));
    assert_eq!((e.line_ex.len(), e.fonts.len()), (MAX_THEME_ENTRIES, MAX_THEME_ENTRIES));
    let Some(FillEntry::Gradient(g)) = &e.effect.fills[0] else { panic!() };
    assert_eq!(g.stops.len(), MAX_THEME_STOPS);
    assert!(report.warning_count("theme_list_capped") >= 1);
}
