//! The gate's GATEBAE6-GROUP-ALTTEXT: a group of an `.odg` with no name that has a title and a description (`svg:title` and `svg:desc`, which reads as the rows
//! `visAltText` and `visAltDescription` of the group) is saved to `.odg` again after a visit through `.diacraft` or through a VSDX file, and the title and the description come back. They were
//! refused in words ("a group with no name that is unique among the page's groups is given data of its own"): the model has the rows as data of the group, and a group of the file that no program
//! named has no key to find its entry by. Each vector is the plain visit: `file.open`, then `file.save_as`, one leg a session.

use serde_json::json;

use crate::native_common::{Dir, opened};
use crate::odg_build::{Drawing, part_of};
use crate::save_check_misses::leg;

/// The group's markup: `name` (an attribute or none), its `title` element when there is one, its description, then two rectangles, the first with `member` inside it.
fn group(name: &str, title: &str, member: &str) -> String {
    format!(
        concat!(
            r#"<draw:g{name}>{title}<svg:desc>Two boxes</svg:desc>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm">{member}</draw:rect>"#,
            r#"<draw:rect draw:style-name="gr1" svg:x="6cm" svg:y="1cm" svg:width="3cm" svg:height="2cm"/></draw:g>"#
        ),
        name = name,
        title = title,
        member = member
    )
}

const TITLE: &str = "<svg:title>Pump group</svg:title>";

/// The visit `a` to `b` to `c`, each leg a session of its own; `c` is the last file's text of `content.xml`.
fn visit(source: &[u8], a: &str, b: &str, c: &str) -> String {
    let dir = Dir::new();
    dir.write(a, source);
    leg(&dir, a, b);
    leg(&dir, b, c);
    part_of(&dir.read(c), "content.xml")
}

fn assert_kept(content: &str) {
    assert!(content.contains(TITLE), "{content}");
    assert!(content.contains("<svg:desc>Two boxes</svg:desc>"), "{content}");
}

/// **The vector**, through `.diacraft` and through a VSDX file: the visit finishes, and the file it ends with holds the group's title and description.
#[test]
fn an_unnamed_group_with_a_title_and_a_description_finishes_the_plain_visit_and_keeps_them() {
    let source = Drawing::new().page(&group("", TITLE, "")).bytes();
    assert_kept(&visit(&source, "a.odg", "b.diacraft", "c.odg"));
    assert_kept(&visit(&source, "a.odg", "b.vsdx", "c.odg"));
}

/// The same drawing as a flat `.fodg`.
#[test]
fn the_flat_drawing_of_the_vector_finishes_the_plain_visit_too() {
    let source = Drawing::new().page(&group("", TITLE, "")).flat_bytes();
    assert_kept(&visit(&source, "a.fodg", "b.diacraft", "c.odg"));
}

/// The controls the gate named: a group with a name of its own, one with no title (and a description), a title on a member, and a plain Save of the file all finish.
#[test]
fn the_controls_of_the_vector_finish_the_plain_visit() {
    let named = Drawing::new().page(&group(r#" draw:name="Pump""#, TITLE, "")).bytes();
    assert_kept(&visit(&named, "a.odg", "b.diacraft", "c.odg"));
    let no_title = Drawing::new().page(&group("", "", "")).bytes();
    let content = visit(&no_title, "a.odg", "b.diacraft", "c.odg");
    assert!(content.contains("<svg:desc>Two boxes</svg:desc>"), "{content}");
    let on_member = Drawing::new().page(&group("", "", "<svg:title>Box</svg:title>")).bytes();
    assert!(visit(&on_member, "a.odg", "b.diacraft", "c.odg").contains("<svg:title>Box</svg:title>"));
    // A plain Save of the `.odg`.
    let dir = Dir::new();
    dir.write("a.odg", &Drawing::new().page(&group("", TITLE, "")).bytes());
    let mut s = opened(&dir.path("a.odg"));
    let saved = s.execute("file.save", &json!({})).unwrap_or_else(|e| panic!("a plain save: {e}"));
    assert_eq!(saved["edited"], false, "{saved}");
    assert_kept(&part_of(&dir.read("a.odg"), "content.xml"));
}

/// A title changed on the group in the session, then a Save into the file: the new title is **never left out of the file without a word**: the save writes it, or is refused in words (a group
/// the file has with no name has no entry to carry its rows, and the splice leaves the group's elements as they are).
#[test]
fn a_title_changed_on_an_unnamed_group_of_the_file_is_written_or_refused_and_never_dropped() {
    let dir = Dir::new();
    dir.write("a.odg", &Drawing::new().page(&group("", TITLE, "")).bytes());
    let mut s = opened(&dir.path("a.odg"));
    let id =
        s.document().and_then(|d| d.pages.get(0)).and_then(|p| p.shapes.iter().find(|x| !x.children.is_empty()).map(|x| x.id)).expect("the group");
    crate::run(&mut s, "shape.alt_text", json!({ "id": id, "title": "Other pump" }));
    match s.execute("file.save", &json!({})) {
        Ok(_) => assert!(
            part_of(&dir.read("a.odg"), "content.xml").contains("<svg:title>Other pump</svg:title>"),
            "the new title was left out of the file"
        ),
        // The engine's words for a group with no name that holds data: the group first, and the ways out; the writer's own words are the library's.
        Err(e) => assert!(
            e.to_string().contains("the group with no name numbered 1") && e.to_string().contains("has data of its own or a link to a master"),
            "{e}"
        ),
    }
}
