//! The synthetic faces other tests lean on must say exactly what they were built to say.

use diacraft_fonts::synth::{ScriptFields, SynthFont};

use crate::{REGULAR, only};

#[test]
fn a_built_face_reads_back_as_it_was_described() {
    let font = SynthFont::new("Test Sans").ascii(500).advance('W', 900);
    let db = only(std::slice::from_ref(&font));
    let face = db.pick("Test Sans", REGULAR);
    assert_eq!(face.family(), "Test Sans");
    assert_eq!(face.style_name(), "Regular");
    assert_eq!(face.units_per_em(), 1000.0);
    assert!(!face.is_bold() && !face.is_italic());
    assert_eq!(face.advance(face.glyph_for('a')), 500.0);
    assert_eq!(face.advance(face.glyph_for('W')), 900.0);
    assert_eq!(face.glyph_for('a'), u32::from(font.gid('a')));
    assert!(face.covers('a') && !face.covers('中'));
    // Not in the cmap: glyph 0, the .notdef box, which is 600 wide.
    assert_eq!(face.glyph_for('中'), 0);
    assert_eq!(face.advance(0), 600.0);
}

#[test]
fn outlines_are_boxes_and_a_space_has_none() {
    let db = only(&[SynthFont::new("Boxy").ascii(400)]);
    let face = db.pick("Boxy", REGULAR);
    let path = face.outline(face.glyph_for('a')).expect("a has an outline");
    let bounds = kurbo::Shape::bounding_box(&path);
    assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (0.0, 0.0, 300.0, 700.0));
    assert!(face.outline(face.glyph_for(' ')).is_none());
}

#[test]
fn script_boxes_and_lines_come_from_the_tables() {
    let mut font = SynthFont::new("Tables").ascii(500);
    font.subscript = ScriptFields { x_size: 650, y_size: 600, x_offset: 0, y_offset: 140 };
    font.superscript = ScriptFields { x_size: 650, y_size: 650, x_offset: 0, y_offset: 477 };
    font.underline = (-120, 60);
    font.strikeout = (40, 250);
    let db = only(&[font]);
    let face = db.pick("Tables", REGULAR);
    let sub = face.script_box(false).expect("subscript box");
    let sup = face.script_box(true).expect("superscript box");
    assert_eq!((sub.size, sub.offset), (0.6, 0.14));
    assert_eq!((sup.size, sup.offset), (0.65, 0.477));
    let underline = face.underline().expect("underline");
    assert_eq!((underline.top, underline.thickness), (-0.12, 0.06));
    let strike = face.strikeout().expect("strikeout");
    assert_eq!((strike.top, strike.thickness), (0.25, 0.04));
}

#[test]
fn a_script_box_holding_zero_counts_as_absent() {
    let mut font = SynthFont::new("No Boxes").ascii(500);
    font.subscript = ScriptFields::default();
    font.superscript = ScriptFields::default();
    let db = only(&[font]);
    let face = db.pick("No Boxes", REGULAR);
    assert!(face.script_box(false).is_none() && face.script_box(true).is_none());
}

#[test]
fn style_names_set_bold_and_italic() {
    let db = only(&[SynthFont::new("Styled").ascii(500).styled("Bold Italic")]);
    let face = db.pick("Styled", REGULAR);
    assert!(face.is_bold() && face.is_italic(), "weight {} style {}", face.weight(), face.style_name());
}

#[test]
fn an_outline_is_drawn_or_blank_or_broken_and_the_three_are_told_apart() {
    use diacraft_fonts::Outline;
    let font = SynthFont::new("Three Kinds").ascii(400);
    let db = only(std::slice::from_ref(&font));
    let face = db.pick("Three Kinds", REGULAR);
    assert!(matches!(face.outline_glyph(u32::from(font.gid('a'))), Outline::Drawn(_)));
    // A space is in the face and has nothing to draw: not an error.
    assert_eq!(face.outline_glyph(u32::from(font.gid(' '))), Outline::Blank);
    // A glyph number the face does not have is one a drawing cannot be right about.
    assert_eq!(face.outline_glyph(60_000), Outline::Broken);
    assert!(face.outline(60_000).is_none() && face.outline(u32::from(font.gid(' '))).is_none());
    assert!(!face.is_broken(), "a missing glyph is not a face that failed");
}

#[test]
fn a_face_may_be_embedded_unless_its_licence_bits_forbid_it() {
    // OS/2 fsType: 0 installable, 4 preview and print, 8 editable allow it; 2 (restricted licence) and 0x200 (bitmaps only) do not.
    for (fs_type, allowed) in [(0u16, true), (4, true), (8, true), (2, false), (0x200, false), (0x204, false), (0x0A, false)] {
        let mut font = SynthFont::new("Licensed").ascii(500);
        font.fs_type = fs_type;
        let face = only(std::slice::from_ref(&font)).pick("Licensed", REGULAR);
        assert_eq!(face.may_embed(), allowed, "fsType {fs_type:#x}");
    }
    // A face with nothing in it embeds nothing, and a bundled face (Hack's licence allows it) does.
    assert!(!diacraft_fonts::Face::null().may_embed());
    assert!(crate::bundled().pick("Hack", REGULAR).may_embed());
}
