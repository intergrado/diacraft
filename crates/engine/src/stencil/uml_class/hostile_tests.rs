//! Hostile input to the class: what a person types into the cells the class's formulas read, and what a page of many classes costs. The class never crashes, draws with nothing
//! skipped, and undo gives the drawing back as it was.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;

use super::super::flowchart_tests::{blank, render, run};
use super::tests::{cached, drop_in, members_of, row_cell};

/// The `Abstract` cell of the class holds text, an error and a very long text instead of a truth value: the name's `Style` reads it through `IF`, and whatever it reads, the class draws, nothing is
/// skipped, and an undo gives the class back.
#[test]
fn a_class_whose_abstract_cell_holds_nonsense_still_draws_and_undoes() {
    let mut s = blank();
    let class = drop_in(&mut s, "uml-class", "uml-class.class", 5.0, 4.0);
    let name = *members_of(s.document().unwrap(), class).last().unwrap();
    let before = s.document().unwrap().clone();
    let very_long = format!("\"{}\"", "z".repeat(60_000));
    let mut written = 0;
    for formula in ["\"maybe\"", "1/0", "2.5", "-7", very_long.as_str(), "Prop.Missing"] {
        // A cell of a row of the Property section: the text is the cell's formula, whatever it is.
        let answer = s.execute("sheet.set_cell", &json!({ "id": class, "cell": "Prop.Abstract", "formula": formula }));
        if answer.is_err() {
            // A refusal in words is an answer too: the drawing is as it was.
            assert_eq!(s.document().unwrap(), &before, "a refused write changes nothing");
            continue;
        }
        written += 1;
        let style = row_cell(s.document().unwrap(), name, "Character", 0, "Style");
        assert!(style.is_none_or(f64::is_finite), "`{formula:.20}`: the name's style is a number or no number, never NaN: {style:?}");
        let drawn = render(&s);
        assert!(drawn.skipped.is_empty(), "`{formula:.20}`: {:?}", drawn.skipped);
        assert_eq!(cached(s.document().unwrap(), class, "LockHeight"), Some(1.0), "the class still holds its height");
    }
    assert!(written >= 3, "the cell takes most of what is written to it ({written} of 6 were)");
    while s.document().unwrap() != &before {
        // Each write was one undo step: undoing them all gives the class as it was dropped.
        if s.execute("edit.undo", &json!({})).is_err() {
            break;
        }
    }
    assert_eq!(s.document().unwrap(), &before, "undo gives the class back");
}

/// A page of three hundred classes is dropped, and a text typed in one of them works out that class and no other, in counted work.
#[test]
fn a_page_of_three_hundred_classes_follows_one_text_in_counted_work() {
    let mut s = blank();
    let mut first = 0;
    for k in 0..300_u32 {
        let id = drop_in(&mut s, "uml-class", "uml-class.class", 1.0 + f64::from(k % 20) * 1.9, 1.0 + f64::from(k / 20) * 1.8);
        if k == 0 {
            first = id;
        }
    }
    let attributes = members_of(s.document().unwrap(), first)[1];
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": "warm" }));
    diacraft_calc::work::reset();
    run(&mut s, "shape.set_text", json!({ "id": attributes, "text": "a\nb\nc" }));
    let steps = diacraft_calc::work::steps();
    assert!(steps > 0 && steps < 5_000, "one edit among 300 classes is a few steps of its own class: {steps}");
}
