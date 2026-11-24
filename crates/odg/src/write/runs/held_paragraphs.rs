//! The paragraphs a text holds (`Text.ext.odf.paragraphs`) are **the file's, in the file's order**, and the writer's
//! paragraph plan matches them to the text's paragraphs by content. So a newline typed before a held paragraph leaves its style on it,
//! and an edit of one paragraph leaves the others': the held list is not read by the place of a paragraph in the new text.
//!
//! The texts are the ones the engine's edits make of `one\ntwo` (`engine/tests/it/text_ext.rs` holds that the three edits carry the slot forward).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, OdfParagraph, OdfText, OdfTextStyle, Row, Section, Sheet, Text, TextPart, Value};

use super::{Rows, Step, View, no_palette, plan, views};

/// The paragraphs of a text of one run of `chars`.
fn paragraphs_of(chars: &str) -> Vec<View> {
    paragraphs_of_parts(vec![TextPart::Cp(0), TextPart::Chars(Arc::from(chars))])
}

/// The paragraphs of a text of these parts, in a sheet with no `Character` row.
fn paragraphs_of_parts(parts: Vec<TextPart>) -> Vec<View> {
    paragraphs_in(&Sheet::default(), parts)
}

/// The paragraphs of a text of these parts, in `sheet` (whose `Character` rows give the runs their properties).
fn paragraphs_in(sheet: &Sheet, parts: Vec<TextPart>) -> Vec<View> {
    let mut counted = 0_u64;
    let mut work = 0_u64;
    let mut characters = Rows::of(sheet, false, &no_palette, &mut counted);
    let mut paragraphs = Rows::of(sheet, true, &no_palette, &mut counted);
    let text = Arc::new(Text { parts, ..Text::default() });
    views(Some(&text), &mut characters, &mut paragraphs, &mut work).unwrap_or_else(|e| panic!("{e}"))
}

/// What the file held of its paragraphs: a style named for each, in the file's order.
fn held(names: &[&str]) -> OdfText {
    let paragraph =
        |name: &&str| OdfParagraph { style: OdfTextStyle { name: Some(Arc::from(*name)), ..OdfTextStyle::default() }, ..OdfParagraph::default() };
    OdfText { paragraphs: names.iter().map(paragraph).collect(), ..OdfText::default() }
}

/// The style name each paragraph of the new text comes to hold: that of the file's paragraph the plan keeps or edits into it, and none for an inserted one.
fn held_by_new(file: &str, new: &str, held: &OdfText) -> Vec<Option<String>> {
    let (old, new) = (paragraphs_of(file), paragraphs_of(new));
    let mut out: Vec<Option<String>> = vec![None; new.len()];
    for step in plan(&old, &new, &mut 0) {
        if let Step::Keep { old, new } | Step::Edit { old, new } = step {
            out[new] = held.paragraphs.get(old).and_then(|p| p.style.name.as_deref().map(str::to_owned));
        }
    }
    out
}

fn some(names: &[Option<&str>]) -> Vec<Option<String>> {
    names.iter().map(|n| n.map(str::to_owned)).collect()
}

#[test]
fn a_newline_typed_before_a_held_paragraph_leaves_its_style_on_it() {
    let file = held(&["P1", "P2"]);
    // Before `two`: `one`, a new empty paragraph, `two`. By the place `P2` would land on the empty one; by content it stays on `two`.
    assert_eq!(held_by_new("one\ntwo", "one\n\ntwo", &file), some(&[Some("P1"), None, Some("P2")]));
    // At the very start: the new paragraph is first, and both held paragraphs move down.
    assert_eq!(held_by_new("one\ntwo", "\none\ntwo", &file), some(&[None, Some("P1"), Some("P2")]));
    // At the end: after both.
    assert_eq!(held_by_new("one\ntwo", "one\ntwo\n", &file), some(&[Some("P1"), Some("P2"), None]));
}

#[test]
fn an_edit_of_one_paragraph_leaves_the_others_styles_where_they_were() {
    let file = held(&["P1", "P2", "P3"]);
    // The middle one retyped: it is edited in place (it keeps the style its file paragraph named), the others are kept.
    assert_eq!(held_by_new("a\nb\nc", "a\nB\nc", &file), some(&[Some("P1"), Some("P2"), Some("P3")]));
    // The first, and the last.
    assert_eq!(held_by_new("a\nb\nc", "A\nb\nc", &file), some(&[Some("P1"), Some("P2"), Some("P3")]));
    assert_eq!(held_by_new("a\nb\nc", "a\nb\nC", &file), some(&[Some("P1"), Some("P2"), Some("P3")]));
}

#[test]
fn a_paragraph_taken_out_takes_its_style_and_no_other() {
    let file = held(&["P1", "P2", "P3"]);
    // The middle one's characters and both newlines of it taken out: `a`, `c`.
    assert_eq!(held_by_new("a\nb\nc", "a\nc", &file), some(&[Some("P1"), Some("P3")]));
    // The first one taken out.
    assert_eq!(held_by_new("a\nb\nc", "b\nc", &file), some(&[Some("P2"), Some("P3")]));
    // Two paragraphs joined by the newline taken out: the first of them takes the old one's place and keeps its style; the second's goes with the newline.
    assert_eq!(held_by_new("a\nb\nc", "ab\nc", &file), some(&[Some("P1"), Some("P3")]));
}

#[test]
fn paragraphs_with_the_same_characters_are_matched_in_order() {
    // Two empty paragraphs in the file (`x`, empty, `y`) and one typed beside them: whichever way the plan pairs them, it takes the file's paragraphs in their
    // order, each once, so a held style is never given to two paragraphs or to one before an earlier.
    for new in ["x\n\n\ny", "\nx\n\ny", "x\n\ny\n", "x\n\n\n\ny", "x\ny", "y"] {
        let mut olds = Vec::new();
        for step in plan(&paragraphs_of("x\n\ny"), &paragraphs_of(new), &mut 0) {
            if let Step::Keep { old, .. } | Step::Edit { old, .. } | Step::Remove { old } = step {
                olds.push(old);
            }
        }
        assert_eq!(olds, [0, 1, 2], "{new:?}");
    }
}

#[test]
fn paragraphs_taken_out_and_typed_further_on_leave_the_held_styles_where_they_were() {
    // The head and the tail of the lists are alike only at `A` and `F`: the middle is matched by content. `B` is gone, `E` is retyped as `Z`, and `PC` and `PD` stay
    // on `C` and `D`, where pairing the middle by place would have slid every style one paragraph up. (Two lists of one length are paired by place, edit by edit:
    // that is what a typed character or a retyped line is, and the rule for a longer or a shorter list is this one.)
    let file = held(&["PA", "PB", "PC", "PD", "PE", "PF"]);
    assert_eq!(held_by_new("A\nB\nC\nD\nE\nF", "A\nC\nD\nZ\nF", &file), some(&[Some("PA"), Some("PC"), Some("PD"), Some("PE"), Some("PF")]));
    // Two paragraphs changed places, with one more taken out: the plan keeps the one it can in the file's order (`C`), and the other (`B`, now after it) takes
    // the place of what is left of the file's paragraphs between them, `D`, and with it the style `PD`: never a place before the one matched last.
    let file = held(&["PA", "PB", "PC", "PD", "PE"]);
    assert_eq!(held_by_new("A\nB\nC\nD\nE", "A\nC\nB\nE", &file), some(&[Some("PA"), Some("PC"), Some("PD"), Some("PE")]));
}

/// The paragraphs of a text of one letter each.
fn lettered(words: &[&str]) -> Vec<View> {
    paragraphs_of(&words.join("\n"))
}

/// Every text of 1 to `max` paragraphs of one of the letters.
fn texts(letters: &[&'static str], max: usize) -> Vec<Vec<&'static str>> {
    let mut all: Vec<Vec<&'static str>> = Vec::new();
    let mut level: Vec<Vec<&'static str>> = vec![Vec::new()];
    for _ in 0..max {
        let mut next = Vec::new();
        for text in &level {
            for letter in letters {
                let mut longer = text.clone();
                longer.push(letter);
                next.push(longer);
            }
        }
        all.extend(next.iter().cloned());
        level = next;
    }
    all
}

/// The plan from the file's paragraphs `old` to the model's `new`, after what it must hold for every pair: each paragraph of the file taken once and each of the
/// model's placed once, both in order, and a paragraph kept only where the two are alike.
fn plan_of(old: &[&str], new: &[&str]) -> Vec<Step> {
    let steps = plan(&lettered(old), &lettered(new), &mut 0);
    let (mut olds, mut news) = (Vec::new(), Vec::new());
    for step in &steps {
        match *step {
            Step::Keep { old: o, new: n } => {
                assert_eq!(old[o], new[n], "{old:?} to {new:?}: a paragraph is kept only where the file's and the model's are alike");
                olds.push(o);
                news.push(n);
            }
            Step::Edit { old: o, new: n } => {
                olds.push(o);
                news.push(n);
            }
            Step::Insert { new: n } => news.push(n),
            Step::Remove { old: o } => olds.push(o),
        }
    }
    assert_eq!(olds, (0..old.len()).collect::<Vec<_>>(), "{old:?} to {new:?}: each paragraph of the file once, in order");
    assert_eq!(news, (0..new.len()).collect::<Vec<_>>(), "{old:?} to {new:?}: each paragraph of the model once, in order");
    steps
}

fn count(steps: &[Step], of: fn(&Step) -> bool) -> usize {
    steps.iter().filter(|s| of(s)).count()
}

#[test]
fn the_plan_takes_each_paragraph_of_each_list_once_and_in_order_whatever_the_pair() {
    // Every pair of texts of 1 to 4 paragraphs of two letters (30 of them), and of 1 to 3 of three letters (39).
    let mut pairs = 0;
    for (letters, max) in [(&["a", "b"][..], 4), (&["a", "b", "c"][..], 3)] {
        let all = texts(letters, max);
        for old in &all {
            for new in &all {
                plan_of(old, new);
                pairs += 1;
            }
        }
    }
    assert_eq!(pairs, 30 * 30 + 39 * 39);
}

#[test]
fn one_paragraph_typed_taken_out_or_retyped_changes_that_paragraph_and_keeps_every_other() {
    let is_keep = |s: &Step| matches!(s, Step::Keep { .. });
    for old in texts(&["a", "b", "c"], 4) {
        let n = old.len();
        // A paragraph typed at any place, of any letter: one is inserted and the file's n are all kept.
        for at in 0..=n {
            for letter in ["a", "b", "c"] {
                let mut new = old.clone();
                new.insert(at, letter);
                let steps = plan_of(&old, &new);
                assert_eq!(
                    (steps.len(), count(&steps, is_keep), count(&steps, |s| matches!(s, Step::Insert { .. }))),
                    (n + 1, n, 1),
                    "{old:?} to {new:?}"
                );
            }
        }
        // One taken out, of a text that has another: one is removed and the others kept.
        if n > 1 {
            for at in 0..n {
                let mut new = old.clone();
                new.remove(at);
                let steps = plan_of(&old, &new);
                assert_eq!(
                    (steps.len(), count(&steps, is_keep), count(&steps, |s| matches!(s, Step::Remove { .. }))),
                    (n, n - 1, 1),
                    "{old:?} to {new:?}"
                );
            }
        }
        // One retyped to another letter: one is edited and the others kept.
        for at in 0..n {
            for letter in ["a", "b", "c"].into_iter().filter(|l| *l != old[at]) {
                let mut new = old.clone();
                new[at] = letter;
                let steps = plan_of(&old, &new);
                assert_eq!(
                    (steps.len(), count(&steps, is_keep), count(&steps, |s| matches!(s, Step::Edit { .. }))),
                    (n, n - 1, 1),
                    "{old:?} to {new:?}"
                );
            }
        }
    }
}

#[test]
fn paragraphs_taken_out_and_typed_elsewhere_keep_every_other_paragraph() {
    // Paragraphs all unlike, so that what is kept is by content alone, and the lists of different lengths (two lists of one length are paired by place): two out and one
    // in, and one out and two in, at any places, and the file's other paragraphs are all kept.
    let words = ["p0", "p1", "p2", "p3", "p4", "p5"];
    let keeps = |old: &[&str], new: &[&str]| count(&plan_of(old, new), |s| matches!(s, Step::Keep { .. }));
    for n in 3..=6 {
        for (first, second) in (0..n).flat_map(|a| (a + 1..n).map(move |b| (a, b))) {
            for at in 0..=n - 2 {
                let mut new: Vec<&str> = words[..n].iter().enumerate().filter(|(i, _)| *i != first && *i != second).map(|(_, w)| *w).collect();
                new.insert(at, "new");
                assert_eq!(keeps(&words[..n], &new), n - 2, "{n} paragraphs, {first} and {second} out, a new one at {at}");
            }
        }
    }
    for n in 2..=5 {
        for out in 0..n {
            for (at, then) in (0..n).flat_map(|a| (a..n).map(move |b| (a, b))) {
                let mut new = words[..n].to_vec();
                new.remove(out);
                new.insert(at, "new1");
                new.insert(then + 1, "new2");
                assert_eq!(keeps(&words[..n], &new), n - 1, "{n} paragraphs, {out} out, new ones at {at} and after");
            }
        }
    }
}

#[test]
fn two_lists_of_one_length_are_paired_place_by_place_and_a_paragraph_of_other_runs_is_edited() {
    // The rule for lists of one length (a typed character, a retyped line): each new paragraph is its place's, so a held style stays where it was even if two
    // paragraphs trade characters. Lists of other lengths are matched by content (the tests above).
    let file = held(&["P1", "P2"]);
    assert_eq!(held_by_new("a\nb", "b\na", &file), some(&[Some("P1"), Some("P2")]));
    assert_eq!(plan(&paragraphs_of("a\nb"), &paragraphs_of("b\na"), &mut 0), [Step::Edit { old: 0, new: 0 }, Step::Edit { old: 1, new: 1 }]);
    // In the middle of lists of different lengths, a paragraph of the same characters whose runs are not the file's is not the file's paragraph: it is edited into
    // (the splice writer keeps the bytes of a paragraph only if its runs are the file's), and the paragraph after it that the model lacks is taken out. Here
    // `Character` row 1 gives `b` a size of its own, so that `ab` is two runs.
    let mut sheet = Sheet::default();
    let mut section = Section::new("Character", None);
    let mut row = Row { ix: Some(1), ..Row::default() };
    row.cells.insert(CellName::new("Size"), Cell::constant(Value::from_text("0.5")));
    section.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(section));
    let old = paragraphs_in(&sheet, vec![TextPart::Cp(0), TextPart::Chars(Arc::from("x\nab\ny\nz"))]);
    let split = paragraphs_in(
        &sheet,
        vec![
            TextPart::Cp(0),
            TextPart::Chars(Arc::from("x\na")),
            TextPart::Cp(1),
            TextPart::Chars(Arc::from("b")),
            TextPart::Cp(0),
            TextPart::Chars(Arc::from("\nz")),
        ],
    );
    assert_eq!(
        plan(&old, &split, &mut 0),
        [Step::Keep { old: 0, new: 0 }, Step::Edit { old: 1, new: 1 }, Step::Remove { old: 2 }, Step::Keep { old: 3, new: 2 }]
    );
}
