//! The fonts the window draws with. egui's `default_fonts` feature also registers `egui-icons`, a subset of
//! emoji-icon-font that holds the Windows, Apple, Android and GitHub logos (private-use code points that no
//! platform font has) and the word `git`. The window never draws them, so they must not be drawable.

use std::sync::{Arc, Mutex, mpsc::channel};
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_engine::measure::{SharedFonts, scan_fonts_with, set_measure_fonts};
use diacraft_fonts::{FontDb, synth::SynthFont};
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::fonts::{font_definitions, font_definitions_with, strong_family};
use egui::{FontDefinitions, FontFamily, FontId};

use crate::support::window;

/// The five code points of `egui-icons`: Android, Windows, GitHub, the word `git`, Apple.
const LOGOS: [char; 5] = ['\u{E618}', '\u{E61F}', '\u{E624}', '\u{E625}', '\u{F8FF}'];

fn installed(window: &crate::support::Window) -> FontDefinitions {
    window.ctx.fonts(|fonts| fonts.definitions().clone())
}

fn has_glyph(window: &crate::support::Window, family: FontFamily, c: char) -> bool {
    window.ctx.fonts_mut(|fonts| fonts.has_glyph(&FontId::new(14.0, family), c))
}

#[test]
fn the_installed_fonts_have_no_icon_font() {
    let window = window(DiacraftApp::new(Session::new(), None), [800.0, 500.0]);
    let definitions = installed(&window);
    let names: Vec<&String> = definitions.font_data.keys().collect();
    assert!(!names.iter().any(|n| n.contains("icon")), "an icon font is installed: {names:?}");
    for (family, chain) in &definitions.families {
        assert!(!chain.iter().any(|n| n.contains("icon")), "{family:?} falls back to an icon font: {chain:?}");
    }
    let expected = font_definitions();
    assert_eq!(definitions.font_data, expected.font_data);
    assert_eq!(definitions.families, expected.families);
}

#[test]
fn no_optional_faces_still_means_only_the_two_bundled_text_typefaces() {
    let definitions = font_definitions_with(&FontDb::empty());
    assert_eq!(definitions.font_data.keys().map(String::as_str).collect::<Vec<_>>(), ["Hack", "Ubuntu-Light"]);
    assert_eq!(definitions.families.get(&FontFamily::Proportional), Some(&vec!["Ubuntu-Light".to_owned()]));
    assert_eq!(definitions.families.get(&FontFamily::Monospace), Some(&vec!["Hack".to_owned(), "Ubuntu-Light".to_owned()]));
}

/// The behaviour, not only the list: no logo can be drawn, and the text the window shows still can.
#[test]
fn no_platform_logo_can_be_drawn_and_the_windows_text_still_can() {
    let mut window = window(DiacraftApp::new(Session::new(), None), [800.0, 500.0]);
    window.step();
    for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
        for logo in LOGOS {
            assert!(!has_glyph(&window, family.clone(), logo), "{family:?} can draw U+{:04X}", logo as u32);
        }
        for shown in "DiaCraft No document open 0123456789 · …".chars() {
            assert!(has_glyph(&window, family.clone(), shown), "{family:?} cannot draw {shown:?}, which the window shows");
        }
    }
}

/// `default_fonts` is what makes egui register `egui-icons` (and embed its bytes, whatever the app installs). It
/// is off, so egui's own default definitions are empty: if a manifest turns it on again for the build this test
/// runs in, it fails. It sees only that build: the feature on another target (`cfg(windows)`), in another member or
/// in a build of another package does not reach it. The guard that covers all of them reads Cargo.lock
/// (`the_lockfile_does_not_link_epaint_to_the_icon_font` in xtask/tests/it/workspace.rs).
#[test]
fn egui_default_fonts_stay_off() {
    assert_eq!(FontDefinitions::builtin_font_names(), &[] as &[&str], "egui's `default_fonts` feature is on again");
    assert!(FontDefinitions::default().font_data.is_empty());
}

fn add(db: &mut FontDb, family: &str, style: &str) {
    assert_eq!(db.add_font_data(SynthFont::new(family).styled(style).chars("UI 012", 550).build()).added, 1);
}

fn proportional(definitions: &FontDefinitions) -> &[String] {
    definitions.families.get(&FontFamily::Proportional).unwrap()
}

#[test]
fn open_interface_faces_have_a_fixed_preference_and_real_bold_chain() {
    let mut db = FontDb::empty();
    for family in ["DejaVu Sans", "Liberation Sans", "Noto Sans", "Inter"] {
        add(&mut db, family, "Regular");
        add(&mut db, family, "Bold");
        let definitions = font_definitions_with(&db);
        assert_eq!(proportional(&definitions), [format!("{family}-UI-Regular"), "Ubuntu-Light".to_owned()]);
        assert_eq!(
            definitions.families.get(&strong_family()).unwrap(),
            &[format!("{family}-UI-Bold"), format!("{family}-UI-Regular"), "Ubuntu-Light".to_owned()]
        );
        assert_eq!(definitions.families.get(&FontFamily::Monospace), font_definitions().families.get(&FontFamily::Monospace));
    }
}

#[test]
fn regular_is_the_strong_fallback_when_no_real_bold_exists() {
    let mut db = FontDb::empty();
    add(&mut db, "Inter", "Regular");
    let definitions = font_definitions_with(&db);
    assert_eq!(definitions.families.get(&strong_family()).unwrap(), proportional(&definitions));
    assert_eq!(definitions.font_data.len(), 3);
}

#[test]
fn aliases_and_document_standins_do_not_enter_the_interface() {
    let mut db = FontDb::empty();
    add(&mut db, "Arimo", "Regular");
    let mut disguised = SynthFont::new("Different Family").chars("UI", 550);
    disguised.legacy_family = Some("Inter".to_owned());
    assert_eq!(db.add_font_data(disguised.build()).added, 1);
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}

#[test]
fn light_italic_oversized_and_logo_faces_are_skipped() {
    for kind in ["light", "italic", "oversized", "logo"] {
        let mut db = FontDb::empty();
        add(&mut db, "Noto Sans", "Regular");
        let mut face = SynthFont::new("Inter").chars("UI", 550);
        match kind {
            "light" => face.weight = Some(300),
            "italic" => face = face.styled("Italic"),
            "logo" => face = face.chars("\u{F8FF}", 550),
            _ => {}
        }
        let mut bytes = face.build();
        if kind == "oversized" {
            bytes.resize((8 << 20) + 1, 0);
        }
        assert_eq!(db.add_font_data(bytes).added, 1);
        assert_eq!(proportional(&font_definitions_with(&db)).first().map(String::as_str), Some("Noto Sans-UI-Regular"), "{kind}");
    }
}

#[test]
fn shared_faces_upgrade_the_interface_once_after_the_scan_finishes() {
    let (release, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let fonts = scan_fonts_with(move || {
        gate.lock().unwrap().recv_timeout(Duration::from_secs(20)).unwrap();
        let mut db = FontDb::empty();
        add(&mut db, "Inter", "Regular");
        add(&mut db, "Inter", "Bold");
        db
    });
    // This helper also shares fonts with the engine process-wide. The test exercises only the UI's
    // explicit SharedFonts handle, so release the global immediately, even if a later assertion fails.
    set_measure_fonts(None);
    let mut w = window(DiacraftApp::new(Session::new(), None).with_fonts(fonts.clone()), [800.0, 500.0]);
    assert!(!fonts.is_ready(), "the first frame must not wait for the scan");
    assert_eq!(installed(&w).font_data, font_definitions().font_data);
    release.send(()).unwrap();
    let _db = fonts.get(); // Only the test waits, after releasing the gate.
    wait_for_interface_face(&mut w, "Inter-UI-Regular");
    let first = installed(&w);
    assert_eq!(proportional(&first).first().map(String::as_str), Some("Inter-UI-Regular"));
    for _ in 0..4 {
        w.step();
    }
    let later = installed(&w);
    assert!(Arc::ptr_eq(first.font_data.get("Inter-UI-Regular").unwrap(), later.font_data.get("Inter-UI-Regular").unwrap()));
    for family in [FontFamily::Proportional, FontFamily::Monospace, strong_family()] {
        for logo in LOGOS {
            assert!(!has_glyph(&w, family.clone(), logo));
        }
        for ch in "DiaCraft 012 · …".chars() {
            assert!(has_glyph(&w, family.clone(), ch), "fallback lost {ch:?}");
        }
    }
}

#[test]
fn watching_document_faces_alone_does_not_change_interface_typography() {
    let mut db = FontDb::empty();
    add(&mut db, "Inter", "Regular");
    let mut w = window(DiacraftApp::new(Session::new(), None).with_font_watch(SharedFonts::ready(Arc::new(db))), [800.0, 500.0]);
    w.step();
    assert_eq!(installed(&w).font_data, font_definitions().font_data);
}

/// A hand-built TrueType collection of synthetic faces, with each table offset made absolute.
fn collection(mut faces: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"ttcf".to_vec();
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    bytes.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    let mut start = 12 + 4 * faces.len();
    for face in &mut faces {
        bytes.extend_from_slice(&(start as u32).to_be_bytes());
        let count = usize::from(u16::from_be_bytes(face[4..6].try_into().unwrap()));
        for record in face[12..12 + count * 16].as_chunks_mut::<16>().0.iter_mut() {
            let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) + start as u32;
            record[8..12].copy_from_slice(&offset.to_be_bytes());
        }
        start += face.len();
    }
    for face in faces {
        bytes.extend_from_slice(&face);
    }
    bytes
}

#[test]
fn collection_faces_keep_their_indices_and_draw_the_selected_faces() {
    let faces = vec![
        SynthFont::new("Unselected").chars("X", 550).build(),
        SynthFont::new("Inter").chars("R", 550).build(),
        SynthFont::new("Inter").styled("Bold").chars("B", 650).build(),
    ];
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(collection(faces)).added, 3);
    let definitions = font_definitions_with(&db);
    assert_eq!(definitions.font_data.get("Inter-UI-Regular").unwrap().index, 1);
    assert_eq!(definitions.font_data.get("Inter-UI-Bold").unwrap().index, 2);
    let mut w = window(DiacraftApp::new(Session::new(), None).with_fonts(SharedFonts::ready(Arc::new(db))), [800.0, 500.0]);
    wait_for_interface_face(&mut w, "Inter-UI-Regular");
    w.ctx.fonts_mut(|fonts| {
        assert!(fonts.glyph_width(&FontId::new(14.0, FontFamily::Proportional), 'R') > 0.0);
    });
    assert_eq!(installed(&w).font_data.get("Inter-UI-Regular").unwrap().index, 1);
}

fn wait_for_interface_face(window: &mut crate::support::Window, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !installed(window).font_data.contains_key(name) {
        assert!(Instant::now() < deadline, "interface font worker did not install {name}");
        window.step();
        std::thread::yield_now();
    }
}

/// Replace or add one small synthetic table and rebuild the absolute offsets.
fn with_table(base: &[u8], tag: [u8; 4], data: Vec<u8>) -> Vec<u8> {
    let count = usize::from(u16::from_be_bytes(base[4..6].try_into().unwrap()));
    let mut tables: Vec<([u8; 4], Vec<u8>)> = base[12..12 + count * 16]
        .as_chunks::<16>()
        .0
        .iter()
        .filter(|record| record[..4] != tag)
        .map(|record| {
            let at = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
            (record[..4].try_into().unwrap(), base[at..at + len].to_vec())
        })
        .collect();
    tables.push((tag, data));
    tables.sort_by_key(|(tag, _)| *tag);
    let mut out = base[..12].to_vec();
    out[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    let mut at = 12 + tables.len() * 16;
    for (tag, bytes) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // The font readers do not require a checksum for synthetic input.
        out.extend_from_slice(&(at as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        at += bytes.len().div_ceil(4) * 4;
    }
    for (_, bytes) in tables {
        out.extend_from_slice(&bytes);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}

#[test]
fn variable_composites_are_rejected_before_any_egui_rasterization() {
    let base = SynthFont::new("Inter").chars("a", 550).build();
    // VARC covers glyph 1 and draws three copies of glyph 0. This fixture is deliberately tiny:
    // rejection depends on the representation, not on provoking a large outline allocation.
    let mut varc = vec![0, 1, 0, 0];
    for offset in [24u32, 0, 0, 0, 30] {
        varc.extend_from_slice(&offset.to_be_bytes());
    }
    varc.extend_from_slice(&[0, 1, 0, 1, 0, 1]);
    varc.extend_from_slice(&1u32.to_be_bytes());
    varc.push(4);
    varc.extend_from_slice(&1u32.to_be_bytes());
    varc.extend_from_slice(&10u32.to_be_bytes());
    varc.extend_from_slice(&[0; 9]);
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(with_table(&base, *b"VARC", varc)).added, 1);
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}

#[test]
fn implausible_ui_glyph_extent_is_rejected_before_egui_rasterization() {
    let base = SynthFont::new("Inter").chars("a", 550).build();
    let count = usize::from(u16::from_be_bytes(base[4..6].try_into().unwrap()));
    let record = base[12..12 + count * 16].as_chunks::<16>().0.iter().find(|r| &r[..4] == b"glyf").unwrap();
    let at = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
    let len = u32::from_be_bytes(record[12..16].try_into().unwrap()) as usize;
    let mut glyf = base[at..at + len].to_vec();
    // The synthetic .notdef is a four-point rectangle: grow its third x delta and xMax only.
    // Keep em, advances, ascender and descender normal, so only the outline-bound check rejects it.
    glyf[6..8].copy_from_slice(&10_000i16.to_be_bytes());
    glyf[22..24].copy_from_slice(&10_000i16.to_be_bytes());
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(with_table(&base, *b"glyf", glyf)).added, 1);
    let face = db.pick("Inter", diacraft_fonts::FontStyle::default());
    assert_eq!(face.units_per_em(), 1000.0);
    assert_eq!(face.static_truetype_glyph_count(4.0), Some(2), "all line metrics pass");
    assert!(face.advance(0) <= 4000.0 && face.advance(1) <= 4000.0, "both advances pass");
    assert!(matches!(face.outline_glyph(0), diacraft_fonts::Outline::Drawn(path) if path.control_box().width() > 4000.0));
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}

#[test]
fn admitted_interface_fonts_do_not_run_hinting_instructions() {
    let mut db = FontDb::empty();
    add(&mut db, "Inter", "Regular");
    let definitions = font_definitions_with(&db);
    assert_eq!(definitions.font_data.get("Inter-UI-Regular").unwrap().tweak.hinting, Some(false));
}

#[test]
fn implausible_ui_line_metrics_are_rejected_without_changing_document_fonts() {
    let mut glyph = SynthFont::new("Inter").chars("a", 550);
    glyph.ascent = 16_000;
    glyph.descent = -16_000;
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(glyph.build()).added, 1);
    assert!(db.has_family("Inter"), "the document database still retains the face");
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}

#[test]
fn extra_glyph_locations_cannot_bypass_the_interface_preflight() {
    let mut bytes = SynthFont::new("Inter").chars("a", 550).build();
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    let record = bytes[12..12 + count * 16].as_chunks::<16>().0.iter().find(|r| &r[..4] == b"maxp").unwrap();
    let at = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
    // Keep two glyphs in loca/glyf, but declare only the first in maxp. No glyph is rasterized.
    bytes[at + 4..at + 6].copy_from_slice(&1u16.to_be_bytes());
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(bytes).added, 1);
    let face = db.pick("Inter", diacraft_fonts::FontStyle::default());
    assert_eq!(face.static_truetype_glyph_count(4.0), None);
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}

#[test]
fn unknown_glyph_location_format_is_not_admitted_to_the_interface() {
    let mut bytes = SynthFont::new("Inter").chars("a", 550).build();
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    let record = bytes[12..12 + count * 16].as_chunks::<16>().0.iter().find(|r| &r[..4] == b"head").unwrap();
    let at = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
    bytes[at + 50..at + 52].copy_from_slice(&2i16.to_be_bytes());
    let mut db = FontDb::empty();
    let report = db.add_font_data(bytes);
    if report.added == 1 {
        let face = db.pick("Inter", diacraft_fonts::FontStyle::default());
        assert_eq!(face.static_truetype_glyph_count(4.0), None);
    }
    assert_eq!(font_definitions_with(&db).font_data, font_definitions().font_data);
}
