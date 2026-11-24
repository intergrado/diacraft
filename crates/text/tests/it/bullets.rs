//! Bullets.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, ParaFormat, Rgba, lay_out};

use crate::{cf, close, db, pt, run};

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn text_face() -> SynthFont {
    SynthFont::new("Em").ascii(1000)
}

/// A face with the marks the last column names, 1,000 units wide each.
fn marks(name: &str) -> SynthFont {
    SynthFont::new(name).chars("\u{2022}\u{25AA}\u{25A1}\u{2756}\u{27A2}\u{2713}", 1000)
}

fn bullet_para(bullet: u8) -> ParaFormat {
    ParaFormat { bullet, halign: 0, ..ParaFormat::default() }
}

fn lay(db: &FontDb, runs: &[diacraft_text::Run]) -> Laid {
    lay_out(runs, &block(), Some(5.0), db)
}

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn glyph_in(laid: &Laid, line: usize) -> (u32, String) {
    let b = laid.lines[line].bullet.as_ref().expect("a bullet");
    (b.glyphs[0].gid, b.face.family().to_owned())
}

#[test]
fn bul_a_table_face_absent_draws_the_last_column_in_a_stand_in() {
    // No `Symbol` on the machine: U+2022 in a face that has it. Drawing the code 0xB7 as a Latin-1 letter is the naive answer.
    let m = marks("Marks");
    let db = db(&[text_face(), m.clone()]);
    let laid = lay(&db, &[run("a", &tenth(), &bullet_para(1))]);
    assert_eq!(glyph_in(&laid, 0), (u32::from(m.gid('\u{2022}')), "Marks".to_owned()));
    for (bullet, ch) in [(3u8, '\u{25AA}'), (4, '\u{25A1}'), (5, '\u{2756}'), (6, '\u{27A2}'), (7, '\u{2713}')] {
        let laid = lay(&db, &[run("a", &tenth(), &bullet_para(bullet))]);
        assert_eq!(glyph_in(&laid, 0).0, u32::from(m.gid(ch)), "bullet {bullet}");
    }
}

#[test]
fn bul_a_table_face_present_gives_its_own_code() {
    // A `Symbol` face on the machine, whose bullet is at its private-use code point for 0xB7.
    let symbol = SynthFont::new("Symbol").chars("\u{F0B7}", 1000);
    let db = db(&[text_face(), marks("Marks"), symbol.clone()]);
    let laid = lay(&db, &[run("a", &tenth(), &bullet_para(1))]);
    assert_eq!(glyph_in(&laid, 0), (u32::from(symbol.gid('\u{F0B7}')), "Symbol".to_owned()));
}

#[test]
fn bul_bullet_two_is_a_lower_case_o_in_a_monospace_face_and_not_a_diamond() {
    let mono = SynthFont::new("Liberation Mono").ascii(600).chars("\u{25C6}", 600);
    let db = db(&[text_face(), mono.clone()]);
    let laid = lay(&db, &[run("a", &tenth(), &bullet_para(2))]);
    assert_eq!(glyph_in(&laid, 0), (u32::from(mono.gid('o')), "Liberation Mono".to_owned()));
    // Where the named face is there, the same letter.
    let courier = SynthFont::new("Courier New").ascii(600);
    let db = crate::db(&[text_face(), courier.clone()]);
    let laid = lay(&db, &[run("a", &tenth(), &bullet_para(2))]);
    assert_eq!(glyph_in(&laid, 0), (u32::from(courier.gid('o')), "Courier New".to_owned()));
}

#[test]
fn bul_a_string_replaces_the_mark_and_a_font_the_face() {
    let some = SynthFont::new("Some Face").ascii(500);
    let other = SynthFont::new("Other Face").ascii(500);
    let db = db(&[some.clone(), other.clone(), marks("Marks")]);
    let c = CharFormat { size: 0.1, ..cf("Other Face", 0.0) };
    let p = ParaFormat { bullet: 1, bullet_string: ">>".into(), bullet_font: "Some Face".into(), halign: 0, ..ParaFormat::default() };
    let laid = lay(&db, &[run("a", &c, &p)]);
    let b = laid.lines[0].bullet.as_ref().expect("bullet");
    assert_eq!(b.face.family(), "Some Face");
    assert_eq!(b.glyphs.iter().map(|g| g.gid).collect::<Vec<_>>(), vec![u32::from(some.gid('>')); 2]);
    // BulletFont 0: the first character's face.
    let p = ParaFormat { bullet_font: "0".into(), ..p };
    assert_eq!(lay(&db, &[run("a", &c, &p)]).lines[0].bullet.as_ref().expect("bullet").face.family(), "Other Face");
    // Bullet 0 with a string: no bullet.
    let p = ParaFormat { bullet: 0, ..p };
    assert!(lay(&db, &[run("a", &c, &p)]).lines[0].bullet.is_none());
    // An empty string takes the table's mark.
    let p = ParaFormat { bullet: 1, bullet_string: String::new(), bullet_font: String::new(), halign: 0, ..ParaFormat::default() };
    assert!(lay(&db, &[run("a", &c, &p)]).lines[0].bullet.is_some());
}

#[test]
fn bul_the_bullet_size_and_where_the_text_starts() {
    let m = marks("Marks");
    let db = db(&[text_face(), m]);
    let c = CharFormat { size: pt(10.0), ..cf("Em", 0.0) };
    let size_of = |bullet_size: f64| {
        let p = ParaFormat { bullet_size, ..bullet_para(1) };
        lay(&db, &[run("a", &c, &p)]).lines[0].bullet.as_ref().expect("bullet").size
    };
    close(size_of(-0.8), pt(8.0));
    close(size_of(0.25), pt(18.0));
    close(size_of(-1.0), pt(10.0));
    close(size_of(0.0), pt(10.0));
    // The text starts 0.25 inch from the bullet's start when TextPosAfterBullet is 0 or less, and where it says otherwise.
    let start_of = |gap: f64| {
        let p = ParaFormat { text_pos_after_bullet: gap, ..bullet_para(1) };
        let laid = lay(&db, &[run("a", &c, &p)]);
        let bullet_x = laid.lines[0].bullet.as_ref().expect("bullet").glyphs[0].x;
        (laid.lines[0].runs[0].glyphs[0].x - bullet_x, laid.lines[0].x)
    };
    close(start_of(0.0).0, 0.25);
    close(start_of(-0.3).0, 0.25);
    close(start_of(0.5).0, 0.5);
    // 0.001 is less than the bullet is wide (10 points), so the text follows the bullet at once.
    close(start_of(0.001).0, pt(10.0));
}

#[test]
fn bul_the_bullet_takes_the_colour_of_the_paragraphs_first_run() {
    let db = db(&[text_face(), marks("Marks")]);
    let green = CharFormat { color: Rgba { r: 0, g: 255, b: 0, a: 255 }, size: 0.1, ..cf("Em", 0.0) };
    let red = CharFormat { color: Rgba { r: 255, g: 0, b: 0, a: 255 }, size: 0.1, ..cf("Em", 0.0) };
    let laid = lay(&db, &[run("ab", &green, &bullet_para(1)), run("cd", &red, &bullet_para(1))]);
    let b = laid.lines[0].bullet.as_ref().expect("bullet");
    assert_eq!((b.color.r, b.color.g, b.color.b), (0, 255, 0));
}

#[test]
fn bullet_every_paragraph_of_a_run_gets_its_own_bullet() {
    let db = db(&[text_face(), marks("Marks")]);
    let laid = lay(&db, &[run("a\nb\n", &tenth(), &bullet_para(1))]);
    assert_eq!(laid.lines.len(), 2);
    assert!(laid.lines.iter().all(|l| l.bullet.is_some()));
}

#[test]
fn only_a_paragraphs_first_line_has_a_bullet_and_the_first_line_alone_is_pushed_in() {
    let db = db(&[text_face(), marks("Marks")]);
    let p = ParaFormat { ind_left: 0.5, ind_first: -0.5, text_pos_after_bullet: 0.5, ..bullet_para(1) };
    let laid = lay_out(&[run("aaaa bbbb cccc dddd", &tenth(), &p)], &block(), Some(1.2), &db);
    assert!(laid.lines.len() >= 2);
    assert!(laid.lines[0].bullet.is_some() && laid.lines[1].bullet.is_none());
    // A hanging bullet: the mark at the margin, the first line's text where the later lines' text is.
    let bullet_x = laid.lines[0].bullet.as_ref().expect("bullet").glyphs[0].x;
    close(bullet_x, 0.0);
    close(laid.lines[0].runs[0].glyphs[0].x, 0.5);
    close(laid.lines[1].x, 0.5);
}

#[test]
fn a_bullet_above_seven_with_no_string_draws_none_and_one_with_a_string_draws_the_string() {
    let db = db(&[text_face()]);
    let p = ParaFormat { bullet: 9, halign: 0, ..ParaFormat::default() };
    assert!(lay(&db, &[run("a", &tenth(), &p)]).lines[0].bullet.is_none());
    let p = ParaFormat { bullet_string: "*".into(), ..p };
    assert!(lay(&db, &[run("a", &tenth(), &p)]).lines[0].bullet.is_some());
}

#[test]
fn a_bullet_no_face_can_draw_is_a_missing_glyph_box_and_never_a_failure() {
    let db = db(&[text_face()]);
    let laid = lay(&db, &[run("a", &tenth(), &bullet_para(5))]);
    assert_eq!(glyph_in(&laid, 0).0, 0);
    // The box is drawn for a bullet as for a letter, and the layout says so the same way.
    assert_eq!(laid.notes, vec![diacraft_text::Note::MissingGlyphs { clusters: 1 }]);
    // A bullet a face can draw says nothing.
    let marks = db_with_marks();
    assert!(lay(&marks, &[run("a", &tenth(), &bullet_para(1))]).notes.is_empty());
}

fn db_with_marks() -> FontDb {
    db(&[text_face(), marks("Marks")])
}

#[test]
fn bullet_paragraphs_use_their_text_when_the_bullet_is_empty_line() {
    let db = db(&[text_face(), marks("Marks")]);
    let laid = lay(&db, &[run("\n", &tenth(), &bullet_para(1))]);
    assert_eq!(laid.lines.len(), 1);
}

#[test]
fn textposafterbullet_a_gap_on_a_paragraph_with_no_bullet_changes_nothing() {
    let db = db(&[text_face(), marks("Marks")]);
    let plain = ParaFormat { bullet: 0, text_pos_after_bullet: 0.5, halign: 0, ..ParaFormat::default() };
    let laid = lay(&db, &[run("a", &tenth(), &plain)]);
    assert!(laid.lines[0].bullet.is_none());
    close(laid.lines[0].runs[0].glyphs[0].x, 0.0);
    close(laid.lines[0].x, 0.0);
}
