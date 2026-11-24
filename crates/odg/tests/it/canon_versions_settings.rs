//! In the `.odg` content check: **`office:version` attributes count as nothing, and an empty `settings.xml` or `meta.xml` counts
//! as absent.** Every vector is a package (or a flat document) written here by hand from the ODF standard, with a control beside each rule that it still catches a difference.
use diacraft_odg::{Limits, compare_content};

use crate::build::{DRAWING, NS, content, package, page, shape};

const OFFICE: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#;

fn styles(version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"{version}\"><office:styles/></office:document-styles>\n"
    )
}

fn content_v(version: &str) -> String {
    content(&page("p1", &shape("s1", "1cm"))).replace("office:version=\"1.3\"", &format!("office:version=\"{version}\""))
}

fn meta_v(version: &str, inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {OFFICE} xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"{version}\">{inner}</office:document-meta>\n"
    )
}

fn settings_v(version: &str, inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {OFFICE} xmlns:config=\"urn:oasis:names:tc:opendocument:xmlns:config:1.0\" office:version=\"{version}\">{inner}</office:document-settings>\n"
    )
}

/// A package of the four parts at `version`, `meta.xml` and `settings.xml` holding `meta` and `settings` (`None`: no such part).
fn drawing(version: &str, meta: Option<&str>, settings: Option<&str>) -> Vec<u8> {
    let mut files = vec![("content.xml".to_owned(), content_v(version).into_bytes()), ("styles.xml".to_owned(), styles(version).into_bytes())];
    if let Some(inner) = meta {
        files.push(("meta.xml".to_owned(), meta_v(version, inner).into_bytes()));
    }
    if let Some(inner) = settings {
        files.push(("settings.xml".to_owned(), settings_v(version, inner).into_bytes()));
    }
    package(DRAWING, files)
}

fn same(a: &[u8], b: &[u8]) -> bool {
    let ab = compare_content(a, b, &Limits::default()).unwrap();
    let ba = compare_content(b, a, &Limits::default()).unwrap();
    assert_eq!(ab.count, ba.count, "the count is symmetric");
    ab.same()
}

const GENERATOR: &str = "<office:meta><meta:generator>TestWriter/1</meta:generator></office:meta>";
/// A `meta.xml` that says something: a title.
const TITLE: &str = "<office:meta><dc:title>A title</dc:title></office:meta>";
const ITEMS: &str = r#"<office:settings><config:config-item-set config:name="ooo:view-settings"><config:config-item config:name="VisibleAreaTop" config:type="int">0</config:config-item></config:config-item-set></office:settings>"#;

/// V48, the first case: two packages that differ only in `office:version` on every part's root (1.2 and 1.4) are the same content. The control: a change beside the version is still
/// one difference, and only that one.
#[test]
fn office_version_counts_as_nothing_on_every_part() {
    let a = drawing("1.2", Some(GENERATOR), Some(ITEMS));
    let b = drawing("1.4", Some(GENERATOR), Some(ITEMS));
    assert!(same(&a, &b), "{:?}", compare_content(&a, &b, &Limits::default()).unwrap().shown);
    // A root that says no version at all is the same as one that says 1.4.
    let none = package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), content_v("1.4").replace(" office:version=\"1.4\"", "").into_bytes()),
            ("styles.xml".to_owned(), styles("1.4").replace(" office:version=\"1.4\"", "").into_bytes()),
            ("meta.xml".to_owned(), meta_v("1.4", GENERATOR).replace(" office:version=\"1.4\"", "").into_bytes()),
            ("settings.xml".to_owned(), settings_v("1.4", ITEMS).replace(" office:version=\"1.4\"", "").into_bytes()),
        ],
    );
    assert!(same(&none, &b));
    // The control: a salt beside the version is a difference, one.
    let salted = drawing("1.2", Some(GENERATOR), Some(&ITEMS.replace(">0<", ">7<")));
    let diff = compare_content(&salted, &b, &Limits::default()).unwrap();
    assert_eq!(diff.count, 1, "{:?}", diff.shown);
    assert_eq!(diff.shown[0].part, "settings.xml");
}

/// The attribute is ODF's `version` in the office namespace, by its namespace: the same local name in another namespace is compared as any attribute. `manifest:version` counts as
/// nothing too (settled): `canon_free_names.rs` holds that.
#[test]
fn only_the_office_namespace_s_version_counts_as_nothing() {
    let other = |v: &str| {
        package(
            DRAWING,
            vec![
                (
                    "content.xml".to_owned(),
                    content_v("1.4").replace("<office:body>", &format!("<office:body xmlns:x=\"urn:x\" x:version=\"{v}\">")).into_bytes(),
                ),
                ("styles.xml".to_owned(), styles("1.4").into_bytes()),
            ],
        )
    };
    assert!(!same(&other("1"), &other("2")), "a version of another namespace is content");
    // The manifest's own version counts as nothing, on the root and on the root row, and a version in the manifest's namespace under any other name does not.
    let a = drawing("1.4", Some(TITLE), None);
    let manifest = diacraft_odg::package::Package::read(&a, &Limits::default()).unwrap();
    let text = String::from_utf8(manifest.part("META-INF/manifest.xml").unwrap().to_vec()).unwrap();
    let files: Vec<(String, Vec<u8>)> = manifest
        .entries()
        .filter(|e| !e.name.ends_with('/') && e.name != "mimetype" && e.name != "META-INF/manifest.xml")
        .map(|e| (e.name.clone(), manifest.part(&e.name).unwrap().to_vec()))
        .collect();
    let with_manifest = |text: String| {
        let mut files = files.clone();
        files.push(("META-INF/manifest.xml".to_owned(), text.into_bytes()));
        crate::build::write(Some(DRAWING), &files)
    };
    let older = with_manifest(text.replace("manifest:version=\"1.3\"", "manifest:version=\"1.2\""));
    assert!(same(&a, &older), "{:?}", compare_content(&a, &older, &Limits::default()).unwrap().shown);
    let noted = with_manifest(text.replace("manifest:version=\"1.3\"", "manifest:note=\"1.3\""));
    let diff = compare_content(&a, &noted, &Limits::default()).unwrap();
    assert!(!diff.same() && diff.shown.iter().all(|d| d.part == "META-INF/manifest.xml"), "{:?}", diff.shown);
}

/// V48, the second case: a `settings.xml` that holds an empty `office:document-settings` root, or one whose `office:settings` holds nothing, is the same as no `settings.xml`, and the
/// two forms are the same as each other. The control: a `settings.xml` that holds a setting is a part.
#[test]
fn an_empty_settings_part_counts_as_absent() {
    let none = drawing("1.4", Some(GENERATOR), None);
    let bare = drawing("1.4", Some(GENERATOR), Some(""));
    let empty_child = drawing("1.2", Some(GENERATOR), Some("<office:settings/>"));
    let spaced = drawing("1.2", Some(GENERATOR), Some("\n <office:settings>\n </office:settings>\n"));
    for (what, b) in [("an empty root", &bare), ("an empty office:settings", &empty_child), ("an empty office:settings with spacing", &spaced)] {
        assert!(same(&none, b), "{what}: {:?}", compare_content(&none, b, &Limits::default()).unwrap().shown);
    }
    assert!(same(&bare, &empty_child), "the two empty forms are one");
    // The controls: a setting is content, and so is anything else in the part.
    let full = drawing("1.4", Some(GENERATOR), Some(ITEMS));
    let diff = compare_content(&none, &full, &Limits::default()).unwrap();
    assert!(!diff.same() && diff.shown.iter().any(|d| d.part == "settings.xml"), "{:?}", diff.shown);
    assert!(!same(&empty_child, &full));
    let attributed = drawing("1.4", Some(GENERATOR), Some(r#"<office:settings xmlns:x="urn:x" x:a="1"/>"#));
    assert!(!same(&none, &attributed), "an office:settings with an attribute holds something");
    let other_child = drawing("1.4", Some(GENERATOR), Some("<office:settings/><office:other/>"));
    assert!(!same(&none, &other_child), "another child of the root is content");
}

/// V48, the third case: the same for `meta.xml`, and a `meta.xml` holding a `dc:title` is a difference.
#[test]
fn an_empty_meta_part_counts_as_absent_and_a_title_does_not() {
    let none = drawing("1.4", None, None);
    for (what, inner) in
        [("an empty root", ""), ("an empty office:meta", "<office:meta/>"), ("an office:meta of spaces", "<office:meta>  </office:meta>")]
    {
        let b = drawing("1.3", Some(inner), None);
        assert!(same(&none, &b), "{what}: {:?}", compare_content(&none, &b, &Limits::default()).unwrap().shown);
    }
    let titled = drawing("1.4", Some("<office:meta><dc:title>A title</dc:title></office:meta>"), None);
    let diff = compare_content(&none, &titled, &Limits::default()).unwrap();
    assert!(!diff.same(), "a title is content");
    assert!(diff.shown.iter().any(|d| d.part == "meta.xml"), "{:?}", diff.shown);
    // A generator counts as nothing so a part that names one and nothing else is an empty part too (`canon_free_names.rs` holds the rule).
    assert!(same(&none, &drawing("1.4", Some(GENERATOR), None)));
    assert!(
        !same(&none, &drawing("1.4", Some(&GENERATOR.replace("</office:meta>", "<dc:title>T</dc:title></office:meta>")), None)),
        "a title beside it is content"
    );
}

/// The ruling holds where the check reads a flat document: a flat drawing that says another version, and one with an empty `office:settings`, are the same as the package that has
/// no `settings.xml`. The control: a flat drawing with a setting differs.
#[test]
fn the_ruling_holds_against_a_flat_document() {
    let flat = |version: &str, settings: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} xmlns:config=\"urn:oasis:names:tc:opendocument:xmlns:config:1.0\" office:mimetype=\"{DRAWING}\" office:version=\"{version}\">{settings}<office:styles/><office:body><office:drawing>{}</office:drawing></office:body></office:document>\n",
            page("p1", &shape("s1", "1cm"))
        )
        .into_bytes()
    };
    let package =
        package(DRAWING, vec![("content.xml".to_owned(), content_v("1.4").into_bytes()), ("styles.xml".to_owned(), styles("1.4").into_bytes())]);
    for (what, f) in [("another version", flat("1.2", "")), ("an empty office:settings", flat("1.4", "<office:settings/>"))] {
        let diff = compare_content(&f, &package, &Limits::default()).unwrap();
        assert!(diff.same(), "{what}: {:?}", diff.shown);
    }
    let diff = compare_content(&flat("1.4", ITEMS), &package, &Limits::default()).unwrap();
    assert!(!diff.same(), "a setting is content in a flat document too");
}
