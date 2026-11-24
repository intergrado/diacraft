//! Layout on the faces the machine has. Synthetic faces say whether a rule is applied; only a real one says whether Arabic joins, a
//! language picks its forms, and the search for a fallback finds the right face, so these run on whatever this machine has and say
//! `SKIPPED` on standard error, with the reason, where it lacks what a test needs. No test depends on a particular installation: each
//! asks the database for a face that has the characters it needs.

use std::sync::OnceLock;

use diacraft_fonts::{Face, FontDb, ShapeParams};
use diacraft_text::{BlockFormat, CharFormat, Laid, Note, lay_out};

use crate::{gids, left, run};

fn machine() -> &'static FontDb {
    static DB: OnceLock<FontDb> = OnceLock::new();
    DB.get_or_init(FontDb::system_and_bundled)
}

fn block() -> BlockFormat {
    BlockFormat { vertical_align: 0, height: Some(4.0), ..BlockFormat::default() }
}

fn skipped(why: &str) {
    eprintln!("SKIPPED: {why}");
}

/// A face that has all of `chars`, found as the layout would find one, as the regular face of its family: a name asks for a family, and
/// the face that has the letters may be a bold or a condensed one of it, whose glyphs are not the regular face's.
fn face_with(chars: &[char]) -> Option<Face> {
    let found = machine().covering(chars)?;
    let regular = machine().pick(found.family(), Default::default());
    chars.iter().all(|c| regular.covers(*c)).then_some(regular)
}

fn format(face: &Face, size: f64) -> CharFormat {
    let name = face.family().to_owned();
    CharFormat { font: name.clone(), asian_font: name.clone(), complex_font: name, size, ..CharFormat::default() }
}

fn lay(runs: &[diacraft_text::Run]) -> Laid {
    lay_out(runs, &block(), None, machine())
}

#[test]
fn arabic_with_a_tatweel_in_it_joins_as_the_shaper_alone_joins_it() {
    let text = "ب\u{0640}ب";
    let Some(face) = face_with(&['ب', '\u{0640}']) else { return skipped("no face has Arabic beh and tatweel") };
    // The run's language is Arabic, which is what the shaper takes for Arabic letters when none is given.
    let c = CharFormat { lang: "ar".into(), ..format(&face, 0.2) };
    let laid = lay(&[run(text, &c, &left())]);
    let direct = face.shape(text, &ShapeParams { rtl: true, script: Some(*b"Arab"), language: Some("ar"), ..ShapeParams::default() });
    let want: Vec<u32> = direct.iter().map(|g| g.gid).collect();
    assert_eq!(gids(&laid.lines[0]), want, "{}: the layout is the shaper's answer for the whole word", face.family());
    // Not each letter alone: the isolated forms are what a split into three items gave.
    let isolated: Vec<u32> = text.chars().rev().map(|ch| face.glyph_for(ch)).collect();
    if isolated == want {
        skipped(&format!("{} has no joining forms to tell joined from isolated", face.family()));
    }
}

#[test]
fn arabic_joins_across_a_change_of_format_in_the_middle_of_a_word() {
    let Some(face) = face_with(&['ب', 'ت', 'ث']) else { return skipped("no face has Arabic") };
    let arabic = CharFormat { lang: "ar".into(), ..format(&face, 0.2) };
    let (a, b) = (arabic.clone(), CharFormat { size: 0.3, color: diacraft_text::Rgba { r: 255, g: 0, b: 0, a: 255 }, ..arabic });
    let one = lay(&[run("بتث", &a, &left())]);
    let split = lay(&[run("ب", &a, &left()), run("ت", &b, &left()), run("ث", &a, &left())]);
    assert_eq!(gids(&one.lines[0]), gids(&split.lines[0]), "{}", face.family());
}

#[test]
fn a_tab_between_hebrew_words_keeps_the_words_in_their_order_on_a_real_face() {
    let Some(face) = face_with(&['א', 'ב', 'ג', 'ד']) else { return skipped("no face has Hebrew") };
    let c = format(&face, 0.2);
    let laid = lay(&[run("אב\tגד", &c, &left())]);
    let g = |ch: char| face.glyph_for(ch);
    assert_eq!(gids(&laid.lines[0]), vec![g('ב'), g('א'), g('ד'), g('ג')], "{}: bet alef, then dalet gimel", face.family());
    // Right to left paragraph, Latin words: the first stays on the right of the tab. On a face that has the Latin letters: a Hebrew face need not
    // (macOS's has none, and the layout rightly drew them from another face).
    let Some(latin) = face_with(&['a', 'b', 'c', 'd']) else { return skipped("no face has the Latin letters") };
    let (c, g) = (format(&latin, 0.2), |ch: char| latin.glyph_for(ch));
    let p = diacraft_text::ParaFormat { flags: 1, halign: 0, ..diacraft_text::ParaFormat::default() };
    let laid = lay(&[run("ab\tcd", &c, &p)]);
    assert_eq!(gids(&laid.lines[0]), "cdab".chars().map(g).collect::<Vec<_>>(), "{}", latin.family());
}

#[test]
fn a_language_picks_the_forms_of_a_han_letter_the_way_the_shaper_does() {
    let candidates = ['直', '骨', '今', '刃', '令', '户', '海', '角', '返', '飞'];
    let langs = ["ja-JP", "zh-CN", "zh-TW", "ko-KR"];
    // The faces of the families that are CJK by name, then any that has the letter: the first that has it need not be one with `locl`.
    let mut faces: Vec<Face> =
        machine().families().iter().filter(|f| f.contains("CJK") || f.contains("Han ")).map(|f| machine().pick(f, Default::default())).collect();
    faces.extend(candidates.iter().filter_map(|ch| face_with(&[*ch])));
    let found = faces.iter().find_map(|face| {
        candidates.iter().find_map(|ch| {
            if !face.covers(*ch) {
                return None;
            }
            let by = |lang: &str| face.shape(&ch.to_string(), &ShapeParams { language: Some(lang), ..ShapeParams::default() }).first().map(|g| g.gid);
            let forms: Vec<Option<u32>> = langs.iter().map(|l| by(l)).collect();
            forms.iter().any(|f| *f != forms[0]).then(|| (*ch, face.clone()))
        })
    });
    let Some((ch, face)) = found else { return skipped("no Han face on this machine has language-specific forms for the letters tried") };
    let by_layout = |lang: &str| {
        let c = CharFormat { lang: lang.into(), ..format(&face, 0.2) };
        gids(&lay(&[run(&ch.to_string(), &c, &left())]).lines[0])
    };
    let by_shaper = |lang: &str| vec![face.shape(&ch.to_string(), &ShapeParams { language: Some(lang), ..ShapeParams::default() })[0].gid];
    for lang in langs {
        assert_eq!(by_layout(lang), by_shaper(lang), "{lang} for {ch} in {}", face.family());
    }
    let distinct: std::collections::HashSet<Vec<u32>> = langs.iter().map(|l| by_layout(l)).collect();
    assert!(distinct.len() > 1, "the language reached the shaper");
}

#[test]
fn hangul_and_khmer_after_a_letter_nobody_has_still_find_their_faces() {
    // The first letter makes the fallback search look at every face; it once spent the database's budget, and what came after
    // was drawn from a face with no glyph for it.
    let (hangul, khmer) = (face_with(&['한']), face_with(&['ក']));
    let (Some(_), Some(_)) = (hangul, khmer) else { return skipped("no face has Hangul and Khmer both") };
    let c = CharFormat { font: "Carlito".into(), ..CharFormat::default() };
    let laid = lay(&[run("\u{2FFFF}한ក", &c, &left())]);
    let drawn: Vec<(String, Vec<u32>)> =
        laid.lines.iter().flat_map(|l| l.runs.iter().map(|r| (r.face.family().to_owned(), r.glyphs.iter().map(|g| g.gid).collect()))).collect();
    let has = |ch: char| {
        drawn.iter().any(|(family, ids)| {
            machine().pick(family, Default::default()).glyph_for(ch) != 0 && ids.contains(&machine().pick(family, Default::default()).glyph_for(ch))
        })
    };
    assert!(has('한'), "Hangul is drawn from a face that has it: {drawn:?}");
    assert!(has('ក'), "Khmer is drawn from a face that has it: {drawn:?}");
    // The letter is missing where no face has it. macOS has a face with a glyph for every code point (Last Resort), so there nothing is missing.
    match machine().covering(&['\u{2FFFF}']) {
        None => assert!(laid.notes.contains(&Note::MissingGlyphs { clusters: 1 }), "{:?}", laid.notes),
        Some(face) => skipped(&format!("{} has a glyph for U+2FFFF, so it is not missing here", face.family())),
    }
}

#[test]
fn which_face_draws_a_han_letter_does_not_depend_on_which_faces_were_used_before() {
    let Some(first) = face_with(&['中']) else { return skipped("no face has Han") };
    let want = first.family().to_owned();
    // A database where other Han-capable faces have been loaded and drawn with first.
    let warm = FontDb::system_and_bundled();
    let mut loaded = 0;
    for name in ["Symbola", "Droid Sans Fallback", "Noto Sans CJK JP", "Noto Serif CJK JP", "DejaVu Sans"] {
        if warm.has_family(name) {
            let _ = warm.pick(name, Default::default()).glyph_for('中');
            loaded += 1;
        }
    }
    let cold = FontDb::system_and_bundled();
    let c = CharFormat { font: "Carlito".into(), ..CharFormat::default() };
    let family_in = |db: &FontDb| {
        let laid = lay_out(&[run("中", &c, &left())], &block(), None, db);
        laid.lines[0].runs[0].face.family().to_owned()
    };
    assert_eq!(family_in(&warm), family_in(&cold), "after loading {loaded} faces");
    // The letter is searched for when the face that draws the run lacks it. Where Carlito is missing its stand-in draws the run, and on macOS that
    // is a face with Han letters of its own (Apple SD Gothic Neo), which then draws this one too, as it should.
    let drawing = machine().pick("Carlito", Default::default());
    if drawing.covers('中') {
        return skipped(&format!("{}, which draws Carlito here, has the letter itself", drawing.family()));
    }
    assert_eq!(family_in(&cold), want);
}

#[test]
fn a_devanagari_conjunct_is_one_glyph_when_the_face_makes_one() {
    let word = "क्ष";
    let chars: Vec<char> = word.chars().collect();
    let Some(face) = face_with(&chars) else { return skipped("no face has Devanagari") };
    let c = CharFormat { lang: "hi".into(), ..format(&face, 0.2) };
    let laid = lay(&[run(word, &c, &left())]);
    let shaped = face.shape(word, &ShapeParams { script: Some(*b"Deva"), language: Some("hi"), ..ShapeParams::default() });
    assert_eq!(gids(&laid.lines[0]), shaped.iter().map(|g| g.gid).collect::<Vec<_>>(), "{}", face.family());
    if shaped.len() == chars.len() {
        skipped(&format!("{} has no conjunct for it, so one glyph a letter is its answer", face.family()));
    }
}

#[test]
fn the_slot_a_face_is_asked_for_follows_the_script_on_real_text() {
    // A run naming a Latin font and a complex font: Arabic letters are drawn from the second, Latin from the first.
    let (Some(latin), Some(arabic)) = (face_with(&['a']), face_with(&['ب'])) else { return skipped("no Latin or no Arabic face") };
    let c = CharFormat { font: latin.family().into(), complex_font: arabic.family().into(), size: 0.2, ..CharFormat::default() };
    let laid = lay(&[run("a ب", &c, &left())]);
    let faces: Vec<String> = laid.lines[0].runs.iter().map(|r| r.face.family().to_owned()).collect();
    assert!(faces.contains(&arabic.family().to_owned()), "{faces:?}");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]

    #[test]
    fn mixed_text_wrapped_and_justified_on_the_machines_faces_keeps_its_lines_in_order_and_its_gaps_inside_the_measure(
        words in proptest::collection::vec(proptest::sample::select(vec!["The", "quick", "(brown)", "fox", "بتث", "אבג", "中文字", "한국어", "12.5", "\u{00A0}", "x-y"]), 1..24),
        spaces in proptest::collection::vec(1usize..3, 1..24),
        measure in 0.3f64..3.0,
        justify in proptest::prelude::any::<bool>(),
    ) {
        let mut text = String::new();
        for (i, w) in words.iter().enumerate() {
            text.push_str(w);
            text.push_str(&" ".repeat(spaces.get(i).copied().unwrap_or(1)));
        }
        let c = CharFormat { font: "Carlito".into(), complex_font: "Carlito".into(), size: 0.15, ..CharFormat::default() };
        let p = diacraft_text::ParaFormat { halign: if justify { 3 } else { 0 }, ..diacraft_text::ParaFormat::default() };
        let laid = lay_out(&[run(&text, &c, &p)], &block(), Some(measure), machine());
        let total = text.chars().count();
        let mut end = 0;
        for line in &laid.lines {
            proptest::prop_assert!(line.chars.start >= end || line.chars.start == line.chars.end, "lines go forward: {:?} after {end}", line.chars);
            proptest::prop_assert!(line.chars.end <= total && line.width.is_finite() && line.x.is_finite());
            end = end.max(line.chars.end);
            let shown: String = text.chars().skip(line.chars.start).take(line.chars.end - line.chars.start).collect();
            if justify && line.width > measure + 1e-6 {
                proptest::prop_assert!(!shown.trim_end_matches(' ').contains(' '), "{shown:?} is wider than {measure} with a gap in it");
            }
        }
    }
}
