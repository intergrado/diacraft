//! What the `.odg` reader does not keep is said when the file opens, and what it can keep comes back from the plain visit (`.odg` to `.diacraft` to `.odg`).
//! The after-save check compares the drawing with the file a save writes, so whatever the reader drops before the drawing exists is invisible to it: the open must say it, with a count, or the
//! visit must write it back. Every file is built in code, by hand from the OASIS ODF 1.4 standard (Part 3: `style:master-page` §16.9, `style:font-face` §16.23, the text properties §16.29 and
//! the drawing shapes §10.3); only the classes and counts of the gate's sample-file check crossed (GATE.ea42).

use serde_json::{Value, json};

use crate::native_common::{Dir, opened};

/// The namespaces of the parts' roots, LibreOffice's extension among them.
const NS: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" "#,
    r#"xmlns:dr3d="urn:oasis:names:tc:opendocument:xmlns:dr3d:1.0" xmlns:form="urn:oasis:names:tc:opendocument:xmlns:form:1.0" "#,
    r#"xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0""#
);

const LAYERS: &str = r#"<draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="background"/><draw:layer draw:name="backgroundobjects"/><draw:layer draw:name="controls"/><draw:layer draw:name="measurelines"/></draw:layer-set>"#;

const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" draw:stroke="solid" svg:stroke-color="#000000"/></style:style>"##;

/// A drawing written by hand: each part's font faces, the common styles, the automatic styles of each part, the master styles and the pages of `content.xml`.
struct Drawing {
    faces_styles: String,
    faces_content: String,
    common: String,
    auto_styles: String,
    auto_content: String,
    masters: String,
    pages: String,
}

impl Drawing {
    /// A drawing of one page on the master page `Default`, whose layout is `PM1`.
    fn new() -> Drawing {
        Drawing {
            faces_styles: String::new(),
            faces_content: String::new(),
            common: String::new(),
            auto_styles: page_layout("PM1", "21cm", "29.7cm"),
            auto_content: GR1.to_owned(),
            masters: r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#.to_owned(),
            pages: String::new(),
        }
    }

    fn page(mut self, master: &str, shapes: &str) -> Drawing {
        let n = self.pages.matches("<draw:page ").count() + 1;
        self.pages.push_str(&format!(r#"<draw:page draw:name="page{n}" draw:master-page-name="{master}">{shapes}</draw:page>"#));
        self
    }

    fn bytes(&self) -> Vec<u8> {
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NS} office:version=\"1.3\"><office:font-face-decls>{}</office:font-face-decls><office:automatic-styles>{}</office:automatic-styles><office:body><office:drawing>{}</office:drawing></office:body></office:document-content>",
            self.faces_content, self.auto_content, self.pages
        );
        let styles = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles {NS} office:version=\"1.3\"><office:font-face-decls>{}</office:font-face-decls><office:styles>{}</office:styles><office:automatic-styles>{}</office:automatic-styles><office:master-styles>{LAYERS}{}</office:master-styles></office:document-styles>",
            self.faces_styles, self.common, self.auto_styles, self.masters
        );
        crate::odg_build::package(
            crate::odg_build::DRAWING,
            vec![("content.xml".to_owned(), content.into_bytes()), ("styles.xml".to_owned(), styles.into_bytes())],
        )
    }
}

fn page_layout(name: &str, width: &str, height: &str) -> String {
    format!(
        r#"<style:page-layout style:name="{name}"><style:page-layout-properties fo:margin-top="1cm" fo:margin-bottom="1cm" fo:margin-left="1cm" fo:margin-right="1cm" fo:page-width="{width}" fo:page-height="{height}" style:print-orientation="portrait"/></style:page-layout>"#
    )
}

/// A rectangle on `gr1`.
fn rect(name: &str, x: f64) -> String {
    format!(
        r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="{name}" svg:x="{x}cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"><text:p>{name}</text:p></draw:rect>"#
    )
}

/// The plain visit of `source`: opened, saved as `.diacraft`, that opened and saved as `.odg`, each leg a session of its own. Gives the `.odg` written, and what its save said.
fn visit(source: &[u8]) -> (Vec<u8>, Value) {
    let dir = Dir::new();
    dir.write("a.odg", source);
    opened(&dir.path("a.odg")).execute("file.save_as", &json!({ "path": dir.path("b.diacraft") })).unwrap_or_else(|e| panic!("to .diacraft: {e}"));
    let said = opened(&dir.path("b.diacraft"))
        .execute("file.save_as", &json!({ "path": dir.path("c.odg") }))
        .unwrap_or_else(|e| panic!("back to .odg: {e}"));
    (dir.read("c.odg"), said)
}

/// What `file.open` says of `source`: the warnings it lists and its count of them.
fn open_says(source: &[u8]) -> (Vec<String>, u64) {
    let dir = Dir::new();
    let path = dir.write("a.odg", source);
    let mut s = diacraft_engine::Session::new();
    let result = s.execute("file.open", &json!({ "path": path })).unwrap();
    let file = &result["file"];
    let warnings = file["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_owned()).collect();
    (warnings, file["warning_count"].as_u64().unwrap())
}

/// The elements `name` of a part, each as written from its `<` to its end (a `/>` or its closing tag; none nests in itself here).
fn elements(part: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = part[from..].find(&format!("<{name} ")).map(|a| a + from) {
        let open_end = at + part[at..].find('>').unwrap();
        let end = if part[..=open_end].ends_with("/>") {
            open_end + 1
        } else {
            open_end + part[open_end..].find(&format!("</{name}>")).unwrap() + name.len() + 3
        };
        out.push(part[at..end].to_owned());
        from = end;
    }
    out
}

/// The part's `office:automatic-styles`, as written.
fn automatic(part: &str) -> &str {
    let at = part.find("<office:automatic-styles").unwrap();
    &part[at..at + part[at..].find("</office:automatic-styles>").unwrap_or(0)]
}

// ---------------------------------------------------------------------------------------------------------------------- master pages (kept)

/// **Every master page comes back with what it holds** (GATE.ea42: master pages and page layouts lost in 27 files of the plain visit). The reader held a master page only as the pages on it
/// held it: one that no page names was lost, the children of the others (LibreOffice's theme, the notes) were held in the document's data and then not written, and a master page's drawing-page
/// style, which `styles.xml` holds, was written into `content.xml`, where the master page does not find it (its background was lost). Now a master page that holds no shape is kept whole and
/// written back, and a drawing-page style goes in the part of what names it. Red on main (770a162c): the master page `Unused` is not in the file, `Default` has no children, and `Mdp1` is
/// not among `styles.xml`'s automatic styles.
#[test]
fn master_pages_and_their_styles_come_back_from_the_plain_visit() {
    let mut d = Drawing::new();
    d.auto_styles.push_str(&page_layout("PM2", "29.7cm", "21cm"));
    d.auto_styles.push_str(r##"<style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties draw:fill="solid" draw:fill-color="#ffffcc" draw:background-size="full"/></style:style>"##);
    d.auto_content.push_str(r#"<style:style style:name="dp1" style:family="drawing-page"/>"#);
    let theme = r##"<loext:theme loext:name="Office"><loext:theme-colors loext:name="LibreOffice"><loext:color loext:name="dark1" loext:color="#000000"/></loext:theme-colors></loext:theme>"##;
    let notes = r#"<presentation:notes><draw:page-thumbnail draw:layer="layout" svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="2cm" draw:page-number="1" presentation:class="page"/></presentation:notes>"#;
    d.masters = format!(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1" draw:style-name="Mdp1">{theme}{notes}</style:master-page><style:master-page style:name="Unused" style:display-name="An unused one" style:page-layout-name="PM2">{theme}</style:master-page><style:master-page style:name="Wide" style:page-layout-name="PM2"/>"#
    );
    let d = d.page("Default", &rect("A", 1.0)).page("Wide", &rect("B", 4.0));
    let source = d.bytes();
    let (back, said) = visit(&source);
    let styles = crate::odg_build::part_of(&back, "styles.xml");
    let masters = elements(&styles, "style:master-page");
    assert_eq!(masters.len(), 3, "{masters:#?}");
    let named = |name: &str| {
        masters.iter().find(|m| m.contains(&format!(r#"style:name="{name}""#))).unwrap_or_else(|| panic!("{name} is back: {masters:#?}"))
    };
    assert!(
        named("Default").contains(r#"draw:style-name="Mdp1""#) && named("Default").contains(theme) && named("Default").contains(notes),
        "{}",
        named("Default")
    );
    assert!(
        named("Unused").contains(r#"style:display-name="An unused one""#)
            && named("Unused").contains(r#"style:page-layout-name="PM2""#)
            && named("Unused").contains(theme)
    );
    assert!(named("Wide").contains(r#"style:page-layout-name="PM2""#));
    // The drawing-page style of a master page is `styles.xml`'s, where the master page finds it, and the page's own is `content.xml`'s.
    assert!(automatic(&styles).contains(r#"<style:style style:name="Mdp1" style:family="drawing-page">"#), "{}", automatic(&styles));
    assert!(automatic(&styles).contains(r##"draw:fill-color="#ffffcc""##));
    let content = crate::odg_build::content_of(&back);
    assert!(automatic(&content).contains(r#"style:name="dp1""#), "{}", automatic(&content));
    assert!(!automatic(&content).contains(r#"style:name="Mdp1""#), "{}", automatic(&content));
    // Both layouts are there, and nothing of the master pages is left unwritten: the save says no piece of the containers' markup is left out.
    assert!(automatic(&styles).contains(r#"style:name="PM1""#) && automatic(&styles).contains(r#"style:name="PM2""#));
    let warnings = said["warnings"].as_array().unwrap();
    assert!(!warnings.iter().any(|w| w.as_str().unwrap().contains("containers' markup")), "{warnings:?}");
    // And the open names nothing of them: the drawing holds them.
    let (open, _) = open_says(&source);
    assert!(!open.iter().any(|w| w.as_str() == diacraft_odg::NOT_KEPT_HEAD), "{open:?}");
}

/// **A second visit writes the master pages as the first did** (the content stamp and the plain visit's "same content"): the kept master pages are no edit, so the file the second visit
/// writes holds the same master pages and drawing-page styles as the first's.
#[test]
fn a_second_visit_writes_the_master_pages_the_first_wrote() {
    let mut d = Drawing::new();
    d.masters.push_str(r#"<style:master-page style:name="Unused" style:page-layout-name="PM1"><presentation:notes/></style:master-page>"#);
    let (first, _) = visit(&d.page("Default", &rect("A", 1.0)).bytes());
    let (second, _) = visit(&first);
    let masters = |b: &[u8]| elements(&crate::odg_build::part_of(b, "styles.xml"), "style:master-page");
    assert_eq!(masters(&first), masters(&second));
    assert_eq!(masters(&first).len(), 2, "{:#?}", masters(&first));
}

// ---------------------------------------------------------------------------------------------------------------------- style properties (kept)

/// **A style's second spelling of a font and the type of a line that is off come back** (GATE.ea42: style properties lost in 423 files). The cells read `style:font-name` and
/// `fo:font-family` into one `Font` (and the Asian and complex pairs likewise), and a line's style and type into one flag; a copy wrote the face's name alone, and a line that is off with no
/// type: `fo:font-family="'Liberation Sans'"` beside its name and `style:text-line-through-type="none"`, which LibreOffice writes in its default styles, were lost on every copy. They are
/// held beside the cells now, stamped by them, as the file's other spellings are. Red on main (770a162c): none of the six attributes is in the visited style.
#[test]
fn a_style_s_font_family_and_line_types_come_back_from_the_plain_visit() {
    let text = concat!(
        r#"style:font-name="Liberation Sans" fo:font-family="'Liberation Sans'" style:font-name-asian="Noto Sans CJK SC" style:font-family-asian="'Noto Sans CJK SC'" "#,
        r#"style:font-name-complex="Lohit Devanagari" style:font-family-complex="'Lohit Devanagari'" style:text-line-through-style="none" style:text-line-through-type="none" "#,
        r#"style:text-underline-style="none" style:text-underline-type="none""#
    );
    let faces = concat!(
        r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'" style:font-family-generic="roman" style:font-pitch="variable"/>"#,
        r#"<style:font-face style:name="Noto Sans CJK SC" svg:font-family="'Noto Sans CJK SC'" style:font-family-generic="system" style:font-pitch="variable"/>"#,
        r#"<style:font-face style:name="Lohit Devanagari" svg:font-family="'Lohit Devanagari'" style:font-family-generic="system" style:font-pitch="variable"/>"#
    );
    let mut d = Drawing::new();
    d.faces_styles = faces.to_owned();
    d.faces_content = faces.to_owned();
    d.common = format!(
        r#"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:fill="none"/><style:text-properties {text}/></style:style>"#
    );
    d.auto_content.push_str(&format!(
        r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" {text}/></style:style>"#
    ));
    let shapes = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="A" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="1cm"><text:p>a <text:span text:style-name="T1">bold</text:span> run</text:p></draw:rect>"#;
    let (back, _) = visit(&d.page("Default", shapes).bytes());
    let attributes = [
        r#"fo:font-family="'Liberation Sans'""#,
        r#"style:font-family-asian="'Noto Sans CJK SC'""#,
        r#"style:font-family-complex="'Lohit Devanagari'""#,
        r#"style:text-line-through-type="none""#,
        r#"style:text-underline-type="none""#,
    ];
    let styles = crate::odg_build::part_of(&back, "styles.xml");
    let standard = elements(&styles, "style:style").into_iter().find(|s| s.contains(r#"style:name="standard""#)).unwrap();
    for attribute in attributes {
        assert!(standard.contains(attribute), "the common style holds {attribute}: {standard}");
    }
    // The run's automatic style, whatever it is called now: the one the span names.
    let content = crate::odg_build::content_of(&back);
    let span = content.find("<text:span text:style-name=\"").map(|at| &content[at + 28..]).unwrap();
    let name = &span[..span.find('"').unwrap()];
    let run = elements(&content, "style:style").into_iter().find(|s| s.contains(&format!(r#"style:name="{name}""#))).unwrap();
    for attribute in &attributes[..4] {
        assert!(run.contains(attribute), "the run's style holds {attribute}: {run}");
    }
}

/// **The held spellings are no stale value**: a run whose font is changed after the open is written with the new face's name and without the old family, since the stamp of its `Font` cell no
/// longer holds; a run left as it was keeps its family.
#[test]
fn a_font_family_held_beside_its_cell_goes_when_the_font_is_changed() {
    let faces = concat!(
        r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/>"#,
        r#"<style:font-face style:name="DejaVu Sans" svg:font-family="'DejaVu Sans'"/>"#
    );
    let mut d = Drawing::new();
    d.faces_styles = faces.to_owned();
    d.faces_content = faces.to_owned();
    d.auto_content.push_str(r#"<style:style style:name="T1" style:family="text"><style:text-properties style:font-name="Liberation Sans" fo:font-family="'Liberation Sans'"/></style:style>"#);
    let shapes = r#"<draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="A" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="1cm"><text:p><text:span text:style-name="T1">all of it</text:span></text:p></draw:rect>"#;
    let dir = Dir::new();
    dir.write("a.odg", &d.page("Default", shapes).bytes());
    let mut s = opened(&dir.path("a.odg"));
    s.execute("file.save_as", &json!({ "path": dir.path("b.diacraft") })).unwrap();
    let mut s = opened(&dir.path("b.diacraft"));
    s.execute("text.format", &json!({ "ids": [1], "font": "DejaVu Sans" })).unwrap();
    s.execute("file.save_as", &json!({ "path": dir.path("c.odg") })).unwrap();
    let content = crate::odg_build::content_of(&dir.read("c.odg"));
    assert!(content.contains(r#"style:font-name="DejaVu Sans""#), "{content}");
    assert!(!content.contains(r#"fo:font-family="'Liberation Sans'""#), "{content}");
}

// ---------------------------------------------------------------------------------------------------------------------- what the open names

/// **The open names what the drawing does not hold, a line for each class with its count**, first among its warnings, and a drawing that holds all of its file is told
/// nothing: shapes of the kinds kept only as placeholders (a dimension line, a callout, a form control, a 3-D scene: a copy writes each as a plain shape), a font declaration that repeats a
/// name the other part declares otherwise, and a default tab distance (no copy writes `DefaultTabStop`). A frame that holds an object is a frame still, and is not counted. Red on main
/// (770a162c): the open lists no such line.
#[test]
fn the_open_names_what_the_drawing_does_not_hold_a_line_for_each_class() {
    let mut d = Drawing::new();
    d.faces_styles = r#"<style:font-face style:name="Arial" svg:font-family="Arial" style:font-family-generic="swiss"/>"#.to_owned();
    d.faces_content = r#"<style:font-face style:name="Arial" svg:font-family="Arial" style:font-family-generic="roman"/>"#.to_owned();
    d.common =
        r#"<style:style style:name="Tabbed" style:family="paragraph"><style:paragraph-properties style:tab-stop-distance="1.25cm"/></style:style>"#
            .to_owned();
    let objects = concat!(
        r#"<draw:measure draw:style-name="gr1" draw:layer="layout" svg:x1="1cm" svg:y1="5cm" svg:x2="6cm" svg:y2="5cm"><text:p/></draw:measure>"#,
        r#"<draw:caption draw:style-name="gr1" draw:layer="layout" svg:x="1cm" svg:y="7cm" svg:width="3cm" svg:height="2cm" draw:caption-point-x="-1cm" draw:caption-point-y="3cm"><text:p>a callout</text:p></draw:caption>"#,
        r#"<draw:g><draw:control draw:control="c1" draw:layer="controls" svg:x="1cm" svg:y="10cm" svg:width="3cm" svg:height="1cm"/>"#,
        r#"<dr3d:scene draw:layer="layout" svg:x="5cm" svg:y="10cm" svg:width="3cm" svg:height="3cm"><dr3d:cube dr3d:min-edge="-1 -1 -1" dr3d:max-edge="1 1 1"/></dr3d:scene></draw:g>"#,
        r#"<draw:frame draw:layer="layout" svg:x="10cm" svg:y="10cm" svg:width="3cm" svg:height="3cm"><draw:object xlink:href="./Object 1" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>"#
    );
    let source = d.page("Default", &format!("{}{objects}", rect("A", 1.0))).bytes();
    let (warnings, count) = open_says(&source);
    assert_eq!(
        warnings[..4],
        [
            diacraft_odg::NOT_KEPT_HEAD.to_owned(),
            "4 shapes of kinds DiaCraft keeps only as placeholders (dimension lines, callouts, form controls or 3-D scenes), which a copy writes as plain shapes".to_owned(),
            "1 font declaration that repeats a name, which DiaCraft's one table of fonts holds once".to_owned(),
            "1 style property that no copy writes (a default tab distance)".to_owned(),
        ],
        "{warnings:#?}"
    );
    assert!(count >= 4, "each line is a warning the count holds: {count}");
    for line in &warnings[..4] {
        assert!(line.chars().count() <= diacraft_odg::NOT_KEPT_LINE_CHARS, "{line}");
    }
    // A drawing that holds all its file says is told nothing of the kind.
    let (clean, _) = open_says(&Drawing::new().page("Default", &rect("A", 1.0)).bytes());
    assert!(!clean.iter().any(|w| w == diacraft_odg::NOT_KEPT_HEAD), "{clean:?}");
}

/// **What the open names is what a copy is without** (the note's words are true): after the plain visit the dimension line and the callout are no longer elements of their kinds, the
/// default tab distance is gone and the shape in a hyperlink is over the shape that was over it, while a save into the file itself keeps them all.
#[test]
fn what_the_open_names_is_what_a_copy_is_without_and_a_save_into_the_file_keeps() {
    let mut d = Drawing::new();
    d.common =
        r#"<style:style style:name="Tabbed" style:family="paragraph"><style:paragraph-properties style:tab-stop-distance="1.25cm"/></style:style>"#
            .to_owned();
    let objects = concat!(
        r#"<draw:measure draw:style-name="gr1" draw:layer="layout" svg:x1="1cm" svg:y1="5cm" svg:x2="6cm" svg:y2="5cm"><text:p/></draw:measure>"#,
        r#"<draw:caption draw:style-name="gr1" draw:layer="layout" svg:x="1cm" svg:y="7cm" svg:width="3cm" svg:height="2cm" draw:caption-point-x="-1cm" draw:caption-point-y="3cm"><text:p>a callout</text:p></draw:caption>"#
    );
    let link = r#"<draw:a xlink:href="https://example.org/" xlink:type="simple"><draw:rect draw:style-name="gr1" draw:layer="layout" draw:name="L" svg:x="5cm" svg:y="1cm" svg:width="2cm" svg:height="1cm"/></draw:a>"#;
    let source = d.page("Default", &format!("{link}{}{objects}", rect("A", 1.0))).bytes();
    let (back, _) = visit(&source);
    let all = format!("{}{}", crate::odg_build::content_of(&back), crate::odg_build::part_of(&back, "styles.xml"));
    assert!(!all.contains("<draw:measure") && !all.contains("<draw:caption") && !all.contains("tab-stop-distance"), "the copy is without them");
    // The hyperlink's shape, first on the page, is written after the shapes the model holds: over them.
    let content = crate::odg_build::content_of(&back);
    assert!(content.find("<draw:a ").unwrap() > content.find(r#"draw:name="A""#).unwrap(), "{content}");
    let (said, _) = open_says(&source);
    assert!(said.iter().any(|w| w == "1 shape in a hyperlink, which a copy writes over the shapes that were over it"), "{said:?}");
    // A save into the file splices: what the drawing does not hold stays in the file's bytes.
    let dir = Dir::new();
    dir.write("a.odg", &source);
    let mut s = opened(&dir.path("a.odg"));
    let a = s
        .document()
        .unwrap()
        .pages
        .iter()
        .next()
        .unwrap()
        .shapes
        .iter()
        .find(|shape| shape.name.as_deref() == Some("A"))
        .map(|shape| shape.id)
        .unwrap();
    s.execute("shape.move", &json!({ "ids": [a], "dx": 0.5, "dy": 0.0 })).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    let saved = dir.read("a.odg");
    let all = format!("{}{}", crate::odg_build::content_of(&saved), crate::odg_build::part_of(&saved, "styles.xml"));
    assert!(
        all.contains("<draw:measure") && all.contains("<draw:caption") && all.contains(r#"style:tab-stop-distance="1.25cm""#),
        "a save into the file keeps them"
    );
    let content = crate::odg_build::content_of(&saved);
    assert!(content.find("<draw:a ").unwrap() < content.find(r#"draw:name="A""#).unwrap(), "and the hyperlink's place: {content}");
}
