//! The writer's automatic styles: a restyle
//! clones a shared style, edits a style one shape names where it is, makes a new one on a common style or none, and an orphan goes (after a delete
//! too, L8). Each case runs with the real writer and with the echo writer, which must fail every one.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Shape, Value};
use diacraft_odg::{Limits, OriginalPackage, WriteError, WriteOptions, WriteWork, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Saver, compare, content_changes, echo, echo_says_edited, open, open_bytes, part, real, shape_mut};

/// A rectangle named `name` on the automatic or common style `style`, laid out as a writer lays a page out.
fn rect(name: &str, style: &str, x: &str) -> String {
    format!(
        "\n    <draw:rect draw:name=\"{name}\" draw:style-name=\"{style}\" svg:x=\"{x}\" svg:y=\"3cm\" svg:width=\"4cm\" svg:height=\"2.5cm\"><text:p/></draw:rect>"
    )
}

/// An automatic graphic style `name` on `Standard`, filled `fill`, with a line of 0.1 cm.
fn gr(name: &str, fill: &str) -> String {
    format!(
        "<style:style style:name=\"{name}\" style:family=\"graphic\" style:parent-style-name=\"Standard\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"{fill}\" svg:stroke-width=\"0.1cm\"/></style:style>"
    )
}

/// A drawing with `Standard` and the automatic styles `autos`, holding `shapes`.
fn drawing(autos: &str, shapes: &str) -> Drawing {
    Drawing::a4().common(r#"<style:style style:name="Standard" style:family="graphic"/>"#).auto(autos).page(shapes)
}

/// Sets the text a cell of the shape holds (a constant: no formula).
fn set_text(shape: &mut Shape, cell: &str, text: &str) {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(cell), Cell::constant(Value::from_text(text)));
}

/// The text a cell of a shape named `name` on the first page holds.
fn cell_of(doc: &Document, name: &str, cell: &str) -> Option<String> {
    let shape = doc.pages.get(0)?.shapes.iter().find(|s| s.name.as_deref() == Some(name))?;
    shape.sheet.cell(cell).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

/// The id of the shape named `name` on the first page.
fn id_of(doc: &Document, name: &str) -> u32 {
    doc.pages.get(0).unwrap().shapes.iter().find(|s| s.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no shape {name}")).id
}

/// One automatic style element of `content.xml`, by name.
fn style_of<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let start = content.find(&format!("<style:style style:name=\"{name}\""))?;
    let end = content[start..].find("</style:style>")? + "</style:style>".len();
    Some(&content[start..start + end])
}

/// V3 (restyle): `a` and `b` on `gr1` in a file whose largest graphic style is `gr7`; `a`'s `FillForegnd` set to `#336699`.
fn restyle_case(save: &Saver) {
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr7", "#00ff00"));
    let mut o = open(&drawing(&autos, &format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr7", "11cm"))));
    let a = id_of(&o.doc, "a");
    assert_eq!(cell_of(&o.doc, "a", "FillForegnd").as_deref(), Some("#FF0000"));
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // A new `gr8`, a copy of `gr1` with the new colour, put after it; `a` names it; `b`, `c` and `gr1` are as they were.
    let (old, new) = (style_of(&before, "gr1").unwrap(), style_of(&after, "gr8").unwrap_or_else(|| panic!("no gr8: {after}")));
    assert_eq!(new, old.replace("gr1", "gr8").replace("#ff0000", "#336699"), "a copy of gr1 with the colour edited, attributes in their places");
    assert_eq!(style_of(&after, "gr1"), Some(old), "gr1 is not edited");
    assert!(after.contains(&format!("{old}{new}")), "the copy is put after the original: {after}");
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr8""#) && after.contains(r#"draw:name="b" draw:style-name="gr1""#), "{after}");
    assert!(after.contains(r#"draw:name="c" draw:style-name="gr7""#));
    assert_eq!(
        after.replace(new, "").replace(r#"draw:name="a" draw:style-name="gr8""#, r#"draw:name="a" draw:style-name="gr1""#),
        before,
        "nothing else changed"
    );
    // Only the shape's element and the new style are different, by element.
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 1, 0), "{diff:?}");
    assert_eq!(changed[0].1, ["draw:style-name"], "the one attribute of the shape");
    // And it reads back so.
    let back = open_bytes(saved.bytes);
    assert_eq!(cell_of(&back.doc, "a", "FillForegnd").as_deref(), Some("#336699"));
    assert_eq!(cell_of(&back.doc, "b", "FillForegnd").as_deref(), Some("#FF0000"));
}

/// R2 in place: a style one shape names is edited where it is, in the places of its attributes, and nothing is cloned.
fn a_style_one_shape_names_is_edited_in_place(save: &Saver) {
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr2", "#00ff00"));
    let mut o = open(&drawing(&autos, &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "gr2", "6cm"))));
    let a = id_of(&o.doc, "a");
    let s = shape_mut(&mut o.doc, 0, a);
    set_text(s, "FillForegnd", "#336699");
    set_text(s, "LineWeight", "0.05");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    // 0.05 in is 0.127 cm: written in the unit the attribute had.
    let expected =
        before.replacen(r##"draw:fill-color="#ff0000" svg:stroke-width="0.1cm""##, r##"draw:fill-color="#336699" svg:stroke-width="0.127cm""##, 1);
    assert_ne!(expected, before);
    assert_eq!(after, expected);
    assert!(!after.contains("gr3"), "no style was made");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (1, 0, 0), "{diff:?}");
    assert_eq!(changed[0].1, ["draw:fill-color", "svg:stroke-width"]);
}

/// A rectangle on a common style gets a new automatic style with that parent; the common style is not touched.
fn a_shape_on_a_common_style_gets_a_new_automatic_style_with_that_parent(save: &Saver) {
    let d = Drawing::a4()
        .common(r##"<style:style style:name="Box_20_Plain" style:display-name="Box Plain" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#112233"/></style:style>"##)
        .auto(&gr("gr7", "#00ff00"))
        .page(&format!("{}{}", rect("c", "Box_20_Plain", "1cm"), rect("d", "gr7", "6cm")));
    let mut o = open(&d);
    let c = id_of(&o.doc, "c");
    set_text(shape_mut(&mut o.doc, 0, c), "FillForegnd", "#224466");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let new = style_of(&after, "gr8").unwrap_or_else(|| panic!("no gr8: {after}"));
    assert_eq!(
        new,
        r##"<style:style style:name="gr8" style:family="graphic" style:parent-style-name="Box_20_Plain"><style:graphic-properties draw:fill-color="#224466" draw:fill="solid"/></style:style>"##
    );
    assert!(after.contains(r#"draw:name="c" draw:style-name="gr8""#), "{after}");
    assert_eq!(part(&saved.bytes, "styles.xml"), part(&o.bytes, "styles.xml"), "the common style is not touched");
    let back = open_bytes(saved.bytes);
    assert_eq!(cell_of(&back.doc, "c", "FillForegnd").as_deref(), Some("#224466"));
}

/// A shape that names no style gets a new automatic style with no parent.
fn a_shape_that_names_no_style_gets_a_new_automatic_style(save: &Saver) {
    let mut o = open(&Drawing::a4().page(r#"<draw:rect draw:name="n" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm"/>"#));
    let n = id_of(&o.doc, "n");
    set_text(shape_mut(&mut o.doc, 0, n), "LinePattern", "0");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(
        after.contains(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="none"/></style:style>"#),
        "{after}"
    );
    assert!(after.contains(r#"draw:name="n" svg:x="1cm" svg:y="1cm" svg:width="1cm" svg:height="1cm" draw:style-name="gr1""#), "{after}");
    assert_eq!(cell_of(&open_bytes(saved.bytes).doc, "n", "LinePattern").as_deref(), Some("0"));
}

/// V4 and L8 (delete): the shape that alone names a style is deleted and the style goes; the style `b` still names stays; a style that
/// nothing named in the file as it was stays.
fn a_style_nothing_names_after_a_delete_is_removed(save: &Saver) {
    let autos = format!("{}{}{}", gr("gr1", "#ff0000"), gr("gr2", "#00ff00"), gr("gr9", "#0000ff"));
    let mut o = open(&drawing(&autos, &format!("{}{}{}", rect("a", "gr2", "1cm"), rect("b", "gr1", "6cm"), rect("e", "gr1", "11cm"))));
    let a = id_of(&o.doc, "a");
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == a).unwrap();
    page.shapes.remove(at);
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(style_of(&after, "gr2").is_none(), "gr2 was named by the deleted shape alone: {after}");
    assert_eq!(style_of(&after, "gr1"), style_of(&before, "gr1"), "gr1 is still named");
    assert_eq!(style_of(&after, "gr9"), style_of(&before, "gr9"), "gr9 was named by nothing before the save, and is the file's own");
    let expected = before.replacen(&rect("a", "gr2", "1cm"), "", 1).replacen(style_of(&before, "gr2").unwrap(), "", 1);
    assert_eq!(after, expected, "the element and the style, and nothing else");
    let diff = compare(&o.bytes, &saved.bytes);
    let (changed, added, removed) = content_changes(&diff);
    assert_eq!((changed.len(), added, removed), (0, 0, 2), "the rectangle and the style: {diff:?}");
}

/// V4 over two saves: a style cloned by a restyle, then the restyled shape deleted: the clone goes and the original stays for `b`.
fn a_clone_is_removed_when_its_shape_is_deleted(save: &Saver) {
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr7", "#00ff00"));
    let mut o = open(&drawing(&autos, &format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr7", "11cm"))));
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    let first = save(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(style_of(&part(&first.bytes, "content.xml"), "gr8").is_some(), "the first save made the clone");
    // The second save goes through the index of the first.
    let mut second = open_bytes(first.bytes.clone());
    let a = id_of(&second.doc, "a");
    let page = Arc::make_mut(second.doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == a).unwrap();
    page.shapes.remove(at);
    let saved = save(&second.doc, &second.bytes, &second.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert!(style_of(&after, "gr8").is_none() && !after.contains(r#"draw:name="a""#), "{after}");
    assert_eq!(style_of(&after, "gr1"), style_of(&before, "gr1"));
    assert_eq!(after, before.replacen(&rect("a", "gr1", "1cm"), "", 1), "the file as it was without the shape: the clone and the shape are gone");
    let _ = &mut second;
}

/// Two shapes of a shared style both restyled: the first gets a copy and the second is the last to name the original, so edits it where it is.
fn two_shapes_of_a_shared_style_restyled_alike_clone_once(save: &Saver) {
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr7", "#00ff00"));
    let mut o = open(&drawing(&autos, &format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr7", "11cm"))));
    for name in ["a", "b"] {
        let id = id_of(&o.doc, name);
        set_text(shape_mut(&mut o.doc, 0, id), "FillForegnd", "#336699");
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r##"style:name="gr1" style:family="graphic" style:parent-style-name="Standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699""##), "{after}");
    assert!(style_of(&after, "gr8").is_some() && style_of(&after, "gr9").is_none(), "one copy: {after}");
    let back = open_bytes(saved.bytes);
    assert_eq!(cell_of(&back.doc, "a", "FillForegnd").as_deref(), Some("#336699"));
    assert_eq!(cell_of(&back.doc, "b", "FillForegnd").as_deref(), Some("#336699"));
    assert_eq!(cell_of(&back.doc, "c", "FillForegnd").as_deref(), Some("#00FF00"));
}

/// Security finding (MEDIUM): a style whose name is over 4,096 bytes is counted like any other. Two shapes share it: restyling one gives it a copy
/// and leaves the original to the other; deleting one leaves the style; deleting both takes it out. A count that skipped long values saw no
/// reference, edited the style where it stood while the other shape named it, and never took it out.
fn a_style_with_a_name_over_4096_bytes_is_counted_like_any_other(save: &Saver) {
    let long = "g".repeat(5_000);
    let body = format!("{}{}{}", rect("a", &long, "1cm"), rect("b", &long, "6cm"), rect("c", "gr7", "11cm"));
    let d = drawing(&format!("{}{}", gr(&long, "#ff0000"), gr("gr7", "#00ff00")), &body);
    // Restyled: the shape gets a copy, and the other shape and the style are as they were.
    let mut o = open(&d);
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let (before, after) = (part(&o.bytes, "content.xml"), part(&saved.bytes, "content.xml"));
    assert_eq!(style_of(&after, &long), style_of(&before, &long), "the shared style is not edited");
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr8""#) && style_of(&after, "gr8").is_some(), "the shape has a copy");
    assert!(after.contains(&format!(r#"draw:name="b" draw:style-name="{long}""#)), "the other shape names the original");
    // One user deleted: the style stays.
    let mut o = open(&d);
    let a = id_of(&o.doc, "a");
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == a).unwrap();
    page.shapes.remove(at);
    let kept = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(style_of(&kept, &long).is_some(), "b still names it");
    // Both deleted: it goes.
    let mut o = open(&d);
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    page.shapes.remove(1);
    page.shapes.remove(0);
    let gone = part(&save(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
    assert!(style_of(&gone, &long).is_none(), "nothing names it: {}", gone.len());
    assert!(style_of(&gone, "gr7").is_some(), "the style c names stays");
}

/// The new name is one more than the largest number in either part.
fn a_new_name_counts_the_styles_of_both_parts(save: &Saver) {
    let mut d = drawing(
        &format!("{}{}{}", gr("gr1", "#ff0000"), gr("gr4", "#00ff00"), gr("gr9", "#00ffff")),
        &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm")),
    );
    d.styles_auto.push_str(&gr("gr12", "#ffff00"));
    let mut o = open(&d);
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "LineColor", "#010203");
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(
        style_of(&after, "gr13").is_some() && after.contains(r#"draw:name="a" draw:style-name="gr13""#),
        "gr13, and not the first free number gr2: {after}"
    );
    assert!(style_of(&after, "gr2").is_none() && style_of(&after, "gr5").is_none());
    assert!(after.contains(r##"svg:stroke-color="#010203""##));
}

/// The opacity, the line and the text block are written; attributes the new kind of fill no longer uses are taken out.
fn a_fill_turned_to_none_takes_the_gradient_names_out_and_the_margins_replace_fo_padding(save: &Saver) {
    let styles = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="g1" draw:opacity="60%" fo:padding="0.35cm"/></style:style>"##;
    let mut o = open(&drawing(styles, &rect("a", "gr1", "1cm")));
    let a = id_of(&o.doc, "a");
    {
        let s = shape_mut(&mut o.doc, 0, a);
        set_text(s, "FillPattern", "0");
        set_text(s, "FillGradientEnabled", "0");
        set_text(s, "FillForegndTrans", "0.25");
        set_text(s, "TopMargin", "0.1");
    }
    let saved = save(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    let s = style_of(&after, "gr1").unwrap();
    assert!(s.contains(r#"draw:fill="none""#) && !s.contains("fill-gradient-name"), "{s}");
    assert!(s.contains(r#"draw:opacity="75%""#), "{s}");
    assert!(
        !s.contains("fo:padding=") && s.contains("fo:padding-top=\"0.254cm\"") && s.contains("fo:padding-bottom="),
        "the four paddings replace it: {s}"
    );
}

type Case = (&'static str, fn(&Saver));

const CASES: [Case; 10] = [
    ("restyle_one_of_two_shapes_sharing_gr1_clones_the_style", restyle_case),
    ("a_style_one_shape_names_is_edited_in_place", a_style_one_shape_names_is_edited_in_place),
    ("a_shape_on_a_common_style_gets_a_new_automatic_style_with_that_parent", a_shape_on_a_common_style_gets_a_new_automatic_style_with_that_parent),
    ("a_shape_that_names_no_style_gets_a_new_automatic_style", a_shape_that_names_no_style_gets_a_new_automatic_style),
    ("orphan_automatic_style_is_removed", a_style_nothing_names_after_a_delete_is_removed),
    ("a_clone_is_removed_when_its_shape_is_deleted", a_clone_is_removed_when_its_shape_is_deleted),
    ("two_shapes_of_a_shared_style_restyled_alike_clone_once", two_shapes_of_a_shared_style_restyled_alike_clone_once),
    ("a_new_name_counts_the_styles_of_both_parts", a_new_name_counts_the_styles_of_both_parts),
    ("a_fill_turned_to_none", a_fill_turned_to_none_takes_the_gradient_names_out_and_the_margins_replace_fo_padding),
    ("a_style_with_a_name_over_4096_bytes_is_counted_like_any_other", a_style_with_a_name_over_4096_bytes_is_counted_like_any_other),
];

#[test]
fn restyle_one_of_two_shapes_sharing_gr1_clones_the_style() {
    restyle_case(&real);
}

#[test]
fn a_style_one_shape_names_is_edited_in_place_test() {
    a_style_one_shape_names_is_edited_in_place(&real);
}

#[test]
fn a_shape_on_a_common_style_gets_a_new_automatic_style_with_that_parent_test() {
    a_shape_on_a_common_style_gets_a_new_automatic_style_with_that_parent(&real);
}

#[test]
fn a_shape_that_names_no_style_gets_a_new_automatic_style_test() {
    a_shape_that_names_no_style_gets_a_new_automatic_style(&real);
}

#[test]
fn orphan_automatic_style_is_removed() {
    a_style_nothing_names_after_a_delete_is_removed(&real);
}

#[test]
fn a_clone_is_removed_when_its_shape_is_deleted_test() {
    a_clone_is_removed_when_its_shape_is_deleted(&real);
}

#[test]
fn two_shapes_of_a_shared_style_restyled_alike_clone_once_test() {
    two_shapes_of_a_shared_style_restyled_alike_clone_once(&real);
}

#[test]
fn a_style_with_a_name_over_4096_bytes_is_counted_like_any_other_test() {
    a_style_with_a_name_over_4096_bytes_is_counted_like_any_other(&real);
}

#[test]
fn a_new_name_counts_the_styles_of_both_parts_test() {
    a_new_name_counts_the_styles_of_both_parts(&real);
}

#[test]
fn a_fill_turned_to_none_takes_the_gradient_names_out_test() {
    a_fill_turned_to_none_takes_the_gradient_names_out_and_the_margins_replace_fo_padding(&real);
}

/// The echo control: a writer that gives the original back fails every case, and the real writer passes each.
#[test]
fn echo_control_fails_every_style_case() {
    for (name, case) in CASES {
        let outcome = catch_unwind(AssertUnwindSafe(|| case(&echo)));
        let Err(payload) = outcome else { panic!("the echo writer passed the case {name}: the case checks nothing") };
        let said = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
        println!("ECHO FAILS {name}: {}", said.lines().next().unwrap_or_default().chars().take(160).collect::<String>());
    }
    for (name, case) in CASES {
        assert!(catch_unwind(AssertUnwindSafe(|| case(&echo_says_edited))).is_err(), "an echo that says it edited passed the case {name}");
        assert!(catch_unwind(AssertUnwindSafe(|| case(&real))).is_ok(), "the real writer failed the case {name}");
    }
}

#[test]
fn a_restyle_that_no_attribute_can_say_is_carried_by_the_extension_and_reads_back() {
    let o = open(&drawing(&gr("gr1", "#ff0000"), &rect("a", "gr1", "1cm")));
    let a = id_of(&o.doc, "a");
    // A cell the style's attributes cannot say (a hatch, a dash, a palette index, a shadow, a transparency past 1, a formula) is carried by DiaCraft's extension
    // The save is made, the style keeps the attributes it had, and the cell reads back as the model holds it.
    let carried = |doc: &Document, cell: &str, expect: &str| {
        let saved = real(doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("{cell}: {e}"));
        assert!(saved.edited, "{cell}");
        let back = open_bytes(saved.bytes);
        let read = back.doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
        let got = read.sheet.cell(cell).map(|c| c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default().clone());
        let formula = read.sheet.cell(cell).and_then(|c| c.formula.text().map(str::to_owned));
        assert!(got.as_deref() == Some(expect) || formula.as_deref() == Some(expect), "{cell}: {got:?} {formula:?}");
    };
    let base = o.doc.clone();
    for (cell, value) in [("FillPattern", "7"), ("LinePattern", "5"), ("FillForegnd", "3"), ("ShdwPattern", "1"), ("FillForegndTrans", "2")] {
        let mut doc = base.clone();
        set_text(shape_mut(&mut doc, 0, a), cell, value);
        carried(&doc, cell, value);
    }
    // A cell that holds a formula: its formula is tier 2.
    let mut doc = base.clone();
    Arc::make_mut(&mut shape_mut(&mut doc, 0, a).sheet)
        .cells
        .insert(CellName::new("FillForegnd"), Cell::read(Some("THEMEVAL()"), Some("#336699"), None, None, &mut 100));
    carried(&doc, "FillForegnd", "THEMEVAL()");
    // A pattern of 1 is the solid fill the style has: not a change.
    let mut doc = base;
    set_text(shape_mut(&mut doc, 0, a), "FillPattern", "1");
    assert!(real(&doc, &o.bytes, &o.index).is_ok_and(|w| !w.edited));
}

/// The fuzz target's first finding: a fill turned to none reads back with no `FillGradientEnabled`, which the model made of a solid fill holds at
/// 0, and the second save of the same model through the new index took the absent cell for a change.
#[test]
fn a_fill_turned_to_none_is_no_edit_when_the_model_is_saved_again() {
    let mut o = open(&drawing(&gr("gr1", "#ff0000"), &rect("a", "gr1", "1cm")));
    let a = id_of(&o.doc, "a");
    assert_eq!(cell_of(&o.doc, "a", "FillGradientEnabled").as_deref(), Some("0"), "the reader makes the cell for a solid fill");
    set_text(shape_mut(&mut o.doc, 0, a), "FillPattern", "0");
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(first.edited);
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited, "the same model saved again is no edit");
    assert_eq!(again.bytes, first.bytes);
}

/// The fuzz target's second finding: shapes named a style the file did not define (`gr2`), and the new style a restyle made was given that
/// name, so that those shapes came to name it and the second save found them changed. A new name is never one an attribute of the part holds.
#[test]
fn a_new_style_is_never_given_a_name_an_element_of_the_part_already_names() {
    let body = format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr2", "11cm"));
    let mut o = open(&drawing(&gr("gr1", "#ff0000"), &body));
    let a = id_of(&o.doc, "a");
    assert_eq!(cell_of(&o.doc, "c", "FillForegnd"), None, "`gr2` is no style of the file");
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&first.bytes, "content.xml");
    assert!(style_of(&after, "gr2").is_none(), "gr2 stays what it was, a name no style has: {after}");
    assert!(style_of(&after, "gr3").is_some() && after.contains(r#"draw:name="a" draw:style-name="gr3""#), "{after}");
    assert!(after.contains(r#"draw:name="c" draw:style-name="gr2""#));
    // The same model saved again is no edit, and the shape that named `gr2` still has no fill colour read back.
    let again = real(&o.doc, &first.bytes, &first.index).unwrap();
    assert!(!again.edited);
    assert_eq!(cell_of(&open_bytes(first.bytes).doc, "c", "FillForegnd"), None);
}

/// A reference is counted whatever the length of the value beside the names: a value shorter than the longest name (`gr1` where `gr10` is a style) and
/// a name written with a character reference (so that it is longer than the name it reads as) both name the style and make it shared.
#[test]
fn a_style_is_counted_when_its_name_is_shorter_than_the_longest_and_when_it_is_written_with_a_character_reference() {
    let restyled = |autos: &str, body: &str| {
        let mut o = open(&drawing(autos, body));
        let a = id_of(&o.doc, "a");
        set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
        part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml")
    };
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr10", "#00ff00"));
    let after = restyled(&autos, &format!("{}{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"), rect("c", "gr10", "11cm")));
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr11""#) && after.contains(r#"draw:name="b" draw:style-name="gr1""#), "{after}");
    assert!(style_of(&after, "gr1").unwrap().contains(r##"draw:fill-color="#ff0000""##), "the shape that stays has its style: {after}");
    // `&#103;r1` reads as `gr1`: the style has two shapes, and the one restyled gets a clone.
    let after = restyled(&gr("gr1", "#ff0000"), &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "&#103;r1", "6cm")));
    assert!(after.contains(r#"draw:name="a" draw:style-name="gr2""#) && after.contains(r#"draw:name="b" draw:style-name="&#103;r1""#), "{after}");
    assert!(style_of(&after, "gr1").unwrap().contains(r##"draw:fill-color="#ff0000""##), "{after}");
}

/// A shape whose `draw:style-name` is an automatic style of another family (a paragraph style) is refused in words: it is not a graphic style to edit
/// where it is, and not a common style to take as a parent.
#[test]
fn a_shape_that_names_an_automatic_style_of_another_family_is_refused_in_words() {
    let autos = r#"<style:style style:name="P1" style:family="paragraph"/>"#;
    let mut o = open(&drawing(autos, &rect("a", "P1", "1cm")));
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("graphic family"), "{words}"),
        other => panic!("expected a refusal: {other:?}"),
    }
}

/// What a restyle of the shape `a` saved, counted: the result and the work.
fn restyled_counted(autos: &str, cells: &[(&str, &str)], limits: Limits) -> (Result<diacraft_odg::Written, WriteError>, WriteWork) {
    let mut o = open(&drawing(autos, &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "gr2", "6cm"))));
    let a = id_of(&o.doc, "a");
    for (cell, text) in cells {
        set_text(shape_mut(&mut o.doc, 0, a), cell, text);
    }
    let mut work = WriteWork::default();
    let options = WriteOptions { limits, ..WriteOptions::default() };
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    (saved, work)
}

/// Each attribute a restyle writes in place is charged to the cap of attributes: two written pass a cap of two and are refused under a cap of one.
#[test]
fn the_attributes_a_restyle_writes_in_place_are_charged_to_the_cap() {
    let autos = format!("{}{}", gr("gr1", "#ff0000"), gr("gr2", "#00ff00"));
    let cells = [("FillForegnd", "#336699"), ("LineWeight", "0.05")];
    let (two, _) = restyled_counted(&autos, &cells, Limits { max_write_attributes: 2, ..Limits::default() });
    assert!(two.is_ok_and(|w| w.edited));
    let (one, _) = restyled_counted(&autos, &cells, Limits { max_write_attributes: 1, ..Limits::default() });
    assert_eq!(one.err().map(|e| e.class()), Some("write_limit"));
}

/// A length the cell gives that is the length the attribute has is left as the file wrote it (`0.10cm`, and not `0.1cm`), where another cell's edit
/// alone is written.
#[test]
fn a_length_that_is_the_one_the_attribute_has_is_left_as_written() {
    let autos = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" svg:stroke-width="0.10cm"/></style:style><style:style style:name="gr2" style:family="graphic"/>"##;
    let nearly = (crate::drawing::cm(0.1) + 1e-15).to_string();
    let (saved, _) = restyled_counted(autos, &[("FillForegnd", "#336699"), ("LineWeight", &nearly)], Limits::default());
    let after = part(&saved.unwrap().bytes, "content.xml");
    assert!(after.contains(r##"draw:fill-color="#336699" svg:stroke-width="0.10cm""##), "{after}");
}

/// What a clone and a new style write is read back by the check of the bytes made: one more attribute on the clone is one more attribute checked.
#[test]
fn the_check_of_the_bytes_made_reads_back_each_attribute_a_clone_has_written() {
    // `a` and `b` share `gr1`, so `a` gets a clone.
    let checked = |cells: &[(&str, &str)]| {
        let autos = gr("gr1", "#ff0000");
        let mut o = open(&drawing(&autos, &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"))));
        let a = id_of(&o.doc, "a");
        for (cell, text) in cells {
            set_text(shape_mut(&mut o.doc, 0, a), cell, text);
        }
        let mut work = WriteWork::default();
        let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &WriteOptions::default(), &mut work);
        assert!(saved.is_ok_and(|w| w.edited));
        work.checked_attributes
    };
    let (one, two) = (checked(&[("FillForegnd", "#336699")]), checked(&[("FillForegnd", "#336699"), ("LineColor", "#010203")]));
    assert!(one >= 1, "{one}");
    assert_eq!(two - one, 1, "{one} then {two}");
}

/// A style with no graphic properties gets them where it is when one shape names it, and its copy has them when others name it too, whether the
/// style is written `<style:style .../>`, with another child or with none.
#[test]
fn a_style_with_no_graphic_properties_gets_them_in_place_or_in_its_copy() {
    let forms = [
        (
            r##"<style:style style:name="gr1" style:family="graphic"/>"##,
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill-color="#336699" draw:fill="solid"/></style:style>"##,
        ),
        (
            r##"<style:style style:name="gr1" style:family="graphic"></style:style>"##,
            r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill-color="#336699" draw:fill="solid"/></style:style>"##,
        ),
        (
            r##"<style:style style:name="gr1" style:family="graphic"><style:text-properties fo:font-weight="bold"/></style:style>"##,
            r##"<style:style style:name="gr1" style:family="graphic"><style:text-properties fo:font-weight="bold"/><style:graphic-properties draw:fill-color="#336699" draw:fill="solid"/></style:style>"##,
        ),
    ];
    for (before, expected) in forms {
        // One shape names it: the style is edited where it is.
        let mut o = open(&drawing(before, &rect("a", "gr1", "1cm")));
        let a = id_of(&o.doc, "a");
        set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
        let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
        assert!(after.contains(&format!("<office:automatic-styles>{expected}</office:automatic-styles>")), "in place, from {before}: {after}");
        // Two shapes name it: the copy has the properties and the original is as it was.
        let mut o = open(&drawing(before, &format!("{}{}", rect("a", "gr1", "1cm"), rect("b", "gr1", "6cm"))));
        let a = id_of(&o.doc, "a");
        set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
        let after = part(&real(&o.doc, &o.bytes, &o.index).unwrap().bytes, "content.xml");
        assert!(after.contains(&format!("<office:automatic-styles>{before}{}</office:automatic-styles>", expected.replace("gr1", "gr2"))), "{after}");
    }
}

/// Review LOW: a style named only by the parent attribute of a style that is taken out is an orphan of an orphan and goes in the same save, and so does
/// its parent in turn, up the chain. `gr1` is the parent of `gr2`, which is the parent of `gr3`, which only shape `b` names: `b` deleted takes out all
/// three; a style that is named by another style that stays, or by a shape, stays; and a style nothing named before the save stays.
#[test]
fn a_style_named_only_by_the_parent_of_a_style_taken_out_is_taken_out_too_up_the_chain() {
    let style = |name: &str, parent: &str| {
        let parent = if parent.is_empty() { String::new() } else { format!(" style:parent-style-name=\"{parent}\"") };
        format!("<style:style style:name=\"{name}\" style:family=\"graphic\"{parent}><style:graphic-properties draw:fill=\"solid\"/></style:style>")
    };
    // gr1 <- gr2 <- gr3 (named by b); gr5 <- gr6 (named by c, which stays); gr7 <- gr8 (named by d, which stays); gr9 was never named; and
    // gr10 is the parent of gr11 (named by e, deleted too) and of gr12 (named by f, which stays): named by a style that goes and by one that stays.
    let autos = [
        style("gr1", ""),
        style("gr2", "gr1"),
        style("gr3", "gr2"),
        style("gr5", ""),
        style("gr6", "gr5"),
        style("gr7", ""),
        style("gr8", "gr7"),
        style("gr9", ""),
        style("gr10", ""),
        style("gr11", "gr10"),
        style("gr12", "gr10"),
    ]
    .concat();
    let body =
        [rect("b", "gr3", "1cm"), rect("e", "gr11", "3cm"), rect("c", "gr6", "6cm"), rect("d", "gr8", "11cm"), rect("f", "gr12", "14cm")].concat();
    let mut o = open(&drawing(&autos, &body));
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    page.shapes.remove(0);
    page.shapes.remove(0);
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    for gone in ["gr1", "gr2", "gr3", "gr11"] {
        assert!(style_of(&after, gone).is_none(), "{gone} was named only by the chain that b or e was the end of: {after}");
    }
    for stays in ["gr5", "gr6", "gr7", "gr8", "gr9", "gr10", "gr12"] {
        assert!(style_of(&after, stays).is_some(), "{stays}: {after}");
    }
    // The same save made twice through the new index is no edit.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    // A chain of two thousand: the work follows the styles, not a walk of all of them for each one taken out.
    let n = 2_000;
    let chain: String = (1..=n).map(|k| style(&format!("gr{k}"), &if k > 1 { format!("gr{}", k - 1) } else { String::new() })).collect();
    let mut o = open(&drawing(&chain, &rect("end", &format!("gr{n}"), "1cm")));
    Arc::make_mut(o.doc.pages.get_mut(0).unwrap()).shapes.remove(0);
    let mut work = diacraft_odg::WriteWork::default();
    let saved = diacraft_odg::write_counted(
        &o.doc,
        Some(&diacraft_odg::OriginalPackage { bytes: &o.bytes, index: &o.index }),
        &diacraft_odg::WriteOptions::default(),
        &mut work,
    )
    .unwrap();
    assert!(!part(&saved.bytes, "content.xml").contains("style:name=\"gr"), "every style of the chain went");
    assert!(work.order_steps < 20 * n as u64, "{} steps for a chain of {n}", work.order_steps);
}

/// A namespace declaration is no reference: one whose value is the name of a style does not make the style shared, so the one shape that names it
/// edits it where it is (a prefix declared on the root, and the default namespace declared on a shape that has only prefixed children).
#[test]
fn a_namespace_declaration_that_holds_the_name_of_a_style_is_no_reference_to_it() {
    let mut d = drawing(&gr("gr1", "#ff0000"), &rect("a", "gr1", "1cm"));
    d.ns.push_str(r#" xmlns:zz="gr1""#);
    d.pages = d.pages.replace("<draw:rect ", r#"<draw:rect xmlns="gr1" "#);
    let mut o = open(&d);
    let a = id_of(&o.doc, "a");
    set_text(shape_mut(&mut o.doc, 0, a), "FillForegnd", "#336699");
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    let after = part(&saved.bytes, "content.xml");
    assert!(after.contains(r##"draw:fill-color="#336699""##) && !after.contains("gr2"), "edited where it is: {after}");
}
