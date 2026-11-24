//! Two limits of the covering search that need a lot of setup to reach: how many answers it remembers, and the biggest file it will load
//! just to see whether it has a glyph.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;

#[test]
fn the_search_remembers_4096_answers_and_not_a_4097th() {
    let mut db = FontDb::empty();
    db.add_font_data(SynthFont::new("Plain").ascii(500).build());
    let letter = |i: u32| char::from_u32(0x4E00 + i).unwrap();
    // 4,100 different letters no face has, each asked for twice: the first 4,096 are remembered and so searched once, the last four
    // twice.
    for _ in 0..2 {
        for i in 0..4100 {
            assert!(db.covering(&[letter(i)]).is_none());
        }
    }
    assert_eq!(db.cover_searches(), 4100 + 4);
}
