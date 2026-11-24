//! Character settings: vectors for `Size`, `Pos`, `Scale`, `Spacing`, `Style`, `Transparency`,
//! `ComplexScriptSize` and the lines through and under text.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::{ScriptFields, SynthFont};
use diacraft_text::{BlockFormat, CharFormat, DecorationKind, Laid, Rgba, lay_out, style};

use crate::{cf, close, db, glyph_count, lay_text, left, pt, run};

const HEBREW: &str = "אבג";

fn fam(style: &str) -> SynthFont {
    SynthFont::new("Fam").ascii(500).chars(HEBREW, 500).styled(style)
}

fn fam_db() -> FontDb {
    db(&[fam("Regular"), fam("Bold"), fam("Italic"), fam("Bold Italic")])
}

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn lay(db: &FontDb, text: &str, c: &CharFormat) -> Laid {
    lay_text(db, text, c, &left(), &block(), None)
}

#[test]
fn all_the_roots_values_draw_12_point_text_at_normal_width() {
    // A shape with no character row of its own: Size 0.1666..., FontScale 1. The R19 defaults of 0 would draw nothing.
    let c = CharFormat { font: "Fam".into(), ..CharFormat::default() };
    let laid = lay(&fam_db(), "ab", &c);
    let r = &laid.lines[0].runs[0];
    close(r.size, pt(12.0));
    assert_eq!(r.x_scale, 1.0);
    close(laid.lines[0].width, 2.0 * pt(6.0));
}

#[test]
fn size_zero_and_negative_draw_nothing_and_add_no_width() {
    let db = fam_db();
    for size in [0.0, -1.0, -1e-9] {
        let c = CharFormat { size, ..cf("Fam", 12.0) };
        let laid = lay(&db, "abc", &c);
        assert_eq!(laid.lines.iter().map(glyph_count).sum::<usize>(), 0, "size {size}");
        close(laid.size.width, 0.0);
    }
}

#[test]
fn font_scale_widens_glyphs_at_the_same_height_and_zero_reads_as_one() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.scale = 2.0;
    let laid = lay(&db, "a", &c);
    close(laid.lines[0].runs[0].size, pt(20.0));
    assert_eq!(laid.lines[0].runs[0].x_scale, 2.0);
    // A letter 10 points wide is 20 across.
    close(laid.lines[0].width, pt(20.0));
    c.scale = 0.0;
    let laid = lay(&db, "a", &c);
    assert_eq!(laid.lines[0].runs[0].x_scale, 1.0);
    close(laid.lines[0].width, pt(10.0));
}

#[test]
fn spacing_letterspace_is_paper_inches_added_after_each_letter() {
    let db = fam_db();
    for (ls, want) in [(0.0, pt(10.0)), (pt(1.0), pt(11.0)), (-pt(1.0), pt(9.0))] {
        let mut c = cf("Fam", 20.0);
        c.letterspace = ls;
        let laid = lay(&db, "abc", &c);
        let xs: Vec<f64> = laid.lines[0].runs[0].glyphs.iter().map(|g| g.x).collect();
        close(xs[1] - xs[0], want);
        close(xs[2] - xs[1], want);
    }
}

#[test]
fn spacing_two_gaps_across_a_three_letter_run_and_the_next_row_adds_none() {
    let db = fam_db();
    let mut spaced = cf("Fam", 20.0);
    spaced.letterspace = pt(1.0);
    let plain = cf("Fam", 20.0);
    let laid = lay_out(&[run("abc", &spaced, &left()), run("d", &plain, &left())], &block(), None, &db);
    let glyphs: Vec<&diacraft_text::PlacedGlyph> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).collect();
    close(glyphs[2].x - glyphs[0].x, 2.0 * pt(11.0));
    // The third letter's own added point is part of the run (assumed), so `d` starts after it; and `d` has none after it.
    close(glyphs[3].x - glyphs[2].x, pt(11.0));
    close(laid.lines[0].width, 3.0 * pt(11.0) + pt(10.0));
}

#[test]
fn transparency_the_opacity_is_one_minus_the_share() {
    let db = fam_db();
    let mut c = cf("Fam", 12.0);
    c.transparency = 0.25;
    c.color = Rgba { r: 1, g: 2, b: 3, a: 255 };
    let r = &lay(&db, "a", &c).lines[0].runs[0];
    close(r.opacity, 0.75);
    assert_eq!((r.color.r, r.color.g, r.color.b), (1, 2, 3));
}

#[test]
fn transparency_a_clear_run_still_has_its_glyphs_and_takes_its_room() {
    let db = fam_db();
    let mut clear = cf("Fam", 12.0);
    clear.transparency = 1.0;
    let laid = lay_out(&[run("ab", &clear, &left()), run("c", &cf("Fam", 12.0), &left())], &block(), None, &db);
    let runs = &laid.lines[0].runs;
    assert_eq!(runs[0].glyphs.len(), 2);
    assert_eq!(runs[0].opacity, 0.0);
    close(runs[1].glyphs[0].x, 2.0 * pt(6.0));
}

#[test]
fn script_a_raised_run_draws_at_the_size_the_face_gives() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.pos = 1;
    close(lay(&db, "a", &c).lines[0].runs[0].size, pt(13.0));
}

#[test]
fn script_the_offset_belongs_to_the_face_and_has_a_sign() {
    let mut font = SynthFont::new("Boxes").ascii(500);
    font.subscript = ScriptFields { x_size: 650, y_size: 650, x_offset: 0, y_offset: 140 };
    font.superscript = ScriptFields { x_size: 650, y_size: 650, x_offset: 0, y_offset: 477 };
    let db = db(&[font]);
    let base = lay(&db, "a", &cf("Boxes", 100.0)).lines[0].baseline;
    let mut c = cf("Boxes", 100.0);
    c.pos = 2;
    close(base - lay(&db, "a", &c).lines[0].runs[0].glyphs[0].y, pt(14.0));
    c.pos = 1;
    close(lay(&db, "a", &c).lines[0].runs[0].glyphs[0].y - base, pt(47.7));
}

#[test]
fn script_a_face_with_no_table_uses_065_and_014_down_and_045_up() {
    let mut font = SynthFont::new("Bare").ascii(500);
    font.subscript = ScriptFields::default();
    font.superscript = ScriptFields::default();
    let db = db(&[font]);
    let base = lay(&db, "a", &cf("Bare", 100.0)).lines[0].baseline;
    let mut c = cf("Bare", 100.0);
    c.pos = 1;
    let up = lay(&db, "a", &c);
    close(up.lines[0].runs[0].size, pt(65.0));
    close(up.lines[0].runs[0].glyphs[0].y - base, pt(45.0));
    c.pos = 2;
    let down = lay(&db, "a", &c);
    close(down.lines[0].runs[0].size, pt(65.0));
    close(base - down.lines[0].runs[0].glyphs[0].y, pt(14.0));
}

#[test]
fn script_a_raised_run_after_a_lowered_one_is_measured_from_the_lines_baseline() {
    let mut font = SynthFont::new("Bare").ascii(500);
    font.subscript = ScriptFields::default();
    font.superscript = ScriptFields::default();
    let db = db(&[font]);
    let base = lay(&db, "a", &cf("Bare", 100.0)).lines[0].baseline;
    let mut low = cf("Bare", 100.0);
    low.pos = 2;
    let mut high = cf("Bare", 100.0);
    high.pos = 1;
    let laid = lay_out(&[run("a", &low, &left()), run("b", &high, &left())], &block(), None, &db);
    let second = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).nth(1).expect("second glyph");
    // 45 above the line's baseline: from the lowered run's baseline it would be 31, and an export's `dy` alone would say 27.
    close(second.y - base, pt(45.0));
}

#[test]
fn pos_the_baseline_returns_and_the_next_advance_starts_from_the_smaller_runs_end() {
    let mut font = SynthFont::new("Bare").ascii(500);
    font.subscript = ScriptFields::default();
    let db = db(&[font]);
    let base = lay(&db, "a", &cf("Bare", 100.0)).lines[0].baseline;
    let mut low = cf("Bare", 100.0);
    low.pos = 2;
    let laid = lay_out(&[run("aa", &low, &left()), run("b", &cf("Bare", 100.0), &left())], &block(), None, &db);
    let glyphs: Vec<_> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter()).collect();
    // Two letters at 65 points are 32.5 wide each.
    close(glyphs[2].x, 2.0 * pt(32.5));
    close(glyphs[2].y, base);
}

#[test]
fn complexscriptsize_a_negative_value_is_a_share_of_size_and_minus_one_is_the_same() {
    let db = fam_db();
    for (cs, want) in [(-1.0, pt(20.0)), (-0.5, pt(10.0)), (0.25, pt(18.0)), (0.0, pt(20.0))] {
        let mut c = cf("Fam", 20.0);
        c.complex_size = cs;
        let laid = lay(&db, "אב", &c);
        close(laid.lines[0].runs[0].size, want);
        // Latin letters are untouched.
        let latin = lay(&db, "ab", &c);
        close(latin.lines[0].runs[0].size, pt(20.0));
    }
}

#[test]
fn bit_style_bits_are_tested_one_at_a_time() {
    let db = fam_db();
    let mut c = cf("Fam", 12.0);
    c.style = style::BOLD | style::ITALIC;
    let r = &lay(&db, "a", &c).lines[0].runs[0];
    assert_eq!(r.face.style_name(), "Bold Italic");
    assert_eq!((r.embolden, r.slant), (0.0, 0.0));
    c.style = style::UNDERLINE | style::SMALL_CAPS;
    let laid = lay(&db, "ab", &c);
    assert!(laid.lines[0].decorations.iter().any(|d| d.kind == DecorationKind::Underline));
    assert_eq!(laid.lines[0].runs[0].face.style_name(), "Regular");
}

#[test]
fn bit_sixteen_and_thirty_two_are_a_second_switch_for_complex_script_letters() {
    let db = fam_db();
    let bold_of = |style: u8, text: &str| {
        let mut c = cf("Fam", 12.0);
        c.style = style;
        lay(&db, text, &c).lines[0].runs[0].face.is_bold()
    };
    // 17: bold Latin letters and bold complex script letters.
    assert!(bold_of(17, "a") && bold_of(17, "א"));
    // 16: plain Latin letters and bold complex script letters. Reading 17 as unknown and drawing plain is the naive answer.
    assert!(!bold_of(16, "a") && bold_of(16, "א"));
    // 1: bold Latin, plain complex script.
    assert!(bold_of(1, "a") && !bold_of(1, "א"));
    // 32 is italic for complex script.
    let mut c = cf("Fam", 12.0);
    c.style = 32;
    assert!(lay(&db, "א", &c).lines[0].runs[0].face.is_italic());
    assert!(!lay(&db, "a", &c).lines[0].runs[0].face.is_italic());
}

#[test]
fn bit_a_family_with_no_bold_is_emboldened_and_one_with_a_bold_is_not() {
    let plain = db(&[SynthFont::new("Plain").ascii(500)]);
    let mut c = cf("Plain", 20.0);
    c.style = style::BOLD | style::ITALIC;
    let r = &lay(&plain, "a", &c).lines[0].runs[0];
    assert!(r.embolden > 0.0 && r.embolden < pt(20.0) * 0.1, "{}", r.embolden);
    assert!(r.slant > 0.0);
    close(r.slant, (12.0f64).to_radians().tan());
    let real = fam_db();
    let mut c = cf("Fam", 20.0);
    c.style = style::BOLD;
    let r = &lay(&real, "a", &c).lines[0].runs[0];
    assert_eq!(r.embolden, 0.0);
}

#[test]
fn bit_small_capitals_draw_lower_case_as_capitals_at_80_percent_unless_case_is_1() {
    let db = fam_db();
    let mut c = cf("Fam", 10.0);
    c.style = style::SMALL_CAPS;
    let laid = lay(&db, "aB", &c);
    let glyphs: Vec<_> = laid.lines[0].runs.iter().flat_map(|r| r.glyphs.iter().map(move |g| (r.size, g.gid))).collect();
    let a_cap = u32::from(fam("Regular").gid('A'));
    let b_cap = u32::from(fam("Regular").gid('B'));
    assert_eq!(glyphs[0].1, a_cap);
    close(glyphs[0].0, pt(8.0));
    assert_eq!(glyphs[1].1, b_cap);
    close(glyphs[1].0, pt(10.0));
    // Where bit 8 meets Case 1, all draw full size.
    c.case = 1;
    let laid = lay(&db, "aB", &c);
    assert!(laid.lines[0].runs.iter().all(|r| (r.size - pt(10.0)).abs() < 1e-9));
}

#[test]
fn decor_a_double_underline_beats_the_underline_bit_and_draws_two_lines_not_three() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.style = style::UNDERLINE;
    c.double_underline = true;
    let laid = lay(&db, "ab", &c);
    let under: Vec<_> = laid.lines[0].decorations.iter().filter(|d| d.top < laid.lines[0].baseline).collect();
    assert_eq!(under.len(), 2);
    assert!(under.iter().all(|d| d.kind == DecorationKind::DoubleUnderline));
    // Two lines sit one thickness apart.
    let (a, b) = (under[0], under[1]);
    close((a.top - b.top).abs(), 2.0 * a.thickness);
}

#[test]
fn decor_a_strikethrough_takes_the_runs_colour_and_see_through_level() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.strikethrough = true;
    c.color = Rgba { r: 255, g: 0, b: 0, a: 255 };
    c.transparency = 0.5;
    let laid = lay(&db, "ab", &c);
    let d = laid.lines[0].decorations.iter().find(|d| d.kind == DecorationKind::Strikethrough).expect("a strikethrough");
    assert_eq!((d.color.r, d.color.g, d.color.b), (255, 0, 0));
    close(d.opacity, 0.5);
    assert!(d.top > laid.lines[0].baseline, "through the letters, above the baseline");
    // A double strikeout beats the single one: two lines.
    c.double_strikethrough = true;
    let laid = lay(&db, "ab", &c);
    assert_eq!(laid.lines[0].decorations.iter().filter(|d| d.kind == DecorationKind::DoubleStrikethrough).count(), 2);
    assert!(laid.lines[0].decorations.iter().all(|d| d.kind != DecorationKind::Strikethrough));
}

#[test]
fn decor_the_position_and_thickness_come_from_the_faces_tables() {
    let mut a = SynthFont::new("Under A").ascii(500);
    a.underline = (-100, 50);
    let mut b = SynthFont::new("Under B").ascii(500);
    b.underline = (-250, 80);
    let db = db(&[a, b]);
    let size = 100.0;
    for (name, top, thick) in [("Under A", -0.10, 0.05), ("Under B", -0.25, 0.08)] {
        let mut c = cf(name, size);
        c.style = style::UNDERLINE;
        let laid = lay(&db, "ab", &c);
        let d = laid.lines[0].decorations.first().expect("underline");
        close(d.top - laid.lines[0].baseline, top * pt(size));
        close(d.thickness, thick * pt(size));
    }
}

#[test]
fn overline_is_a_line_above_the_run_and_spans_its_width() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.overline = true;
    let laid = lay(&db, "ab", &c);
    let d = laid.lines[0].decorations.iter().find(|d| d.kind == DecorationKind::Overline).expect("overline");
    assert!(d.top > laid.lines[0].baseline + pt(10.0), "above the letters");
    close(d.x1 - d.x0, pt(20.0));
    close(d.x0, laid.lines[0].x);
}

#[test]
fn a_decoration_covers_a_run_and_not_the_spaces_that_end_the_line() {
    let db = fam_db();
    let mut c = cf("Fam", 20.0);
    c.style = style::UNDERLINE;
    let laid = lay(&db, "ab   ", &c);
    let d = laid.lines[0].decorations.first().expect("underline");
    close(d.x1 - d.x0, pt(20.0));
}
