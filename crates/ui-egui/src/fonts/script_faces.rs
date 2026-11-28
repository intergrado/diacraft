//! Faces for the scripts the interface fonts may lack: Arabic and Hebrew.
//!
//! Hack and Ubuntu Light, the faces the window always carries, have no Arabic and no Hebrew, and nor have most of the open sans families the window prefers
//! (`UI_FAMILIES`). The window lays out the text a person types in egui's own faces, so without a face for these scripts the text box shows a box for each letter
//! (it does not panic: egui draws the missing-glyph box of the family's first face, which is `MissingGlyphPolicy::Tofu`, eframe's default). The drawing is not
//! affected, since the renderer picks a face for each letter from the machine's faces itself.
//!
//! For each script this adds **one** face behind the proportional faces, taken from the same database the interface family comes from: first the face the drawing
//! falls back on for the script's letters ([`FontDb::covering`]), then, when that face is not one egui may take, the first of a short list of open families
//! that is. A face is taken by the checks the interface faces pass (`safe_interface_outlines`: a static TrueType face whose every glyph passes the bounded path
//! sink, within the size and count budgets), and its bytes are capped before they are copied. A script no admitted face has is left out, and the box shows boxes for it
//! as before. This runs where `font_definitions_with` runs, on the worker that prepares the interface fonts, never on the thread of the window.

use diacraft_fonts::{Face, FontDb, FontStyle, MatchKind};
use egui::{FontDefinitions, FontFamily};

use super::{LOGOS, MAX_UI_FACE_BYTES, insert_face, safe_interface_outlines, strong_family};

/// A script the box must be able to show, the letters a face has to have for it, and open families to try when the database's own choice is not admitted.
struct Script {
    name: &'static str,
    letters: &'static [char],
    families: &'static [&'static str],
}

const SCRIPTS: [Script; 2] = [
    Script {
        name: "Arabic",
        // Alef, beh, lam, meem, yeh.
        letters: &['\u{627}', '\u{628}', '\u{644}', '\u{645}', '\u{64A}'],
        families: &["Noto Sans Arabic", "Noto Naskh Arabic", "DejaVu Sans", "Amiri", "Scheherazade New", "Vazirmatn", "Droid Arabic Kufi"],
    },
    Script {
        name: "Hebrew",
        // Alef, bet, gimel, lamed, shin.
        letters: &['\u{5D0}', '\u{5D1}', '\u{5D2}', '\u{5DC}', '\u{5E9}'],
        families: &["Noto Sans Hebrew", "DejaVu Sans", "Liberation Sans", "Noto Serif Hebrew", "Heebo", "Rubik", "Assistant"],
    },
];

/// Whether egui may take `face` for text in `script`: upright, of ordinary weight, with the script's letters, and passing the checks of the interface faces.
fn admitted(face: &Face, script: &Script) -> bool {
    !face.is_null()
        && !face.is_broken()
        && !face.is_italic()
        && (300..=600).contains(&face.weight())
        && face.data().len() <= MAX_UI_FACE_BYTES
        && script.letters.iter().all(|&letter| face.glyph_for(letter) != 0)
        && LOGOS.iter().all(|&logo| face.glyph_for(logo) == 0)
        && safe_interface_outlines(face)
}

/// The face for `script` from `db`: the one the drawing falls back on if egui may take it, else the first of the open families that has the letters and may be taken.
fn face_for(db: &FontDb, script: &Script) -> Option<Face> {
    if let Some(face) = db.covering(script.letters).filter(|face| admitted(face, script)) {
        return Some(face);
    }
    script.families.iter().find_map(|family| {
        // Names a drawing might use for another family are not asked for: a name that is not on the machine gets a stand-in, which is never taken.
        if !db.has_family(family) {
            return None;
        }
        let resolved = db.resolve(family, FontStyle { bold: false, italic: false });
        (resolved.kind == MatchKind::Named && resolved.face.family().eq_ignore_ascii_case(family) && admitted(&resolved.face, script))
            .then_some(resolved.face)
    })
}

/// Puts a face for each script of [`SCRIPTS`] behind the proportional faces and the strong ones, where `db` has one egui may take. A face that has both scripts is
/// copied once.
pub(super) fn add_script_faces(db: &FontDb, definitions: &mut FontDefinitions) {
    let mut added: Vec<Face> = Vec::new();
    for script in &SCRIPTS {
        // A face already added that has the script's letters serves it too.
        if added.iter().any(|face| script.letters.iter().all(|&letter| face.glyph_for(letter) != 0)) {
            continue;
        }
        let Some(face) = face_for(db, script) else { continue };
        let name = format!("{}-Script-{}", script.name, face.family());
        insert_face(definitions, &name, &face);
        for family in [FontFamily::Proportional, strong_family()] {
            definitions.families.entry(family).or_default().push(name.clone());
        }
        added.push(face);
    }
}
