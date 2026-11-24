//! The caps and the rules' edges, one test each, with the numbers written out: a test that reads a cap from the code it tests would
//! follow the cap wherever it moved. A cap is proved by the case just inside it, which is let through, and the case just outside,
//! which is not.

use std::path::{Path, PathBuf};

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, ShapeParams, xdg_font_dirs};

use crate::{BOLD, REGULAR};

pub fn font(family: &str) -> SynthFont {
    SynthFont::new(family).ascii(500)
}

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("diacraft-fonts-bound-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
    pub fn db(&self) -> FontDb {
        FontDb::with_font_dirs(std::slice::from_ref(&self.0))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tables_in(font: &[u8]) -> usize {
    usize::from(u16::from_be_bytes([font[4], font[5]]))
}

// ---- faces in a collection: 64

#[test]
fn a_collection_gives_at_most_64_faces() {
    let make = |n: usize| -> Vec<Vec<u8>> { (0..n).map(|i| font(&format!("Cap {i}")).build()).collect() };
    for (count, want) in [(63usize, 63usize), (64, 64), (65, 64), (200, 64)] {
        let fonts = make(count);
        let refs: Vec<&Vec<u8>> = fonts.iter().collect();
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(crate::add::collection(&refs)).added, want, "{count} faces in memory");
        let d = Dir::new(&format!("coll{count}"));
        d.write("c.ttc", &crate::add::collection(&refs));
        assert_eq!(d.db().face_count(), 2 + want, "{count} faces on disk");
    }
}

// ---- the font directory: 512 tables

#[test]
fn a_font_file_with_512_tables_is_catalogued_and_one_with_513_is_not() {
    let d = Dir::new("tables");
    let ok = font("Tables Ok");
    let (mut a, mut b) = (ok.clone(), font("Tables Too Many"));
    a.extra_tables = 502;
    b.extra_tables = 503;
    let (a_bytes, b_bytes) = (a.build(), b.build());
    assert_eq!((tables_in(&a_bytes), tables_in(&b_bytes)), (512, 513));
    d.write("a.ttf", &a_bytes);
    d.write("b.ttf", &b_bytes);
    let db = d.db();
    assert!(db.has_family("Tables Ok"));
    assert!(!db.has_family("Tables Too Many"));
}

// ---- the name table: 1 MiB

#[test]
fn a_name_table_of_a_megabyte_or_less_is_read_and_a_bigger_one_is_not() {
    let d = Dir::new("names");
    let (mut small, mut big) = (font("Name Small"), font("Name Big"));
    small.name_padding = 900_000;
    big.name_padding = 1_300_000;
    d.write("a.ttf", &small.build());
    d.write("b.ttf", &big.build());
    let db = d.db();
    assert!(db.has_family("Name Small"));
    assert!(!db.has_family("Name Big"));
}

// ---- a font file: 512 MiB

#[test]
fn a_font_file_of_512_mib_is_catalogued_and_one_byte_more_is_not() {
    let d = Dir::new("filecap");
    for (name, extra) in [("File At Cap", 0u64), ("File Over Cap", 1)] {
        let bytes = font(name).build();
        let path = d.write(&format!("{extra}.ttf"), &bytes);
        // A sparse file: the font at its start and nothing after it, so no disk is used and the catalogue reads only the tables.
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len((512 << 20) + extra).unwrap();
    }
    let db = d.db();
    assert!(db.has_family("File At Cap"));
    assert!(!db.has_family("File Over Cap"));
}

#[test]
fn a_font_of_exactly_128_mib_can_be_added_and_one_byte_more_cannot() {
    let mut bytes = font("Added At Cap").build();
    bytes.resize(128 << 20, 0);
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(bytes).added, 1);
    let mut bytes = font("Added Over Cap").build();
    bytes.resize((128 << 20) + 1, 0);
    let report = db.add_font_data(bytes);
    assert_eq!((report.added, report.rejected), (0, 1));
}

// ---- outline tables

#[test]
fn a_file_with_no_outline_tables_is_not_catalogued_but_is_added_from_memory() {
    let mut f = font("No Outlines");
    f.omit_outlines = true;
    let d = Dir::new("outlines");
    d.write("a.ttf", &f.build());
    d.write("b.ttf", &font("Has Outlines").build());
    let db = d.db();
    assert!(!db.has_family("No Outlines") && db.has_family("Has Outlines"));
}

// ---- weight and slant

#[test]
fn bold_starts_at_a_weight_of_600_and_a_name_that_says_bold_counts() {
    let face = |weight: u16, style: &str| {
        let mut f = font("Weighted").styled(style);
        f.weight = Some(weight);
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(f.build()).added, 1);
        db.pick("Weighted", REGULAR)
    };
    assert!(face(600, "SemiBold").is_bold());
    assert!(!face(599, "Medium").is_bold());
    // The table says 400 and the name says Bold: bold. And for black and heavy.
    for style in ["Bold", "Black", "Heavy"] {
        assert!(face(400, style).is_bold(), "{style}");
    }
    assert!(!face(400, "Regular").is_bold());
    assert_eq!(face(300, "Light").weight(), 300);
}

#[test]
fn italic_comes_from_the_slant_bits_or_from_a_name_that_says_italic_or_oblique() {
    let face = |bits: bool, style: &str| {
        let mut f = font("Slanted").styled("Regular");
        f.style = style.to_owned();
        f.slant_bits = Some(bits);
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(f.build()).added, 1);
        db.pick("Slanted", REGULAR)
    };
    assert!(face(true, "Regular").is_italic());
    assert!(face(false, "Italic").is_italic());
    assert!(face(false, "Oblique").is_italic());
    assert!(!face(false, "Regular").is_italic());
}

#[test]
fn a_style_is_chosen_by_weight_class_first_and_then_by_how_near_the_weight_is() {
    let faces = |weights: &[(u16, &str)]| {
        let mut db = FontDb::empty();
        for (w, style) in weights {
            let mut f = font("Family").styled(style);
            f.weight = Some(*w);
            f.bold = *w >= 600;
            assert_eq!(db.add_font_data(f.build()).added, 1);
        }
        db
    };
    // Bold asked for: the face nearest 700 among the bold ones.
    let db = faces(&[(400, "Regular"), (600, "SemiBold"), (700, "Bold"), (800, "ExtraBold")]);
    assert_eq!(db.pick("Family", BOLD).weight(), 700);
    assert_eq!(db.pick("Family", REGULAR).weight(), 400);
    // Regular asked for, with a light face and a semibold one: the light one, a regular weight, though the semibold is nearer 400.
    let db = faces(&[(100, "Thin"), (600, "SemiBold")]);
    assert_eq!(db.pick("Family", REGULAR).weight(), 100);
    // Bold asked for, with a regular and a semibold: the semibold, which is bold, though the regular is no further than 300 away.
    let db = faces(&[(400, "Regular"), (600, "SemiBold")]);
    assert_eq!(db.pick("Family", BOLD).weight(), 600);
}

// ---- lines the faces recommend

#[test]
fn a_recommended_line_is_used_only_if_it_could_be_one() {
    let line = |underline: (i16, i16), strikeout: (i16, i16)| {
        let mut f = font("Lines");
        f.underline = underline;
        f.strikeout = strikeout;
        let mut db = FontDb::empty();
        db.add_font_data(f.build());
        let face = db.pick("Lines", REGULAR);
        (face.underline().map(|s| (s.top, s.thickness)), face.strikeout().map(|s| (s.top, s.thickness)))
    };
    // Thickness: more than 0 and less than half an em.
    assert!(line((-100, 1), (50, 300)).0.is_some());
    assert!(line((-100, 0), (50, 300)).0.is_none());
    assert!(line((-100, 499), (50, 300)).0.is_some());
    assert!(line((-100, 500), (50, 300)).0.is_none());
    // (The strikeout is given as its size, then its position.)
    assert!(line((-100, 50), (499, 300)).1.is_some());
    assert!(line((-100, 50), (500, 300)).1.is_none());
    assert!(line((-100, 50), (0, 300)).1.is_none());
    // Position: within two ems.
    assert!(line((-1999, 50), (50, 300)).0.is_some());
    assert!(line((-2000, 50), (50, 300)).0.is_none());
    assert!(line((-100, 50), (100, 1999)).1.is_some());
    assert!(line((-100, 50), (100, 2000)).1.is_none());
}

#[test]
fn a_name_in_a_font_is_cut_at_256_characters() {
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(font(&"N".repeat(1000)).build()).added, 1);
    assert_eq!(db.families().first().map(|f| f.chars().count()), Some(256));
    let mut db = FontDb::empty();
    db.add_font_data(font(&"N".repeat(256)).build());
    assert_eq!(db.families().first().map(|f| f.chars().count()), Some(256));
}

#[test]
fn a_name_in_a_font_is_cut_before_anything_lower_cases_it_and_is_found_by_the_name_that_drawing_gives() {
    // `İ` lower-cases to two characters: the name kept is 256 of them as written, and the key that finds the face is capped at 256
    // as well, the same key for the name in the face and for the same long name in a drawing.
    let long = "İ".repeat(1000);
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(font(&long).build()).added, 1);
    assert_eq!(db.families().first().map(|f| f.chars().count()), Some(256));
    assert!(db.has_family(&long));
    assert_eq!(db.families().first().map(|f| diacraft_fonts::standin::key(f).chars().count()), Some(256));
}

#[test]
fn the_face_with_no_glyphs_shapes_right_to_left_text_in_visual_order() {
    let null = FontDb::empty().pick("x", REGULAR);
    let clusters = |rtl: bool| null.shape("abc", &ShapeParams { rtl, ..ShapeParams::default() }).iter().map(|g| g.cluster).collect::<Vec<_>>();
    assert_eq!(clusters(true), vec![2, 1, 0]);
    assert_eq!(clusters(false), vec![0, 1, 2]);
}

// ---- nominal shaping

#[test]
fn a_ligature_the_face_makes_is_not_made_at_nominal_advances_by_any_of_the_features_that_makes_them() {
    // The shaper turns on `liga`, `clig` and `calt` by itself; nominal shaping has to turn each off, and not only the first.
    for feature in [*b"liga", *b"clig", *b"calt"] {
        let mut f = font("Ligated");
        f.ligatures = vec![('f', 'i', 1300)];
        f.ligature_feature = feature;
        let f = f.advance('f', 500).advance('i', 300);
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(f.build()).added, 1);
        let face = db.pick("Ligated", REGULAR);
        let name = String::from_utf8_lossy(&feature).into_owned();
        let nominal = face.shape("fi", &ShapeParams::default());
        assert_eq!(nominal.iter().map(|g| g.x_advance).collect::<Vec<_>>(), vec![500, 300], "{name}: two glyphs, and the sum of the advances");
        // The table is there and works: with the shaper's own choices, one glyph and its advance.
        let natural = face.shape("fi", &ShapeParams { nominal: false, ..ShapeParams::default() });
        assert_eq!(natural.iter().map(|g| g.x_advance).collect::<Vec<_>>(), vec![1300], "{name}");
    }
}

// ---- the covering search remembers what it found

#[test]
fn a_covering_search_is_made_once_for_a_cluster_of_up_to_eight_characters() {
    let db = {
        let mut db = FontDb::empty();
        db.add_font_data(font("Plain").build());
        db
    };
    let at = |n: usize| -> Vec<char> { (0..n).filter_map(|i| char::from_u32(0x4E00 + i as u32)).collect() };
    for _ in 0..50 {
        assert!(db.covering(&at(1)).is_none());
    }
    assert_eq!(db.cover_searches(), 1, "one search, then the answer from memory");
    for _ in 0..3 {
        db.covering(&at(8));
    }
    assert_eq!(db.cover_searches(), 2, "eight characters are remembered too");
    for _ in 0..3 {
        db.covering(&at(9));
    }
    assert_eq!(db.cover_searches(), 5, "nine are not, and each call searches");
}

// ---- declared Unicode ranges skip faces that couldn't have the glyph

#[test]
fn a_face_that_declares_the_block_is_looked_at_before_one_that_declares_other_ranges() {
    // Both faces really have the letter, so only what they declare can decide: the first, which declares Basic Latin alone, is looked
    // at after the second, which declares the block, and the second is the answer. The bit numbers are the OpenType `OS/2` table's.
    let check = |letter: &str, bit: u32| {
        let d = Dir::new(&format!("ranges{bit}"));
        let mut other = font("A Declares Latin").chars(letter, 700);
        other.unicode_ranges = [1, 0, 0, 0];
        let mut wanted = font("B Declares Block").chars(letter, 700);
        wanted.unicode_ranges[(bit / 32) as usize] = 1 << (bit % 32);
        d.write("a.ttf", &other.build());
        d.write("b.ttf", &wanted.build());
        let db = d.db();
        let c = letter.chars().next().unwrap();
        assert_eq!(db.covering(&[c]).map(|f| f.family().to_owned()), Some("B Declares Block".to_owned()), "bit {bit}");
    };
    check("中", 59);
    check("א", 11);
    check("ب", 13);
    check("ก", 24);
    check("あ", 49);
    check("ア", 50);
    check("한", 56);
    check("र", 15);
}

#[test]
fn a_collection_is_read_to_16_mib_and_the_faces_after_that_are_left_out() {
    // Twenty faces that each have a name table of a million bytes of their own: a file is catalogued from at most 16 MiB of reads, so
    // sixteen are (16 million bytes) and the seventeenth, which would take it to 17 million, is not, however many the directory lists.
    let d = Dir::new("cap16");
    let fonts: Vec<Vec<u8>> = (0..20)
        .map(|i| {
            let mut f = font(&format!("Heavy Name {i:02}"));
            f.name_padding = 1_000_000;
            f.build()
        })
        .collect();
    let refs: Vec<&Vec<u8>> = fonts.iter().collect();
    d.write("heavy.ttc", &crate::add::collection(&refs));
    let db = d.db();
    let found = (0..20).filter(|i| db.has_family(&format!("Heavy Name {i:02}"))).count();
    assert_eq!(found, 16);
    assert!(db.has_family("Heavy Name 00") && db.has_family("Heavy Name 15") && !db.has_family("Heavy Name 16"));
}

#[test]
fn a_file_is_catalogued_from_exactly_16_mib_of_reads_and_not_a_byte_more() {
    // Twenty faces whose reads (the collection's header and list, then for each face its directory and its `name`, `OS/2`, `head` and
    // `maxp` tables) come to exactly 16 MiB, with the last face's name table sized to make it so; and the same with one byte more.
    let cap: usize = 16 << 20;
    let build = |extra: usize| -> Vec<Vec<u8>> {
        let mut fonts: Vec<Vec<u8>> = (0..20)
            .map(|i| {
                let mut f = font(&format!("Exact Name {i:02}"));
                f.name_padding = 830_000;
                f.build()
            })
            .collect();
        let reads = |fonts: &[Vec<u8>]| -> usize {
            let per_face: usize = fonts
                .iter()
                .map(|f| {
                    let tables = usize::from(u16::from_be_bytes([f[4], f[5]]));
                    12 + 16 * tables
                        + crate::add::table_len(f, b"name").unwrap()
                        + crate::add::table_len(f, b"OS/2").unwrap()
                        + crate::add::table_len(f, b"head").unwrap()
                        + crate::add::table_len(f, b"maxp").unwrap()
                })
                .sum();
            12 + 4 * fonts.len() + per_face
        };
        let short = cap + extra - reads(&fonts);
        let last = fonts.len() - 1;
        let mut f = font(&format!("Exact Name {last:02}"));
        f.name_padding = 830_000 + short;
        fonts[last] = f.build();
        assert_eq!(reads(&fonts), cap + extra, "the sum is as the test counts it");
        fonts
    };
    let found = |fonts: &[Vec<u8>], name: &str| {
        let d = Dir::new(name);
        let refs: Vec<&Vec<u8>> = fonts.iter().collect();
        d.write("exact.ttc", &crate::add::collection(&refs));
        let db = d.db();
        (0..20).filter(|i| db.has_family(&format!("Exact Name {i:02}"))).count()
    };
    assert_eq!(found(&build(0), "exact16"), 20, "all of 16 MiB are read");
    assert_eq!(found(&build(1), "exact16plus"), 19, "one byte more and the last face is left out");
}

#[test]
fn a_font_with_more_than_16384_separate_ranges_of_characters_is_indexed_up_to_the_16384th() {
    // Every other code point of the CJK block, 17,000 of them: each is a range of its own. The index keeps 16,384 ranges, and a face is
    // still loaded whole and answers for all of them.
    let letter = |i: u32| char::from_u32(0x4E00 + 2 * i).unwrap();
    let all: String = (0..17_000).map(letter).collect();
    let mut db = FontDb::empty();
    // No ASCII in it, which would be a range of its own.
    db.add_font_data(SynthFont::new("Sparse").chars(&all, 500).build());
    assert!(db.covering(&[letter(16_383)]).is_some(), "the 16,384th range is kept");
    assert!(db.covering(&[letter(16_384)]).is_none(), "the 16,385th is not");
    assert!(db.pick("Sparse", REGULAR).covers(letter(16_999)), "the face itself has it");
}

#[test]
fn a_face_that_declares_nothing_is_tried_and_so_is_a_block_the_table_does_not_know() {
    let d = Dir::new("undeclared");
    d.write("a.ttf", &font("Declares Nothing").chars("中", 1000).build());
    let mut latin = font("Declares Latin").chars("é⌘", 500);
    latin.unicode_ranges = [1, 0, 0, 0];
    d.write("b.ttf", &latin.build());
    let db = d.db();
    assert_eq!(db.covering(&['中']).map(|f| f.family().to_owned()), Some("Declares Nothing".to_owned()));
    // U+2318 is in no block the table knows, so a face declaring only Latin is still asked.
    assert_eq!(db.covering(&['⌘']).map(|f| f.family().to_owned()), Some("Declares Latin".to_owned()));
}

// ---- the folders

#[test]
fn folders_are_read_to_a_depth_of_8_and_not_9() {
    let d = Dir::new("depth9");
    d.write("a/b/c/d/e/f/g/h/at8.ttf", &font("At Depth 8").build());
    d.write("a/b/c/d/e/f/g/h/i/at9.ttf", &font("At Depth 9").build());
    let db = d.db();
    assert!(db.has_family("At Depth 8"));
    assert!(!db.has_family("At Depth 9"));
}

#[test]
fn a_font_file_is_a_font_by_its_extension_in_any_case() {
    let d = Dir::new("ext");
    for (name, family) in
        [("a.ttf", "E ttf"), ("b.OTF", "E otf"), ("c.ttc", "E ttc"), ("d.OTC", "E otc"), ("e.pfb", "E pfb"), ("f", "E none"), ("g.ttf.bak", "E bak")]
    {
        d.write(name, &font(family).build());
    }
    let db = d.db();
    for yes in ["E ttf", "E otf", "E ttc", "E otc"] {
        assert!(db.has_family(yes), "{yes}");
    }
    for no in ["E pfb", "E none", "E bak"] {
        assert!(!db.has_family(no), "{no}");
    }
}

#[test]
fn of_two_faces_with_one_name_the_file_that_sorts_first_is_the_one_found() {
    // Written in the wrong order on purpose: the catalogue does not depend on the order a folder lists its files in.
    let d = Dir::new("sorted");
    d.write("b.ttf", &SynthFont::new("Twin").ascii(250).build());
    d.write("a.ttf", &SynthFont::new("Twin").ascii(500).build());
    let db = d.db();
    let face = db.pick("Twin", REGULAR);
    assert_eq!(face.advance(face.glyph_for('a')), 500.0);
}

#[test]
fn the_xdg_variables_name_absolute_folders_only_and_the_list_has_64() {
    let p = |s: &str| PathBuf::from(s);
    assert_eq!(
        xdg_font_dirs(Some("/home/u/.local/share"), Some("/usr/share:/opt/share")),
        vec![p("/home/u/.local/share/fonts"), p("/usr/share/fonts"), p("/opt/share/fonts")]
    );
    // No variables, or an empty list, is the default pair.
    assert_eq!(xdg_font_dirs(None, None), vec![p("/usr/local/share/fonts"), p("/usr/share/fonts")]);
    assert_eq!(xdg_font_dirs(None, Some("")), vec![p("/usr/local/share/fonts"), p("/usr/share/fonts")]);
    // A relative folder is ignored, in either variable.
    assert_eq!(xdg_font_dirs(Some("relative/dir"), Some("rel:/abs:also/rel")), vec![p("/abs/fonts")]);
    let many = (0..100).map(|i| format!("/d{i}")).collect::<Vec<_>>().join(":");
    let dirs = xdg_font_dirs(None, Some(&many));
    assert_eq!(dirs.len(), 64);
    assert_eq!(dirs.last(), Some(&p("/d63/fonts")));
    assert!(Path::new("/x").is_absolute());
}
