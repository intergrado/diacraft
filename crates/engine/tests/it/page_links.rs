//! **The links to a page by its name follow the page the VSDX writer renames** (the page name `page1` an `.odg` gives is written `Page-1`, a name the person did not choose). A link of this
//! drawing names a page as plain text, in the `SubAddress` of a `Hyperlink` row (`<page>` or `<page>/<shape>`), and an `.odg`'s own page link is markup the model keeps and a VSDX file
//! carries in DiaCraft's part: a `text:a` round some text and a `draw:a` round a shape, each with an `xlink:href` of the form `#<page>`. Each is written to name the page by its new
//! name, so that the link still finds the page in the file and in an `.odg` made from it again; `page.rename` by the person is not changed.

use diacraft_engine::Session;
use serde_json::json;

use crate::odg_build::{Dir, Drawing, open_drawing, part_of, rect};
use crate::{page_names, run};

const SAID: &str = "that named such a page now name it by its new name";

/// The text of the part of a package whose name ends with `suffix`.
fn part_text(bytes: &[u8], suffix: &str) -> String {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let name = pkg.parts().find(|p| p.ends_with(suffix)).unwrap_or_else(|| panic!("no part ends with {suffix}")).to_owned();
    String::from_utf8(pkg.part(&name).expect("the part").to_vec()).unwrap()
}

/// An `.odg` of two pages whose first holds a link to page 2 as a `text:a` round some text, a link to page 2 as a `draw:a` round a shape, a link to a page that is not there and a web link.
fn odg_with_links() -> Drawing {
    let text_link = r##"<text:a xlink:href="#page2" xlink:type="simple">go</text:a> <text:a xlink:href="#nowhere" xlink:type="simple">lost</text:a> <text:a xlink:href="https://example.org/page2" xlink:type="simple">web</text:a>"##;
    let one = format!(
        "{}<draw:a xlink:href=\"#page2\" xlink:type=\"simple\">{}</draw:a>",
        rect("A", (2.0, 3.0, 4.0, 2.0), text_link),
        rect("B", (12.0, 3.0, 4.0, 2.0), "wrapped")
    );
    Drawing::new().page(&one).page(&rect("C", (2.0, 3.0, 4.0, 2.0), ""))
}

fn said(out: &serde_json::Value) -> Vec<String> {
    out["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains(SAID)).map(str::to_owned).collect()
}

#[test]
fn an_odg_page_link_names_the_new_name_of_its_page_in_a_vsdx_file_and_in_the_odg_made_from_it_again() {
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &odg_with_links());
    assert_eq!(page_names(&s), ["page1", "page2"]);
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("out.vsdx") }));
    let words = said(&out);
    assert_eq!(words.len(), 1, "{out}");
    assert!(words[0].contains("2 hyperlinks that named such a page now name it by its new name"), "{}", words[0]);
    // The part of DiaCraft's own that carries the markup names the new name, and the links to nowhere and to the web are as they were.
    let carried = part_text(&dir.read("out.vsdx"), "/solutions/solution1.xml");
    assert_eq!(carried.matches(r##"value="#Page-2""##).count(), 2, "{carried}");
    assert!(!carried.contains(r##"value="#page2""##), "{carried}");
    assert!(carried.contains(r##"value="#nowhere""##) && carried.contains(r#"value="https://example.org/page2""#), "{carried}");
    // The check after the save found nothing that differs, and the drawing is what the file reads as.
    assert_eq!(out["checked"]["classes"], json!([]), "{out}");
    assert_eq!(page_names(&s), ["Page-1", "Page-2"]);
    // An `.odg` made from the drawing again has the page by its new name and the links with it.
    run(&mut s, "file.save_as", json!({ "path": dir.path("again.odg") }));
    let content = part_of(&dir.read("again.odg"), "content.xml");
    assert_eq!(content.matches(r##"xlink:href="#Page-2""##).count(), 2, "{content}");
    assert!(!content.contains(r##"xlink:href="#page2""##) && content.contains(r##"xlink:href="#nowhere""##), "{content}");
    assert!(content.contains(r#"draw:name="Page-2""#), "{content}");
}

#[test]
fn the_odg_writer_and_a_rename_by_the_person_leave_the_links_by_name_as_they_are() {
    // An `.odg` saved as an `.odg` keeps `page1` and the links to it: the writer of that format keeps the names as they are.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &odg_with_links());
    run(&mut s, "file.save_as", json!({ "path": dir.path("copy.odg") }));
    let content = part_of(&dir.read("copy.odg"), "content.xml");
    assert_eq!(content.matches(r##"xlink:href="#page2""##).count(), 2, "{content}");
    assert!(content.contains(r#"draw:name="page2""#), "{content}");
    // A page the person renames is the person's choice: the links by name stay as they are, and a save as an `.odg` writes the new name beside them.
    let second = s.document().unwrap().pages.get(1).unwrap().id;
    run(&mut s, "page.rename", json!({"id": second, "name": "Second"}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("renamed.odg") }));
    let content = part_of(&dir.read("renamed.odg"), "content.xml");
    assert_eq!(content.matches(r##"xlink:href="#page2""##).count(), 2, "{content}");
    assert!(content.contains(r#"draw:name="Second""#), "{content}");
}

#[test]
fn a_hyperlink_made_in_a_drawing_whose_page_the_save_renames_still_finds_the_page_in_the_drawing_and_in_the_file() {
    // A link of this drawing names a page by its name: to `page2`, to a shape of it (`page2/C`), and a web link whose sub-address is a fragment that only looks like the page.
    let dir = Dir::new();
    let mut s = open_drawing(&dir, &odg_with_links());
    let (a, c) = {
        let doc = s.document().unwrap();
        (crate::odg_build::id_of(doc, 0, "A"), crate::odg_build::id_of(doc, 1, "C"))
    };
    let first = s.document().unwrap().pages.get(0).unwrap().id;
    let links = json!([
        {"description": "page", "sub_address": "page2"},
        {"description": "shape", "sub_address": "page2/C"},
        {"description": "web", "address": "https://example.org/a", "sub_address": "page2"},
    ]);
    run(&mut s, "hyperlink.set", json!({"id": a, "page": first, "links": links}));
    assert_eq!(run(&mut s, "hyperlink.follow", json!({"id": a, "page": first, "link": "Row_2"}))["target_shape"], c);
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("links.vsdx") }));
    let words =
        out["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).filter(|w| w.contains("hyperlinks that named such a page")).count();
    assert_eq!(words, 1, "{out}");
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({ "path": dir.path("links.vsdx") }));
    for (what, session) in [("the drawing the save left", &mut s), ("the file", &mut reopened)] {
        let (first, to) = {
            let doc = session.document().unwrap();
            (doc.pages.get(0).unwrap().id, doc.pages.get(1).unwrap().id)
        };
        let listed = run(session, "hyperlink.list", json!({"id": a, "page": first}));
        let subs: Vec<&str> = listed["links"].as_array().unwrap().iter().map(|l| l["sub_address"].as_str().unwrap()).collect();
        assert_eq!(subs, ["Page-2", "Page-2/C", "page2"], "{what}: {listed}");
        let kinds: Vec<&str> = listed["links"].as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["page", "shape", "web"], "{what}");
        let followed = run(session, "hyperlink.follow", json!({"id": a, "page": first, "link": "Row_1"}));
        assert_eq!((followed["kind"].as_str(), followed["target_page"].as_u64()), (Some("page"), Some(u64::from(to))), "{what}: {followed}");
    }
}
