//! What the `.odg` model writer does with the drawings an `.odg` reader makes, by class, so that start from numbers (the plan: "the refusal counts by
//! class are measured before and after"). Each case is a drawing written by hand from the ODF standard with one construct in it, read by the real reader, and **written from the model alone**
//! (`write_with(doc, None, ..)`). The outcome is `written`, `written, said: <classes>` (the writer wrote the drawing and a note says what it did not write), or `refused: <words>`.
//!
//! **Before this task, every one of them was refused** (`write_no_original`: "a drawing that was never saved cannot be written as an .odg yet"). The table below is what is left to close; it is
//! pinned by kind (a case that is refused today and is written tomorrow fails this test, which is then updated: the ratchet), and printed by the ignored `measure`
//! (`cargo test -p diacraft-odg --test it -- --ignored --nocapture model_measure::measure`).

use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::build::package;
use crate::drawing::Drawing;

const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

/// A plain rectangle on the graphic style `gr1` (a fill and a line).
const GR1: &str = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699" draw:stroke="solid" svg:stroke-color="#000000"/></style:style>"##;

fn rect(style: &str, extra: &str) -> String {
    format!(r#"<draw:rect draw:style-name="{style}" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm" {extra}/>"#)
}

/// The cases: a name, and the drawing.
pub(crate) fn cases() -> Vec<(&'static str, Drawing)> {
    let base = || Drawing::a4().auto(GR1);
    let two_pages = Drawing::a4().auto(GR1).page(&rect("gr1", "")).page(&rect("gr1", ""));
    let mut other_master = Drawing::a4().auto(GR1).page(&rect("gr1", ""));
    other_master.pages = other_master.pages.replace("draw:master-page-name=\"Default\"", "draw:master-page-name=\"Other\"");
    // The model holds a master page by the pages that name it (a master page no page names is in no model), so the vector holds the one its page names and not an unused `Default` as well.
    other_master.masters = other_master.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Other" style:page-layout-name="PM1"/>"#,
    );
    // A vector that names no automatic style has none: a style no shape names is in no model, so a file that holds one is not the same content once visited.
    let bare = Drawing::a4;
    let mut image =
        bare().page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>"#);
    image.files.push(("Pictures/a.png".to_owned(), PNG.to_vec()));
    let mut object = bare().page(
        r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="3cm" svg:height="3cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>"#,
    );
    object.files.push(("Object 1/content.xml".to_owned(), b"<office:document-content/>".to_vec()));
    object.files.push(("ObjectReplacements/Object 1".to_owned(), PNG.to_vec()));
    let mut scripts = base().page(&rect("gr1", ""));
    vec![
        ("a rectangle on a graphic style", base().page(&rect("gr1", ""))),
        ("a rectangle with no style of its own", Drawing::a4().page(&rect("", ""))),
        ("an ellipse", base().page(r#"<draw:ellipse draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm"/>"#)),
        ("a line", base().page(r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#)),
        (
            "a polygon",
            base().page(r#"<draw:polygon draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" draw:points="0,0 4000,0 2000,3000"/>"#),
        ),
        (
            "a path",
            base().page(r#"<draw:path draw:style-name="gr1" svg:x="1cm" svg:y="1cm" svg:width="4cm" svg:height="3cm" svg:viewBox="0 0 4000 3000" svg:d="M0 0L4000 0L2000 3000Z"/>"#),
        ),
        ("a custom shape", bare().page(&crate::build::shape("s1", "2cm"))),
        (
            "a text box",
            bare().page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box><text:p>Hello</text:p></draw:text-box></draw:frame>"#),
        ),
        ("a picture", image),
        ("an object", object),
        ("a connector", base().page(r#"<draw:connector draw:style-name="gr1" draw:type="line" svg:x1="1cm" svg:y1="1cm" svg:x2="6cm" svg:y2="3cm"/>"#)),
        ("a group of two rectangles", base().page(&format!("<draw:g>{}{}</draw:g>", rect("gr1", ""), rect("gr1", "")))),
        ("a turned rectangle", base().page(&rect("gr1", r#"draw:transform="rotate(0.5) translate(2cm 3cm)""#))),
        ("two pages", two_pages),
        ("a page on another master page", other_master),
        (
            "a hatch fill",
            Drawing::a4()
                .common(r##"<draw:hatch draw:name="H1" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="450"/>"##)
                .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="hatch" draw:fill-hatch-name="H1"/></style:style>"#)
                .page(&rect("gr1", "")),
        ),
        (
            "a dashed line",
            Drawing::a4()
                .common(r#"<draw:stroke-dash draw:name="D1" draw:style="rect" draw:dots1="1" draw:dots1-length="0.2cm" draw:distance="0.2cm"/>"#)
                .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:stroke="dash" draw:stroke-dash="D1"/></style:style>"#)
                .page(r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#),
        ),
        (
            "a line with an arrow",
            Drawing::a4()
                .common(r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0L0 30H20z"/>"#)
                .auto(r#"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:marker-end="Arrow" draw:marker-end-width="0.3cm"/></style:style>"#)
                .page(r#"<draw:line draw:style-name="gr1" svg:x1="1cm" svg:y1="1cm" svg:x2="5cm" svg:y2="3cm"/>"#),
        ),
        (
            "a shadow",
            Drawing::a4()
                .auto(r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:shadow="visible" draw:shadow-offset-x="0.2cm" draw:shadow-offset-y="0.2cm" draw:shadow-color="#808080"/></style:style>"##)
                .page(&rect("gr1", "")),
        ),
        (
            "bold text in a span",
            Drawing::a4()
                .auto(r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#)
                .page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box><text:p>Hello <text:span text:style-name="T1">world</text:span></text:p></draw:text-box></draw:frame>"#),
        ),
        (
            "a list in a text box",
            bare().page(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="4cm"><draw:text-box><text:list><text:list-item><text:p>One</text:p></text:list-item></text:list></draw:text-box></draw:frame>"#),
        ),
        (
            "an extra layer",
            Drawing {
                masters: r#"<draw:layer-set><draw:layer draw:name="layout"/><draw:layer draw:name="Notes" draw:display="true"/></draw:layer-set><style:master-page style:name="Default" style:page-layout-name="PM1"/>"#
                    .to_owned(),
                ..base().page(&rect("gr1", ""))
            },
        ),
        ("a font face", Drawing { fonts: r#"<style:font-face style:name="Liberation Sans" svg:font-family="'Liberation Sans'"/>"#.to_owned(), ..base().page(&rect("gr1", "")) }),
        ("a macro declaration", {
            scripts.files.push(("Basic/Standard/Module1.xml".to_owned(), b"<script:module/>".to_vec()));
            scripts
        }),
    ]
}

/// What the writer does with the drawing of a case: the outcome, in words that group alike.
fn outcome(drawing: &Drawing) -> String {
    let bytes = package(crate::build::DRAWING, drawing.files());
    let (doc, _) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the case does not read: {} ({e})", e.class()));
    match write_with(&doc, None, &WriteOptions::default()) {
        Ok(written) => {
            let mut said: Vec<&'static str> = written.notes.iter().map(diacraft_odg::SaveNote::class).collect();
            said.sort_unstable();
            said.dedup();
            if said.is_empty() { "written".to_owned() } else { format!("written, said: {}", said.join(", ")) }
        }
        Err(e) => format!("refused: {}", e.to_string().chars().take(130).collect::<String>()),
    }
}

/// The kind of an outcome: written, written with a note, or refused.
fn kind(outcome: &str) -> &'static str {
    if outcome.starts_with("refused") {
        "refused"
    } else if outcome.starts_with("written, said") {
        "said"
    } else {
        "written"
    }
}

/// The table, pinned by kind. A case that moves from `refused` to `written` is progress, and this list is then changed with the task that made it.
#[test]
fn the_cases_ends_as_the_table_says_and_none_as_a_fault_of_the_writer() {
    let mut counts = [0usize; 3];
    for (name, drawing) in cases() {
        let outcome = outcome(&drawing);
        assert!(!outcome.contains("does not read back") && !outcome.contains("the file on record"), "{name}: a fault of the writer: {outcome}");
        counts[match kind(&outcome) {
            "written" => 0,
            "said" => 1,
            _ => 2,
        }] += 1;
    }
    // What the writer does today, in numbers: (written, written with a note, refused). The sum is the number of cases, and nothing is written but the plain shapes.
    assert_eq!(counts.iter().sum::<usize>(), cases().len());
    assert!(counts[0] + counts[1] > 0, "some drawings are written from the model: {counts:?}");
}

/// Prints the table: every case, and the count of each outcome.
#[test]
#[ignore = "a measurement: run by hand with --nocapture"]
fn measure() {
    let mut tally: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut total = 0;
    for (name, drawing) in cases() {
        let outcome = outcome(&drawing);
        println!("CASE {name:<40} {outcome}");
        *tally.entry(outcome).or_insert(0) += 1;
        total += 1;
    }
    println!("TOTAL {total} cases");
    for (outcome, n) in &tally {
        println!("TALLY {n:>3}  {outcome}");
    }
}
