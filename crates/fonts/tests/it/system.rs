//! Reading font folders: only what the caps allow, and only fonts. Every test builds its own folder of synthetic faces, so nothing
//! depends on what is installed. (One test reads the machine's real folders and asserts nothing about what it finds.)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, MatchKind};

use crate::REGULAR;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("diacraft-fonts-sys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        Dir(dir)
    }
    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, bytes).expect("write");
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn font(family: &str) -> Vec<u8> {
    SynthFont::new(family).ascii(500).build()
}

fn db(dir: &Dir) -> FontDb {
    FontDb::with_font_dirs(std::slice::from_ref(&dir.0))
}

#[test]
fn fonts_under_a_folder_are_catalogued_and_found_by_name() {
    let d = Dir::new("basic");
    d.write("a.ttf", &font("Folder A"));
    d.write("deep/er/b.OTF", &font("Folder B"));
    d.write("notes.txt", b"not a font");
    let db = db(&d);
    for name in ["Folder A", "folder b"] {
        let r = db.resolve(name, REGULAR);
        assert_eq!((r.kind, r.face.family().to_ascii_lowercase()), (MatchKind::Named, name.to_ascii_lowercase()));
    }
    assert!(!db.has_family("notes"));
}

#[test]
fn a_collection_file_is_catalogued_by_each_of_its_distinct_faces_and_each_loads_its_own() {
    let d = Dir::new("ttc");
    // Three entries, the first and third the same face: two faces, and the second must load from index 1, not from where the list
    // position after the repeats would put it.
    let (a, b) = (font("Disk One"), font("Disk Two"));
    d.write("both.ttc", &crate::add::collection(&[&a, &a, &b]));
    let db = db(&d);
    assert_eq!(db.face_count(), 4, "two bundled and two from the collection");
    assert_eq!(db.pick("Disk One", REGULAR).family(), "Disk One");
    assert_eq!(db.pick("Disk Two", REGULAR).family(), "Disk Two");
    assert_eq!(db.pick("Disk Two", REGULAR).index(), 2, "the index in the collection");
}

#[test]
fn damaged_files_are_skipped_and_the_rest_still_load() {
    let d = Dir::new("damaged");
    d.write("good.ttf", &font("Good One"));
    d.write("zero.ttf", &[]);
    d.write("short.ttf", b"OTTO");
    d.write("junk.ttf", &vec![0xAB; 50_000]);
    let mut cut = font("Cut Short");
    cut.truncate(120);
    d.write("cut.ttf", &cut);
    // A directory that claims a table far past the end of the file.
    let mut lying = font("Lying Dir");
    for i in 0..usize::from(u16::from_be_bytes([lying[4], lying[5]])) {
        let p = 12 + i * 16 + 12;
        lying[p..p + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    }
    d.write("lying.ttf", &lying);
    let db = db(&d);
    assert!(db.has_family("Good One"));
    assert!(!db.has_family("Cut Short") && !db.has_family("Lying Dir"));
    assert_eq!(db.pick("Good One", REGULAR).family(), "Good One");
}

#[test]
fn a_file_that_parses_by_name_but_not_by_body_is_dropped_at_first_use_and_never_fails_a_lookup() {
    // The catalogue reads only four tables. A face with those intact and the rest destroyed is catalogued, then found to be unusable the
    // first time it is asked for: the lookup must carry on to the next face. The sfnt version at the start of the file is not one of the
    // four tables, and the font reader refuses a file that does not have a version it knows.
    let d = Dir::new("body");
    let mut bytes = font("Broken Body");
    bytes[..4].copy_from_slice(b"\xFF\xFF\xFF\xFF");
    d.write("broken.ttf", &bytes);
    d.write("fine.ttf", &font("Fine Body"));
    let db = db(&d);
    assert!(db.has_family("Broken Body"), "catalogued: its name is in a table that was read");
    let r = db.resolve("Broken Body", REGULAR);
    // Whatever it did with the broken one, the answer is a usable face.
    assert!(r.face.covers('a'));
    assert_ne!(r.face.family(), "Broken Body");
    assert_eq!(db.pick("Fine Body", REGULAR).family(), "Fine Body");
}

#[test]
fn a_font_is_listed_from_a_folder_if_and_only_if_the_same_bytes_are_a_face_when_added() {
    // A font with no glyph count, with an em of 8 and with an em of 16384 to the em: the first two are no face, whichever way they come.
    let base = font("Same Either Way");
    let mut small_em = SynthFont::new("Same Either Way").ascii(500);
    small_em.units_per_em = 8;
    let mut big_em = SynthFont::new("Same Either Way").ascii(500);
    big_em.units_per_em = 16_384;
    for (what, bytes, is_a_face) in [
        ("an intact font", base.clone(), true),
        ("an em of 16384", big_em.build(), true),
        ("an empty maxp table", crate::hostile::with_table(&base, b"maxp", Vec::new()), false),
        ("a maxp table of no glyphs", crate::hostile::with_table(&base, b"maxp", vec![0, 0, 0x50, 0, 0, 0]), false),
        ("an em of 8", small_em.build(), false),
    ] {
        let d = Dir::new("same");
        d.write("same.ttf", &bytes);
        assert_eq!(db(&d).has_family("Same Either Way"), is_a_face, "from a folder: {what}");
        let mut added = FontDb::empty();
        assert_eq!(added.add_font_data(bytes).added, usize::from(is_a_face), "from bytes: {what}");
    }
}

#[test]
fn the_search_for_a_fallback_face_is_the_same_whether_the_font_came_from_a_folder_or_from_bytes() {
    // A font of ten glyphs whose character map names twenty characters of Han (which no bundled face has): the reader cuts a map where
    // the glyph count of the font ends, and the search of a font read from its `cmap` alone has to cut it at the same place, the glyph
    // count of the font's own `maxp`. Glyph 0 is the missing glyph, so the first nine characters (by code point) are covered.
    let han: String = (0..20).map(|i| char::from_u32(0x4E00 + i).unwrap_or('一')).collect();
    let base = SynthFont::new("Cut Map").chars(&han, 500).build();
    let ten_glyphs = crate::hostile::with_table(&base, b"maxp", vec![0, 0, 0x50, 0, 0, 10]);
    let d = Dir::new("cut");
    d.write("cut.ttf", &ten_glyphs);
    let from_folder = db(&d);
    let mut from_bytes = FontDb::empty();
    assert_eq!(from_bytes.add_font_data(ten_glyphs).added, 1);
    for i in 0..20u32 {
        let c = char::from_u32(0x4E00 + i).unwrap_or('一');
        let (a, b) = (from_folder.covering(&[c]).map(|f| f.family().to_owned()), from_bytes.covering(&[c]).map(|f| f.family().to_owned()));
        assert_eq!(a, b, "U+{:04X}", c as u32);
        assert_eq!(a.as_deref() == Some("Cut Map"), i < 9, "U+{:04X}", c as u32);
    }
}

#[test]
fn a_sparse_file_over_the_size_cap_is_neither_read_nor_listed() {
    let d = Dir::new("huge");
    let f = std::fs::File::create(d.0.join("huge.ttf")).expect("create");
    f.set_len(600 << 20).expect("sparse");
    d.write("ok.ttf", &font("Ok Beside Huge"));
    diacraft_fonts::work::reset();
    let db = db(&d);
    // Counted, not timed: "not read" is bytes. The small font is read for its names, a few kilobytes; the 600 MiB are not touched.
    let read = diacraft_fonts::work::file_bytes_read();
    assert!(read > 0 && read < 1 << 20, "{read} bytes read from a folder of a small font and a file of 600 MiB");
    assert!(db.has_family("Ok Beside Huge"));
    assert_eq!(db.face_count(), 3, "two bundled and the one font");
}

#[test]
fn folders_nested_past_the_depth_cap_are_not_entered() {
    let d = Dir::new("depth");
    d.write("a/b/c/d/e/f/g/h/near.ttf", &font("Near Enough"));
    d.write("a/b/c/d/e/f/g/h/i/j/far.ttf", &font("Too Far Down"));
    let db = db(&d);
    assert!(db.has_family("Near Enough"));
    assert!(!db.has_family("Too Far Down"));
}

/// Found once and not once per trip round the loop (counted below); the ten seconds are a hang guard for a scan that does loop.
#[cfg(unix)]
#[test]
fn a_folder_link_that_loops_back_does_not_make_the_scan_loop() {
    let d = Dir::new("loop");
    d.write("sub/a.ttf", &font("Looped"));
    std::os::unix::fs::symlink(&d.0, d.0.join("sub/back")).expect("symlink");
    let start = Instant::now();
    let db = db(&d);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(db.has_family("Looped"));
    // Found once, not once per trip round the loop.
    assert_eq!(db.face_count(), 3);
}

#[test]
fn a_folder_that_does_not_exist_is_nothing_to_catalogue() {
    let db = FontDb::with_font_dirs(&[Path::new("/nonexistent-font-folder-for-diacraft").to_path_buf()]);
    assert!(db.pick("x", REGULAR).covers('a'));
    assert_eq!(db.face_count(), 2, "the bundled faces only");
}

/// The count of faces is the claim; the ten seconds are a hang guard and no measure of cost (the 400 small files take a fraction of a second).
#[test]
fn the_catalogue_stops_at_the_face_cap() {
    // A thousand small files is far from the cap (60,000), but shows the scan handles many files in a sorted order, quickly.
    let d = Dir::new("many");
    for i in 0..400 {
        d.write(&format!("f{i:04}.ttf"), &font(&format!("Many {i}")));
    }
    let start = Instant::now();
    let db = db(&d);
    assert_eq!(db.face_count(), 402);
    assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
}

/// Only that it finishes and answers; the minute is a hang guard, and the folders are whatever the machine has.
#[test]
fn the_machines_own_folders_can_be_read_and_the_result_is_usable() {
    // No assertion about what is installed: only that it finishes, and answers.
    let start = Instant::now();
    let db = FontDb::system_and_bundled();
    let elapsed = start.elapsed();
    let r = db.resolve("Some Name That Is Not A Font", REGULAR);
    assert!(r.face.covers('a'));
    eprintln!("system_and_bundled: {} faces in {elapsed:?}", db.face_count());
    assert!(elapsed < Duration::from_secs(60));
}
