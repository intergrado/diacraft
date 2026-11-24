//! Hostile names and hostile faces: a drawing names a font, and a font file may be damaged. Nothing here may panic, and the work
//! has to stay bounded whatever the input is. Where a cost is the claim it is counted (`diacraft_fonts::work`: characters of a language
//! tag read, bytes read from a file, path elements held), which does not move with the load on the machine; the bounds on time that remain
//! are hang guards of ten seconds or a minute, for tests that take well under a second.

use std::time::{Duration, Instant};

use diacraft_fonts::standin::key;
use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::work;
use diacraft_fonts::{FontDb, ShapeParams};
use proptest::prelude::*;

use crate::{BOLD, REGULAR};

/// The key is cut at 256 characters (asserted below); the ten seconds are a hang guard and no measure of cost (the test takes milliseconds).
#[test]
fn a_family_name_of_ten_megabytes_costs_a_bounded_amount() {
    let db = FontDb::bundled();
    let huge = "A".repeat(10 << 20);
    let start = Instant::now();
    let r = db.resolve(&huge, REGULAR);
    assert!(r.face.covers('a'));
    assert!(!db.has_family(&huge));
    assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
    assert!(key(&huge).chars().count() <= 256, "the key is capped");
}

/// As above: the name is the empty name, which is the claim; the ten seconds are a hang guard.
#[test]
fn a_name_of_ten_megabytes_of_spaces_and_controls_is_the_empty_name() {
    let db = FontDb::bundled();
    let junk = " \u{0}\u{7}\t\n".repeat(2 << 20);
    let start = Instant::now();
    assert_eq!(key(&junk), "");
    let r = db.resolve(&junk, BOLD);
    assert!(r.face.covers('a'));
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn names_that_look_like_paths_or_markup_are_only_ever_names() {
    // A name is matched against the catalogue; it is never opened as a file or run as anything.
    let db = FontDb::bundled();
    for name in [
        "../../../etc/passwd",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/dev/zero",
        "<script>alert(1)</script>",
        "\u{202E}evil",
        "Hack\0Hidden",
        "%s%s%s%n",
        "' OR 1=1 --",
    ] {
        let r = db.resolve(name, REGULAR);
        assert!(r.face.covers('a'), "{name:?}");
    }
    // A NUL inside a name is dropped, so it is the name without it: this one finds Hack.
    assert_eq!(db.resolve("Hack\0", REGULAR).face.family(), "Hack");
}

#[test]
fn keys_are_lower_case_with_the_spaces_squeezed() {
    assert_eq!(key("  Times   New\tRoman "), "times new roman");
    assert_eq!(key("ÀB"), "àb");
    assert_eq!(key(""), "");
    assert_eq!(key("0"), "0");
}

#[test]
fn the_name_that_failed_the_ci_gives_a_key_of_at_most_256_characters() {
    // The smallest name proptest found for `any_name_resolves_to_a_face_that_can_measure` on GitHub (main, d7df42c), kept as it was
    // printed: under 300 characters, one of them `İ` (U+0130), which lower-cases to two characters, so the 256 taken from the name
    // were more than 256 once folded.
    let name = "*כּ`*ᝰYn𐠸𝓣ጒ./𞟦ਫ਼꓅^l\\`NῗtbȺ:$ꦟￌC%::Ü{b/\"𐕹ৈ¥D꩷Ⱥv𑍢\u{16af4}Ⱥ𐻄B'&/=*a$:*=&\"..|�ⷍ¥`ÞWOখ�࠽ü𖨢\\\u{a4c}¥V=Ls𝋫O�*𑵗𑊈🕴𐠼<FἼ$𑾰🕴🕴𑝄𐔌はp*𐴝t$Ὑ🉄Ⱥ𞣏l𐼌<%w$Ⴭ$%\u{ac7}ୈ&𐠂i𑈄v&𝔽$N<𐠸מּౘ?ଵİ{𐖬nਸZ/*🫳:?🕴茎ஞ7&iMȺ{Ὕ=_`4𐗢'ে<𡚨𑄛J𐩣⵰/.Ⱥ.`\":Êແ\"?I`'\u{ac3}/3Ëu*t/=ศቪ\u{20da}'\\?﹒2𝔊ﭲ\\ቝ%{𞹢𛲜i*¥𐢬Ѩe'𑵨{?%>⵰$\u{e0152}9ଛ¥/MF~ﻡ`ଂE<🕴J𜵸Ѩ?&\u{20ee}Ｏ\u{dca}nଧ𞹗𞺩ⲌﳟHb𐖹/H";
    assert!(name.chars().count() <= 300, "a name the generator can make");
    assert!(name.contains('İ'));
    let k = key(name);
    assert!(k.chars().count() <= 256, "the key has {} characters", k.chars().count());
}

#[test]
fn a_run_of_dotted_capital_i_gives_a_key_of_exactly_256_characters() {
    // `İ` folds to `i` and a combining dot above, so 256 of them are 512 characters folded: the key stops at 256, counted after the
    // fold and not before it.
    let key_of = |n: usize| key(&"İ".repeat(n));
    assert_eq!(key_of(256).chars().count(), 256);
    assert_eq!(key_of(256), "i\u{307}".repeat(128));
    assert_eq!(key_of(128).chars().count(), 256, "the most that fits whole");
    assert_eq!(key_of(127).chars().count(), 254, "below the cap nothing is cut");
    // An odd count of characters to keep is cut between the pair.
    assert_eq!(key(&format!("a{}", "İ".repeat(256))).chars().count(), 256);
    assert!(key(&format!("a{}", "İ".repeat(256))).ends_with('i'));
    // A space is kept only when a character follows it inside the cap: no key ends in one.
    let dotted = "i\u{307}".repeat(127);
    assert_eq!(key(&format!("{}  b", "İ".repeat(127))), format!("{dotted} b"), "a space and a letter fit: 254, 1, 1");
    assert_eq!(key(&format!("{}a  b", "İ".repeat(127))), format!("{dotted}a"), "a space and no room for the letter after it");
    // A megabyte of them gives the same key as the first 256 characters of it do.
    assert_eq!(key_of(1 << 20), key_of(256));
}

/// Never panics, which is the claim; the minute at the end is a hang guard over the sweep, which takes well under a second.
#[test]
fn a_font_with_every_byte_flipped_in_turn_never_panics() {
    // Each single-byte corruption of a built font, then every operation a layout does with a face. A parse that fails is fine.
    let bytes = SynthFont::new("Flip").ascii(500).chars("אב", 400).build();
    let start = Instant::now();
    for at in (0..bytes.len()).step_by(3) {
        for xor in [0xFF, 0x80, 0x01] {
            let mut copy = bytes.clone();
            copy[at] ^= xor;
            let mut db = FontDb::empty();
            if db.add_font_data(copy).added == 1 {
                let face = db.pick("Flip", REGULAR);
                let _ = (
                    face.units_per_em(),
                    face.covers('a'),
                    face.glyph_for('b'),
                    face.advance(2),
                    face.outline(2),
                    face.underline(),
                    face.script_box(true),
                );
                let _ = face.shape("abc אב", &ShapeParams::default());
                let _ = face.shape("אב", &ShapeParams { rtl: true, ..ShapeParams::default() });
            }
        }
    }
    assert!(start.elapsed() < Duration::from_secs(60), "took {:?}", start.elapsed());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_bytes_are_never_a_panic_as_a_font(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let mut db = FontDb::empty();
        let _ = db.add_font_data(bytes);
        prop_assert!(db.pick("anything", REGULAR).advance(0) >= 0.0);
    }

    #[test]
    fn random_bytes_after_a_valid_header_are_never_a_panic(tail in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut bytes = SynthFont::new("Head").ascii(500).build();
        bytes.truncate(12 + 16 * 10);
        bytes.extend(tail);
        let mut db = FontDb::empty();
        if db.add_font_data(bytes).added == 1 {
            let face = db.pick("Head", REGULAR);
            let _ = face.shape("hello", &ShapeParams::default());
        }
    }

    #[test]
    fn any_name_resolves_to_a_face_that_can_measure(name in "\\PC{0,300}") {
        let db = FontDb::bundled();
        let face = db.pick(&name, REGULAR);
        prop_assert!(face.advance(face.glyph_for('a')) > 0.0);
        prop_assert!(key(&name).chars().count() <= 256);
    }
}

/// Run `f`, counting the panics it makes on this thread without printing them. Other threads' panics (other tests contain some of
/// their own) still reach the hook that was there, which is handed back before this returns.
fn count_panics<T>(f: impl FnOnce() -> T) -> (T, usize) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    // The panic hook is the process's, and tests run in threads: one window at a time, or one test's hook would be taken by another.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _window = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let counted = Arc::new(AtomicUsize::new(0));
    let (mine, count) = (std::thread::current().id(), counted.clone());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == mine {
            count.fetch_add(1, Ordering::SeqCst);
        } else {
            previous(info);
        }
    }));
    let out = f();
    let _ = std::panic::take_hook();
    (out, counted.load(Ordering::SeqCst))
}

/// A synthetic face in a database of its own, whole: nothing has failed yet.
fn whole_face() -> diacraft_fonts::Face {
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(SynthFont::new("Panics").ascii(600).build()).added, 1);
    let face = db.pick("Panics", REGULAR);
    assert!(!face.is_broken(), "a face is whole until it fails");
    assert_ne!(face.glyph_for('a'), 0, "the face reads: what follows is not a face that never could");
    assert!(!face.shape_checked("abc", &ShapeParams::default()).1, "the shaper is in use");
    face
}

/// The panic of a read of the tables, raised inside the containment every such read runs in. `read-fonts` 0.41.0 had a panic that a
/// damaged font provoked (in the `glyf` reader, on a glyph with no points), and these tests used it. 0.46 has none that a one-byte
/// change of a built font provokes in the reads a layout and a renderer make (searched: 129,832 changes, every byte of three
/// built fonts with eight values each; the same search on 0.41.0 found 256), so nothing here depends on a font that provokes one: the
/// panic is injected where one would be raised. This one is raised by the test's own closure, outside the reader, in a face the
/// database handed out; the tests at the end of this file raise it inside the reader (`Face::with_table_watch`).
fn the_reader_panics(face: &diacraft_fonts::Face) -> (Option<()>, usize) {
    count_panics(|| face.guarded_read(|| panic!("the reader met a table it could not take")))
}

#[test]
fn a_face_that_makes_the_reader_panic_is_set_aside_after_one_panic_and_says_so() {
    let face = whole_face();
    let (read, panics) = the_reader_panics(&face);
    assert_eq!((read, panics), (None, 1), "the panic is contained, and counted once");
    assert!(face.is_broken());
    // From then on the face answers as one with nothing in it, without calling the reader again: the font is whole, and still
    // none of this reads it.
    assert_eq!(face.outline_glyph(1), diacraft_fonts::Outline::Broken);
    assert!(!face.covers('a'));
    assert_eq!(face.glyph_for('a'), 0);
    assert_eq!(face.advance(1), 500.0, "half an em, as for a face that can't say");
    assert!(!face.may_embed());
    let (glyphs, fell_back) = face.shape_checked("abc", &ShapeParams::default());
    assert!(fell_back);
    assert_eq!(glyphs.len(), 3, "one glyph a character, from the cmap alone, which is empty now");
}

#[test]
fn a_face_that_panicked_is_not_called_again_so_the_panic_is_one_and_not_one_a_call() {
    let face = whole_face();
    let reads = std::cell::Cell::new(0);
    let (_, panics) = count_panics(|| {
        for _ in 0..3 {
            let _: Option<()> = face.guarded_read(|| {
                reads.set(reads.get() + 1);
                panic!("the reader met a table it could not take")
            });
        }
    });
    assert!(face.is_broken());
    assert_eq!(panics, 1, "three rounds of the same read, one panic: the face is set aside by the first");
    assert_eq!(reads.get(), 1, "the later reads were not run");
}

#[test]
fn a_face_set_aside_stays_set_aside_for_every_clone_of_it() {
    // A layout holds clones of a face; the database holds another. One failure is every holder's.
    let face = whole_face();
    let other = face.clone();
    assert_eq!(the_reader_panics(&other).1, 1);
    assert!(face.is_broken() && other.is_broken());
}

// ---- a panic raised inside the reader, in the middle of a real read ----
//
// `guarded_read` raises its panic outside the reader. `Face::with_table_watch` hands the reader its tables through a function that
// the test can make panic when one is asked for, so the panic is raised where a damaged font's would be: below the call a face makes.

/// A face of a built font whose reader panics when it first asks for the `tag` table, which nothing read while the face was made.
fn face_whose_reader_panics_on(tag: &'static [u8; 4]) -> diacraft_fonts::Face {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    let armed = Arc::new(AtomicBool::new(false));
    let asked = Arc::new(Mutex::new(Vec::<[u8; 4]>::new()));
    let (armed_in, asked_in) = (armed.clone(), asked.clone());
    let bytes = SynthFont::new("Watched").ascii(600).chars("אב", 400).build();
    let face = diacraft_fonts::Face::with_table_watch(bytes, move |t| {
        asked_in.lock().unwrap_or_else(PoisonError::into_inner).push(t);
        if armed_in.load(Ordering::SeqCst) && &t == tag {
            panic!("the reader met a table it could not take: {}", String::from_utf8_lossy(&t));
        }
    })
    .expect("a face");
    let before = asked.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert!(!before.is_empty(), "the face was made by reading some tables: {before:?}");
    assert!(!before.contains(tag), "making the face reads no {tag:?} table, so the panic is for a later read");
    assert!(!face.is_broken());
    armed.store(true, Ordering::SeqCst);
    face
}

#[test]
fn a_panic_while_the_reader_reads_the_cmap_is_contained_and_sets_the_face_aside() {
    let face = face_whose_reader_panics_on(b"cmap");
    let (covers, panics) = count_panics(|| face.covers('a'));
    assert_eq!((covers, panics), (false, 1), "the read of the character map panicked, once, and the face says it covers nothing");
    assert!(face.is_broken());
    assert_eq!(face.glyph_for('a'), 0);
    assert!(!face.covers('b'));
    let (glyphs, fell_back) = face.shape_checked("abc", &ShapeParams::default());
    assert!(fell_back);
    assert_eq!(glyphs.len(), 3, "one glyph a character, as for any face that is set aside");
}

#[test]
fn a_panic_while_the_reader_reads_the_outlines_is_contained_and_sets_the_face_aside() {
    let face = face_whose_reader_panics_on(b"glyf");
    let (outline, panics) = count_panics(|| face.outline_glyph(1));
    assert_eq!((outline, panics), (diacraft_fonts::Outline::Broken, 1));
    assert!(face.is_broken());
    assert_eq!(face.outline_glyph(2), diacraft_fonts::Outline::Broken, "nothing is read from a face that is set aside");
    assert_eq!(face.glyph_for('a'), 0);
}

#[test]
fn a_panic_while_the_reader_reads_the_advances_is_contained_and_gives_half_an_em() {
    let face = face_whose_reader_panics_on(b"hmtx");
    let (advance, panics) = count_panics(|| face.advance(1));
    assert_eq!((advance, panics), (500.0, 1), "half an em, as for a face that can't say");
    assert!(face.is_broken());
}

#[test]
fn a_panic_while_the_shaper_reads_the_layout_tables_is_contained_and_the_text_is_measured_from_the_cmap() {
    let face = face_whose_reader_panics_on(b"GSUB");
    let (shaped, panics) = count_panics(|| face.shape_checked("abc", &ShapeParams::default()));
    assert_eq!(panics, 1);
    assert!(shaped.1, "the shaper could not be used");
    assert_eq!(shaped.0.len(), 3);
    assert!(face.is_broken());
}

#[test]
fn a_panic_in_the_reader_while_a_face_is_made_makes_no_face_and_no_panic_escapes() {
    let bytes = SynthFont::new("Never").ascii(600).build();
    let (face, panics) = count_panics(|| {
        diacraft_fonts::Face::with_table_watch(bytes, |t| {
            if &t == b"head" {
                panic!("the reader met a table it could not take");
            }
        })
    });
    assert!(face.is_none());
    assert_eq!(panics, 1);
}

// ---- a name table that costs the reader more than its size says ----

/// The font `base` with its `tag` table replaced by `new`.
pub fn with_table(base: &[u8], tag: &[u8; 4], new: Vec<u8>) -> Vec<u8> {
    with_tables(base, &[(*tag, new)])
}

/// The font `base` with each of the tables in `new` put in place of the one of its tag, or added if it has none; the directory is kept
/// sorted by tag.
pub fn with_tables(base: &[u8], new: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = usize::from(u16::from_be_bytes([base[4], base[5]]));
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for i in 0..n {
        let rec = 12 + i * 16;
        let t: [u8; 4] = base[rec..rec + 4].try_into().unwrap();
        let off = u32::from_be_bytes(base[rec + 8..rec + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(base[rec + 12..rec + 16].try_into().unwrap()) as usize;
        if !new.iter().any(|(tag, _)| *tag == t) {
            tables.push((t, base[off..off + len].to_vec()));
        }
    }
    tables.extend(new.iter().cloned());
    tables.sort_by_key(|(t, _)| *t);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    for (t, d) in &tables {
        out.extend_from_slice(t);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        offset += d.len().div_ceil(4) * 4;
    }
    for (_, d) in &tables {
        out.extend_from_slice(d);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    out
}

/// A version 1 `name` table of `count` records. Each is a Windows English-less family name `"A"` whose language is the table's one
/// language tag, `tag_chars` characters of ASCII long. `count` is at most 5,400 here: the offset of the strings is 16 bits.
fn name_table_of_long_language_tags(count: u16, tag_chars: u16) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&count.to_be_bytes());
    t.extend_from_slice(&((6 + 12 * usize::from(count) + 2 + 4) as u16).to_be_bytes());
    for _ in 0..count {
        // Windows, Unicode BMP, the first language tag, name id 1, two bytes long, at the start of the strings.
        for v in [3u16, 1, 0x8000, 1, 2, 0] {
            t.extend_from_slice(&v.to_be_bytes());
        }
    }
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&(tag_chars * 2).to_be_bytes());
    t.extend_from_slice(&2u16.to_be_bytes());
    t.extend_from_slice(&0x0041u16.to_be_bytes());
    for _ in 0..tag_chars {
        t.extend_from_slice(&0x0061u16.to_be_bytes());
    }
    t
}

/// A version 1 `name` table of 65,535 records, the most there can be, that does not fit its own string offset: the strings start at the
/// seventh byte, on the records themselves. The one language tag is 32,766 characters of the first 5,461 records, each of whose six 16-bit
/// fields is a character below 0x80, so it is ASCII all through; the 60,000 or so records after them each name it.
fn name_table_of_records_that_are_their_own_language_tag() -> Vec<u8> {
    let count = u16::MAX;
    let mut t = Vec::new();
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&count.to_be_bytes());
    t.extend_from_slice(&6u16.to_be_bytes());
    for i in 0..usize::from(count) {
        // Windows, Unicode BMP, no language for the first records and the first tag for the rest, id 1, two bytes, at the first byte.
        let language = if i < 5_461 { 0u16 } else { 0x8000 };
        for v in [3u16, 1, language, 1, 2, 0] {
            t.extend_from_slice(&v.to_be_bytes());
        }
    }
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&65_532u16.to_be_bytes());
    t.extend_from_slice(&0u16.to_be_bytes());
    t
}

#[test]
fn thousands_of_name_records_that_each_name_a_very_long_language_tag_cost_a_bounded_amount() {
    // The reader checks that a language tag is ASCII over all of its length, 32,767 characters here, once for each record it looks at:
    // 5,400 records took 0.3 s a call to choose a name, where the reader before it stopped at 30 characters. A face is named three
    // times in being catalogued and made, the family twice. Such a font took 0.7 s with every record looked at (45 s for these sixty-four),
    // 2.9 s for the sixty-four with 128 records looked at, and takes a few hundredths of a second for them with the few characters of a
    // tag that tell English from the rest read.
    let base = SynthFont::new("Tagged").ascii(500).build();
    let font = with_table(&base, b"name", name_table_of_long_language_tags(5_400, 32_767));
    work::reset();
    for i in 0..64 {
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(font.clone()).added, 1, "font {i} is a face, named by the one family name it has");
        assert_eq!(db.pick("A", REGULAR).family(), "A");
    }
    // Counted, not timed: every record that names the tag is looked at, and no tag is read past its sixth character (the reading of all
    // 32,767 would be 5,000 times that for each record).
    tags_are_read_to_their_prefix_only();
    assert!(work::language_tags() <= 64 * 8 * 5_400, "{} records looked at", work::language_tags());
}

/// What the counts of the work must show of a font whose records name a language tag of tens of thousands of characters: tags were read,
/// and none past its sixth character (`LANGUAGE_TAG_PREFIX`).
#[track_caller]
fn tags_are_read_to_their_prefix_only() {
    let (tags, chars, longest) = (work::language_tags(), work::language_tag_chars(), work::longest_tag_read());
    assert!(tags > 0, "no tag was read: the count does not count");
    assert!(longest <= 6, "{longest} characters of one tag were read, of {chars} in all, in {tags} tags: a tag is read past its sixth character");
}

#[test]
fn sixty_five_thousand_name_records_that_name_one_language_tag_of_thirty_two_thousand_characters_cost_a_bounded_amount() {
    let base = SynthFont::new("Overlap").ascii(500).build();
    let font = with_table(&base, b"name", name_table_of_records_that_are_their_own_language_tag());
    work::reset();
    for _ in 0..16 {
        let mut db = FontDb::empty();
        // The font may or may not be named by this table: it must not cost much either way, and it must not fail the lookup.
        let _ = db.add_font_data(font.clone());
        assert!(db.pick("anything", REGULAR).advance(0) >= 0.0);
    }
    // Counted, not timed: 60,000 records name the one tag of 32,000 characters, and none of them reads past the sixth.
    tags_are_read_to_their_prefix_only();
    assert!(work::language_tags() <= 16 * 8 * 65_535, "{} records looked at", work::language_tags());
}

/// A version 1 `name` table of family names (id 1) in Windows records: each is the text and a language id, and an id of 0x8000 and up
/// names the tag of that number in `tags`.
fn name_table_with_tags(records: &[(u16, &str)], tags: &[&str]) -> Vec<u8> {
    let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_be_bytes).collect() };
    let (mut strings, mut record_bytes, mut tag_bytes) = (Vec::new(), Vec::new(), Vec::new());
    for (language, text) in records {
        let units = utf16(text);
        for v in [3u16, 1, *language, 1, units.len() as u16, strings.len() as u16] {
            record_bytes.extend_from_slice(&v.to_be_bytes());
        }
        strings.extend(units);
    }
    for tag in tags {
        let units = utf16(tag);
        tag_bytes.extend_from_slice(&(units.len() as u16).to_be_bytes());
        tag_bytes.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        strings.extend(units);
    }
    let mut t = Vec::new();
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&(records.len() as u16).to_be_bytes());
    t.extend_from_slice(&((6 + record_bytes.len() + 2 + tag_bytes.len()) as u16).to_be_bytes());
    t.extend(record_bytes);
    t.extend_from_slice(&(tags.len() as u16).to_be_bytes());
    t.extend(tag_bytes);
    t.extend(strings);
    t
}

/// The one family name a built font with this `name` table has.
fn family_named_by(table: Vec<u8>) -> String {
    let base = SynthFont::new("Names").ascii(500).build();
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(with_table(&base, b"name", table)).added, 1);
    db.families().into_iter().next().unwrap_or_default()
}

#[test]
fn a_family_name_is_the_us_english_one_then_the_english_one_then_one_with_no_language_then_the_first() {
    let records = [(0x8000, "Chinese"), (0x8001, "Second"), (0x8002, "Third")];
    for (tags, expected, why) in [
        (["zh-CN", "en-US", "fr"], "Second", "US English wins wherever it is"),
        (["zh-CN", "fr", "en-US"], "Third", "US English, last of the records"),
        (["zh-CN", "en", "fr"], "Second", "English, when there is no US English"),
        (["zh-CN", "fr", "de"], "Chinese", "none of them English: the first"),
        (["en-GB", "en-USA", "fr"], "Chinese", "a tag that starts like US English, or like English, is not it: the first"),
    ] {
        assert_eq!(family_named_by(name_table_with_tags(&records, &tags)), expected, "{why}: {tags:?}");
    }
    // A record whose tag is not ASCII, or that has no tag, names no language, which ranks above a language that is not English.
    assert_eq!(family_named_by(name_table_with_tags(&[(0x8000, "First"), (0x8001, "No language")], &["fr", "é"])), "No language");
    assert_eq!(family_named_by(name_table_with_tags(&[(0x8000, "First"), (0x8005, "No tag")], &["fr"])), "No tag");
    // A tag of 32,000 characters that is not English, before the English one: the English one is found, and quickly.
    let long = "a".repeat(32_000);
    work::reset();
    assert_eq!(family_named_by(name_table_with_tags(&[(0x8000, "Long"), (0x8001, "English")], &[long.as_str(), "en-US"])), "English");
    // Found by reading a few characters of the long tag, not 32,000.
    tags_are_read_to_their_prefix_only();
}

/// A version 0 `name` table with `count` family-name records in Chinese and one for US English after them.
fn name_table_with_english_last(count: u16) -> Vec<u8> {
    let names: Vec<(u16, u16, String)> = (0..count).map(|i| (3, 0x0804, format!("Other{i}"))).chain([(3, 0x0409, "English".to_owned())]).collect();
    let mut strings = Vec::new();
    let mut records = Vec::new();
    for (platform, language, text) in &names {
        let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [*platform, 1, *language, 1, units.len() as u16, strings.len() as u16] {
            records.extend_from_slice(&v.to_be_bytes());
        }
        strings.extend(units);
    }
    let mut t = Vec::new();
    t.extend_from_slice(&0u16.to_be_bytes());
    t.extend_from_slice(&(names.len() as u16).to_be_bytes());
    t.extend_from_slice(&((6 + records.len()) as u16).to_be_bytes());
    t.extend(records);
    t.extend(strings);
    t
}

#[test]
fn the_english_name_is_chosen_wherever_it_is_among_the_records_of_an_id() {
    for count in [10, 127, 128, 129, 1_000] {
        assert_eq!(family_named_by(name_table_with_english_last(count)), "English", "after {count} records of another language");
    }
}

// ---- what the reader does with a font whose tables are missing or empty ----

#[test]
fn a_font_that_states_no_glyph_count_is_not_a_face() {
    // The reader limits a character map by the glyph count and gives glyphs that are zero wide when there is none, so a font without one
    // would be a face that covers nothing and measures nothing: it is refused, as a font with no `head` table is.
    let base = SynthFont::new("No Maxp").ascii(500).build();
    let mut intact = FontDb::empty();
    assert_eq!(intact.add_font_data(base.clone()).added, 1);
    for (what, maxp) in [("an empty maxp table", Vec::new()), ("a maxp table of no glyphs", vec![0, 0, 0x50, 0, 0, 0])] {
        let mut db = FontDb::empty();
        let report = db.add_font_data(with_table(&base, b"maxp", maxp));
        assert_eq!((report.added, report.rejected), (0, 1), "{what}");
    }
}

#[test]
fn a_glyph_past_the_end_of_the_font_and_a_font_without_readable_widths_measure_half_an_em() {
    let base = SynthFont::new("Widths").ascii(600).build();
    let advance_of = |bytes: Vec<u8>, gid: u32| {
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(bytes).added, 1);
        db.pick("Widths", REGULAR).advance(gid)
    };
    let a = SynthFont::new("Widths").ascii(600).gid('a');
    assert_eq!(advance_of(base.clone(), u32::from(a)), 600.0, "the width the font states");
    for gid in [1_000u32, 65_535, 1_000_000, u32::MAX] {
        assert_eq!(advance_of(base.clone(), gid), 500.0, "glyph {gid} is past the end of the font");
    }
    let no_widths = with_table(&base, b"hmtx", Vec::new());
    assert_eq!(advance_of(no_widths, u32::from(a)), 500.0, "a font whose widths can't be read says half an em, as a face that can't say does");
}

#[test]
fn a_type_1_font_is_no_face() {
    // The reader also takes a Type 1 font and a bare CFF font, which have no `head` table to give a face: they are refused, not parsed.
    let mut db = FontDb::empty();
    let type1 = b"%!PS-AdobeFont-1.0: Fake 001.000\n/FontName /Fake def\ncurrentfile eexec\n".to_vec();
    let report = db.add_font_data(type1);
    assert_eq!((report.added, report.rejected), (0, 1));
}
