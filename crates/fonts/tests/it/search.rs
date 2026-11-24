//! What the search for a fallback face reads and keeps, and what a hostile folder can make the catalogue read.

use std::time::Duration;

use diacraft_fonts::{Cover, FontDb};

use crate::REGULAR;
use crate::add::{collection, table_offset};
use crate::boundaries::{Dir, font};

fn han(family: &str) -> Vec<u8> {
    font(family).chars("中", 1000).build()
}

fn family(db: &FontDb, ch: char) -> Option<String> {
    db.covering(&[ch]).map(|f| f.family().to_owned())
}

#[test]
fn a_search_among_hundreds_of_faces_loads_only_the_face_it_finds() {
    let d = Dir::new("noload");
    for i in 0..300 {
        d.write(&format!("f{i:04}.ttf"), &font(&format!("Filler {i}")).build());
    }
    let wanted = han("Has Han");
    d.write("zz.ttf", &wanted);
    let db = d.db();
    assert_eq!(db.held_bytes(), 0);
    assert_eq!(family(&db, '中'), Some("Has Han".to_owned()));
    assert_eq!(db.held_bytes(), wanted.len() as u64, "the file found is held, and no other");
    assert!(db.indexed_bytes() > 0 && db.indexed_bytes() < 301 * 8192, "only cmap tables were read: {}", db.indexed_bytes());
}

#[test]
fn what_is_found_does_not_depend_on_what_is_loaded() {
    let d = Dir::new("order");
    d.write("a.ttf", &han("Han First"));
    d.write("b.ttf", &han("Han Second"));
    let cold = d.db();
    assert_eq!(family(&cold, '中'), Some("Han First".to_owned()));
    // The second is loaded for drawing: a search that tried loaded faces first would now answer with it.
    let warm = d.db();
    assert_eq!(warm.pick("Han Second", REGULAR).family(), "Han Second");
    assert_eq!(family(&warm, '中'), Some("Han First".to_owned()));
}

#[test]
fn a_face_that_declares_other_ranges_but_has_the_glyph_is_found_when_no_other_is() {
    // The ranges are a hint: faces on a real machine hold Han with the bit clear.
    let d = Dir::new("denied");
    let mut f = font("Declares Latin Only").chars("中", 1000);
    f.unicode_ranges = [1, 0, 0, 0];
    d.write("a.ttf", &f.build());
    assert_eq!(family(&d.db(), '中'), Some("Declares Latin Only".to_owned()));
}

#[test]
fn a_search_that_could_not_read_every_cmap_says_it_was_cut_short_and_is_not_remembered() {
    let d = Dir::new("cut");
    d.write("a.ttf", &font("Filler A").build());
    d.write("b.ttf", &font("Filler B").build());
    d.write("zz.ttf", &han("Has Han"));
    let full = d.db();
    assert_eq!(family(&full, '中'), Some("Has Han".to_owned()));
    let all = full.indexed_bytes();
    // One byte short of what the three tables take: the last cannot be read.
    let short = d.db().with_byte_limits(1 << 30, all - 1);
    assert!(matches!(short.covering_checked(&['中']), Cover::Cut));
    assert!(matches!(short.covering_checked(&['中']), Cover::Cut), "asked again, it looks again");
    assert_eq!(short.cover_searches(), 2);
    assert!(short.covering(&['中']).is_none());
    // The same bytes in a database that may read them all.
    let enough = d.db().with_byte_limits(1 << 30, all);
    assert!(matches!(enough.covering_checked(&['中']), Cover::Found(_)));
}

#[test]
fn a_miss_is_a_miss_only_when_every_face_was_looked_at() {
    let d = Dir::new("miss");
    d.write("a.ttf", &font("Filler A").build());
    let db = d.db();
    assert!(matches!(db.covering_checked(&['中']), Cover::Missing));
    assert!(matches!(db.covering_checked(&['中']), Cover::Missing));
    assert_eq!(db.cover_searches(), 1, "a complete miss is remembered");
}

#[test]
fn the_files_a_database_may_hold_are_limited_and_the_face_it_cannot_hold_is_not_drawn_with() {
    let d = Dir::new("held");
    let (a, b) = (font("Holds A").build(), font("Holds B").build());
    d.write("a.ttf", &a);
    d.write("b.ttf", &b);
    let cap = a.len() as u64 + 1;
    let db = d.db().with_byte_limits(cap, 1 << 30);
    assert_eq!(db.pick("Holds A", REGULAR).family(), "Holds A");
    assert_ne!(db.pick("Holds B", REGULAR).family(), "Holds B", "no room for a second file");
    assert_eq!(db.held_bytes(), a.len() as u64);
}

#[test]
fn a_face_found_that_cannot_be_held_is_a_search_cut_short_and_not_a_miss() {
    // The cmap says the file has the letter; the database may hold ten bytes of files, so it cannot be drawn from. That is not the
    // same as no face having the letter.
    let d = Dir::new("unheld");
    d.write("han.ttf", &han("Has Han"));
    let db = d.db().with_byte_limits(10, 1 << 30);
    assert!(matches!(db.covering_checked(&['中']), Cover::Cut));
    assert!(matches!(d.db().covering_checked(&['中']), Cover::Found(_)));
}

#[test]
fn what_a_database_keeps_of_the_cmaps_it_reads_is_limited_too() {
    // Three files of three separate ranges each: a database that may keep five ranges keeps the first file's and not the second's.
    let d = Dir::new("rangecap");
    let three = |name: &str, letters: &str| diacraft_fonts::synth::SynthFont::new(name).chars(letters, 500).build();
    // None of the three letters of a file is next to another, so each is a range of its own.
    d.write("a.ttf", &three("Ranges A", "中丁七"));
    d.write("b.ttf", &three("Ranges B", "中丁七"));
    d.write("zz.ttf", &three("Ranges Z", "万三与"));
    let tight = d.db().with_range_limit(5);
    assert!(matches!(tight.covering_checked(&['万']), Cover::Cut), "the third file's ranges don't fit either");
    let enough = d.db().with_range_limit(9);
    assert!(matches!(enough.covering_checked(&['万']), Cover::Found(_)));
}

#[test]
fn the_faces_of_a_collection_are_one_file_held_once() {
    let d = Dir::new("shared");
    let (one, two, three) = (font("Shared One").build(), font("Shared Two").build(), font("Shared Three").build());
    let ttc = collection(&[&one, &two, &three]);
    d.write("all.ttc", &ttc);
    let db = d.db();
    for name in ["Shared One", "Shared Two", "Shared Three"] {
        assert_eq!(db.pick(name, REGULAR).family(), name);
    }
    assert_eq!(db.held_bytes(), ttc.len() as u64);
}

#[test]
fn a_face_with_a_broken_cmap_covers_nothing_and_the_search_goes_on() {
    let d = Dir::new("brokencmap");
    let mut bad = han("Broken Cmap");
    let at = table_offset(&bad, b"cmap").unwrap();
    bad[at..at + 24].fill(0xFF);
    d.write("a.ttf", &bad);
    d.write("b.ttf", &han("Good Cmap"));
    assert_eq!(family(&d.db(), '中'), Some("Good Cmap".to_owned()));
}

#[test]
fn a_face_added_after_a_miss_is_found() {
    let mut db = FontDb::empty();
    assert!(matches!(db.covering_checked(&['中']), Cover::Missing));
    db.add_font_data(han("Added Later"));
    assert_eq!(family(&db, '中'), Some("Added Later".to_owned()));
}

/// A collection of 64 faces whose directories each list 512 records, every one a megabyte `name` table at the same place: written to
/// make a catalogue read 512 megabytes a face.
fn hostile_collection() -> Vec<u8> {
    let (faces, records, blob) = (64usize, 512usize, 1usize << 20);
    let dir_len = 12 + records * 16;
    let header = 12 + 4 * faces;
    let data_at = header + faces * dir_len;
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(faces as u32).to_be_bytes());
    for i in 0..faces {
        out.extend_from_slice(&((header + i * dir_len) as u32).to_be_bytes());
    }
    for _ in 0..faces {
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(records as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        for r in 0..records {
            out.extend_from_slice(if r == 0 { b"glyf" } else { b"name" });
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(data_at as u32).to_be_bytes());
            out.extend_from_slice(&(blob as u32).to_be_bytes());
        }
    }
    out.resize(data_at + blob, 0xAB);
    out
}

#[test]
fn a_collection_that_names_the_same_megabyte_in_every_record_is_catalogued_in_a_moment() {
    // It once took 31 seconds and read 32 GiB; each tag is now read once and a range already read is not read again.
    let d = Dir::new("hostilettc");
    d.write("evil.ttc", &hostile_collection());
    diacraft_fonts::work::reset();
    let db = d.db();
    // Counted, not timed: the megabyte is read once and not for each of the 64 faces (a file's reads are capped at 16 MiB, which they
    // would have reached), with the header and its list of faces, and the 64 directories of 512 records.
    let read = diacraft_fonts::work::file_bytes_read();
    let expected = (1 << 20) + (12 + 4 * 64) + 64 * (12 + 512 * 16);
    assert_eq!(read, expected, "bytes read of a file whose one megabyte is named by 64 faces");
    assert_eq!(db.families().len(), 2, "only the bundled faces: none of the 64 is a font");
}

#[test]
fn a_scan_that_runs_out_of_time_keeps_what_it_has() {
    let d = Dir::new("notime");
    d.write("a.ttf", &font("Late Face").build());
    let none = FontDb::with_font_dirs_for(std::slice::from_ref(&d.0), Duration::ZERO);
    assert!(!none.has_family("Late Face"));
    let some = FontDb::with_font_dirs_for(std::slice::from_ref(&d.0), Duration::from_secs(60));
    assert!(some.has_family("Late Face"));
}
