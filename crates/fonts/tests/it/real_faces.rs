//! The faces the machine has, which synthetic ones can't stand in for: what `covers` says against what the cmap gives, and what the
//! search for a fallback face answers. A test that finds none of the faces it wants says `SKIPPED` and why, on standard error.

use std::sync::OnceLock;

use diacraft_fonts::{Cover, Face, FontDb};

use crate::REGULAR;

fn machine() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::system_and_bundled)
}

/// Up to a dozen faces of different kinds: whichever of these families the machine has, and the bundled ones.
fn some_faces() -> Vec<Face> {
    let mut faces: Vec<Face> = Vec::new();
    for name in [
        "DejaVu Sans",
        "Noto Sans CJK JP",
        "Droid Sans Fallback",
        "FreeSans",
        "Liberation Sans",
        "Symbola",
        "Noto Sans Arabic",
        "Noto Sans Khmer",
        "Ubuntu",
        "Hack",
    ] {
        if machine().has_family(name) {
            faces.push(machine().pick(name, REGULAR));
        }
    }
    faces
}

#[test]
fn covers_is_what_the_cmap_says_on_every_code_point_of_the_basic_plane_and_a_sample_beyond() {
    let faces = some_faces();
    assert!(faces.len() >= 2, "the bundled faces at least");
    for face in &faces {
        for cp in (0..0x1_0000u32).chain((0x1_0000..0x3_1400).step_by(7)) {
            let Some(c) = char::from_u32(cp) else { continue };
            assert_eq!(face.covers(c), face.glyph_for(c) != 0, "{} U+{cp:04X}", face.family());
        }
    }
}

#[test]
fn what_the_search_finds_has_the_letters_and_is_the_same_whichever_faces_were_used_before() {
    let samples = ['中', '한', 'ก', 'ក', 'ب', 'א', 'न', 'é', 'я', 'Ω'];
    let cold = FontDb::system_and_bundled();
    let warm = FontDb::system_and_bundled();
    for face in some_faces() {
        let _ = warm.pick(face.family(), REGULAR).glyph_for('a');
    }
    let mut found = 0;
    for ch in samples {
        let (a, b) = (cold.covering_checked(&[ch]), warm.covering_checked(&[ch]));
        match (a, b) {
            (Cover::Found(x), Cover::Found(y)) => {
                assert!(x.covers(ch), "{} has {ch}", x.family());
                assert_eq!((x.family(), x.style_name()), (y.family(), y.style_name()), "{ch}");
                found += 1;
            }
            (Cover::Missing, Cover::Missing) => {}
            (a, b) => panic!("{ch}: {a:?} against {b:?}"),
        }
    }
    if found == 0 {
        eprintln!("SKIPPED the search: no face on this machine has any of the letters tried");
    }
}
