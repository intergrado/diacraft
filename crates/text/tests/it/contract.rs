//! What Task 8 reads off `Laid`, pinned where a mutation run found it loose: the source range of each glyph, how marks move with
//! `FontScale`, which size a script's letters draw at, where a soft hyphen is drawn, and the direction of a cluster that holds two.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_text::{BlockFormat, CharFormat, Laid, PlacedGlyph, lay_out};

use crate::{cf, close, db, em_db, glyph_count, lay_text, left, run};

fn top_block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(2.0), ..BlockFormat::default() }
}

fn tenth() -> CharFormat {
    CharFormat { size: 0.1, ..cf("Em", 0.0) }
}

fn glyphs(laid: &Laid) -> Vec<&PlacedGlyph> {
    laid.lines.iter().flat_map(|l| l.runs.iter().flat_map(|r| r.glyphs.iter())).collect()
}

#[test]
fn each_glyph_says_which_source_characters_it_is_for() {
    // `chars` are indexes into the text of all the runs, counting every character and not only the drawn ones.
    let laid = lay_text(&em_db(), "ab", &tenth(), &left(), &top_block(), None);
    let ranges: Vec<_> = glyphs(&laid).iter().map(|g| g.chars.clone()).collect();
    assert_eq!(ranges, vec![0..1, 1..2]);
    // A letter and its marks are one cluster: each glyph of it is for all three characters.
    let marks = db(&[SynthFont::new("Em").ascii(1000).chars("\u{0301}\u{0302}", 0)]);
    let laid = lay_text(&marks, "xe\u{0301}\u{0302}y", &tenth(), &left(), &top_block(), None);
    let ranges: Vec<_> = glyphs(&laid).iter().map(|g| g.chars.clone()).collect();
    assert_eq!(ranges.first(), Some(&(0..1)));
    assert_eq!(ranges.iter().filter(|r| **r == (1..4)).count(), 3, "{ranges:?}");
    assert_eq!(ranges.last(), Some(&(4..5)));
    // A capital that is longer than its letter is still for the one letter.
    let cases = db(&[SynthFont::new("Em").ascii(1000).chars("ß", 1000)]);
    let c = CharFormat { case: 1, ..tenth() };
    let laid = lay_text(&cases, "aß", &c, &left(), &top_block(), None);
    let ranges: Vec<_> = glyphs(&laid).iter().map(|g| g.chars.clone()).collect();
    assert_eq!(ranges, vec![0..1, 1..2, 1..2]);
}

#[test]
fn the_source_range_of_a_glyph_counts_on_across_lines_paragraphs_and_tabs() {
    let laid = lay_text(&em_db(), "ab\ncd\tef", &tenth(), &left(), &top_block(), None);
    let by_line: Vec<Vec<_>> = laid.lines.iter().map(|l| l.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.chars.clone())).collect()).collect();
    assert_eq!(by_line, vec![vec![0..1, 1..2], vec![3..4, 4..5, 6..7, 7..8]]);
    assert_eq!(laid.lines[1].chars, 3..8);
}

#[test]
fn a_mark_moves_by_the_shapers_offsets_and_font_scale_stretches_only_across() {
    // The bundled face puts the acute of `q` back over it and up. FontScale is horizontal: twice the scale doubles the way across, in
    // the offset and in the advance, and leaves the height as it was.
    let place = |scale: f64| {
        let c = CharFormat { font: "Segoe UI".into(), size: 1.0, scale, ..CharFormat::default() };
        let laid = lay_text(&FontDb::bundled(), "q\u{0301}", &c, &left(), &top_block(), None);
        let base = laid.lines[0].baseline;
        let g = glyphs(&laid);
        assert_eq!(g.len(), 2);
        (g[1].x - g[0].x, g[1].y - base, g[0].advance)
    };
    let (dx1, dy1, adv1) = place(1.0);
    let (dx2, dy2, adv2) = place(2.0);
    assert!(dx1.abs() > 1e-3 && dy1 > 1e-3, "the mark is offset both ways: {dx1} {dy1}");
    close(adv2, 2.0 * adv1);
    close(dx2, 2.0 * dx1);
    close(dy2, dy1);
}

#[test]
fn a_hyphen_is_drawn_once_where_the_line_was_cut_and_not_at_every_soft_hyphen_before_it() {
    // `aa` `bb` `cc` with a soft hyphen after each pair, letters 0.1 wide, room for 0.55: the line takes `aabb`, then the hyphen.
    let laid = lay_text(&em_db(), "aa\u{00AD}bb\u{00AD}cc", &tenth(), &left(), &top_block(), Some(0.55));
    assert_eq!(laid.lines.len(), 2);
    assert_eq!(glyph_count(&laid.lines[0]), 5, "four letters and one hyphen");
    assert_eq!(glyph_count(&laid.lines[1]), 2);
}

#[test]
fn a_script_draws_at_the_size_its_own_cell_gives_and_the_asian_letters_at_the_plain_size() {
    // Size 0.1, ComplexScriptSize 0.2: Hebrew is twice the size, Han is not.
    let d = db(&[SynthFont::new("Em").ascii(1000), SynthFont::new("Han").chars("中", 1000), SynthFont::new("Heb").chars("א", 1000)]);
    let c =
        CharFormat { size: 0.1, complex_size: 0.2, font: "Em".into(), asian_font: "Han".into(), complex_font: "Heb".into(), ..CharFormat::default() };
    let width = |text: &str| lay_text(&d, text, &c, &left(), &top_block(), None).lines[0].width;
    close(width("中"), 0.1);
    close(width("א"), 0.2);
    close(width("a"), 0.1);
}

#[test]
fn a_cluster_that_holds_two_directions_is_shaped_in_the_direction_of_its_first_letter() {
    // U+0600, an Arabic number sign, is a prepended mark: it and the letter after it are one cluster, and it is a number (level 2) where
    // the Hebrew letter after it is level 1. Shaped as the number reads, left to right, its glyph comes first.
    let f = SynthFont::new("Mixed").ascii(1000).chars("\u{0600}א", 1000);
    let d = db(std::slice::from_ref(&f));
    let c = CharFormat { size: 0.1, font: "Mixed".into(), complex_font: "Mixed".into(), ..CharFormat::default() };
    let laid = lay_out(&[run("x\u{0600}א", &c, &left())], &top_block(), None, &d);
    let ids: Vec<u32> = glyphs(&laid).iter().map(|g| g.gid).collect();
    let (sign, alef) = (u32::from(f.gid('\u{0600}')), u32::from(f.gid('א')));
    assert!(ids.windows(2).any(|w| w == [sign, alef]), "{ids:?}: the sign before the letter");
}
