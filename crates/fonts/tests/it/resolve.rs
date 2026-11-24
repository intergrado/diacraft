//! Which face draws a name (the name, then its stand-in, then the default).

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, FontStyle, GenericClass, MatchKind, Origin};

use crate::{BOLD, ITALIC, REGULAR, bundled, only};

fn face(family: &str) -> SynthFont {
    SynthFont::new(family).ascii(500)
}

#[test]
fn a_name_the_machine_has_gets_its_own_face_whatever_the_case() {
    let db = only(&[face("Some Face")]);
    for name in ["Some Face", "some face", "SOME FACE", "  Some   Face  "] {
        let r = db.resolve(name, REGULAR);
        assert_eq!((r.face.family(), r.kind), ("Some Face", MatchKind::Named), "{name:?}");
    }
    assert!(db.has_family("sOmE fAcE"));
}

#[test]
fn a_missing_name_gets_the_stand_in_for_that_name() {
    // The name is kept by the caller, the glyphs come from the stand-in. The makers' pairs only.
    let db = only(&[face("Carlito"), face("Liberation Sans"), face("Liberation Serif"), face("Liberation Mono"), face("Caladea")]);
    for (asked, got) in [
        ("Calibri", "Carlito"),
        ("calibri light", "Carlito"),
        ("Arial", "Liberation Sans"),
        ("Times New Roman", "Liberation Serif"),
        ("Courier New", "Liberation Mono"),
        ("Cambria", "Caladea"),
    ] {
        let r = db.resolve(asked, REGULAR);
        assert_eq!((r.face.family(), r.kind), (got, MatchKind::StandIn), "{asked}");
    }
    assert!(!db.has_family("Calibri"), "a stand-in is not the name");
}

#[test]
fn a_name_present_beats_its_stand_in() {
    let db = only(&[face("Calibri"), face("Carlito")]);
    let r = db.resolve("Calibri", REGULAR);
    assert_eq!((r.face.family(), r.kind), ("Calibri", MatchKind::Named));
}

#[test]
fn an_unknown_name_gets_the_default_of_its_class() {
    let db = only(&[face("Carlito"), face("Caladea"), face("Liberation Serif"), face("Liberation Mono")]);
    // Sans is the default class; a serif-looking or monospace-looking name takes its own class's default. Liberation Serif comes
    // before Caladea for a serif name that has no twin, `mono` in a name is enough, and a name that says sans is never serif.
    let cases = [
        ("Segoe UI", "Carlito"),
        ("Garamond", "Liberation Serif"),
        ("Consolas", "Liberation Mono"),
        ("Lucida Sans Typewriter", "Liberation Mono"),
        ("DejaVu Sans Mono", "Liberation Mono"),
        ("MS Sans Serif", "Carlito"),
        ("Some Serif Face", "Liberation Serif"),
    ];
    for (asked, got) in cases {
        let r = db.resolve(asked, REGULAR);
        assert_eq!((r.face.family(), r.kind), (got, MatchKind::Default), "{asked}");
    }
}

#[test]
fn nothing_matches_so_a_bundled_face_draws_and_monospace_names_get_the_monospace_one() {
    let db = bundled();
    let sans = db.resolve("No Such Face Anywhere", REGULAR);
    let mono = db.resolve("Consolas", REGULAR);
    assert_eq!(sans.kind, MatchKind::Default);
    assert_eq!(mono.kind, MatchKind::Default);
    assert_eq!(mono.face.family(), "Hack");
    assert_ne!(sans.face.family(), "Hack", "a proportional name should not get the monospace face");
    assert!(sans.face.covers('a') && mono.face.covers('a'));
}

#[test]
fn no_font_specified_takes_the_default_and_never_names_a_face() {
    // An empty name or the number 0 says no font. The caller should have resolved it; if it arrives anyway
    // the default draws.
    let db = bundled();
    for name in ["", "0", "   ", " 0"] {
        let r = db.resolve(name, REGULAR);
        assert_eq!(r.kind, MatchKind::Default, "{name:?}");
        assert!(r.face.covers('a'));
    }
}

#[test]
fn the_default_family_is_the_one_a_run_with_no_face_is_drawn_in_whatever_the_style() {
    // The name a window can show for a run no cell gives a face. It is the family of the face `resolve("")` picks, bold or italic or neither,
    // so that what is shown is what is drawn.
    let sans = [face("Carlito"), face("Liberation Sans"), face("Arimo")];
    for (db, want) in
        [(only(&sans), "Carlito"), (only(&sans[1..]), "Liberation Sans"), (only(&sans[2..]), "Arimo"), (only(&[face("Some Face")]), "Some Face")]
    {
        assert_eq!(db.default_family().as_deref(), Some(want));
        for style in [REGULAR, BOLD, ITALIC] {
            assert_eq!(db.resolve("", style).face.family(), want, "{want}: the run is drawn in the family that is shown");
        }
    }
    // The bundled database answers with its sans face; one with no faces has no name to give.
    let bundled = bundled();
    assert_eq!(bundled.default_family().as_deref(), Some(bundled.resolve("", REGULAR).face.family()));
    assert_eq!(FontDb::empty().default_family(), None);
}

#[test]
fn a_face_called_0_is_not_what_the_number_0_asks_for() {
    // The number 0 says no font; a family that happens to be called "0" must not be taken for the name.
    let mut db = bundled();
    assert_eq!(db.add_font_data(face("0").build()).added, 1);
    assert!(db.has_family("0"));
    let r = db.resolve("0", REGULAR);
    assert_eq!(r.kind, MatchKind::Default);
    assert_ne!(r.face.family(), "0");
}

#[test]
fn a_face_equals_itself_and_its_clones_and_no_other_face() {
    let db = only(&[face("One"), face("Two")]);
    let one = db.pick("One", REGULAR);
    assert!(one == db.pick("One", REGULAR));
    assert!(one == one.clone());
    assert!(one != db.pick("Two", REGULAR));
}

#[test]
fn bold_and_italic_use_the_real_face_when_the_family_has_one() {
    let db = only(&[face("Fam"), face("Fam").styled("Bold"), face("Fam").styled("Italic"), face("Fam").styled("Bold Italic")]);
    let check = |style: FontStyle, bold: bool, italic: bool| {
        let r = db.resolve("Fam", style);
        assert_eq!((r.face.is_bold(), r.face.is_italic()), (bold, italic), "{style:?}");
        assert!(!r.synthetic_bold && !r.synthetic_italic, "{style:?}");
    };
    check(REGULAR, false, false);
    check(BOLD, true, false);
    check(ITALIC, false, true);
    check(FontStyle { bold: true, italic: true }, true, true);
}

#[test]
fn a_family_with_no_bold_asks_the_renderer_to_embolden() {
    // Outlines made heavier, no error. And slanted when there is no italic.
    let db = only(&[face("Plain")]);
    let r = db.resolve("Plain", BOLD);
    assert_eq!((r.synthetic_bold, r.synthetic_italic, r.face.is_bold()), (true, false, false));
    let r = db.resolve("Plain", ITALIC);
    assert_eq!((r.synthetic_bold, r.synthetic_italic), (false, true));
    let r = db.resolve("Plain", FontStyle { bold: true, italic: true });
    assert_eq!((r.synthetic_bold, r.synthetic_italic), (true, true));
    // The bundled faces are one weight each, so bold there is made up too.
    let r = bundled().resolve("anything", BOLD);
    assert!(r.synthetic_bold);
}

#[test]
fn bold_is_not_made_up_for_a_face_that_already_is() {
    let db = only(&[face("Only Bold").styled("Bold")]);
    let r = db.resolve("Only Bold", BOLD);
    assert!(!r.synthetic_bold && r.face.is_bold());
    // Asked for regular, the bold face is all there is, and nothing is undone.
    let r = db.resolve("Only Bold", REGULAR);
    assert!(r.face.is_bold() && !r.synthetic_bold);
}

#[test]
fn faces_the_caller_adds_come_before_bundled_and_system_ones() {
    // Origin order: Added, Craft, System, Bundled. Hack is bundled; an added face of the same name wins.
    let mut db = FontDb::bundled();
    let before = db.resolve("Hack", REGULAR);
    assert_eq!(before.face.family(), "Hack");
    let added = SynthFont::new("Hack").ascii(250).build();
    assert_eq!(db.add_font_data(added).added, 1);
    let after = db.resolve("Hack", REGULAR);
    assert_eq!(after.face.advance(after.face.glyph_for('a')), 250.0, "the added Hack should be the one drawn");
    assert!(Origin::Added < Origin::Craft && Origin::Craft < Origin::System && Origin::System < Origin::Bundled);
}

#[test]
fn an_empty_database_still_answers_with_a_face_that_draws_nothing() {
    let db = FontDb::empty();
    let r = db.resolve("Anything", REGULAR);
    assert!(r.face.is_null());
    assert_eq!(r.face.glyph_for('a'), 0);
    assert_eq!(r.face.advance(0), 500.0, "half an em, so a measure still moves on");
    assert!(r.face.shape("abc", &Default::default()).len() == 3);
    assert!(r.face.outline(0).is_none());
}

#[test]
fn class_from_panose_reads_the_digits() {
    // The number is a Latin text face with a rounded sans serif style and a modern proportion: sans.
    assert_eq!(GenericClass::from_panose(&[2, 15, 5, 2, 2, 2, 4, 3, 2, 4]), Some(GenericClass::Sans));
    assert_eq!(GenericClass::from_panose(&[2, 2, 6, 3, 5, 4, 5, 2, 3, 4]), Some(GenericClass::Serif));
    assert_eq!(GenericClass::from_panose(&[2, 11, 6, 9, 2, 2, 4, 3, 2, 4]), Some(GenericClass::Mono));
    assert_eq!(GenericClass::from_panose(&[5, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None, "a symbol family kind is not Latin text");
    // Serif styles 2 to 10 have serifs and 11 to 15 have none; 0 and 1 say nothing, so they read as sans.
    for (style, class) in
        [(1u8, GenericClass::Sans), (2, GenericClass::Serif), (10, GenericClass::Serif), (11, GenericClass::Sans), (15, GenericClass::Sans)]
    {
        assert_eq!(GenericClass::from_panose(&[2, style, 5, 3, 0, 0, 0, 0, 0, 0]), Some(class), "serif style {style}");
    }
}

#[test]
fn a_face_with_a_legacy_family_name_is_found_by_it_and_by_the_typographic_one() {
    // A family of many weights has a legacy name (ID 1) for each of four styles and a typographic one (ID 16) for them all.
    let mut display = face("Fam Display");
    display.legacy_family = Some("Fam".to_owned());
    let db = only(&[display, face("Other")]);
    assert_eq!(db.families(), vec!["Fam Display".to_owned(), "Other".to_owned()], "listed by the typographic name only");
    for name in ["Fam Display", "fam display", "Fam", "FAM", "  fam "] {
        let r = db.resolve(name, REGULAR);
        assert_eq!((r.face.family(), r.kind), ("Fam Display", MatchKind::Named), "{name:?}");
        assert!(db.has_family(name));
    }
    assert!(!db.has_family("Fam Display Black"));
    // A legacy name that differs only by case is the same name, found once.
    let mut shout = face("Quiet");
    shout.legacy_family = Some("QUIET".to_owned());
    let db = only(&[shout]);
    assert_eq!(db.families(), vec!["Quiet".to_owned()]);
    assert_eq!(db.resolve("quiet", REGULAR).kind, MatchKind::Named);
    assert_eq!(db.face_count(), 1);
}

#[test]
fn a_name_is_taken_for_monospaced_by_its_words_and_not_by_letters_inside_them() {
    let db = bundled();
    for name in ["Foo Mono", "DejaVu Sans Mono", "Courier10 BT", "Source Code Pro", "Lucida Console", "Fixedsys"] {
        assert_eq!(db.pick(name, REGULAR).family(), "Hack", "{name}");
    }
    // `mono` in Monotype, `code` in Encoded, `fixed` in Prefixed: not the word.
    for name in ["Monotype Corsiva", "Monoton", "Encoded Sans", "Prefixed Display"] {
        assert_ne!(db.pick(name, REGULAR).family(), "Hack", "{name}");
    }
}

#[test]
fn a_face_the_machine_has_whose_name_says_its_class_comes_before_the_bundled_ones() {
    // Nothing in the stand-in table names these, so the default of the class decides: a system face of the class, then bundled.
    let mut db = bundled();
    for f in ["Foo Sans", "Bar Serif", "Baz Mono"] {
        assert_eq!(db.add_font_data(face(f).build()).added, 1);
    }
    assert_eq!(db.resolve("Zapfino", REGULAR).face.family(), "Foo Sans");
    assert_eq!(db.resolve("Palatino Linotype", REGULAR).face.family(), "Bar Serif");
    assert_eq!(db.resolve("Some Typewriter Face", REGULAR).face.family(), "Baz Mono");
    // The name asked for still wins, and a class nothing of the machine's is in falls to the bundled face.
    assert_eq!(db.resolve("Foo Sans", REGULAR).kind, MatchKind::Named);
    let mut sans_only = bundled();
    sans_only.add_font_data(face("Foo Sans").build());
    assert_eq!(sans_only.resolve("Courier-ish Mono", REGULAR).face.family(), "Hack");
    // A face whose name says nothing is not taken for any class.
    let mut odd = bundled();
    odd.add_font_data(face("Zapfino").build());
    assert_ne!(odd.resolve("Something Else", REGULAR).face.family(), "Zapfino");
}

/// The faces of a Mac with nothing added: no Carlito and no Liberation face, but Arial, Times New Roman and Courier New, and
/// faces whose names say their class ahead of them (a database lists them in the machine's order).
fn a_mac() -> FontDb {
    only(&[
        face("Apple SD Gothic Neo"),
        face("Arial"),
        face("Courier New"),
        face("Helvetica"),
        face("Hiragino Mincho ProN"),
        face("Menlo"),
        face("Times New Roman"),
    ])
}

#[test]
fn a_machine_without_the_open_twins_draws_the_default_in_the_face_they_match_not_in_a_gothic_one() {
    // On a Mac the first face whose name says sans (Apple SD Gothic Neo, a Korean face) drew every run with no face and every
    // sans name the Mac lacks. The faces the Liberation fonts are made to match come next after them, so Arial draws it.
    let mac = a_mac();
    assert_eq!(mac.default_family().as_deref(), Some("Arial"));
    for (asked, got) in [
        ("", "Arial"),
        ("Calibri", "Arial"),
        ("Segoe UI", "Arial"),
        ("Zapfino", "Arial"),
        ("Garamond", "Times New Roman"),
        ("Cambria", "Times New Roman"),
        ("Consolas", "Courier New"),
    ] {
        let r = mac.resolve(asked, REGULAR);
        assert_eq!((r.face.family(), r.kind), (got, MatchKind::Default), "{asked:?}");
    }
    // A name the Mac has is still its own face.
    assert_eq!(mac.resolve("Helvetica", REGULAR).kind, MatchKind::Named);
    assert_eq!(mac.resolve("Menlo", REGULAR).face.family(), "Menlo");
}

#[test]
fn the_open_twin_comes_before_the_face_it_matches_and_calibri_before_arial() {
    // Each default is an open twin first, then the face its makers say it matches, so a machine that has the twin draws as before. Carlito
    // comes first because Calibri is the commonest face in the files read, so Calibri itself, where the machine has it (Windows,
    // a Mac with an office suite), comes before the Arial group.
    let windows = only(&[
        face("Malgun Gothic"),
        face("MS Gothic"),
        face("Arial"),
        face("Calibri"),
        face("Cambria"),
        face("Courier New"),
        face("Times New Roman"),
    ]);
    assert_eq!(windows.default_family().as_deref(), Some("Calibri"));
    for (asked, got) in [("Segoe UI", "Calibri"), ("Garamond", "Times New Roman"), ("Lucida Console", "Courier New")] {
        let r = windows.resolve(asked, REGULAR);
        assert_eq!((r.face.family(), r.kind), (got, MatchKind::Default), "{asked:?}");
    }
    let with_twins = only(&[face("Arial"), face("Calibri"), face("Carlito"), face("Liberation Serif"), face("Times New Roman")]);
    assert_eq!(with_twins.default_family().as_deref(), Some("Carlito"));
    assert_eq!(with_twins.resolve("Garamond", REGULAR).face.family(), "Liberation Serif");
    let liberation = only(&[face("Arial"), face("Liberation Sans")]);
    assert_eq!(liberation.default_family().as_deref(), Some("Liberation Sans"));
    // Cambria, Caladea's original, is the serif default's last resort, after Times New Roman.
    assert_eq!(only(&[face("Foo Serif"), face("Cambria")]).resolve("Garamond", REGULAR).face.family(), "Cambria");
}
