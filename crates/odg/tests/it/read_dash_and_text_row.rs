//! The fixes took over from the earlier rulings: a dash matched by DiaCraft's name before its runs, and
//! a text row 0 that holds what the style chain resolves to.

use crate::drawing::{Drawing, cell, close, num, only};
use diacraft_doc::TextPart;

fn rect(style: &str, inner: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm">{inner}</draw:rect>"#)
}

fn graphic(name: &str, props: &str) -> String {
    format!(r#"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties {props}/></style:style>"#)
}

#[test]
fn odg_styles_a_dash_is_matched_by_diacraft_s_name_before_its_runs() {
    let named = |name: &str, dash: &str, cap: &str| {
        Drawing::a4()
            .common(&format!(r#"<draw:stroke-dash draw:name="{name}" {dash}/>"#))
            .auto(&graphic("gr1", &format!(r#"draw:stroke="dash" draw:stroke-dash="{name}" svg:stroke-width="1pt" svg:stroke-linecap="{cap}""#)))
            .page(&rect("gr1", ""))
            .read()
    };
    let round = r#"draw:dots1="1" draw:dots1-length="1900%" draw:dots2="1" draw:dots2-length="700%" draw:distance="500%""#;
    let flat = r#"draw:dots1="1" draw:dots1-length="2000%" draw:dots2="1" draw:dots2-length="800%" draw:distance="400%""#;
    // The flat form under a flat cap matches by runs alone; so does the round form under a round cap.
    close(num(cell(only(&named("Other", flat, "butt").0), "LinePattern")), 7.0);
    // The flat form under a round cap matches no pattern by runs: the stand-in 2, kept.
    let (doc, report) = named("Other", flat, "round");
    close(num(cell(only(&doc), "LinePattern")), 2.0);
    assert_eq!(report.kept("dash"), 1);
    // By name it is pattern 7, whatever cap the style holds.
    let (doc, report) = named("DiaCraft_20_dash_20_7_20_flat", flat, "round");
    close(num(cell(only(&doc), "LinePattern")), 7.0);
    assert_eq!(report.kept("dash"), 0);
    close(num(cell(only(&named("DiaCraft_20_dash_20_7", round, "butt").0), "LinePattern")), 7.0);
    // The name says a pattern whose runs these are not: by the runs, as before.
    let (doc, report) = named("DiaCraft_20_dash_20_8", round, "round");
    close(num(cell(only(&doc), "LinePattern")), 7.0);
    assert_eq!(report.kept("dash"), 0);
    // A name DiaCraft does not write (a leading zero, a sign, a pattern out of 2 to 23, no number) is any other name.
    for name in [
        "DiaCraft_20_dash_20_07",
        "DiaCraft_20_dash_20_+7",
        "DiaCraft_20_dash_20_24",
        "DiaCraft_20_dash_20_1",
        "DiaCraft_20_dash_20_",
        "DiaCraft_20_dash_20_07_20_flat",
        "DiaCraft_20_dash_20_+7_20_flat",
        "DiaCraft_20_dash_20_24_20_flat",
        "DiaCraft_20_dash_20_1_20_flat",
        "DiaCraft_20_dash_20__20_flat",
        "DiaCraft_20_dash_20_7_20_flat_20_flat",
    ] {
        let (doc, report) = named(name, flat, "round");
        close(num(cell(only(&doc), "LinePattern")), 2.0);
        assert_eq!(report.kept("dash"), 1, "{name}");
    }
}

fn paragraph_style(name: &str, parent: Option<&str>, props: &str) -> String {
    let parent = parent.map_or(String::new(), |p| format!(r#" style:parent-style-name="{p}""#));
    format!(r#"<style:style style:name="{name}" style:family="paragraph"{parent}><style:text-properties {props}/></style:style>"#)
}

/// The markers of a shape's text: `cp` and `pp` rows in order, and the characters between.
fn markers(shape: &diacraft_doc::Shape) -> Vec<String> {
    let text = shape.text.as_ref().expect("no text");
    text.parts
        .iter()
        .map(|p| match p {
            TextPart::Chars(c) => format!("'{c}'"),
            TextPart::Cp(n) => format!("cp{n}"),
            TextPart::Pp(n) => format!("pp{n}"),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn odg_styles_a_paragraph_whose_size_comes_from_its_automatic_style_s_common_parent_draws_at_that_size() {
    // The `PA` and `PB`: automatic paragraph styles that set nothing themselves, over the common styles `Big` (14 pt) and `Small` (10 pt).
    // The first run's chain resolves a size of 14 pt, which the shape's links do not give, so row 0 holds it; the second's
    // resolves 10 pt and is row 1, which holds every cell it resolves.
    let (doc, _) = Drawing::a4()
        .common(&paragraph_style("Big", None, r#"fo:font-size="14pt""#))
        .common(&paragraph_style("Small", None, r#"fo:font-size="10pt""#))
        .auto(r#"<style:style style:name="PA" style:family="paragraph" style:parent-style-name="Big"/>"#)
        .auto(r#"<style:style style:name="PB" style:family="paragraph" style:parent-style-name="Small"/>"#)
        .page(&rect("", r#"<text:p text:style-name="PA">ab</text:p><text:p text:style-name="PB">cd</text:p>"#))
        .read();
    let shape = only(&doc);
    let size = |row: u32| crate::drawing::row_cell(shape, "Character", None, row, "Size");
    close(num(size(0)), 14.0 / 72.0);
    close(num(size(1)), 10.0 / 72.0);
    assert!(crate::drawing::row_cell(shape, "Character", None, 2, "Size").is_none(), "two sets, two rows");
    assert_eq!(markers(shape), ["cp0", "'ab\n'", "cp1", "'cd'"]);
}

#[test]
fn odg_styles_text_with_no_style_of_its_own_has_no_marker_and_a_style_equal_to_the_shape_s_leaves_row_0_empty() {
    // No paragraph style: row 0 holds no cell, so no marker.
    let (doc, _) = Drawing::a4().page(&rect("", r#"<text:p>ab</text:p>"#)).read();
    assert_eq!(markers(only(&doc)), ["'ab'"]);
    // A paragraph style that restates what the shape's chain gives (the root's size is whatever the root holds; the style sets none) draws as
    // the shape does, and still opens no row.
    let (doc, _) = Drawing::a4()
        .common(&paragraph_style("Same", None, r##"fo:color="#112233""##))
        .page(&rect("", r#"<text:p text:style-name="Same">ab</text:p>"#))
        .read();
    // The colour is not the shape's: row 0 holds it, and the paragraph's text is opened by a marker.
    assert_eq!(markers(only(&doc)), ["cp0", "'ab'"]);
    let shape = only(&doc);
    assert_eq!(crate::drawing::text(crate::drawing::row_cell(shape, "Character", None, 0, "Color")), "#112233");
}
