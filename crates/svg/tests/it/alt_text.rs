//! The alt text of a shape and a page in the SVG file (and the owner's ruling of 2026-11-26 that a shape with no alt text
//! has its name as the `<title>`). Each file is read back with the strict parser of `opc`, which knows nothing of the writer, and every assertion is a value of what it read.

use diacraft_doc::{Document, RowKey};
use diacraft_opc::xml::El;
use diacraft_render::Renderer;
use diacraft_svg::{SvgOptions, export_page};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::support::*;

/// Gives a shape (or a page) a row of its `User` section, as the files hold it.
fn user<T>(item: T, row: &str, text: &str, with: impl FnOnce(T, &str, &str) -> T) -> T {
    with(item, row, text)
}

fn titled(shape: ShapeBuilder, title: &str) -> ShapeBuilder {
    user(shape, "visAltText", title, |s, r, t| s.row_cell("User", None, RowKey::name(r), "Value", t))
}

fn described(shape: ShapeBuilder, text: &str) -> ShapeBuilder {
    user(shape, "visAltDescription", text, |s, r, t| s.row_cell("User", None, RowKey::name(r), "Value", t))
}

fn page_with(title: Option<&str>, description: Option<&str>, shapes: Vec<ShapeBuilder>) -> Document {
    let mut p: PageBuilder = page(4.0, 4.0);
    if let Some(t) = title {
        p = p.row_cell("User", None, RowKey::name("visAltText"), "Value", t);
    }
    if let Some(d) = description {
        p = p.row_cell("User", None, RowKey::name("visAltDescription"), "Value", d);
    }
    for s in shapes {
        p = p.shape(s);
    }
    base().page(1, p).build()
}

/// The `<g>` elements whose first child is a `<title>`, with their title and their `<desc>` if the next child is one.
fn groups(root: &El) -> Vec<(&El, String, Option<String>)> {
    named(root, "g")
        .into_iter()
        .filter_map(|g| {
            let mut kids = g.elements();
            let first = kids.next()?;
            if !first.is("title") && !first.is("desc") {
                return None;
            }
            let (title, desc) = if first.is("title") {
                (first.text(), kids.next().filter(|k| k.is("desc")).map(El::text))
            } else {
                (String::new(), Some(first.text()))
            };
            Some((g, title, desc))
        })
        .collect()
}

fn exported(doc: &Document, options: &SvgOptions) -> (String, El) {
    let out = export_page(&mut Renderer::new(), doc, 1, options).expect("exports");
    let root = diacraft_opc::xml::parse(&out.bytes).expect("a well-formed SVG file");
    (String::from_utf8(out.bytes).expect("UTF-8"), root)
}

fn no_names() -> SvgOptions {
    SvgOptions::default()
}

#[test]
fn a_shapes_group_holds_its_title_then_its_description_escaped_and_then_its_drawing() {
    let shape = described(titled(rect(1, 1.0, 1.0, 2.0, 1.0), "Pump"), "Feeds <boiler> & tank");
    let (text, root) = exported(&page_with(None, None, vec![shape]), &no_names());
    let found = groups(&root);
    assert_eq!(found.len(), 1, "One group");
    let (g, title, desc) = &found[0];
    assert_eq!((title.as_str(), desc.as_deref()), ("Pump", Some("Feeds <boiler> & tank")));
    assert!(text.contains("<title>Pump</title>\n<desc>Feeds &lt;boiler&gt; &amp; tank</desc>"), "The text is escaped, title before desc: {text}");
    let kids: Vec<&str> = g.elements().map(|e| e.name.as_str()).collect();
    assert!(kids[0].ends_with("title") && kids[1].ends_with("desc") && kids[2].ends_with("path"), "{kids:?}");
}

#[test]
fn a_shape_with_no_alt_text_has_its_name_as_the_title_and_no_description() {
    let named_shape = rect(1, 0.5, 0.5, 1.0, 1.0).named("Rectangle.1", "Rectangle.1");
    let nameless = rect(2, 2.0, 2.0, 1.0, 1.0);
    let (text, root) = exported(&page_with(None, None, vec![named_shape, nameless]), &no_names());
    let found = groups(&root);
    assert_eq!(found.len(), 1, "the shape with no name has no group");
    assert_eq!((found[0].1.as_str(), found[0].2.as_deref()), ("Rectangle.1", None));
    assert!(!text.contains("<desc>"), "no empty description");
    // The nameless shape's path stands directly under the root, as it always did.
    let direct: Vec<&El> = root.elements().filter(|e| e.is("path")).collect();
    assert!(direct.len() >= 2, "the paper and the shape with no name: {}", direct.len());
}

#[test]
fn a_shape_with_a_description_only_has_no_title_made_of_its_name() {
    let shape = described(rect(1, 1.0, 1.0, 1.0, 1.0).named("Rectangle.1", "Rectangle.1"), "A valve");
    let (_, root) = exported(&page_with(None, None, vec![shape]), &no_names());
    let found = groups(&root);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].1.as_str(), found[0].2.as_deref()), ("", Some("A valve")), "the name is the title of a shape that has no alt text at all");
    let first = found[0].0.elements().next().unwrap();
    assert!(first.is("desc"), "no <title> is written for it");
}

#[test]
fn the_alt_title_wins_over_the_name_and_a_blank_one_is_no_title() {
    let a = titled(rect(1, 0.5, 0.5, 1.0, 1.0).named("Rectangle.1", "Rectangle.1"), "Boiler");
    let b = titled(rect(2, 2.0, 0.5, 1.0, 1.0).named("Rectangle.2", "Rectangle.2"), " \t\u{a0}");
    let (_, root) = exported(&page_with(None, None, vec![a, b]), &no_names());
    let got: Vec<(String, Option<String>)> = groups(&root).into_iter().map(|g| (g.1, g.2)).collect();
    assert_eq!(
        got,
        [("Boiler".to_owned(), None), ("Rectangle.2".to_owned(), None)],
        "a title of white space is as good as none, and the name stands in"
    );
}

#[test]
fn the_pages_title_and_description_are_the_files() {
    let options = SvgOptions { title: Some("Report".to_owned()), ..SvgOptions::default() };
    let (text, root) = exported(&page_with(Some("Plant overview"), None, vec![]), &options);
    let kids: Vec<&El> = root.elements().collect();
    assert!(kids[0].is("title") && kids[0].text() == "Plant overview", "The page's title is the root's <title>");
    assert!(!text.contains("<desc>"), "No root description, and nothing empty");
    assert_eq!(named(&root, "title").len(), 1);
    // Without the page's title, the option stands.
    let (_, root) = exported(&page_with(None, None, vec![]), &options);
    assert_eq!(root.elements().next().unwrap().text(), "Report", "V33");
    // The description follows the title.
    let (_, root) = exported(&page_with(Some("Plant"), Some("All units & lines"), vec![]), &options);
    let kids: Vec<&El> = root.elements().collect();
    assert!(kids[0].is("title") && kids[1].is("desc") && kids[1].text() == "All units & lines");
    // A description alone leaves the title to the option.
    let (_, root) = exported(&page_with(None, Some("Only this"), vec![]), &options);
    let kids: Vec<&El> = root.elements().collect();
    assert_eq!((kids[0].text(), kids[1].text()), ("Report".to_owned(), "Only this".to_owned()));
}

#[test]
fn a_title_or_description_of_white_space_and_a_blank_option_are_never_written() {
    let options = SvgOptions { title: Some("  ".to_owned()), ..SvgOptions::default() };
    let (text, _) = exported(&page_with(Some(" "), Some("\n"), vec![]), &options);
    assert!(!text.contains("<title") && !text.contains("<desc"), "{text}");
}

#[test]
fn a_group_holds_its_members_and_a_member_with_alt_text_is_a_group_in_it() {
    let member = titled(rect(2, 1.0, 1.0, 1.0, 1.0), "Handle");
    let other = rect(3, 2.0, 1.0, 1.0, 1.0);
    let group = titled(shape_at(1, 0.5, 0.5, 3.0, 2.0).group().child(member).child(other), "Valve assembly");
    let (_, root) = exported(&page_with(None, None, vec![group]), &no_names());
    let found = groups(&root);
    assert_eq!(found.iter().map(|g| g.1.as_str()).collect::<Vec<_>>(), ["Valve assembly", "Handle"]);
    // The member's group is inside the group's, and the other member's drawing is in the group's too.
    let outer = found[0].0;
    assert!(all(outer).iter().any(|e| std::ptr::eq(*e, found[1].0)), "the member's group is inside the group's");
    assert_eq!(named(outer, "path").len(), 4, "both members' paths, a fill and a line each, are inside the group (the group itself draws none)");
}

#[test]
fn an_instance_has_its_masters_alt_text_in_the_file() {
    let master = MasterBuilder::new().name("Valve").shape(titled(rect(1, 0.0, 0.0, 1.0, 1.0), "Gate valve"));
    let doc = DocBuilder::new().master(5, master).page(1, page(4.0, 4.0).shape(shape_at(2, 1.0, 1.0, 1.0, 1.0).master(5))).build();
    let (_, root) = exported(&doc, &no_names());
    assert_eq!(groups(&root).into_iter().map(|g| g.1).collect::<Vec<_>>(), ["Gate valve"]);
}

#[test]
fn a_shape_on_a_hidden_layer_is_not_in_the_file_and_has_no_group() {
    let p = page(4.0, 4.0)
        .row_cell("Layer", None, RowKey::Ix(0), "Visible", "0")
        .shape(titled(rect(1, 1.0, 1.0, 1.0, 1.0), "Hidden").cell("LayerMember", "0"))
        .shape(titled(rect(2, 2.0, 2.0, 1.0, 1.0), "Shown"));
    let (_, root) = exported(&base().page(1, p).build(), &no_names());
    assert_eq!(groups(&root).into_iter().map(|g| g.1).collect::<Vec<_>>(), ["Shown"]);
}

#[test]
fn characters_xml_cannot_hold_and_the_ones_it_reserves_are_written_safely() {
    let shape = titled(rect(1, 1.0, 1.0, 1.0, 1.0), "a\u{1}b\"c'd&e<f>g\u{fffe}");
    let (text, root) = exported(&page_with(None, None, vec![shape]), &no_names());
    assert_eq!(groups(&root)[0].1, "a b\"c'd&e<f>g ", "a control becomes a space, the rest comes back as it was");
    assert!(!text.contains('\u{1}'));
}

#[test]
fn text_is_kept_character_for_character_with_its_line_breaks_and_spaces() {
    let shape = described(titled(rect(1, 1.0, 1.0, 1.0, 1.0), "  Pump  "), "line one\nline two\ttabbed ");
    let (_, root) = exported(&page_with(None, None, vec![shape]), &no_names());
    let g = &groups(&root)[0];
    assert_eq!((g.1.as_str(), g.2.as_deref()), ("  Pump  ", Some("line one\nline two\ttabbed ")));
}

#[test]
fn a_text_of_a_million_characters_is_written_whole() {
    let big = "x".repeat(1 << 20);
    let (text, root) = exported(&page_with(None, None, vec![titled(rect(1, 1.0, 1.0, 1.0, 1.0), &big)]), &no_names());
    assert_eq!(groups(&root)[0].1.len(), 1 << 20);
    assert!(text.len() > 1 << 20);
}

#[test]
fn thirty_thousand_named_shapes_are_thirty_thousand_groups() {
    let shapes: Vec<ShapeBuilder> = (0..30_000u32)
        .map(|i| {
            let s = rect(i + 1, f64::from(i % 150) * 0.026, f64::from(i / 150) * 0.019, 0.02, 0.015).named(&format!("Box.{i}"), &format!("Box.{i}"));
            if i % 3 == 0 { titled(s, &format!("Alt {i}")) } else { s }
        })
        .collect();
    let (_, root) = exported(&page_with(None, None, shapes), &no_names());
    let found = groups(&root);
    assert_eq!(found.len(), 30_000);
    assert_eq!(found[0].1, "Alt 0");
    assert_eq!(found[1].1, "Box.1");
}

#[test]
fn groups_nested_a_hundred_deep_are_a_hundred_nested_groups_each_closed() {
    let mut inner = titled(rect(1, 1.0, 1.0, 1.0, 1.0), "level 0");
    for depth in 1..100u32 {
        inner = titled(shape_at(depth + 1, 0.5, 0.5, 2.0, 2.0).group().child(inner), &format!("level {depth}"));
    }
    let (_, root) = exported(&page_with(None, None, vec![inner]), &no_names());
    let found = groups(&root);
    assert_eq!(found.len(), 100);
    assert_eq!(found[0].1, "level 99", "the outermost first");
    assert_eq!(found[99].1, "level 0");
    // Each is inside the one before it.
    for pair in found.windows(2) {
        assert!(all(pair[0].0).iter().any(|e| std::ptr::eq(*e, pair[1].0)));
    }
}

#[test]
fn a_file_with_no_alt_text_and_no_names_is_the_file_it_was() {
    let (text, root) = exported(&page_with(None, None, vec![rect(1, 1.0, 1.0, 1.0, 1.0)]), &no_names());
    assert!(groups(&root).is_empty());
    assert!(!text.contains("<g>") && !text.contains("<title") && !text.contains("<desc"), "{text}");
}
