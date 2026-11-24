//! Part names: what an item name may be, what a relationship target resolves to, and the hostile names of V11.
//!
//! Each hostile name has a defined result and a test: `../`, absolute, backslash, drive letter, empty, a dot at the end, a control
//! character, case-only duplicates (in `package.rs`), a name smuggled through percent-encoding, and a name past the length limit.

use diacraft_opc::name::{fold, rels_part_name, resolve};
use diacraft_opc::{Limits, Package};

use crate::zipbuild::Item;
use crate::{base_items, class_of_more, package_with};

fn item_class(name: &str) -> &'static str {
    class_of_more(vec![Item::stored(name, b"x")])
}

fn item_class_bytes(name: &[u8]) -> &'static str {
    class_of_more(vec![Item::stored("x", b"x").named_bytes(name)])
}

#[test]
fn names_that_leave_the_package_are_path_traversal() {
    // `../evil.xml`, `/etc/x`, `a\b.xml`; and the drive letter R22 names.
    for name in [
        "../evil.xml",
        "/etc/x",
        "a\\b.xml",
        "C:evil.xml",
        "c:/windows/x.xml",
        "..",
        "visio/..",
        "visio/../x.xml",
        "a/b/../../../x.xml",
        "\\evil.xml",
        "visio\\pages\\page1.xml",
        "//server/share/x.xml",
        "../",
    ] {
        assert_eq!(item_class(name), "path_traversal", "item name {name:?}");
    }
}

#[test]
fn a_name_smuggled_through_percent_encoding_is_judged_after_it_is_decoded() {
    for name in ["%2e%2e/evil.xml", "%2E%2E/evil.xml", "visio/%2e%2e/x.xml", "a%2Fb.xml", "a%2fb.xml", "a%5Cb.xml", "a%5cb.xml", "%2Fetc/x"] {
        assert_eq!(item_class(name), "path_traversal", "item name {name:?}");
    }
}

#[test]
fn a_percent_escape_of_an_unreserved_character_is_refused() {
    // A part name "must not percent-encode ... any unreserved character": a letter, a digit, `-`, `.`, `_`, `~`. (Review finding 4:
    // `%41.xml` read as `/A.xml`, and a test pinned `a%2eb.xml` as `/a.b.xml`.) The class is `invalid_part_name`, and a traversal
    // spelled this way (`%2e%2e/`, above) is still `path_traversal`, because that is judged first.
    for name in
        ["%41.xml", "a%2eb.xml", "a%2Eb.xml", "a%7Eb.xml", "a%5Fb.xml", "a%2Db.xml", "a%30b.xml", "visio/%64ocument.xml", "a%61.xml", "a%20b%41.xml"]
    {
        assert_eq!(item_class(name), "invalid_part_name", "item name {name:?}");
    }
    // An escape of something that is not unreserved is how R4 writes a space or a letter outside ASCII, and reads.
    let p = crate::read_items(&[
        base_items().remove(0),
        Item::stored("a%20b.xml", b"1"),
        Item::stored("caf%C3%A9.xml", b"2"),
        Item::stored("a%23b.xml", b"3"),
    ])
    .unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/a b.xml", "/café.xml", "/a#b.xml"]);
}

#[test]
fn a_name_that_only_decodes_to_the_content_types_stream_is_refused() {
    // Review finding 5: `%5BContent_Types%5D.xml` read as a part named like the stream, and `part("[Content_Types].xml")` returned its
    // bytes. Under R4 a writer would put two items of one name in the file. Refused after decoding, in any ASCII case.
    for name in ["%5BContent_Types%5D.xml", "%5bcontent_types%5d.XML", "%5BContent_Types]%2Exml"] {
        let class = class_of_more(vec![Item::stored(name, b"<Types/>")]);
        assert!(class == "invalid_part_name", "{name}: {class}");
    }
    // Also when it is the stream's own name written in another case that is the real stream: that one is the stream, the other is not.
    let class = class_of_more(vec![Item::stored("%5BContent_Types%5D.xml", b"x")]);
    assert_eq!(class, "invalid_part_name");
    let p = crate::read_items(&base_items()).unwrap();
    assert_eq!(p.part("[Content_Types].xml"), None);
    assert_eq!(p.part("/[Content_Types].xml"), None);
}

#[test]
fn names_that_are_not_legal_part_names_are_refused_with_their_own_class() {
    // Non-empty segments, none ending in a dot; and no control characters.
    for name in [
        "",
        "a//b.xml",
        "a/./b.xml",
        "./a.xml",
        "visio/doc.",
        "visio./doc.xml",
        "a\u{1}b.xml",
        "a\tb.xml",
        "a\nb.xml",
        "a\0b.xml",
        "a\u{7f}b.xml",
        "a%00b.xml",
        "a%0Ab.xml",
    ] {
        assert_eq!(item_class(name), "invalid_part_name", "item name {name:?}");
    }
}

#[test]
fn percent_escapes_that_do_not_make_utf_8_are_refused_and_a_lone_percent_sign_is_kept() {
    assert_eq!(item_class("a%FFb.xml"), "invalid_part_name");
    assert_eq!(item_class("a%C3b.xml"), "invalid_part_name");
    // A `%` that does not start an escape is only a character: the reader does not guess.
    let p =
        crate::read_items(&[base_items().remove(0), Item::stored("100%.xml", b"1"), Item::stored("a%zzb.xml", b"2"), Item::stored("c%4.xml", b"3")])
            .unwrap();
    assert_eq!(p.parts().collect::<Vec<_>>(), ["/100%.xml", "/a%zzb.xml", "/c%4.xml"]);
}

#[test]
fn a_name_longer_than_the_limit_is_refused_and_one_at_the_limit_is_not() {
    let limits = Limits { max_part_name_bytes: 20, ..Limits::default() };
    let at = format!("{}.xml", "a".repeat(16)); // 20 bytes
    let over = format!("{}.xml", "a".repeat(17)); // 21 bytes
    let ok = Package::read(&package_with(vec![Item::stored(&at, b"1")]), &limits).unwrap();
    assert_eq!(ok.parts().count(), 1);
    assert_eq!(crate::class_of(&package_with(vec![Item::stored(&over, b"1")]), &limits), "invalid_part_name");
    // The length that counts is the decoded name: `é` is two bytes, six spelled out as an escape.
    let p = Package::read(&package_with(vec![Item::stored(&format!("{}%C3%A9.xml", "a".repeat(14)), b"1")]), &limits).unwrap();
    assert_eq!(p.parts().count(), 1);
    let over_by_one = Item::stored(&format!("{}%C3%A9.xml", "a".repeat(15)), b"1");
    assert_eq!(crate::class_of(&package_with(vec![over_by_one]), &limits), "invalid_part_name");
}

#[test]
fn bytes_the_zip_format_reads_as_code_page_437_never_make_a_separator_or_a_dot() {
    // Every byte from 0x80 up is a letter or a symbol in CP437, none is `/`, `\` or `.`: so what the checks see is the decoded
    // name, and a name of high bytes is only a name.
    let high: Vec<u8> = (0x80..=0xFFu8).collect();
    let p = crate::read_items(&[base_items().remove(0), Item::stored("x", b"1").named_bytes(&high)]).unwrap();
    let name = p.parts().next().unwrap();
    assert!(name.starts_with('/') && !name[1..].contains(['/', '\\', '.']), "{name:?}");
    // And a raw name that is a traversal is judged as written.
    assert_eq!(item_class_bytes(b"\x82/../x"), "path_traversal");
    assert_eq!(item_class_bytes(b"a/..\\\x82"), "path_traversal");
}

#[test]
fn a_leading_slash_or_drive_letter_is_refused_for_an_item_name_but_a_relative_name_with_a_colon_is_not() {
    assert_eq!(item_class("/visio/document.xml"), "path_traversal");
    let p = crate::read_items(&[base_items().remove(0), Item::stored("visio/a:b.xml", b"1"), Item::stored("1:x/y.xml", b"2")]).unwrap();
    assert_eq!(p.parts().count(), 2, "only a letter and a colon at the start is a drive");
}

#[test]
fn a_relative_target_resolves_against_the_source_parts_folder() {
    // Source `/visio/pages/page1.xml` and `Target="../masters/master1.xml"` is `/visio/masters/master1.xml`.
    assert_eq!(resolve("/visio/pages/page1.xml", "../masters/master1.xml").as_deref(), Some("/visio/masters/master1.xml"));
    // And an absolute target is that exact part.
    assert_eq!(resolve("/visio/pages/page1.xml", "/visio/media/image1.png").as_deref(), Some("/visio/media/image1.png"));
}

#[test]
fn a_target_resolves_by_the_usual_rule_for_dots() {
    let src = "/visio/pages/page1.xml";
    assert_eq!(resolve(src, "page2.xml").as_deref(), Some("/visio/pages/page2.xml"));
    assert_eq!(resolve(src, "./page2.xml").as_deref(), Some("/visio/pages/page2.xml"));
    assert_eq!(resolve(src, "sub/./deeper/../page2.xml").as_deref(), Some("/visio/pages/sub/page2.xml"));
    assert_eq!(resolve(src, "../../docProps/core.xml").as_deref(), Some("/docProps/core.xml"));
    // More `..` than there are folders stays at the root, as RFC 3986 section 5.2.4 says; the name is a dangling one and nothing
    // outside the package is named.
    assert_eq!(resolve(src, "../../../../../etc/passwd").as_deref(), Some("/etc/passwd"));
    // From the package itself, and from a part in the root.
    assert_eq!(resolve("/", "visio/document.xml").as_deref(), Some("/visio/document.xml"));
    assert_eq!(resolve("", "visio/document.xml").as_deref(), Some("/visio/document.xml"));
    assert_eq!(resolve("/document.xml", "visio/pages.xml").as_deref(), Some("/visio/pages.xml"));
}

#[test]
fn a_target_is_decoded_and_loses_its_fragment() {
    let src = "/visio/pages/page1.xml";
    assert_eq!(resolve(src, "../media/image%201.png").as_deref(), Some("/visio/media/image 1.png"));
    assert_eq!(resolve(src, "../media/caf%C3%A9.png").as_deref(), Some("/visio/media/café.png"));
    assert_eq!(resolve(src, "../media/a.png#frag").as_deref(), Some("/visio/media/a.png"));
    assert_eq!(resolve(src, "../media/a%23b.png").as_deref(), Some("/visio/media/a#b.png"));
    assert_eq!(resolve(src, "%2e%2e/masters/m.xml").as_deref(), Some("/visio/masters/m.xml"), "an escaped dot is a dot");
}

#[test]
fn a_target_that_cannot_name_a_part_resolves_to_nothing() {
    let src = "/visio/pages/page1.xml";
    for target in
        ["", ".", "..", "../", "folder/", "#only-a-fragment", "a%2Fb.xml", "a%5Cb.xml", "a%00b.xml", "a\u{1}b.xml", "a%FFb.xml", "foo.", "dir./x.xml"]
    {
        assert_eq!(resolve(src, target), None, "target {target:?}");
    }
    assert_eq!(resolve(src, &format!("{}x", "a/".repeat(2000))), None, "a name past the length limit");
    assert!(resolve(src, &format!("{}x", "a/".repeat(200))).is_some(), "and the same shape under it is fine");
}

#[test]
fn a_source_with_a_control_character_or_a_backslash_in_its_folder_gives_no_target() {
    // Found by the fuzz test (`a_target_resolves_to_a_part_name_or_to_nothing`): the source is the caller's, and its folders go into the
    // result, so the result is judged whole. `resolve("\0/\\/", "a")` was `/\0/a`.
    assert_eq!(resolve("\0/\\/", "a"), None);
    assert_eq!(resolve("/a\u{0}b/c.xml", "d.xml"), None);
    assert_eq!(resolve("/a\u{7f}b/c.xml", "d.xml"), None);
    assert_eq!(resolve("/a\\b/c.xml", "d.xml"), None);
    assert_eq!(resolve("/a./c.xml", "d.xml"), None, "a folder that ends in a dot is not one a part can be in");
    assert_eq!(resolve("/ab/c.xml", "d.xml").as_deref(), Some("/ab/d.xml"), "and an ordinary one is");
}

#[test]
fn a_backslash_in_a_target_is_read_as_a_slash() {
    // Labelled choice: some writers have used the wrong slash in targets. A target is only a lookup key, and no name from a
    // package reaches a file system, so reading the intent costs nothing; an item *name* with a backslash is refused instead.
    assert_eq!(resolve("/visio/pages/page1.xml", "..\\media\\image1.png").as_deref(), Some("/visio/media/image1.png"));
    assert_eq!(resolve("/visio/pages/page1.xml", "\\visio\\media\\image1.png").as_deref(), Some("/visio/media/image1.png"));
}

#[test]
fn the_relationships_part_of_a_part_sits_beside_it() {
    // R10.
    assert_eq!(rels_part_name("/visio/document.xml"), "/visio/_rels/document.xml.rels");
    assert_eq!(rels_part_name("visio/document.xml"), "/visio/_rels/document.xml.rels");
    assert_eq!(rels_part_name("/visio/pages/page1.xml"), "/visio/pages/_rels/page1.xml.rels");
    assert_eq!(rels_part_name("/document.xml"), "/_rels/document.xml.rels");
    assert_eq!(rels_part_name("/visio/vbaProject.bin"), "/visio/_rels/vbaProject.bin.rels");
    // The package itself.
    assert_eq!(rels_part_name("/"), "/_rels/.rels");
    assert_eq!(rels_part_name(""), "/_rels/.rels");
}

#[test]
fn folding_is_ascii_only_and_ignores_the_leading_slash() {
    assert_eq!(fold("/Visio/Document.XML"), fold("visio/document.xml"));
    assert_ne!(fold("/café"), fold("/CAFÉ"));
    assert_eq!(fold("/ÀB"), "/Àb");
}
