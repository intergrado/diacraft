//! The content check sees every change planted in a drawing our writers wrote. The gate planted changes ("salts") in `.odg` files written by the
//! trip and asked `compare --content` whether each was seen. Here a drawing written by hand (shapes, a connector, a group, a text box with a span, automatic and
//! common styles, a marker, a picture, `meta.xml` fields and `settings.xml` items) is read and written again by the model writer, as a package and as a flat file, and
//! one change of each kind is planted in each part of what was written: an attribute's value, an attribute added and one taken out, an element added and one taken out, a
//! character of text, a property of a style, an automatic style's content, a length (each one of `content.xml`, from the last back), a picture's bytes, a manifest row, a
//! field of `meta.xml` and an item of `settings.xml`. **Against the file it was planted in, each is a difference.** Against the source, each raises the count, unless it
//! lands on a place the check already counts against the source (a value the writer wrote otherwise, an element the source has none of), where one place is one
//! difference however much it differs.

use diacraft_odg::{Form, Limits, OriginalPackage, WriteOptions, compare_content, write_with};

use crate::build::DRAWING;
use crate::drawing::{Drawing, meta_with, settings};
use crate::write_core::{move_by, open_bytes};

/// A picture's bytes: a PNG header and a few bytes, enough for an entry the check compares byte for byte.
const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x02\x00\x00\x00\x02\x08\x02\x00\x00\x00salt";

fn custom(name: &str, (x, y, w, h): (f64, f64, f64, f64), text: &str, style: &str) -> String {
    let para = if text.is_empty() { String::new() } else { format!(r#"<text:p text:style-name="P1">{text}</text:p>"#) };
    format!(
        r#"<draw:custom-shape draw:style-name="{style}" draw:text-style-name="P1" draw:layer="layout" draw:name="{name}" xml:id="{name}" draw:id="{name}" svg:width="{w}cm" svg:height="{h}cm" svg:x="{x}cm" svg:y="{y}cm">{para}<draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>"#
    )
}

/// The source drawing: what the trips of the gate held, written by hand.
fn source() -> Drawing {
    let auto = concat!(
        r##"<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#3465a4" svg:stroke-width="0.05cm" svg:stroke-color="#000000" fo:padding-left="0.25cm" fo:padding-right="0.25cm"/></style:style>"##,
        r##"<style:style style:name="gr2" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="none" svg:stroke-width="0.03cm" draw:marker-end="Arrow" draw:marker-end-width="0.3cm"/></style:style>"##,
        r##"<style:style style:name="gr3" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#cc0000" draw:stroke="none"/></style:style>"##,
        r#"<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>"#,
        r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-size="12pt"/></style:style>"#,
    );
    let common = concat!(
        r#"<draw:marker draw:name="Arrow" svg:viewBox="0 0 20 30" svg:d="M10 0l-10 30h20z"/>"#,
        r##"<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-width="0cm" draw:fill="solid" draw:fill-color="#729fcf"/></style:style>"##,
    );
    let connector = r#"<draw:connector draw:style-name="gr2" draw:text-style-name="P1" draw:layer="layout" draw:name="c" draw:start-shape="a" draw:start-glue-point="3" draw:end-shape="b" draw:end-glue-point="1" svg:x1="6cm" svg:y1="4cm" svg:x2="9cm" svg:y2="4cm" svg:d="M 6000 4000 L 9000 4000" svg:viewBox="0 0 9001 4001"><text:p/></draw:connector>"#;
    let text_box = r#"<draw:frame draw:style-name="gr3" draw:text-style-name="P1" draw:layer="layout" draw:name="t" svg:width="6cm" svg:height="1.5cm" svg:x="2cm" svg:y="9cm"><draw:text-box><text:p text:style-name="P1">Text <text:span text:style-name="T1">bold</text:span></text:p></draw:text-box></draw:frame>"#;
    let picture = r#"<draw:frame draw:style-name="gr3" draw:layer="layout" draw:name="pic" svg:width="3cm" svg:height="2cm" svg:x="12cm" svg:y="12cm"><draw:image xlink:href="Pictures/a.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"><text:p/></draw:image></draw:frame>"#;
    let group = format!("<draw:g>{}{}</draw:g>", custom("g1", (1.0, 14.0, 2.0, 2.0), "One", "gr1"), custom("g2", (4.0, 14.0, 2.0, 2.0), "", "gr1"));
    let shapes = format!(
        "{}{}{connector}{text_box}{picture}{group}",
        custom("a", (2.0, 3.0, 4.0, 2.0), "Alpha", "gr1"),
        custom("b", (9.0, 3.0, 4.0, 2.0), "Beta", "gr1")
    );
    let mut d = Drawing::a4().common(common).auto(auto).page(&shapes).page(&custom("p2", (3.0, 3.0, 5.0, 1.0), "Second", "gr3"));
    d.meta = Some(meta_with("TestWriter/1", "<dc:title>A test</dc:title><meta:keyword>alpha</meta:keyword><dc:subject>Subject</dc:subject>"));
    d.settings = Some(settings(
        r#"<config:config-item config:name="IsPrintFitPage" config:type="boolean">false</config:config-item><config:config-item config:name="PrinterName" config:type="string">lp</config:config-item>"#,
    ));
    d.files.push(("Pictures/a.png".to_owned(), PICTURE.to_vec()));
    d
}

/// The drawing written again: as a package by the model writer (the writer of a trip's last leg), and as a flat file by the splice writer from the package, with the
/// first shape moved (the model writer writes no flat file of a drawing that has none).
fn written(source: &[u8], form: Form) -> Vec<u8> {
    let opened = open_bytes(source.to_vec());
    match form {
        Form::Package => {
            write_with(&opened.doc, None, &WriteOptions { form: Some(form), ..WriteOptions::default() })
                .unwrap_or_else(|e| panic!("the model writer refused: {e}"))
                .bytes
        }
        Form::Flat => {
            let mut doc = opened.doc.clone();
            let id = doc.pages.get(0).and_then(|p| p.shapes.get(0)).map(|s| s.id).expect("a shape");
            move_by(&mut doc, 0, id, 1.0, 0.5);
            write_with(
                &doc,
                Some(&OriginalPackage { bytes: &opened.bytes, index: &opened.index }),
                &WriteOptions { form: Some(form), ..WriteOptions::default() },
            )
            .unwrap_or_else(|e| panic!("the splice writer refused: {e}"))
            .bytes
        }
    }
}

/// The count and the differences of the content check of `a` against `b`.
fn check(a: &[u8], b: &[u8]) -> (u64, Vec<String>) {
    let d = compare_content(a, b, &Limits::default()).unwrap_or_else(|e| panic!("the check refused: {} ({e})", e.class()));
    (d.count, d.shown.iter().map(|x| format!("{:?} {}", x.found, x.path)).collect())
}

/// The parts of a package, as names and bytes, in the archive's order.
fn parts_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let package = diacraft_odg::Package::read(bytes, &Limits::default()).unwrap();
    package.entries().filter(|e| !e.folder).map(|e| (e.name.clone(), package.part(&e.name).unwrap_or_default().to_vec())).collect()
}

/// A package of exactly `parts` (the manifest as given).
fn raw_package(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = diacraft_container::zip::ArchiveWriter::new();
    w.add("mimetype", DRAWING.as_bytes(), diacraft_container::zip::Method::Stored).unwrap();
    for (name, data) in parts.iter().filter(|(n, _)| n != "mimetype") {
        w.add(name, data, diacraft_container::zip::Method::Deflated).unwrap();
    }
    w.finish().unwrap()
}

/// A salt: a change to the text of one part, or `None` where the part has no place for it.
type Salt = fn(&str) -> Option<String>;

/// The `n`-th match of `pattern` (a literal) from the end replaced by `with` applied to it.
fn nth_from_end(text: &str, pattern: &str, n: usize, with: impl Fn(&str) -> String) -> Option<String> {
    let at = text.rmatch_indices(pattern).nth(n)?.0;
    Some(format!("{}{}{}", &text[..at], with(pattern), &text[at + pattern.len()..]))
}

/// The value of the last attribute `name` of the text, changed.
fn attribute_value(text: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = text.rfind(&key)? + key.len();
    let end = at + text[at..].find('"')?;
    Some(format!("{}{}x{}", &text[..at], &text[at..end], &text[end..]))
}

/// Every length of the text (a number directly followed by `cm`, `mm`, `in` or `pt`, inside a quoted value), as the byte ranges of the numbers, in order.
fn lengths(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            if bytes.get(j) == Some(&b'-') {
                j += 1;
            }
            let digits = j;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                j += 1;
            }
            let unit = text.get(j..j + 3);
            if j > digits && matches!(unit, Some("cm\"" | "mm\"" | "in\"" | "pt\"")) {
                out.push((start, j));
            }
        }
        i += 1;
    }
    out
}

const SALTS: &[(&str, Salt)] = &[
    ("an attribute's value (a name)", |t| attribute_value(t, "draw:name")),
    ("an attribute's value (a style's name)", |t| attribute_value(t, "style:name")),
    ("an attribute added", |t| nth_from_end(t, "<draw:custom-shape ", 0, |p| format!("{p}draw:z-index=\"77\" "))),
    ("an attribute taken out", |t| nth_from_end(t, " draw:layer=\"layout\"", 0, |_| String::new())),
    ("an element added", |t| {
        nth_from_end(t, "</draw:page>", 0, |p| format!("<draw:rect svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>{p}"))
    }),
    ("an element taken out", |t| {
        nth_from_end(t, "<draw:enhanced-geometry ", 0, |p| p.replace("<draw:enhanced-geometry ", "<draw:enhanced-geometry-gone ")).and_then(|s| {
            let at = s.rfind("<draw:enhanced-geometry-gone ")?;
            let end = at + s[at..].find("/>")? + 2;
            Some(format!("{}{}", &s[..at], &s[end..]))
        })
    }),
    ("a character of text", |t| {
        nth_from_end(t, "Alpha", 0, |_| "Alphb".to_owned()).or_else(|| nth_from_end(t, "Second", 0, |_| "Secone".to_owned()))
    }),
    ("a style's property", |t| {
        nth_from_end(t, "draw:fill-color=\"#", 0, |p| format!("{p}0")).map(|s| s.replacen("draw:fill-color=\"#0", "draw:fill-color=\"#1", 1))
    }),
    ("an automatic style's content", |t| {
        let at = t.find("<office:automatic-styles>")?;
        let style = at + t[at..].find("<style:style ")?;
        let end = style + t[style..].find('>')? + 1;
        if t[style..end].ends_with("/>") {
            return Some(format!("{}{}><style:text-properties fo:color=\"#123456\"/></style:style>{}", &t[..end - 2], "", &t[end..]));
        }
        Some(format!("{}<style:text-properties fo:color=\"#123456\"/>{}", &t[..end], &t[end..]))
    }),
    ("a field of meta.xml", |t| nth_from_end(t, "</dc:title>", 0, |p| format!("!{p}"))),
    ("a field added to meta.xml", |t| nth_from_end(t, "</office:meta>", 0, |p| format!("<meta:keyword>salt</meta:keyword>{p}"))),
    ("an item of settings.xml", |t| nth_from_end(t, "</config:config-item>", 0, |p| format!("salt{p}"))),
    ("a layer taken out", |t| nth_from_end(t, "<draw:layer draw:name=\"controls\"/>", 0, |_| String::new())),
    ("a marker's path", |t| nth_from_end(t, "svg:d=\"M10 0l-10 30h20z\"", 0, |_| "svg:d=\"M10 0l-10 30h21z\"".to_owned())),
];

/// Plants `salt` in the part `name` of the package `bytes`: `None` where the part has no place for it.
fn plant_in_package(bytes: &[u8], name: &str, salt: Salt) -> Option<Vec<u8>> {
    let mut parts = parts_of(bytes);
    let (_, data) = parts.iter_mut().find(|(n, _)| n == name)?;
    let text = String::from_utf8(data.clone()).ok()?;
    let salted = salt(&text).filter(|s| *s != text)?;
    *data = salted.into_bytes();
    Some(raw_package(&parts))
}

/// What a salt did: against the file it was planted in, and against the source (the count before and after).
struct Seen {
    what: String,
    own: u64,
    before: u64,
    after: u64,
    already: bool,
}

/// Whether every difference the salt makes against the written file is at a place the source and the written file already differ at, or inside one: the path of each
/// (what follows the kind) is one of theirs or lies under one.
fn already_counted(own: &[String], against_source: &[String]) -> bool {
    let path = |d: &str| d.split_once(' ').map_or(String::new(), |(_, p)| p.to_owned());
    let listed: Vec<String> = against_source.iter().map(|d| path(d)).collect();
    !own.is_empty() && own.iter().map(|d| path(d)).all(|p| listed.iter().any(|l| p == *l || (!l.is_empty() && p.starts_with(&format!("{l}/")))))
}

fn salted_checks(source: &[u8], written: &[u8], plant: impl Fn(Salt) -> Vec<(String, Vec<u8>)>) -> Vec<Seen> {
    let (before, listed) = check(source, written);
    let mut seen = Vec::new();
    for (name, salt) in SALTS {
        for (place, salted) in plant(*salt) {
            let (own, own_list) = check(written, &salted);
            let (after, _) = check(source, &salted);
            seen.push(Seen { what: format!("{name} in {place}"), own, before, after, already: already_counted(&own_list, &listed) });
        }
    }
    seen
}

#[test]
fn every_salt_in_a_package_the_model_writer_wrote_is_a_difference_against_it_and_counts_against_the_source() {
    let source = source().bytes();
    let written = written(&source, Form::Package);
    let names: Vec<String> = parts_of(&written).into_iter().map(|(n, _)| n).collect();
    let mut seen = salted_checks(&source, &written, |salt| {
        names.iter().filter_map(|name| plant_in_package(&written, name, salt).map(|b| (name.clone(), b))).collect()
    });
    // A picture's bytes, and a manifest row's media type and a row taken out.
    let mut parts = parts_of(&written);
    if let Some((_, data)) = parts.iter_mut().find(|(n, _)| n.starts_with("Pictures/")) {
        let last = data.len() - 1;
        data[last] ^= 1;
        let salted = raw_package(&parts);
        let (own, _) = check(&written, &salted);
        let (before, _) = check(&source, &written);
        let (after, _) = check(&source, &salted);
        seen.push(Seen { what: "a picture's bytes".to_owned(), own, before, after, already: false });
    }
    for (what, salt) in [
        (
            "a manifest row's media type",
            (|t: &str| {
                nth_from_end(t, "manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"", 0, |_| {
                    "manifest:full-path=\"content.xml\" manifest:media-type=\"text/plain\"".to_owned()
                })
            }) as Salt,
        ),
        ("a manifest row taken out", |t: &str| {
            let at = t.find("manifest:full-path=\"Pictures/a.png\"")?;
            let start = t[..at].rfind('<')?;
            let end = at + t[at..].find("/>")? + 2;
            Some(format!("{}{}", &t[..start], &t[end..]))
        }),
    ] {
        if let Some(salted) = plant_in_package(&written, "META-INF/manifest.xml", salt) {
            let (own, own_list) = check(&written, &salted);
            let (before, listed) = check(&source, &written);
            let (after, _) = check(&source, &salted);
            seen.push(Seen { what: what.to_owned(), own, before, after, already: already_counted(&own_list, &listed) });
        }
    }
    // Every length of `content.xml`, from the last back (the gate's salt).
    let content = String::from_utf8(parts_of(&written).into_iter().find(|(n, _)| n == "content.xml").unwrap().1).unwrap();
    let (before, listed) = check(&source, &written);
    for (k, (start, end)) in lengths(&content).into_iter().rev().enumerate() {
        let salted_text = format!("{}{}{}", &content[..start], "9.75", &content[end..]);
        let mut parts = parts_of(&written);
        parts.iter_mut().find(|(n, _)| n == "content.xml").unwrap().1 = salted_text.into_bytes();
        let salted = raw_package(&parts);
        let (own, own_list) = check(&written, &salted);
        let (after, _) = check(&source, &salted);
        seen.push(Seen {
            what: format!("the length {k} from the end of content.xml"),
            own,
            before,
            after,
            already: already_counted(&own_list, &listed),
        });
    }
    let unseen: Vec<&str> = seen.iter().filter(|s| s.own == 0).map(|s| s.what.as_str()).collect();
    assert!(unseen.is_empty(), "salts the check called the same against the file they were planted in: {unseen:?}");
    let uncounted: Vec<String> =
        seen.iter().filter(|s| s.after <= s.before && !s.already).map(|s| format!("{} ({} -> {})", s.what, s.before, s.after)).collect();
    assert!(uncounted.is_empty(), "salts that did not raise the count against the source at a place it did not count: {uncounted:?}");
    assert!(seen.len() > 40, "{} salts", seen.len());
    // The places already counted are few: most salts raise the count (so the test is not passed by calling every place one already counted).
    let raised = seen.iter().filter(|s| s.after > s.before).count();
    assert!(raised * 10 >= seen.len() * 9, "{raised} of {} raised the count", seen.len());
}

#[test]
fn every_salt_in_a_flat_file_our_writer_wrote_is_a_difference_against_it_and_counts_against_the_source() {
    let source = source().bytes();
    let written = written(&source, Form::Flat);
    let text = String::from_utf8(written.clone()).unwrap();
    let mut seen = salted_checks(&source, &written, |salt| {
        salt(&text).filter(|s| *s != text).map(|s| ("the flat file".to_owned(), s.into_bytes())).into_iter().collect()
    });
    let (before, listed) = check(&source, &written);
    for (k, (start, end)) in lengths(&text).into_iter().rev().enumerate() {
        let salted = format!("{}{}{}", &text[..start], "9.75", &text[end..]).into_bytes();
        let (own, own_list) = check(&written, &salted);
        let (after, _) = check(&source, &salted);
        seen.push(Seen { what: format!("the length {k} from the end"), own, before, after, already: already_counted(&own_list, &listed) });
    }
    let unseen: Vec<&str> = seen.iter().filter(|s| s.own == 0).map(|s| s.what.as_str()).collect();
    assert!(unseen.is_empty(), "salts the check called the same against the file they were planted in: {unseen:?}");
    let uncounted: Vec<String> =
        seen.iter().filter(|s| s.after <= s.before && !s.already).map(|s| format!("{} ({} -> {})", s.what, s.before, s.after)).collect();
    assert!(uncounted.is_empty(), "salts that did not raise the count against the source at a place it did not count: {uncounted:?}");
    assert!(seen.len() > 30, "{} salts", seen.len());
    let raised = seen.iter().filter(|s| s.after > s.before).count();
    assert!(raised * 10 >= seen.len() * 9, "{raised} of {} raised the count", seen.len());
}

/// The group's members come back from the model writer as the source wrote them: a member's frame and the lengths of the style made for it are the source's
/// numbers, not numbers rounded on the way (2cm is not 2.0000000005cm), so a trip leaves them the same content.
#[test]
fn a_groups_members_are_written_with_the_lengths_the_source_has() {
    let source = source().bytes();
    let written = written(&source, Form::Package);
    let content = String::from_utf8(parts_of(&written).into_iter().find(|(n, _)| n == "content.xml").unwrap().1).unwrap();
    let group = &content[content.find("<draw:g").unwrap()..content.find("</draw:g>").unwrap()];
    for want in [r#"svg:width="2cm""#, r#"svg:height="2cm""#, r#"svg:x="1cm""#, r#"svg:x="4cm""#, r#"svg:y="14cm""#] {
        assert!(group.contains(want), "{want}: {group}");
    }
}
