//! A paragraph's bullet.
//!
//! `Bullet` 1 to 7 picks a mark from MS-VSDX's table, each a code in a face, and the faces named there (`Symbol`, `Wingdings`) are
//! symbol faces whose codes don't mean those letters anywhere else. So where the named face is on the machine its code is used, and
//! where it isn't the mark is the table's last column, a character, in whichever face has it. `Bullet` 2 is the letter `o`
//! in `Courier New` or a monospaced stand-in, not the diamond the table pictures. A `BulletStr` replaces the mark and a
//! `BulletFont` the face. The mark takes the colour of the paragraph's first run and its size is a distance, the size of the
//! first character, or a share of it. Where its text starts is in `place`.

use diacraft_fonts::{Face, ShapeParams};

use crate::formats::{Sanitizer, no_name, short};
use crate::limits::MAX_BULLET_CHARS;
use crate::model::{ParaFormat, Rgba};
use crate::shape::{FaceEntry, Shaping};

/// What the table gives for one `Bullet` value: the code, the face it is in, and the character MS-VSDX shows, which is what is
/// drawn where that face is missing.
struct TableMark {
    code: u32,
    face: &'static str,
    stand_in: char,
}

const TABLE: [TableMark; 7] = [
    TableMark { code: 0xB7, face: "Symbol", stand_in: '\u{2022}' },
    // The code in `Courier New` is the letter, and that is what the export writes, so the diamond is not drawn.
    TableMark { code: 0x6F, face: "Courier New", stand_in: 'o' },
    TableMark { code: 0xA7, face: "Wingdings", stand_in: '\u{25AA}' },
    TableMark { code: 0x71, face: "Wingdings", stand_in: '\u{25A1}' },
    TableMark { code: 0x76, face: "Wingdings", stand_in: '\u{2756}' },
    TableMark { code: 0xD8, face: "Wingdings", stand_in: '\u{27A2}' },
    TableMark { code: 0xFC, face: "Wingdings", stand_in: '\u{2713}' },
];

/// A resolved bullet: glyphs in a face at a size, with the width they take.
#[derive(Clone, Debug)]
pub(crate) struct BulletMark {
    pub face: usize,
    pub glyphs: Vec<(u32, f64)>,
    pub size: f64,
    pub width: f64,
    pub color: Rgba,
    pub opacity: f64,
}

/// What a paragraph's first cluster gives its bullet.
pub(crate) struct FirstText {
    /// The run of the paragraph's first character.
    pub run: u32,
    /// The size of its character, and the face entry that draws it.
    pub size: f64,
    pub face: usize,
}

/// The character a table code is in a face: where a symbol font keeps it (its private-use area), else the code itself, if the face has
/// a glyph for it.
fn code_char(face: &Face, code: u32) -> Option<char> {
    [0xF000 + code, code].into_iter().filter_map(char::from_u32).find(|c| face.glyph_for(*c) != 0)
}

/// Shape `text` in a face entry at `size` with nominal advances, as glyph ids and advances in inches, and whether the shaper could not
/// be used (the glyphs are then the cmap's).
fn mark_glyphs(entry: &FaceEntry, text: &str, size: f64) -> (Vec<(u32, f64)>, bool) {
    let upem = entry.face.units_per_em().max(1.0);
    let (shaped, fell_back) = entry.face.shape_checked(text, &ShapeParams::default());
    (shaped.iter().map(|g| (g.gid, f64::from(g.x_advance) / upem * size)).collect(), fell_back)
}

/// The bullet of a paragraph, or `None` if it has none. `fmt` is the paragraph's format.
pub(crate) fn resolve<'r>(sh: &mut Shaping<'_, 'r>, fmt: &'r ParaFormat, first: &FirstText, san: &mut Sanitizer) -> Option<BulletMark> {
    // `Bullet` 0 is no bullet, whatever the string says. Above 7 the table has nothing, and a string is drawn if there is one.
    if fmt.bullet == 0 {
        return None;
    }
    let string: String = fmt.bullet_string.chars().take(MAX_BULLET_CHARS).collect();
    if string.len() < fmt.bullet_string.len() {
        sh.bullet_cut = true;
    }
    let table = usize::from(fmt.bullet).checked_sub(1).and_then(|i| TABLE.get(i));
    if table.is_none() && string.is_empty() {
        return None;
    }
    // BulletFontSize: above 0 a distance, 0 the first character's size, below 0 a share of it.
    let raw = san.share(fmt.bullet_size, -1.0);
    let size = if raw > 0.0 {
        raw.min(crate::limits::MAX_LENGTH)
    } else if raw < 0.0 {
        -raw * first.size
    } else {
        first.size
    };
    let named = short(&fmt.bullet_font);
    let (face, text) = if !string.is_empty() {
        // A string replaces the mark, in the named face or the first character's.
        (if no_name(named) { first.face } else { sh.entry_for(named) }, string)
    } else {
        table_mark(sh, table?, named, first)
    };
    let entry = sh.entries.get(face)?.clone();
    let (glyphs, fell_back) = mark_glyphs(&entry, &text, size);
    if fell_back {
        sh.shaping_failed.insert(entry.face.id());
    }
    // A mark no face can draw is the missing-glyph box, and the layout says so like it does for text.
    if glyphs.iter().any(|g| g.0 == 0) {
        sh.missing_glyphs += 1;
    }
    let style = sh.styles.get(first.run as usize)?;
    let width = glyphs.iter().map(|g| g.1).sum();
    Some(BulletMark { face, glyphs, size, width, color: style.color, opacity: style.opacity })
}

/// The face and text of a table mark. The named face if the machine has one, and its code in it; otherwise the last column's
/// character, in the paragraph's first face if that has it, else any face that does.
fn table_mark<'r>(sh: &mut Shaping<'_, 'r>, mark: &TableMark, named: &'r str, first: &FirstText) -> (usize, String) {
    let wanted = if no_name(named) { mark.face } else { named };
    if sh.db.has_family(wanted) {
        let face = sh.entry_for(wanted);
        if let Some(ch) = sh.entries.get(face).and_then(|e| code_char(&e.face, mark.code)) {
            return (face, ch.to_string());
        }
    }
    // Bullet 2 is a letter, in a monospaced face: `Courier New`'s stand-in, never the face of the paragraph's text.
    if mark.stand_in == 'o' {
        return (sh.entry_for(mark.face), "o".to_owned());
    }
    let face = if sh.entries.get(first.face).is_some_and(|e| e.face.covers(mark.stand_in)) {
        first.face
    } else if let Some(f) = sh.db.covering(&[mark.stand_in]) {
        sh.entry_for_face(f)
    } else {
        first.face
    };
    (face, mark.stand_in.to_string())
}
