//! The rule of empty containers in the content check (`docs/diacraft-format.md` 4.7, and the rows O9, O9d and O9e of 8.5): an empty
//! `office:meta`, `office:scripts`, `office:font-face-decls`, `office:styles`, `office:automatic-styles` or `office:master-styles` is the same as an absent
//! one, and nothing else is, in a flat file, in a package and in a package against a flat file. Every file is built in code from the standard (`drawing.rs`).
//!
//! A container is *empty* when it has no attribute, no child element, no processing instruction and no text but white space. The controls are the
//! containers that hold one thing each, and the elements that are not on the list, which must still differ from an absent one.

use diacraft_container::canon::Found;
use diacraft_odg::{CANON_RULES, Limits, Work, compare_content, compare_content_counted};

use crate::build::{DRAWING, package};
use crate::drawing::NS;
use crate::flat::MIME;

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

/// Where a package holds each of the six: the part whose root has it as a child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Home {
    Content,
    Styles,
    Meta,
}

/// The six, each with the parts of a package that may hold it: `office:font-face-decls` and `office:automatic-styles` in two.
const CASES: [(&str, Home); 8] = [
    ("meta", Home::Meta),
    ("scripts", Home::Content),
    ("font-face-decls", Home::Content),
    ("font-face-decls", Home::Styles),
    ("styles", Home::Styles),
    ("automatic-styles", Home::Content),
    ("automatic-styles", Home::Styles),
    ("master-styles", Home::Styles),
];

/// The two forms a drawing is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Flat,
    Package,
}

const FORMS: [Form; 2] = [Form::Flat, Form::Package];

/// A flat file whose root holds exactly `inner`, as the rows of 8.5 state them (the root says the media type a drawing's does, so that a package that says
/// it too is compared with it as one document).
fn root(inner: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:version=\"1.3\" office:mimetype=\"{MIME}\">{inner}</office:document>")
        .into_bytes()
}

/// A drawing's body with one page, so that a file is a drawing and what is compared is only what stands beside it.
const BODY: &str = r#"<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body>"#;

/// `element` (the whole text of one of the six, or nothing) in the form `form` of a drawing: a flat file holds it as a child of its root, a package in the
/// root of the part that `home` says, the other parts being there without it.
fn drawing(form: Form, home: Home, element: &str) -> Vec<u8> {
    match form {
        Form::Flat => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:version=\"1.3\" office:mimetype=\"{MIME}\">{element}{BODY}</office:document>"
        )
        .into_bytes(),
        Form::Package => {
            let part = |name: &str, root: &str, inner: &str| {
                (name.to_owned(), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:{root} {NS} office:version=\"1.3\">{inner}</office:{root}>").into_bytes())
            };
            let held = |h: Home| if h == home { element } else { "" };
            package(
                DRAWING,
                vec![
                    part("content.xml", "document-content", &format!("{}{BODY}", held(Home::Content))),
                    part("styles.xml", "document-styles", held(Home::Styles)),
                    part("meta.xml", "document-meta", held(Home::Meta)),
                    part("settings.xml", "document-settings", ""),
                ],
            )
        }
    }
}

/// The comparison, in both directions, and nothing else said about it: the count of both is the answer, or the test says which direction differs.
#[track_caller]
fn counts(what: &str, a: &[u8], b: &[u8]) -> (u64, u64, Vec<diacraft_container::canon::Difference>) {
    let ab = compare_content(a, b, &Limits::default()).unwrap_or_else(|e| panic!("{what}: {e}"));
    let ba = compare_content(b, a, &Limits::default()).unwrap_or_else(|e| panic!("{what}: {e}"));
    (ab.count, ba.count, ab.shown)
}

#[track_caller]
fn same(what: &str, a: &[u8], b: &[u8]) {
    let (ab, ba, shown) = counts(what, a, b);
    assert_eq!((ab, ba), (0, 0), "{what}: {shown:?}");
}

/// Exactly one difference, an element on one side only, whichever side is first.
#[track_caller]
fn one_element(what: &str, a: &[u8], b: &[u8]) {
    let (ab, ba, shown) = counts(what, a, b);
    assert_eq!((ab, ba), (1, 1), "{what}: {shown:?}");
    assert_eq!(shown.first().map(|d| d.found), Some(Found::Element), "{what}: {shown:?}");
}

/// [`one_element`], but for two packages where the side without the container holds a `meta.xml` with nothing in it: by that part is no part, so the difference is the
/// other side's `meta.xml` (one: its manifest row says what the part says), and never a false same.
#[track_caller]
fn one_element_or_meta(what: &str, a: &[u8], b: &[u8], (fa, fb, home): (Form, Form, Home)) {
    if (fa, fb, home) != (Form::Package, Form::Package, Home::Meta) {
        return one_element(what, a, b);
    }
    let (ab, ba, shown) = counts(what, a, b);
    assert_eq!((ab, ba), (1, 1), "{what}: {shown:?}");
    assert!(shown.iter().any(|d| d.part == "meta.xml" && matches!(d.found, Found::MissingPart | Found::ExtraPart)), "{what}: {shown:?}");
}

fn empty(local: &str) -> String {
    format!("<office:{local}/>")
}

fn holding(local: &str, inner: &str) -> String {
    format!("<office:{local}>{inner}</office:{local}>")
}

/// Every pairing of the forms, so that the container is held by the file that is compared first and by the one that is compared second, in each form.
fn every_pair(mut check: impl FnMut(Form, Form)) {
    for a in FORMS {
        for b in FORMS {
            check(a, b);
        }
    }
}

// --------------------------------------------------------------------------------------------------------------- the rows of 8.5, as stated

#[test]
fn row_o9_an_empty_styles_container_is_an_absent_one() {
    let (a, b) = (root("<office:styles/><office:body/>"), root("<office:body/>"));
    same("O9 (the empty container first)", &a, &b);
    same("O9 (the absent one first)", &b, &a);
}

#[test]
fn row_o9b_a_page_is_content() {
    let a = root(r#"<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body>"#);
    let b = root("<office:body><office:drawing/></office:body>");
    one_element("O9b", &a, &b);
}

#[test]
fn row_o9c_an_empty_paragraph_is_content() {
    one_element("O9c", &root("<text:p/>"), &root(""));
}

#[test]
fn row_o9d_the_five_empty_elements_are_the_same_as_nothing() {
    let five = "<office:automatic-styles/><office:font-face-decls/><office:master-styles/><office:scripts/><office:meta/>";
    same("O9d (the five first)", &root(five), &root(""));
    same("O9d (nothing first)", &root(""), &root(five));
    // The same five as each of the parts of a package holds them, against nothing and against the flat file that holds them.
    let part = |name: &str, root_name: &str, inner: &str| {
        (name.to_owned(), format!("<office:{root_name} {NS} office:version=\"1.3\">{inner}</office:{root_name}>").into_bytes())
    };
    let parts = |content: &str, styles: &str, meta: &str| {
        package(
            DRAWING,
            vec![
                part("content.xml", "document-content", content),
                part("styles.xml", "document-styles", styles),
                part("meta.xml", "document-meta", meta),
            ],
        )
    };
    let with =
        parts("<office:scripts/><office:font-face-decls/><office:automatic-styles/>", "<office:master-styles/><office:styles/>", "<office:meta/>");
    let without = parts("", "", "");
    same("O9d, two packages", &with, &without);
    same("O9d, a package against nothing", &with, &root(""));
    same("O9d, nothing against a package", &root(""), &with);
    same("O9d, the five against a package without them", &root(five), &without);
    same("O9d, a package without them against the five", &without, &root(five));
}

/// Row O9e: an empty `office:settings` under the root is the same as none, as an empty `settings.xml` is no part (had it an
/// element, for ODF wants one set of items in it). The control: one that holds a set of items is an element.
#[test]
fn an_empty_settings_counts_as_absent_by_the_rule() {
    same("O9e", &root("<office:settings/>"), &root(""));
    one_element("O9e, a setting", &root(&format!("<office:settings>{ITEM_SET}</office:settings>")), &root(""));
}

/// A set of items, the least `office:settings` holds by the schema.
const ITEM_SET: &str = r#"<config:config-item-set xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" config:name="s"/>"#;

// ----------------------------------------------------------------------------------------------------------- each of the six, in each form

#[test]
fn each_of_the_six_empty_is_the_same_as_absent_in_each_form_and_in_both_orders() {
    for (local, home) in CASES {
        every_pair(|fa, fb| {
            let (with, without) = (drawing(fa, home, &empty(local)), drawing(fb, home, ""));
            let what = format!("{local} in {home:?}: {fa:?} with it, {fb:?} without");
            same(&what, &with, &without);
            // The empty container is also the second file, and the first file's.
            let (without, with) = (drawing(fa, home, ""), drawing(fb, home, &empty(local)));
            same(&format!("{local} in {home:?}: {fa:?} without it, {fb:?} with"), &without, &with);
        });
    }
}

#[test]
fn an_empty_container_that_is_written_open_and_shut_is_empty_too() {
    let spellings = [
        "<office:@></office:@>",
        "<office:@> </office:@>",
        "<office:@>\r\n\t  \n</office:@>",
        "<office:@><!-- nothing --></office:@>",
        "<office:@> <!-- c --> </office:@>",
        "<office:@><![CDATA[]]></office:@>",
        "<office:@><![CDATA[ \n ]]></office:@>",
        "<office:@>&#32;&#10;</office:@>",
        r#"<office:@ xmlns:other="urn:x"/>"#,
        r#"<office:@ xmlns="urn:x" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#,
        // Another prefix for the office namespace, and the office namespace as the default one: a name is its namespace and its local part.
        r#"<o:@ xmlns:o="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#,
        r#"<@ xmlns="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#,
    ];
    for (local, home) in CASES {
        for spelling in spellings {
            let element = spelling.replace('@', local);
            every_pair(|fa, fb| {
                let (with, without) = (drawing(fa, home, &element), drawing(fb, home, ""));
                same(&format!("{element} in {home:?}: {fa:?} with it, {fb:?} without"), &with, &without);
                same(&format!("{element} in {home:?}: {fa:?} without it, {fb:?} with"), &without, &with);
            });
        }
    }
}

#[test]
fn an_empty_container_is_the_same_as_another_empty_one_and_as_itself() {
    // Written two ways: both are the same as an absent one, so as each other.
    for (local, home) in CASES {
        let (open_shut, short) = (holding(local, ""), empty(local));
        every_pair(|fa, fb| {
            same(&format!("{local} in {home:?}: {fa:?} open and shut, {fb:?} short"), &drawing(fa, home, &open_shut), &drawing(fb, home, &short));
            same(&format!("{local} in {home:?}: {fa:?} short against itself"), &drawing(fa, home, &short), &drawing(fb, home, &short));
        });
    }
}

// --------------------------------------------------------------------------------------------------- the controls: what is not empty differs

/// The ways a container is not empty: one attribute, one child, an instruction, text that is not blank, each in the spellings nearest to blank. The first
/// of each says whether the thing is an attribute of the start tag, and the second is the markup.
const HOLDING: [(&str, bool, &str); 11] = [
    ("an attribute", true, r#" foo="1""#),
    ("an attribute with an empty value", true, r#" foo="""#),
    ("xml:space", true, r#" xml:space="preserve""#),
    ("a child", false, "<x:y xmlns:x=\"urn:x\"/>"),
    ("a child that is empty", false, "<office:annotation/>"),
    ("an instruction", false, "<?go now?>"),
    ("an instruction with no data", false, "<?go?>"),
    ("text", false, "x"),
    ("text between white space", false, "  x\n"),
    ("a no-break space", false, "&#160;"),
    ("text in a CDATA section", false, "<![CDATA[x]]>"),
];

/// The container `local` holding what `(is_attribute, markup)` says.
fn holds(local: &str, is_attribute: bool, markup: &str) -> String {
    if is_attribute { format!("<office:{local}{markup}/>") } else { holding(local, markup) }
}

#[test]
fn a_container_that_holds_anything_still_differs_from_an_absent_one_in_each_form() {
    for (local, home) in CASES {
        for (what, is_attribute, markup) in HOLDING {
            let element = holds(local, is_attribute, markup);
            every_pair(|fa, fb| {
                let (with, without) = (drawing(fa, home, &element), drawing(fb, home, ""));
                one_element_or_meta(&format!("{local} in {home:?} holding {what}: {fa:?} with it, {fb:?} without"), &with, &without, (fa, fb, home));
                one_element_or_meta(&format!("{local} in {home:?} holding {what}: {fa:?} without it, {fb:?} with"), &without, &with, (fb, fa, home));
            });
        }
    }
}

#[test]
fn a_container_that_holds_something_is_not_an_empty_one_and_the_difference_is_the_one_container() {
    // An empty one against one that holds a thing, in each form: the empty one is as good as absent, so the other is one element on one side only.
    for (local, home) in CASES {
        for (what, is_attribute, markup) in HOLDING {
            let element = holds(local, is_attribute, markup);
            every_pair(|fa, fb| {
                let (with, empty) = (drawing(fa, home, &element), drawing(fb, home, &empty(local)));
                one_element_or_meta(
                    &format!("{local} in {home:?} holding {what}: {fa:?} against an empty one in {fb:?}"),
                    &with,
                    &empty,
                    (fa, fb, home),
                );
                one_element_or_meta(&format!("{local} in {home:?} holding {what}: {fb:?} empty against {fa:?}"), &empty, &with, (fb, fa, home));
            });
        }
    }
}

#[test]
fn what_a_container_holds_is_still_compared_beside_the_rule() {
    // Two containers that hold different things are two things, and one that is the same is the same, in each form: the rule leaves the rest alone.
    for (local, home) in CASES {
        every_pair(|fa, fb| {
            let (a, b) = (
                drawing(fa, home, &holding(local, "<x:y xmlns:x=\"urn:x\" k=\"1\"/>")),
                drawing(fb, home, &holding(local, "<x:y xmlns:x=\"urn:x\" k=\"2\"/>")),
            );
            let (ab, ba, shown) = counts(&format!("{local} in {home:?}: {fa:?} against {fb:?}"), &a, &b);
            assert_eq!((ab, ba), (1, 1), "{local} in {home:?}: {shown:?}");
            assert_eq!(shown.first().map(|d| d.found), Some(Found::Value), "{shown:?}");
            let again = drawing(fb, home, &holding(local, "<x:y xmlns:x=\"urn:x\" k=\"1\"/>"));
            same(&format!("{local} in {home:?}: {fa:?} against {fb:?}, the same thing"), &a, &again);
        });
    }
}

#[test]
fn the_rule_hides_no_difference_that_stands_beside_an_empty_container() {
    // A page that differs is one difference, whether or not an empty container stands on either side.
    let page = |name: &str| format!(r#"<office:body><office:drawing><draw:page draw:name="{name}"/></office:drawing></office:body>"#);
    let a = root(&format!("<office:styles/><office:scripts/>{}", page("p")));
    let b = root(&page("q"));
    let (ab, ba, shown) = counts("a page named another way beside two empty containers", &a, &b);
    assert_eq!((ab, ba), (1, 1), "{shown:?}");
    assert_eq!(shown.first().map(|d| d.found), Some(Found::Value));
    // Two empty containers on a side and one holding a thing on the other: one difference, the thing's.
    one_element("an empty one and a holding one", &root("<office:styles/><office:master-styles>x</office:master-styles>"), &root(""));
    // The same thing in two containers is the same.
    same("an empty one beside the same page", &root(&format!("<office:styles/>{}", page("p"))), &root(&page("p")));
}

// --------------------------------------------------------------------------------------------------------------- nothing else is treated so

#[test]
fn an_element_that_is_not_one_of_the_six_is_compared_as_it_is() {
    let nothing_else = [
        // (`office:settings` under the root was one of these until a later change, which makes an empty one none there: `row_o9e_...`.)
        "<office:body/>",
        "<office:annotation/>",
        "<office:script/>",
        "<office:event-listeners/>",
        "<text:p/>",
        "<text:h/>",
        "<draw:g/>",
        "<draw:frame/>",
        "<draw:page/>",
        "<style:style/>",
        "<style:default-style/>",
        "<style:page-layout/>",
        "<style:master-page/>",
        "<draw:layer-set/>",
        "<draw:marker/>",
        // The name of one of the six in another namespace is not that element.
        "<x:styles xmlns:x=\"urn:x\"/>",
        "<x:meta xmlns:x=\"urn:x\"/>",
        "<styles xmlns=\"urn:x\"/>",
        "<office:styles-x/>",
        "<office:Styles/>",
        "<styles/>",
        "<style:styles/>",
        "<text:meta/>",
        "<style:font-face-decls/>",
    ];
    for element in nothing_else {
        one_element(element, &root(element), &root(""));
        // Inside the one body too, where no container of the six stands.
        one_element(
            &format!("{element} in the body"),
            &root(&format!("<office:body><office:drawing>{element}</office:drawing></office:body>")),
            &root("<office:body><office:drawing/></office:body>"),
        );
    }
}

/// An empty `settings.xml` is no part, and an empty `office:settings` under a flat root is none, in every form (had each an element,
/// for ODF wants one set of items in it). The control: a set of items is a setting, in every form, and differs from none.
#[test]
fn the_settings_of_a_package_and_of_a_flat_file_are_absent_when_empty_by_the_rule() {
    let settings = |inner: &str| {
        package(
            DRAWING,
            vec![
                (
                    "content.xml".to_owned(),
                    format!("<office:document-content {NS} office:version=\"1.3\">{BODY}</office:document-content>").into_bytes(),
                ),
                (
                    "settings.xml".to_owned(),
                    format!("<office:document-settings {NS} office:version=\"1.3\">{inner}</office:document-settings>").into_bytes(),
                ),
            ],
        )
    };
    let flat = |inner: &str| root(&format!("{inner}{BODY}"));
    same("settings in two packages", &settings("<office:settings/>"), &settings(""));
    same("settings, a package against a flat file", &settings("<office:settings/>"), &flat(""));
    same("settings, a flat file against a package", &flat("<office:settings/>"), &settings(""));
    same("settings in two flat files", &flat("<office:settings/>"), &flat(""));
    same("empty settings on both sides, two packages", &settings("<office:settings/>"), &settings("<office:settings/>"));
    same("empty settings on both sides, a package against a flat file", &settings("<office:settings/>"), &flat("<office:settings/>"));
    same("empty settings on both sides, two flat files", &flat("<office:settings/>"), &flat("<office:settings/>"));
    let held = format!("<office:settings>{ITEM_SET}</office:settings>");
    // Two packages: the empty part is no part, so the one that holds a setting is a part on one side only (its row says what the part says).
    let (ab, ba, shown) = counts("a setting in two packages", &settings(&held), &settings("<office:settings/>"));
    assert_eq!((ab, ba), (1, 1), "{shown:?}");
    assert!(
        shown.iter().any(|d| d.part == "settings.xml" && d.found == Found::ExtraPart || d.part == "settings.xml" && d.found == Found::MissingPart),
        "{shown:?}"
    );
    one_element("a setting, a package against a flat file", &settings(&held), &flat(""));
    one_element("a setting, a flat file against a package", &flat(&held), &settings(""));
    one_element("a setting in two flat files", &flat(&held), &flat(""));
}

#[test]
fn a_page_and_an_empty_drawing_differ_and_an_empty_paragraph_is_content() {
    one_element("a page against none", &root(BODY), &root("<office:body><office:drawing/></office:body>"));
    one_element("a paragraph against none", &root("<text:p/>"), &root(""));
    one_element("a drawing against none", &root("<office:body><office:drawing/></office:body>"), &root("<office:body/>"));
}

// ------------------------------------------------------------------------------------------------------------------- repeats, parts, joins

#[test]
fn a_container_written_twice_is_left_out_only_when_every_copy_of_it_is_empty() {
    // A file's repeated child is kept as it is and never merged: the reader takes the first, so an empty first beside a full second is not a full one alone.
    same("two empty ones against none", &root("<office:styles/><office:styles/>"), &root(""));
    one_element(
        "an empty one and then a full one, against the full one",
        &root("<office:styles/><office:styles><style:style style:name=\"a\" style:family=\"graphic\"/></office:styles>"),
        &root("<office:styles><style:style style:name=\"a\" style:family=\"graphic\"/></office:styles>"),
    );
    one_element(
        "a full one and then an empty one, against the full one",
        &root("<office:styles><style:style style:name=\"a\" style:family=\"graphic\"/></office:styles><office:styles/>"),
        &root("<office:styles><style:style style:name=\"a\" style:family=\"graphic\"/></office:styles>"),
    );
    // Two containers of two names each, each name once empty and once full: only the full ones are left, and they are compared.
    one_element(
        "an empty one of each name beside a full one of one",
        &root("<office:styles/><office:scripts/><office:scripts>x</office:scripts>"),
        &root("<office:scripts>x</office:scripts>"),
    );
}

#[test]
fn a_missing_part_is_not_an_empty_container() {
    // A part is an entry of the package, and an entry that one package has and the other has not is a difference whatever it holds, but for an empty `meta.xml` or `settings.xml`
    // A `meta.xml` that holds a property is the part it is (a generator is none).
    let with_meta =
        drawing(Form::Package, Home::Meta, "<office:meta><dc:title xmlns:dc=\"http://purl.org/dc/elements/1.1/\">g</dc:title></office:meta>");
    let with_empty_meta = drawing(Form::Package, Home::Meta, &empty("meta"));
    let no_meta = package(
        DRAWING,
        vec![
            ("content.xml".to_owned(), format!("<office:document-content {NS} office:version=\"1.3\">{BODY}</office:document-content>").into_bytes()),
            ("styles.xml".to_owned(), format!("<office:document-styles {NS} office:version=\"1.3\"></office:document-styles>").into_bytes()),
            ("settings.xml".to_owned(), format!("<office:document-settings {NS} office:version=\"1.3\"></office:document-settings>").into_bytes()),
        ],
    );
    let (ab, ba, shown) = counts("a package with meta.xml against one without", &with_meta, &no_meta);
    // The part: the manifest's row for it says what the part says, and is no second difference.
    assert_eq!((ab, ba), (1, 1), "{shown:?}");
    assert!(shown.iter().any(|d| (d.found, d.part.as_str()) == (Found::MissingPart, "meta.xml")), "{shown:?}");
    // An empty `meta.xml` is no part: the same as none.
    same("a package with an empty meta.xml against one without", &with_empty_meta, &no_meta);
    // Against a flat file, where there are no parts, the empty one is the same as none, whichever package has it.
    same("an empty meta against a flat file without meta", &with_empty_meta, &drawing(Form::Flat, Home::Meta, ""));
    same("no meta part against a flat file with an empty meta", &no_meta, &drawing(Form::Flat, Home::Meta, &empty("meta")));
}

#[test]
fn an_empty_container_in_either_part_of_a_package_joins_to_what_the_other_part_holds() {
    // The two parts of a package that may hold `office:automatic-styles` and `office:font-face-decls` are joined into the one a flat file has: an empty one
    // in either part is the same as none, so the other part's is the container, whichever part holds it.
    let style = r#"<style:style style:name="gr1" style:family="graphic"/>"#;
    let face = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#;
    let part = |name: &str, root_name: &str, inner: &str| {
        (name.to_owned(), format!("<office:{root_name} {NS} office:version=\"1.3\">{inner}</office:{root_name}>").into_bytes())
    };
    let containers = |empty_ones: bool| {
        if empty_ones {
            format!("{}{}", empty("font-face-decls"), empty("automatic-styles"))
        } else {
            format!("{}{}", holding("font-face-decls", face), holding("automatic-styles", style))
        }
    };
    let both = format!("{}{BODY}", containers(false));
    let flat = root(&both);
    // The content part empty and the styles part full, the reverse, and each part holding the whole.
    for (content_empty, styles_empty) in [(true, false), (false, true)] {
        let content = format!("{}{BODY}", containers(content_empty));
        let pkg = package(
            DRAWING,
            vec![part("content.xml", "document-content", &content), part("styles.xml", "document-styles", &containers(styles_empty))],
        );
        same(&format!("content empty {content_empty}, styles empty {styles_empty}, against the flat file"), &pkg, &flat);
    }
    // Both parts empty: neither a face nor a style at all, the same as a flat file with none and not the same as one with both.
    let pkg = package(
        DRAWING,
        vec![
            part("content.xml", "document-content", &format!("{}{BODY}", containers(true))),
            part("styles.xml", "document-styles", &containers(true)),
        ],
    );
    same("both parts empty, against a flat file with neither", &pkg, &root(BODY));
    let (ab, ba, shown) = counts("both parts empty, against a flat file with both", &pkg, &flat);
    assert_eq!((ab, ba), (2, 2), "the faces and the styles: {shown:?}");
}

#[test]
fn a_drawing_that_has_an_empty_container_is_the_same_as_itself_and_as_one_laid_out_another_way() {
    let flat = |gap: &str, containers: &str| {
        format!("<?xml version=\"1.0\"?>\n<office:document {NS} office:version=\"1.3\">{gap}{containers}{gap}{BODY}\n</office:document>").into_bytes()
    };
    let with = flat("\n  ", "<office:styles/>\n  <office:font-face-decls> </office:font-face-decls>\n  <office:meta></office:meta>");
    same("a file against itself", &with, &with);
    same("an empty set of containers against none, in two layouts", &with, &flat("\n\t", ""));
    same("two layouts, the containers in another order", &with, &flat("\n  ", "<office:meta/><office:font-face-decls/><office:styles/>"));
}

#[test]
fn every_container_of_the_six_is_listed_by_its_namespace_and_the_list_has_six() {
    // The names the rule lists are the six of 4.7 in the office namespace, and no others.
    let mut listed: Vec<(&str, &str)> = CANON_RULES.absent_when_empty.iter().map(|n| (n.ns, n.local)).collect();
    let mut expected: Vec<(&str, &str)> = CASES.iter().map(|(local, _)| (OFFICE, *local)).collect();
    expected.sort_unstable();
    expected.dedup();
    listed.sort_unstable();
    assert_eq!(listed, expected);
    assert_eq!(listed.len(), 6);
}

// ------------------------------------------------------------------------------------------------------------------------------------ cost

/// The steps the comparison counts for a flat file of `kids` against `other`, which are the same.
fn steps(kids: &str, other: &str) -> u64 {
    let mut work = Work::default();
    let d = compare_content_counted(&root(kids), &root(other), &Limits::default(), &mut work).unwrap();
    assert!(d.same(), "{:?}", d.shown);
    work.canon_steps
}

#[test]
fn a_flat_file_of_many_empty_containers_costs_a_pass_over_them() {
    // The same container written `n` times, empty: all are left out, and the cost goes with `n` (a repeat is kept after the first, and each is looked at once).
    let flood = |n: usize| "<office:styles/>".repeat(n);
    let (small, large) = (steps(&flood(20_000), ""), steps(&flood(40_000), ""));
    assert!(small <= 30 * 20_000, "{small} steps for 20,000 empty containers");
    assert!(large * 10 <= small * 21, "doubling the containers ({small} to {large} steps) does not do more than double the work");
    // One of them full: all the others stay, and are compared with their twins.
    let kept = |n: usize| format!("{}<office:styles>x</office:styles>", flood(n));
    let (small, large) = (steps(&kept(10_000), &kept(10_000)), steps(&kept(20_000), &kept(20_000)));
    assert!(small <= 60 * 10_000, "{small} steps for 10,000 empty containers beside a full one");
    assert!(large * 10 <= small * 21, "doubling the containers ({small} to {large} steps) does not do more than double the work");
}
