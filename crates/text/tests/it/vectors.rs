//! Vectors of that the first version of this suite covered under other names or not at all, run here with
//! the specs' own numbers and named by their ids. `crates/text/VECTORS.md` lists every vector, and the ones no test of this crate runs.

use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, ParaFormat, lay_out};

use crate::{baselines, cf, close, db, em_db, lay_text, left, pt, run};

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(4.0), ..BlockFormat::default() }
}

fn ten() -> CharFormat {
    cf("Em", 10.0)
}

#[test]
fn spbefore_the_space_is_above_the_second_paragraph_and_not_the_first() {
    // 0.1388... inch is 10 points. At 10 points and SpLine -1.2 a line box is 12: the step is 12 and the 10 of space.
    let gap = ParaFormat { sp_before: 10.0 / 72.0, ..left() };
    let laid = lay_out(&[run("a\n", &ten(), &left()), run("b", &ten(), &gap)], &block(), Some(5.0), &em_db());
    let b = baselines(&laid);
    close(b[0] - b[1], pt(22.0));
    // On the first paragraph, nothing above it.
    let alone = lay_text(&em_db(), "a", &ten(), &gap, &block(), Some(5.0));
    let plain = lay_text(&em_db(), "a", &ten(), &left(), &block(), Some(5.0));
    close(baselines(&alone)[0], baselines(&plain)[0]);
    close(alone.size.height, plain.size.height);
}

#[test]
fn spbefore_a_root_of_six_points_gives_six_on_the_second_paragraph_and_zero_gives_none() {
    let six = ParaFormat { sp_before: 0.0833333333, ..left() };
    let laid = lay_out(&[run("a\n", &ten(), &left()), run("b", &ten(), &six)], &block(), Some(5.0), &em_db());
    let b = baselines(&laid);
    close(b[0] - b[1], pt(18.0));
    let none = lay_out(&[run("a\n", &ten(), &left()), run("b", &ten(), &left())], &block(), Some(5.0), &em_db());
    close(baselines(&none)[0] - baselines(&none)[1], pt(12.0));
}

#[test]
fn spafter_the_space_is_below_the_first_paragraph_and_not_the_last() {
    let gap = ParaFormat { sp_after: 10.0 / 72.0, ..left() };
    let laid = lay_out(&[run("a\n", &ten(), &gap), run("b", &ten(), &left())], &block(), Some(5.0), &em_db());
    let b = baselines(&laid);
    close(b[0] - b[1], pt(22.0));
    // One paragraph: nothing below it.
    let alone = lay_text(&em_db(), "a", &ten(), &gap, &block(), Some(5.0));
    let plain = lay_text(&em_db(), "a", &ten(), &left(), &block(), Some(5.0));
    close(alone.size.height, plain.size.height);
}

#[test]
fn spline_minus_one_is_the_largest_size_on_the_line_not_the_first_runs() {
    let p = ParaFormat { sp_line: -1.0, ..left() };
    let laid = lay_out(&[run("a", &ten(), &p), run("b", &cf("Em", 20.0), &p)], &block(), Some(5.0), &em_db());
    close(laid.lines[0].height, pt(20.0));
}

#[test]
fn indfirst_the_first_line_is_past_the_left_indent_and_the_second_at_it() {
    let p = ParaFormat { ind_first: 0.25, ..left() };
    let c = CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let laid = lay_text(&em_db(), "aaaa bbbb cccc dddd", &c, &p, &block(), Some(1.0));
    assert!(laid.lines.len() >= 2);
    close(laid.lines[0].x, 0.25);
    close(laid.lines[1].x, 0.0);
}

#[test]
fn indfirst_a_hanging_first_line_is_at_the_margin_and_the_rest_at_the_left_indent() {
    let p = ParaFormat { ind_first: -0.25, ind_left: 0.25, ..left() };
    let c = CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let laid = lay_text(&em_db(), "aaaa bbbb cccc dddd", &c, &p, &block(), Some(1.0));
    assert!(laid.lines.len() >= 2);
    close(laid.lines[0].x, 0.0);
    close(laid.lines[1].x, 0.25);
}

#[test]
fn indleft_every_line_is_in_from_the_margin_and_the_wrap_is_narrower() {
    let p = ParaFormat { ind_left: 0.5, ..left() };
    let c = CharFormat { size: 0.1, ..cf("Em", 0.0) };
    let laid = lay_text(&em_db(), "aaaa bbbb cccc", &c, &p, &block(), Some(1.0));
    // Room for 0.5: each word alone on its line, where a measure of 1.0 would hold two.
    assert_eq!(laid.lines.len(), 3);
    for line in &laid.lines {
        close(line.x, 0.5);
    }
}

#[test]
fn bulletfont_the_tables_code_is_drawn_in_the_named_face() {
    let some = SynthFont::new("Some Face").chars("\u{00B7}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), some.clone()]);
    let p = ParaFormat { bullet: 1, bullet_font: "Some Face".into(), ..left() };
    let laid = lay_text(&d, "a", &ten(), &p, &block(), Some(5.0));
    let bullet = laid.lines[0].bullet.as_ref().unwrap();
    assert_eq!(bullet.face.family(), "Some Face");
    assert_eq!(bullet.glyphs[0].gid, u32::from(some.gid('\u{00B7}')));
}

#[test]
fn bulletfont_a_zero_is_the_tables_own_face_and_not_the_roots_font() {
    let wingdings = SynthFont::new("Wingdings").chars("\u{F0A7}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), wingdings]);
    let p = ParaFormat { bullet: 3, bullet_font: "0".into(), ..left() };
    let laid = lay_text(&d, "a", &ten(), &p, &block(), Some(5.0));
    assert_eq!(laid.lines[0].bullet.as_ref().unwrap().face.family(), "Wingdings");
}

#[test]
fn bulletsize_a_distance_is_the_size_whatever_the_texts() {
    let marks = SynthFont::new("Marks").chars("\u{2022}", 1000);
    let d = db(&[SynthFont::new("Em").ascii(1000), marks]);
    let p = ParaFormat { bullet: 1, bullet_size: 12.0 / 72.0, ..left() };
    for text_size in [10.0, 24.0] {
        let laid = lay_text(&d, "a", &cf("Em", text_size), &p, &block(), Some(5.0));
        close(laid.lines[0].bullet.as_ref().unwrap().size, pt(12.0));
    }
}

#[test]
fn scale_a_scale_of_two_makes_the_letter_twice_as_wide_and_no_taller() {
    let c = CharFormat { scale: 2.0, ..ten() };
    let laid = lay_text(&em_db(), "a", &c, &left(), &block(), Some(5.0));
    close(laid.lines[0].width, pt(20.0));
    close(laid.lines[0].height, pt(12.0));
    let run = &laid.lines[0].runs[0];
    close(run.size, pt(10.0));
    close(run.x_scale, 2.0);
}

#[test]
fn complexscriptsize_a_root_of_minus_one_is_the_plain_size_and_minus_a_half_is_half_of_it() {
    let d = db(&[SynthFont::new("Em").ascii(1000), SynthFont::new("Heb").chars("א", 1000)]);
    let width = |complex_size: f64| {
        let c = CharFormat { complex_size, complex_font: "Heb".into(), ..ten() };
        lay_text(&d, "א", &c, &left(), &block(), Some(5.0)).lines[0].width
    };
    close(width(-1.0), pt(10.0));
    close(width(-0.5), pt(5.0));
}
