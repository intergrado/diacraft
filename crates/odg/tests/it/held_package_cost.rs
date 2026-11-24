//! What the package, `meta.xml`, `settings.xml` and the font faces cost the read, on the inputs a hostile file is made of: a `meta.xml` of 65,536 user-defined
//! properties, a `settings.xml` of 64 MiB and one of 1,000,000 items, a manifest of 100,000 rows (naming entries that are not there, and leaving entries unnamed), 65,536 font faces. Each
//! case has **a counted test** (the work and the items the read took, which grow in step with the input and are charged before what they pay for is made) and **a timed run beside a
//! yardstick of plain content of the same size** (`#[ignore]`: run on request, `--ignored`, as the other timed tests of this crate are). Nothing here asserts an elapsed time but the
//! ignored ones, and those hold a ratio and not a time.
//!
//! The memory of the largest is in `held_package_memory.rs`.

use std::time::Instant;

use diacraft_odg::{Limits, ReadReport, read, read_partial};

use crate::build::{DRAWING, write};
use crate::drawing::{Drawing, NS, as_text};
use crate::held_package::meta_of;

// ---- the inputs -------------------------------------------------------------------------------------------------------------------------

/// `n` user-defined fields with names of one width, so that every field is the same size.
fn properties(n: usize) -> String {
    (0..n).map(|k| format!(r#"<meta:user-defined meta:name="n{k:05}"/>"#)).collect()
}

/// The same bytes in plain content: `n` elements of a namespace nothing reads, which the model keeps whole.
fn foreign_fields(n: usize) -> String {
    (0..n).map(|k| format!(r#"<x:user-defined meta:name="n{k:05}"/>"#)).collect()
}

fn with_meta(fields: &str) -> Drawing {
    let mut d = Drawing::a4().page("");
    d.meta = Some(meta_of(fields));
    d
}

/// A `settings.xml` of `n` elements of one byte of name: 4 bytes each, so that a million are 4 MB and under the cap of a small part.
fn settings_of_items(n: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings {NS} office:version=\"1.3\"><office:settings>{}</office:settings></office:document-settings>",
        "<a/>".repeat(n)
    )
}

/// A `settings.xml` of about `bytes` bytes of configuration items: valid XML of 70 bytes an item (and so compressible by no more than a ratio the container allows).
fn settings_of_bytes(bytes: usize) -> String {
    let item = r#"<config:config-item config:name="x" config:type="int">1</config:config-item>"#;
    crate::drawing::settings(&item.repeat(bytes / item.len()))
}

fn drawing_with_settings(settings: String) -> Vec<u8> {
    let mut d = Drawing::a4().page("");
    d.settings = Some(settings);
    d.bytes()
}

/// `n` font faces of one width of name, declared once for both parts.
fn faces(n: usize) -> String {
    (0..n).map(|k| format!(r#"<style:font-face style:name="f{k:05}" svg:font-family="f{k:05}"/>"#)).collect()
}

fn with_faces(n: usize) -> Drawing {
    let mut d = Drawing::a4().page("");
    d.fonts = faces(n);
    d
}

/// A manifest of `rows` rows in the namespace `f`, one row each for `present` entries, then `missing` rows for entries the package lacks and `twice` second rows for entries it has: with
/// `unnamed` more entries the manifest does not name. A row is 31 bytes and a name's width, so that 100,000 of them are under the cap of the manifest (4 MiB).
pub struct Crafted {
    pub present: usize,
    pub unnamed: usize,
    pub missing: usize,
    pub twice: usize,
}

impl Crafted {
    fn rows(&self) -> String {
        let row = |name: &str| format!(r#"<f:file-entry f:full-path="{name}"/>"#);
        let mut out = String::new();
        for k in 0..self.present {
            out.push_str(&row(&format!("p{k}")));
        }
        for k in 0..self.missing {
            out.push_str(&row(&format!("m{k}")));
        }
        for k in 0..self.twice {
            out.push_str(&row(&format!("p{k}")));
        }
        out
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut entries = Drawing::a4().page("").files();
        for k in 0..self.present {
            entries.push((format!("p{k}"), vec![u8::try_from(k % 251).unwrap_or(0)]));
        }
        for k in 0..self.unnamed {
            entries.push((format!("u{k}"), vec![1]));
        }
        let manifest = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<f:manifest xmlns:f=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" f:version=\"1.3\">{}</f:manifest>",
            self.rows()
        );
        entries.push(("META-INF/manifest.xml".to_owned(), manifest.into_bytes()));
        write(Some(DRAWING), &entries)
    }
}

fn report_of(bytes: &[u8]) -> (diacraft_doc::Document, ReadReport) {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("did not read: {} ({e})", e.class()))
}

// ---- 65,536 user-defined properties --------------------------------------------------------------------------------------------------

/// Every user-defined field is held, and each costs the same items and the same work as every other: the cost is linear in their number, whatever the number.
#[test]
fn sixty_five_thousand_properties_are_all_held_and_cost_the_same_each() {
    let measure = |n: usize| {
        let (doc, report) = with_meta(&properties(n)).read();
        (doc.props.custom.len(), report.work.items, report.work.elements, report.work.attributes)
    };
    let (n0, i0, e0, a0) = measure(0);
    let (n1, i1, e1, a1) = measure(1);
    let (n2, i2, e2, a2) = measure(1_000);
    let (n3, i3, e3, a3) = measure(65_536);
    assert_eq!((n0, n1, n2, n3), (0, 1, 1_000, 65_536));
    let (per_items, per_elements, per_attributes) = (i1 - i0, e1 - e0, a1 - a0);
    assert!(per_items >= 2, "a property costs the items of its own type: {per_items}");
    assert_eq!((per_elements, per_attributes), (2, 1), "an element walked twice (to find the generator, and to hold it) and an attribute once");
    assert_eq!((i2 - i0, e2 - e0, a2 - a0), (1_000 * per_items, 1_000 * per_elements, 1_000 * per_attributes));
    assert_eq!((i3 - i0, e3 - e0, a3 - a0), (65_536 * per_items, 65_536 * per_elements, 65_536 * per_attributes), "linear to the last property");
}

/// What they cost is charged before they are made: with the items one short, the read is refused as too many items, and with exactly enough it reads.
#[test]
fn the_properties_are_charged_before_they_are_made() {
    let bytes = with_meta(&properties(65_536)).bytes();
    let items = read_partial(&bytes, &Limits::default()).1.work.items;
    assert!(items >= 2 * 65_536, "the properties are in the items: {items}");
    let exact = Limits { max_items: usize::try_from(items).unwrap(), ..Limits::default() };
    assert!(read(&bytes, &exact).is_ok());
    let short = Limits { max_items: usize::try_from(items).unwrap() - 1, ..Limits::default() };
    assert_eq!(read(&bytes, &short).err().map(|e| e.class()), Some("too_many_items"));
}

/// **Against a yardstick**: the same bytes of plain content (fields of a namespace nothing reads, kept whole) read in about the time the properties do. Timed, so run on request.
#[test]
#[ignore = "times the crafted file back to back with its yardstick; run on request"]
fn the_properties_are_read_in_the_time_of_their_yardstick() {
    ratio(
        "65,536 user-defined properties against the same bytes of kept elements",
        &with_meta(&properties(65_536)).bytes(),
        &with_meta(&foreign_fields(65_536)).bytes(),
        2.0,
    );
}

// ---- settings.xml ------------------------------------------------------------------------------------------------------------------

/// A `settings.xml` of 1,000,000 items is held whole: its charge is the tree's own, in step with the number of items, and every element is walked once.
#[test]
fn a_settings_tree_of_a_million_items_is_charged_by_its_items() {
    let measure = |n: usize| {
        let bytes = drawing_with_settings(settings_of_items(n));
        let (doc, report) = report_of(&bytes);
        let held = doc.ext.get().and_then(|d| d.odf.settings.clone()).expect("held");
        let settings =
            held.children.iter().find_map(|c| if let diacraft_doc::RawNode::Element(e) = c { Some(e) } else { None }).expect("office:settings");
        (settings.children.len(), report.work.items, report.work.elements)
    };
    let (n0, i0, e0) = measure(0);
    let (n1, i1, e1) = measure(100_000);
    let (n2, i2, e2) = measure(1_000_000);
    assert_eq!((n0, n1, n2), (0, 100_000, 1_000_000));
    // An element is 3 items and a byte of name, as text at nine eighths, in whole items over the tree.
    for (n, items) in [(100_000u64, i1 - i0), (1_000_000, i2 - i0)] {
        assert!(items.abs_diff(3 * n + as_text(n) / 64) <= 8, "{items} items for {n}");
    }
    assert_eq!((e1 - e0, e2 - e0), (200_000, 2_000_000), "each element is walked twice (the cost, then the scale of the settings)");
}

/// It is charged before it is made: one item short and the read is refused, and the tree is not built.
#[test]
fn a_settings_tree_is_charged_before_it_is_made() {
    let bytes = drawing_with_settings(settings_of_items(100_000));
    let items = read_partial(&bytes, &Limits::default()).1.work.items;
    assert!(items >= 3 * 100_000, "the tree is in the items: {items}");
    assert!(read(&bytes, &Limits { max_items: usize::try_from(items).unwrap(), ..Limits::default() }).is_ok());
    let short = Limits { max_items: usize::try_from(items).unwrap() - 1, ..Limits::default() };
    assert_eq!(read(&bytes, &short).err().map(|e| e.class()), Some("too_many_items"));
}

/// A `settings.xml` past the cap of a small part (4 MiB) is an entry of the package, whatever it holds and however large: the read does the same work for one of 5 MiB and for one of
/// 64 MiB, none of it parsed, and charges its bytes to the media budget and nothing more.
#[test]
fn a_settings_part_of_64_mib_costs_its_bytes_and_no_work() {
    let measure = |bytes: usize| {
        let (doc, report) = report_of(&drawing_with_settings(settings_of_bytes(bytes)));
        let part = doc.parts.iter().find(|p| &*p.name == "settings.xml").expect("an entry");
        (part.bytes.len(), report.work.items, report.work.elements, report.work.media_bytes, report.work.small_part_bytes)
    };
    let (len1, items1, elements1, media1, small1) = measure(5 << 20);
    let (len2, items2, elements2, media2, small2) = measure(64 << 20);
    assert!(len1 >= 5 << 20 && len2 >= 64 << 20);
    assert_eq!(
        (items1, elements1, small1, small2),
        (items2, elements2, 0, 0),
        "the work is the same for 5 MiB and for 64 MiB: none is done on the part"
    );
    assert_eq!((media1, media2), (len1 as u64, len2 as u64), "the bytes are the media budget's");
}

/// **Against a yardstick**: a `settings.xml` of 1,000,000 items against the same bytes as kept foreign elements, and one of 64 MiB against a font of 64 MiB. Timed.
#[test]
#[ignore = "times the crafted files back to back with their yardsticks; run on request"]
fn the_settings_are_read_in_the_time_of_their_yardsticks() {
    let items = drawing_with_settings(settings_of_items(1_000_000));
    // The yardstick of a tree is the same tree where the reader keeps it whole already: the same elements as markup of a shape it does not model.
    let plain = {
        let shape = format!("<draw:rect svg:width=\"1cm\" svg:height=\"1cm\">{}</draw:rect>", "<x:a/>".repeat(1_000_000));
        let mut d = Drawing::a4().page(&shape);
        d.ns = d.ns.replace("xmlns:diacraft", "xmlns:x=\"urn:example:extension\" xmlns:diacraft");
        d.bytes()
    };
    ratio("1,000,000 settings items against 1,000,000 kept elements", &items, &plain, 3.0);
    let big = drawing_with_settings(settings_of_bytes(64 << 20));
    let mut d = Drawing::a4().page("");
    d.files.push(("Fonts/big.ttf".to_owned(), settings_of_bytes(64 << 20).into_bytes()));
    ratio("a settings part of 64 MiB against a font of 64 MiB", &big, &d.bytes(), 1.5);
}

// ---- the manifest --------------------------------------------------------------------------------------------------------------------------

/// A manifest of 100,000 rows: 40,000 name entries the package has, 40,000 name entries it lacks, 20,000 are second rows for entries it has, and 9,990 entries are not named at all.
/// Every entry is a part, the rows the parts took are on them, every other row is held by name, and the work grows with the rows.
#[test]
fn a_manifest_of_100000_rows_holds_every_row_and_every_entry() {
    let c = Crafted { present: 40_000, unnamed: 9_980, missing: 40_000, twice: 20_000 };
    let (doc, report) = report_of(&c.bytes());
    assert_eq!(doc.parts.len(), 40_000 + 9_980);
    let odf = &doc.ext.get().expect("data").odf;
    // 40,000 rows were taken by parts; the other 60,000 are held by name: the second rows and those of the entries the package lacks.
    assert_eq!(odf.manifest_rows.len(), 20_000 + 40_000);
    assert_eq!(
        report.warned("manifest_incomplete"),
        200 + 1,
        "the entries the manifest does not name (the four XML parts and 9,980 more): 200 named, the rest counted in one more"
    );
    assert_eq!(report.warned("manifest_lists_missing_file"), 200 + 1);
    // Linear: half the rows cost half.
    let half = Crafted { present: 20_000, unnamed: 4_990, missing: 20_000, twice: 10_000 };
    let (hdoc, hreport) = report_of(&half.bytes());
    assert_eq!(hdoc.parts.len(), 20_000 + 4_990);
    let (items, half_items) = (report.work.items, hreport.work.items);
    let base = report_of(&Crafted { present: 0, unnamed: 0, missing: 0, twice: 0 }.bytes()).1.work.items;
    assert!(
        (items - base).abs_diff(2 * (half_items - base)) <= (items - base) / 100,
        "{items} items for 100,000 rows and {half_items} for 50,000 (base {base})"
    );
}

/// **Against a yardstick**: a manifest of 100,000 rows with its entries against the same entries with a manifest that names each once. Timed.
#[test]
#[ignore = "times the crafted files back to back with their yardsticks; run on request"]
fn the_manifest_is_read_in_the_time_of_its_yardstick() {
    let crafted = Crafted { present: 40_000, unnamed: 9_980, missing: 40_000, twice: 20_000 };
    let plain = Crafted { present: 49_980, unnamed: 0, missing: 0, twice: 0 };
    ratio("a manifest of 100,000 rows against one that names each entry once", &crafted.bytes(), &plain.bytes(), 3.0);
}

// ---- the font faces ------------------------------------------------------------------------------------------------------------------

/// 65,536 faces are held, each once however many parts declare it, and cost the same items and the same work each; one more is refused, in words, and nothing is left out.
#[test]
fn sixty_five_thousand_faces_are_held_and_one_more_is_refused() {
    let measure = |n: usize| {
        let (doc, report) = with_faces(n).read();
        (doc.fonts.len(), report.work.items, report.work.elements, report.work.attributes)
    };
    let (n0, i0, e0, a0) = measure(0);
    let (n1, i1, e1, a1) = measure(1);
    let (n3, i3, e3, a3) = measure(65_536);
    assert_eq!((n0, n1, n3), (0, 1, 65_536));
    let (per_items, per_elements, per_attributes) = (i1 - i0, e1 - e0, a1 - a0);
    assert_eq!((i3 - i0, e3 - e0, a3 - a0), (65_536 * per_items, 65_536 * per_elements, 65_536 * per_attributes), "linear to the last face");
    // The work of a face is the two parts' walks of it, the hash and the find of the equal one in the other part: bounded per face and no more.
    assert!(per_elements <= 8 && per_attributes <= 16, "{per_elements} elements and {per_attributes} attributes a face");
    let refused = read(&with_faces(65_537).bytes(), &Limits::default());
    match refused {
        Err(e) => assert_eq!((e.class(), e.to_string()), ("xml_too_complex", "the drawing holds more than 65536 font faces".to_owned())),
        Ok(_) => panic!("a face past the cap was left out"),
    }
}

/// A declaration repeated hundreds of thousands of times is one face and costs the work of walking each: the items the reader charges for each declaration it reads and no more, and
/// the work in step with the repeats.
#[test]
fn one_declaration_many_times_is_one_face_and_linear_work() {
    let one = r#"<style:font-face style:name="F" svg:font-family="F" style:font-pitch="fixed"/>"#;
    let measure = |n: usize| {
        let mut d = Drawing::a4().page("");
        d.fonts = one.repeat(n);
        let (doc, report) = d.read();
        (doc.fonts.len(), report.work.items, report.work.elements)
    };
    let (n1, i1, e1) = measure(1);
    let (n2, i2, e2) = measure(100_000);
    let (n3, i3, e3) = measure(400_000);
    assert_eq!((n1, n2, n3), (1, 1, 1));
    // Items: the reader's own charge for each declaration it reads (the name and the family), and the one face's.
    assert!(i3 > i2 && i2 > i1);
    let (per2, per3) = ((i2 - i1) as f64 / 99_999.0, (i3 - i1) as f64 / 399_999.0);
    assert!((per2 - per3).abs() < 0.01, "{per2} and {per3} items a repeat");
    let (we2, we3) = ((e2 - e1) as f64 / 99_999.0, (e3 - e1) as f64 / 399_999.0);
    assert!((we2 - we3).abs() < 0.01, "{we2} and {we3} elements walked a repeat");
}

/// **Against a yardstick**: 65,536 faces against 65,536 definitions of the same size that the reader holds nothing of. Timed.
#[test]
#[ignore = "times the crafted files back to back with their yardsticks; run on request"]
fn the_faces_are_read_in_the_time_of_their_yardstick() {
    // The faces are declared in both parts, so the markers are too: the same bytes are parsed.
    let plain = {
        let mut d = Drawing::a4().page("");
        let markers: String = (0..65_536).map(|k| format!(r#"<draw:marker draw:name="f{k:05}" svg:viewBox="0 0 1 1" svg:d="M 0 0"/>"#)).collect();
        d.common.clone_from(&markers);
        d.auto = markers;
        d.bytes()
    };
    ratio(
        "65,536 font faces (declared in both parts) against 65,536 markers in each part of about the same bytes",
        &with_faces(65_536).bytes(),
        &plain,
        3.0,
    );
}

// ---- the packages' entries ------------------------------------------------------------------------------------------------------------

/// An entry nothing reads costs its bytes and a fixed work: the same for a part of 1 KiB and one of 100 MB (not parsed, not scanned), and what it costs the model is its charge to the
/// media budget, once, whatever the number of the entries that share the allocation.
#[test]
fn an_entry_costs_its_bytes_and_a_fixed_work() {
    let measure = |len: usize| {
        let mut d = Drawing::a4().page("");
        d.files.push(("Fonts/a.ttf".to_owned(), pattern(len)));
        let (doc, report) = d.read();
        let part = doc.parts.iter().find(|p| &*p.name == "Fonts/a.ttf").expect("kept");
        (part.bytes.len(), report.work.items, report.work.elements, report.work.media_bytes)
    };
    let (l1, i1, e1, m1) = measure(1 << 10);
    let (l2, i2, e2, m2) = measure(100 << 20);
    assert_eq!((l1, l2), (1 << 10, 100 << 20));
    assert_eq!((i1, e1), (i2, e2), "the same items and the same work, whatever the size");
    assert_eq!((m1, m2), (1 << 10, 100 << 20));
}

/// Bytes that compress by the ratio of a real file and not the format's limit: a pattern of 251 bytes repeated.
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|k| u8::try_from(k % 251).unwrap_or(0)).collect()
}

// ---- timing ----------------------------------------------------------------------------------------------------------------------------

/// The time `bytes` take to read, the least of three runs (a run is a read and a drop).
fn time_of(bytes: &[u8]) -> f64 {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let result = read(bytes, &Limits::default());
            assert!(result.is_ok(), "the crafted file did not read");
            drop(result);
            start.elapsed().as_secs_f64()
        })
        .fold(f64::MAX, f64::min)
}

/// Reads `crafted` and its `yardstick` back to back and holds the ratio of their times to `allowance`.
fn ratio(what: &str, crafted: &[u8], yardstick: &[u8], allowance: f64) {
    let (a, b) = (time_of(yardstick), time_of(crafted));
    println!("{what}: {b:.3} s against {a:.3} s, {:.2} times (the allowance is {allowance})", b / a);
    assert!(b <= a * allowance, "{what}: {b:.3} s against {a:.3} s");
}
