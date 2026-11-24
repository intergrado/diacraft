//! The unit tests of Quick Shapes: the data's limits and what a hostile stencil costs. The commands, the formats and the undo of the choice are in
//! `crates/engine/tests/it/stencil_quick.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Sheet, Value};

use super::*;
use crate::stencil::opened::ModelCost;

fn basic() -> View<'static> {
    View::Builtin(builtin_stencil("basic").unwrap())
}

fn data(entries: &[(&str, &str, &[&str])]) -> String {
    let list: Vec<serde_json::Value> = entries.iter().map(|(kind, stencil, masters)| json!([kind, stencil, masters])).collect();
    serde_json::Value::Array(list).to_string()
}

#[test]
fn the_data_reads_back_as_written_and_nothing_held_is_no_text() {
    assert_eq!(Held::default().text(), None);
    let mut held = Held::default();
    add(&mut held, &basic(), "hexagon", Some(0)).unwrap();
    let text = held.text().unwrap();
    assert_eq!(text, data(&[("b", "basic", &["hexagon", "rectangle", "square", "ellipse", "circle"])]));
    assert_eq!(Held::read(&text).unwrap(), held);
    // A drawing with no child holds nothing, and the document is untouched by reading it.
    assert_eq!(Held::of(None).unwrap(), Held::default());
    assert_eq!(Held::of(Some(&Document::new_blank())).unwrap(), Held::default());
}

#[test]
fn the_data_has_exact_limits_and_what_is_past_them_is_unreadable() {
    let key = |n: usize| "k".repeat(n);
    // One master key of the most bytes, and one more.
    assert!(Held::read(&data(&[("f", "s", &[key(MAX_KEY_BYTES).as_str()])])).is_ok());
    assert_eq!(Held::read(&data(&[("f", "s", &[key(MAX_KEY_BYTES + 1).as_str()])])), Err(Problem::Unreadable));
    assert_eq!(Held::read(&data(&[("f", key(MAX_KEY_BYTES + 1).as_str(), &["a"])])), Err(Problem::Unreadable));
    // The most masters in a stencil's group, and one more.
    let masters: Vec<String> = (0..MAX_QUICK_PER_STENCIL).map(|i| format!("m{i}")).collect();
    let refs: Vec<&str> = masters.iter().map(String::as_str).collect();
    assert!(Held::read(&data(&[("b", "basic", &refs)])).is_ok());
    let mut more = refs.clone();
    more.push("extra");
    assert_eq!(Held::read(&data(&[("b", "basic", &more)])), Err(Problem::Unreadable));
    // The most stencils, and one more.
    let names: Vec<String> = (0..=MAX_QUICK_STENCILS).map(|i| format!("s{i}")).collect();
    let entries: Vec<(&str, &str, &[&str])> = names.iter().map(|n| ("f", n.as_str(), &["a"][..])).collect();
    assert!(Held::read(&data(&entries[..MAX_QUICK_STENCILS])).is_ok());
    assert_eq!(Held::read(&data(&entries)), Err(Problem::Unreadable));
    // Past the text that is read at all.
    assert_eq!(Held::read(&format!("[{}]", " ".repeat(MAX_DATA_BYTES))), Err(Problem::Unreadable));
}

#[test]
fn data_that_is_not_the_shape_is_unreadable_and_a_repeated_master_is_one() {
    for text in [
        "",
        "null",
        "{}",
        "[1]",
        "[[]]",
        r#"[["b","basic"]]"#,
        r#"[["b","basic",["a"],"extra"]]"#,
        r#"[["x","basic",[]]]"#,
        r#"[["b","",[]]]"#,
        r#"[["b","basic",[1]]]"#,
        r#"[["b","basic",["a\u0007"]]]"#,
        r#"[["b","basic",[]],["b","basic",[]]]"#,
        r#"[["b","basic","rectangle"]]"#,
    ] {
        assert_eq!(Held::read(text), Err(Problem::Unreadable), "{text:?}");
    }
    // A built-in stencil and a file of one name are two stencils; a master twice is once.
    let held = Held::read(&data(&[("b", "basic", &["a", "b", "a"]), ("f", "basic", &["z"])])).unwrap();
    assert_eq!(held.entries.len(), 2);
    assert_eq!(held.entries[0].masters, ["a", "b"]);
    // Nested deeper than JSON is read is refused, and does not recurse.
    assert_eq!(Held::read(&"[".repeat(10_000)), Err(Problem::Unreadable));
}

#[test]
fn a_group_for_one_stencil_too_many_is_refused() {
    let mut held = Held::default();
    for i in 0..MAX_QUICK_STENCILS {
        held.entry_mut(false, &format!("s{i}"), vec![]).unwrap();
    }
    assert_eq!(held.entry_mut(false, "one more", vec![]).err(), Some(Problem::TooManyStencils));
    // A stencil that has a group is still found.
    assert!(held.entry_mut(false, "s3", vec![]).is_ok());
    assert_eq!(held.entries.len(), MAX_QUICK_STENCILS);
}

#[test]
fn adding_moving_and_removing_keep_the_order_and_the_limit() {
    let mut held = Held::default();
    let view = basic();
    // The default group is copied in when the first change is made, so that a change is a change of what is shown.
    add(&mut held, &view, "hexagon", None).unwrap();
    assert_eq!(held.entries[0].masters, ["rectangle", "square", "ellipse", "circle", "hexagon"]);
    add(&mut held, &view, "circle", Some(1)).unwrap();
    assert_eq!(held.entries[0].masters, ["rectangle", "circle", "square", "ellipse", "hexagon"]);
    assert!(!remove(&mut held, &view, "no-such").unwrap());
    assert!(remove(&mut held, &view, "circle").unwrap());
    assert_eq!(held.entries[0].masters, ["rectangle", "square", "ellipse", "hexagon"]);
    assert!(reset(&mut held, &view));
    assert!(!reset(&mut held, &view));
    // The group holds 24 at the most: the 25th is refused and the group is as it was.
    let masters: Vec<String> = (0..MAX_QUICK_PER_STENCIL).map(|i| format!("m{i}")).collect();
    let mut full = Held { entries: vec![Entry { builtin: true, stencil: "basic".to_owned(), masters: masters.clone() }] };
    assert_eq!(add(&mut full, &view, "extra", None), Err(Problem::TooMany));
    assert_eq!(full.entries[0].masters, masters);
    add(&mut full, &view, "m3", Some(0)).unwrap();
    assert_eq!(full.entries[0].masters.len(), MAX_QUICK_PER_STENCIL);
}

fn file_master(id: u32, name: &str, one_d: bool) -> Arc<diacraft_doc::Master> {
    let mut shape = diacraft_doc::Shape::new(1);
    let mut sheet = Sheet::default();
    let cell = |n: f64| Cell::constant(Value::from_text(&n.to_string()));
    if one_d {
        for name in ["BeginX", "BeginY", "EndX", "EndY"] {
            sheet.cells.insert(CellName::new(name), cell(1.0));
        }
    }
    shape.sheet = Arc::new(sheet);
    Arc::new(diacraft_doc::Master {
        id,
        name: Some(Arc::from(name)),
        shapes: std::iter::once(Arc::new(shape)).collect(),
        ..diacraft_doc::Master::default()
    })
}

fn file_stencil(masters: Vec<Arc<diacraft_doc::Master>>) -> OpenedStencils {
    named_stencil("Many", masters)
}

fn named_stencil(name: &str, masters: Vec<Arc<diacraft_doc::Master>>) -> OpenedStencils {
    let mut doc = Document::new_blank();
    for m in masters {
        doc.masters.push(m);
    }
    let mut opened = OpenedStencils::new();
    let id = opened.next_id();
    opened.add(OpenedStencil::new(id, name.to_owned(), None, 0, ModelCost::default(), doc));
    opened
}

fn six_boxes() -> Vec<Arc<diacraft_doc::Master>> {
    (1..=6).map(|i| file_master(i, &format!("Box {i}"), false)).collect()
}

/// A write-read differential: a stencil file's name is what the data names its group by, and the reader refuses a key with a control character or past
/// [`MAX_KEY_BYTES`]. A name of either kind is kept in a form the reader takes (each control character as U+FFFD, cut at a character boundary at the limit), so what is written
/// reads back the same and the group is the one the window showed; a name the reader takes is its own key.
#[test]
fn a_file_name_the_data_cannot_hold_is_kept_in_a_form_that_reads_back() {
    let limit = "k".repeat(MAX_KEY_BYTES);
    let past = "k".repeat(MAX_KEY_BYTES + 1);
    // Three bytes a character: 43 of them are one byte past the limit, and the cut falls inside the 43rd.
    let wide = "表".repeat(43);
    for (name, key) in [
        ("Mine\u{7}", "Mine\u{fffd}".to_owned()),
        ("\u{1b}[2J\n", "\u{fffd}[2J\u{fffd}".to_owned()),
        (limit.as_str(), limit.clone()),
        (past.as_str(), limit.clone()),
        (wide.as_str(), "表".repeat(42)),
        ("Plain", "Plain".to_owned()),
    ] {
        let opened = named_stencil(name, six_boxes());
        let view = View::find(&opened, "file-1").unwrap();
        assert_eq!(*view.key(), key, "{name:?}");
        assert!(valid_key(&view.key()), "{name:?}");
        let mut held = Held::default();
        add(&mut held, &view, "5", Some(0)).unwrap();
        let shown: Vec<String> = view.group(&held).0.into_iter().map(|i| i.key).collect();
        assert_eq!(shown, ["5", "1", "2", "3", "4"], "{name:?}");
        let text = held.text().unwrap();
        let back = Held::read(&text).unwrap_or_else(|e| panic!("{name:?}: {e:?} for {text}"));
        assert_eq!(back, held, "{name:?}");
        let (again, customized) = view.group(&back);
        assert!(customized, "{name:?}");
        assert_eq!(again.into_iter().map(|i| i.key).collect::<Vec<_>>(), shown, "{name:?}: the group the window showed");
        // Written into a drawing and read from it, the same.
        let mut doc = Document::new_blank();
        assert_eq!(held.write(&mut doc), Ok(true));
        assert_eq!(Held::of(Some(&doc)), Ok(held.clone()), "{name:?}");
        // Taken away and reset by the same key.
        assert!(remove(&mut held, &view, "5").unwrap(), "{name:?}");
        assert!(reset(&mut held, &view), "{name:?}");
        assert_eq!(held, Held::default());
    }
}

/// A key the data cannot hold at all (a stencil with no name; a master key that is empty, too long or holds a control character, which only a caller of these functions can give:
/// the commands resolve a master to its id) is refused in words and nothing changes.
#[test]
fn a_key_the_data_cannot_hold_is_refused_in_words_and_nothing_changes() {
    let view = basic();
    let mut held = Held::default();
    add(&mut held, &view, "hexagon", None).unwrap();
    let before = held.clone();
    let long = "m".repeat(MAX_KEY_BYTES + 1);
    for key in ["", "a\u{7}", "\n", long.as_str()] {
        assert_eq!(add(&mut held, &view, key, None), Err(Problem::BadKey), "{key:?}");
        assert_eq!(remove(&mut held, &view, key), Err(Problem::BadKey), "{key:?}");
        assert_eq!(held, before);
    }
    assert!(add(&mut held, &view, &"m".repeat(MAX_KEY_BYTES), None).is_ok(), "a master key at the limit is one");
    let unnamed = named_stencil("", six_boxes());
    let view = View::find(&unnamed, "file-1").unwrap();
    let mut held = Held::default();
    assert_eq!(add(&mut held, &view, "1", None), Err(Problem::BadKey));
    assert_eq!(remove(&mut held, &view, "1"), Err(Problem::BadKey));
    assert_eq!(held.entry_mut(false, "", vec![]).err(), Some(Problem::BadKey));
    assert_eq!(held.entry_mut(false, "s\u{0}", vec![]).err(), Some(Problem::BadKey));
    assert_eq!(held, Held::default());
    let words = Problem::BadKey.words();
    assert!(words.contains("128 bytes") && words.contains("control character"), "{words}");
}

/// The last guard of the differential: a group that would not read back as it is (more text than is read, a master twice) is not written, and the drawing is as it was.
#[test]
fn what_would_not_read_back_the_same_is_not_written() {
    let long = |m: usize| format!("{m:0>width$}", width = MAX_KEY_BYTES);
    let big = Held {
        entries: (0..MAX_QUICK_STENCILS)
            .map(|i| Entry { builtin: false, stencil: format!("s{i}"), masters: (0..MAX_QUICK_PER_STENCIL).map(long).collect() })
            .collect(),
    };
    assert!(big.text().unwrap().len() > MAX_DATA_BYTES, "within each limit, and past the text that is read");
    let twice = Held { entries: vec![Entry { builtin: true, stencil: "basic".to_owned(), masters: vec!["a".into(), "a".into()] }] };
    let mut doc = Document::new_blank();
    let mut set = Held::default();
    add(&mut set, &basic(), "hexagon", None).unwrap();
    assert_eq!(set.write(&mut doc), Ok(true));
    let kept = held_attrs(&doc);
    for held in [big, twice] {
        assert_eq!(held.write(&mut doc), Err(Problem::Unwritable));
        assert_eq!(held_attrs(&doc), kept, "the drawing is as it was");
    }
    assert!(Problem::Unwritable.words().contains("read back"));
}

/// A stencil whose first 10,000 masters (the ones a listing sizes) are connectors: nothing before the first of the rest is a default Quick Shape, and the strip finds the
/// first four that are not, at the price of one pass over the masters each time.
#[test]
fn a_stencil_that_begins_with_thousands_of_connectors_has_its_group_after_them() {
    let mut masters: Vec<_> = (1..=10_000).map(|i| file_master(i, &format!("Link {i}"), true)).collect();
    masters.extend((10_001..=10_006).map(|i| file_master(i, &format!("Box {i}"), false)));
    let opened = file_stencil(masters);
    let view = View::find(&opened, "file-1").unwrap();
    let held = Held::default();
    let keys: Vec<String> = view.group(&held).0.into_iter().map(|i| i.key).collect();
    assert_eq!(keys, ["10001", "10002", "10003", "10004"]);
    let offered: Vec<String> = view.offer(&held).into_iter().map(|i| i.key).collect();
    assert_eq!(offered, keys);
    assert_eq!(
        offer_gathered(&opened, &held).iter().filter(|(stencil, _)| stencil == "file-1").count(),
        0,
        "the built-in stencils fill the strip first"
    );
    // A stencil of connectors alone has none, and the strip offers nothing from it.
    let links = file_stencil((1..=100).map(|i| file_master(i, &format!("Link {i}"), true)).collect());
    let view = View::find(&links, "file-1").unwrap();
    assert!(view.group(&held).0.is_empty() && view.offer(&held).is_empty());
}

/// A group names masters by their `ID`s, each found by one lookup: a name that no master has, a hidden master and an `ID` that is gone are passed over.
#[test]
fn a_file_group_reads_its_masters_by_id_and_passes_over_those_that_are_gone() {
    let mut hidden = diacraft_doc::Master::clone(&file_master(3, "Hidden", false));
    hidden.raw = diacraft_doc::RawAttrs::from_pairs([("Hidden", "1")]);
    let opened = file_stencil(vec![file_master(1, "One", false), file_master(2, "Two", false), Arc::new(hidden), file_master(4, "Four", false)]);
    let view = View::find(&opened, "file-1").unwrap();
    let held = Held {
        entries: vec![Entry {
            builtin: false,
            stencil: "Many".to_owned(),
            masters: vec!["4".into(), "3".into(), "99".into(), "x".into(), "1".into()],
        }],
    };
    let (items, customized) = view.group(&held);
    let names: Vec<&str> = items.iter().map(|i| i.json["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Four", "One"]);
    assert!(customized);
    assert_eq!(view.resolve("two").as_deref(), Some("2"));
    assert_eq!(view.resolve("Hidden"), None, "a hidden master is not a Quick Shape");
    // The default group leaves the hidden master out as well.
    let default: Vec<String> = view.default_group().into_iter().map(|i| i.key).collect();
    assert_eq!(default, ["1", "2", "4"]);
}

/// What a file's data may be is anything: random edits of a valid text never panic, what reads holds the limits, and what is held writes a text that reads back the same.
#[test]
fn data_edited_at_random_is_unreadable_or_within_the_limits_and_reads_back() {
    let valid = data(&[("b", "basic", &["rectangle", "square", "ellipse"]), ("f", "Mine", &["5", "7"]), ("b", "flowchart", &[])]);
    let pieces = ["\"", "[", "]", ",", "b", "f", "\\", "\u{7}", "é", "[[", "]]", "null", "1", " "];
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = |n: usize| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(state >> 33).unwrap() % n
    };
    let (mut readable, mut unreadable) = (0_usize, 0_usize);
    for _ in 0..4_000 {
        let mut chars: Vec<char> = valid.chars().collect();
        for _ in 0..=next(4) {
            let at = next(chars.len() + 1);
            match next(3) {
                0 if at < chars.len() => {
                    chars.remove(at);
                }
                1 => {
                    let piece = pieces[next(pieces.len())];
                    for (k, c) in piece.chars().enumerate() {
                        chars.insert((at + k).min(chars.len()), c);
                    }
                }
                _ if at < chars.len() => chars[at] = pieces[next(pieces.len())].chars().next().unwrap(),
                _ => {}
            }
        }
        let text: String = chars.into_iter().collect();
        match Held::read(&text) {
            Err(problem) => {
                assert_eq!(problem, Problem::Unreadable);
                unreadable += 1;
            }
            Ok(held) => {
                readable += 1;
                assert!(held.entries.len() <= MAX_QUICK_STENCILS);
                for e in &held.entries {
                    assert!(e.masters.len() <= MAX_QUICK_PER_STENCIL && valid_key(&e.stencil) && e.masters.iter().all(|m| valid_key(m)));
                }
                if let Some(written) = held.text() {
                    assert_eq!(Held::read(&written), Ok(held.clone()), "{text:?}");
                }
            }
        }
    }
    assert!(readable > 100 && unreadable > 100, "both kinds are met: {readable} readable, {unreadable} not");
}
