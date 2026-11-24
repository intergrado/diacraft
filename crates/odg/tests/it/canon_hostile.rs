//! What the flat path of the content check does with a package built to make it work (done-criteria): a part root whose declarations are many or long
//! and whose children are many, containers of many attributes, and parts that are each under the cap on one part and over it together. Every bound is a count
//! of the work (`Work::canon_steps`, `Work::canon_bytes`) beside a yardstick of the same size made of plain content, and each refusal is in words.

use diacraft_container::Work;
use diacraft_container::canon::Found;
use diacraft_odg::{Limits, compare_content, compare_content_counted};

use crate::build::{DRAWING, package, replace};
use crate::drawing::Drawing;
use crate::flat::Flat;

fn text_of(files: &[(String, Vec<u8>)], name: &str) -> String {
    files.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8(b.clone()).unwrap()).unwrap()
}

/// A drawing with a face and a common style, so that no container of it is empty (the flat file of the same pieces leaves an empty one out).
fn base() -> Drawing {
    let mut d = Drawing::a4();
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d.common = r#"<style:style style:name="S" style:family="graphic"/>"#.to_owned();
    d
}

/// The declarations `xmlns:p0` to `xmlns:p{decls - 1}` (the address of `p{i}` is `{address}{i}`), as the text of attributes.
pub fn declared(decls: usize, address: &str) -> String {
    (0..decls).map(|i| format!(" xmlns:p{i}=\"{address}{i}\"")).collect()
}

/// `children` foreign children, the `i`-th using prefix `p{i % uses}`.
pub fn kids(children: usize, uses: usize) -> String {
    (0..children).map(|i| format!("<p{}:k/>", i % uses)).collect()
}

/// A package whose `content.xml` root binds `decls` prefixes and holds `children` foreign children, and the flat file of the same pieces.
pub fn declaring(decls: usize, children: usize, address: &str, uses: usize) -> (Vec<u8>, Vec<u8>) {
    let d = base();
    let files = d.files();
    let content = text_of(&files, "content.xml")
        .replacen("<office:document-content ", &format!("<office:document-content{} ", declared(decls, address)), 1)
        .replace("</office:document-content>", &format!("{}</office:document-content>", kids(children, uses)));
    let pkg = package(DRAWING, replace(files, "content.xml", content.into_bytes()));
    let mut f = Flat::of(&d);
    f.root = f.root.replacen("<office:document ", &format!("<office:document{} ", declared(decls, address)), 1);
    f.tail = kids(children, uses);
    (f.bytes(), pkg)
}

/// A package whose `content.xml` and `styles.xml` each hold `attrs` attributes on their automatic styles container, the same names and values, and the flat
/// file of the same pieces.
pub fn crowded(attrs: usize) -> (Vec<u8>, Vec<u8>) {
    let d = base();
    let files = d.files();
    let named: String = (0..attrs).map(|i| format!(" a{i}=\"{i}\"")).collect();
    let content = text_of(&files, "content.xml").replacen("<office:automatic-styles>", &format!("<office:automatic-styles{named}>"), 1);
    let styles = text_of(&files, "styles.xml").replacen("<office:automatic-styles>", &format!("<office:automatic-styles{named}>"), 1);
    let pkg = package(DRAWING, replace(replace(files, "content.xml", content.into_bytes()), "styles.xml", styles.into_bytes()));
    let mut f = Flat::of(&d);
    f.auto = f.auto.replacen("<office:automatic-styles>", &format!("<office:automatic-styles{named}>"), 1);
    (f.bytes(), pkg)
}

fn counted(flat: &[u8], pkg: &[u8]) -> (bool, Work) {
    let mut work = Work::default();
    let diff = compare_content_counted(flat, pkg, &Limits::default(), &mut work).expect("compares");
    (diff.same(), work)
}

// ------------------------------------------------------------------------------------------------------------------------------ counted

/// Measured on 20,000 declarations and 20,000 children (see the report), the bound set a little above.
const STEPS_PER_ITEM: f64 = 8.0;
const BYTES_PER_ITEM: f64 = 52.0;

#[test]
fn many_declarations_and_many_children_cost_their_count_and_not_their_product() {
    let n = 20_000;
    // Every child uses its own prefix, and each is carried the declaration of that prefix and no other.
    let (flat, pkg) = declaring(n, n, "urn:p", n);
    let (same, work) = counted(&flat, &pkg);
    assert!(same, "the flat file of the same pieces is the same content");
    let (steps, bytes) = (work.canon_steps as f64 / (2 * n) as f64, work.canon_bytes as f64 / (2 * n) as f64);
    println!("MEASURED declarations steps {steps:.3} bytes {bytes:.3} per item");
    assert!(steps <= STEPS_PER_ITEM, "{steps} steps for each declaration and child");
    assert!(bytes <= BYTES_PER_ITEM, "{bytes} bytes for each");
    // Linear: a quarter as many cost a bit less than a quarter of the work.
    let q = n / 4;
    let (flat, pkg) = declaring(q, q, "urn:p", q);
    let (_, quarter) = counted(&flat, &pkg);
    assert!(
        quarter.canon_steps as f64 * 4.0 <= work.canon_steps as f64 * 1.05 + 400.0,
        "{} for {q}, {} for {n}",
        quarter.canon_steps,
        work.canon_steps
    );
}

#[test]
fn children_that_all_use_one_prefix_among_many_declared_are_carried_that_one() {
    let n = 20_000;
    let (flat, pkg) = declaring(n, n, "urn:p", 1);
    let (same, work) = counted(&flat, &pkg);
    assert!(same);
    // One declaration is put on each child: the copies' bytes are the children's count times one declaration, not the declarations' count.
    let one = "xmlns:p0".len() + "urn:p0".len();
    let copied = work.canon_bytes as f64 / (n as f64 * one as f64);
    println!("MEASURED one prefix copied bytes {copied:.3} times (children x one declaration)");
    assert!(copied <= 6.0, "{copied} times what one copy for each child would be");
}

#[test]
fn a_container_of_many_attributes_in_both_parts_is_joined_in_a_pass() {
    let n = 20_000;
    let (flat, pkg) = crowded(n);
    let (same, work) = counted(&flat, &pkg);
    assert!(same, "two containers with the same attributes are one with them once");
    let steps = work.canon_steps as f64 / (2 * n) as f64;
    println!("MEASURED crowded steps {steps:.3} per attribute");
    assert!(steps <= STEPS_PER_ATTRIBUTE, "{steps} steps for each attribute");
    let q = n / 4;
    let (flat, pkg) = crowded(q);
    let (_, quarter) = counted(&flat, &pkg);
    assert!(
        quarter.canon_steps as f64 * 4.0 <= work.canon_steps as f64 * 1.05 + 400.0,
        "{} for {q}, {} for {n}",
        quarter.canon_steps,
        work.canon_steps
    );
}

/// Measured on 20,000 attributes in each of two containers (see the report), the bound set a little above.
const STEPS_PER_ATTRIBUTE: f64 = 7.5;

/// Measured on 20,000 automatic styles in each of two parts (72.8 steps for each style: the join, the names and the keyed sort of 40,000 siblings; 93.0 whose pass
/// weighs the container and matches the styles by their content: a step for each element of the container and the content of each style written out once), the bound set a little above.
const STEPS_PER_NAMED_STYLE: f64 = 98.0;

#[test]
fn many_automatic_styles_in_both_parts_are_joined_and_named_in_a_pass() {
    // The join gives each child of a shared container its name (a number of its element, family and name) and looks it up among the first part's, once.
    let n = 20_000;
    let styles = |prefix: &str| -> String { (0..n).map(|i| crate::canon::graphic(&format!("{prefix}{i}"), "#000000")).collect() };
    let mut d = base();
    d.styles_auto = format!("{}{}", d.styles_auto, styles("a"));
    d.auto = styles("c");
    let (flat, pkg) = (Flat::of(&d).bytes(), d.bytes());
    let (same, work) = counted(&flat, &pkg);
    assert!(same, "no name is in both parts");
    let steps = work.canon_steps as f64 / (2 * n) as f64;
    println!("MEASURED named styles steps {steps:.3} per style");
    assert!(steps <= STEPS_PER_NAMED_STYLE, "{steps} steps for each style");
    // A quarter as many cost a quarter of the work, and the same names in both parts are found as clashes in a pass too.
    let q = n / 4;
    let mut e = base();
    let quarter: String = (0..q).map(|i| crate::canon::graphic(&format!("a{i}"), "#000000")).collect();
    e.styles_auto = format!("{}{quarter}", e.styles_auto);
    e.auto = (0..q).map(|i| crate::canon::graphic(&format!("c{i}"), "#000000")).collect();
    let (_, small) = counted(&Flat::of(&e).bytes(), &e.bytes());
    assert!(small.canon_steps as f64 * 4.0 <= work.canon_steps as f64 * 1.05 + 400.0, "{} for {q}, {} for {n}", small.canon_steps, work.canon_steps);
    let mut clash = base();
    clash.styles_auto = format!("{}{}", clash.styles_auto, styles("x"));
    clash.auto = styles("x");
    let (different, found) = counted(&Flat::of(&clash).bytes(), &clash.bytes());
    assert!(!different, "the same 20,000 names in both parts are a clash");
    assert!(found.canon_steps as f64 <= work.canon_steps as f64 * 1.2, "{} against {}", found.canon_steps, work.canon_steps);
}

// ------------------------------------------------------------------------------------------------------------------------------ refused

#[test]
fn a_long_address_used_by_many_children_is_refused_before_it_is_copied() {
    // The address is 100 KB and 1,000 children use it: 100 MB of copies, where one side may add a quarter of a part's cap (here 64 KiB).
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 256 * 1024;
    let (flat, pkg) = declaring(1, 1_000, &"x".repeat(100_000), 1);
    let refused = compare_content(&flat, &pkg, &limits).map(|_| ()).map_err(|e| e.class());
    assert_eq!(refused, Err("too_much_xml"));
    // With the children few enough the same package compares.
    let (flat, pkg) = declaring(1, 0, &"x".repeat(100_000), 1);
    assert!(compare_content(&flat, &pkg, &limits).is_ok());
    // Five copies of a long address fit and the sixth does not: each copy is its text and a fixed cost, and the part's own children that carry the prefixes
    // of the vocabulary add a few hundred bytes each, which the cap leaves room for (6,000 bytes) and which are less than one copy.
    let address = "x".repeat(20_000);
    let copy = "xmlns:p0".len() + address.len() + "0".len() + 64;
    let mut small = Limits::default();
    small.container.max_part_bytes = (copy as u64 * 5 + 6_000) * 4;
    let (flat, pkg) = declaring(1, 5, &address, 1);
    assert!(compare_content(&flat, &pkg, &small).is_ok(), "five copies fit");
    let (flat, pkg) = declaring(1, 6, &address, 1);
    assert_eq!(compare_content(&flat, &pkg, &small).map(|_| ()).map_err(|e| e.class()), Err("too_much_xml"), "the sixth is past it");
}

#[test]
fn what_a_side_may_add_is_at_most_a_quarter_of_the_cap_on_a_part_and_not_a_byte_more() {
    // A picture held inline in a flat file is charged `characters / 4 * 3 + 3` bytes, and nothing else adds to the flat side: the budget is met exactly by
    // 4,000 characters (3,003) when a quarter of the cap is 3,003, and one more group of four is past it.
    let side = |characters: usize| {
        let mut f = Flat::of(&base().page(""));
        let data = "A".repeat(characters);
        f.body = f.body.replace(
            "</draw:page>",
            &format!("{}</draw:page>", crate::flat::frame(&format!("<draw:image><office:binary-data>{data}</office:binary-data></draw:image>"))),
        );
        f.bytes()
    };
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 4 * 3_003;
    let package = Flat::of(&base().page("")).bytes();
    assert!(compare_content(&side(4_000), &package, &limits).is_ok(), "3,003 bytes are the budget of 3,003");
    assert_eq!(compare_content(&side(4_004), &package, &limits).map(|_| ()).map_err(|e| e.class()), Err("too_much_xml"), "3,006 are past it");
    // A cap that is not a multiple of four gives a quarter rounded down: 12,011 is a budget of 3,002, which 3,003 passes.
    limits.container.max_part_bytes = 4 * 3_003 - 1;
    assert_eq!(compare_content(&side(4_000), &package, &limits).map(|_| ()).map_err(|e| e.class()), Err("too_much_xml"));
}

/// Two parts, each holding `items` empty elements under its page (`content.xml`) or its master styles (`styles.xml`).
fn two_big_parts(items: usize) -> Vec<u8> {
    let g = "<draw:g/>".repeat(items);
    let mut d = Drawing::a4().page(&g);
    d.masters = format!("{}{g}", d.masters);
    d.bytes()
}

#[test]
fn two_parts_each_under_the_item_limit_and_over_it_together_are_refused() {
    let mut limits = Limits::default();
    limits.container.xml.max_nodes = 1_000;
    let flat = Flat::of(&Drawing::a4()).bytes();
    // Each part alone is within 1,000 items; together they are not.
    let refused = compare_content(&flat, &two_big_parts(500), &limits).map(|_| ()).map_err(|e| e.class());
    assert_eq!(refused, Err("xml_too_complex"), "two parts of 500 items and a few more");
    assert!(compare_content(&flat, &two_big_parts(300), &limits).is_ok(), "two parts of 300 items are 600 and a few more");
    // The budget is shared, not halved: one part may hold nearly all of it.
    let d = Drawing::a4().page(&"<draw:g/>".repeat(800));
    assert!(compare_content(&flat, &d.bytes(), &limits).is_ok(), "one part of 800 items");
    // Exactly at the limit and one past it, counted as the parser counts: the package's items are what the parts hold.
    // (A comment in the first part is one of its items, so that a count that leaves out what is no element gives the later part more room than it has.)
    let items = |extra: usize| {
        let mut d = Drawing::a4().page(&"<draw:g/>".repeat(extra));
        d.masters.push_str("<!-- one item -->");
        d.bytes()
    };
    // The fixed items of the four parts of an empty drawing, as the budget counts them.
    let fixed = parts_items(&items(0));
    let mut at_limit = Limits::default();
    at_limit.container.xml.max_nodes = fixed + 50;
    assert!(compare_content(&flat, &items(50), &at_limit).is_ok(), "exactly at the budget");
    assert_eq!(compare_content(&flat, &items(51), &at_limit).map(|_| ()).map_err(|e| e.class()), Err("xml_too_complex"), "one item past it");
}

#[test]
fn a_package_that_inflates_past_half_of_what_a_comparison_holds_is_refused_by_its_stated_sizes() {
    // 200 MiB of zeros deflate to about 200 KB: the archive states its size, and a package may inflate to 192 MiB for a comparison (384 MiB for both sides).
    let d = base();
    let mut files = d.files();
    files.push(("Pictures/zeros.bin".to_owned(), vec![0u8; 200 << 20]));
    let big = package(DRAWING, files);
    assert!(big.len() < 1 << 20, "the package is small: {} bytes", big.len());
    let flat = Flat::of(&d).bytes();
    let class = |a: &[u8], b: &[u8]| compare_content(a, b, &Limits::default()).map(|_| ()).map_err(|e| e.class());
    assert_eq!(class(&big, &big), Err("package_too_large"), "two packages");
    assert_eq!(class(&big, &flat), Err("package_too_large"), "a package against a flat file");
    assert_eq!(class(&flat, &big), Err("package_too_large"), "a flat file against a package");
    // Below half of the budget a package compares.
    let mut files = d.files();
    files.push(("Pictures/zeros.bin".to_owned(), vec![0u8; 100 << 20]));
    let fits = package(DRAWING, files);
    assert_eq!(class(&fits, &fits), Ok(()), "a package of 100 MiB, twice, fits in 384 MiB");
}

#[test]
fn the_readers_item_limit_holds_a_package_and_a_flat_file_too() {
    // `max_read_xml_nodes` is the reader's cap on the items of a tree, and the flat path builds its trees under it as the reader would.
    let limits = Limits { max_read_xml_nodes: 1_000, ..Limits::default() };
    let flat = Flat::of(&Drawing::a4()).bytes();
    let class = |a: &[u8], b: &[u8]| compare_content(a, b, &limits).map(|_| ()).map_err(|e| e.class());
    assert_eq!(class(&flat, &two_big_parts(500)), Err("xml_too_complex"), "two parts of 500 items and a few more");
    assert_eq!(class(&flat, &two_big_parts(300)), Ok(()), "two parts of 300 items");
    let big_flat = Flat::of(&Drawing::a4().page(&"<draw:g/>".repeat(1_100))).bytes();
    assert_eq!(class(&big_flat, &flat), Err("xml_too_complex"), "a flat file of 1,100 items");
    assert_eq!(class(&Flat::of(&Drawing::a4().page(&"<draw:g/>".repeat(800))).bytes(), &flat), Ok(()), "a flat file of 800");
}

#[test]
fn the_xml_a_package_holds_is_capped_at_the_byte() {
    let d = base().page("");
    let (pkg, flat) = (d.bytes(), Flat::of(&d).bytes());
    let package = diacraft_odg::Package::read(&pkg, &Limits::default()).unwrap();
    let size = |name: &str| package.part(name).map_or(0, <[u8]>::len) as u64;
    let parts: u64 = ["content.xml", "styles.xml", "meta.xml", "settings.xml"].iter().map(|n| size(n)).sum();
    let with_manifest = parts + size("META-INF/manifest.xml");
    assert!(flat.len() as u64 <= parts, "a flat file of the same pieces is no larger than the parts it joins");
    let class = |a: &[u8], b: &[u8], max: u64| {
        let limits = Limits { max_xml_bytes: max, ..Limits::default() };
        compare_content(a, b, &limits).map(|_| ()).map_err(|e| e.class())
    };
    // The parts of a package against a flat file (the flat file is within the cap in each case: it is smaller than the parts).
    assert_eq!(class(&pkg, &flat, parts), Ok(()), "the four parts are the cap");
    assert_eq!(class(&pkg, &flat, parts - 1), Err("too_much_xml"), "one byte more");
    assert_eq!(class(&flat, &pkg, parts - 1), Err("too_much_xml"), "whichever side");
    // Two packages: the parts and the manifest, which are all XML.
    assert_eq!(class(&pkg, &pkg, with_manifest), Ok(()), "the parts and the manifest are the cap");
    assert_eq!(class(&pkg, &pkg, with_manifest - 1), Err("too_much_xml"), "one byte more");
    // A flat file: its own length.
    assert_eq!(class(&flat, &flat, flat.len() as u64), Ok(()), "a flat file of the cap");
    assert_eq!(class(&flat, &flat, flat.len() as u64 - 1), Err("too_much_xml"), "one byte more");
}

/// The items of the four parts of the package `bytes`, counted as the parser counts them.
fn parts_items(bytes: &[u8]) -> usize {
    use diacraft_container::xml;
    let package = diacraft_odg::Package::read(bytes, &Limits::default()).unwrap();
    ["styles.xml", "content.xml", "meta.xml", "settings.xml"]
        .iter()
        .filter_map(|n| package.part(n))
        .map(|data| {
            let root = xml::parse(data).unwrap();
            let mut items = 0usize;
            let mut stack = vec![&root];
            while let Some(el) = stack.pop() {
                items += 1 + el.attrs.len();
                for node in &el.children {
                    match node {
                        xml::Node::El(child) => stack.push(child),
                        _ => items += 1,
                    }
                }
            }
            items
        })
        .sum()
}

#[test]
fn a_declaration_carried_to_a_child_that_binds_the_prefix_itself_is_not_added() {
    // The child binds `p0` to another address itself: it keeps its own, and the root's is not put beside it (a repeated attribute is not XML).
    let d = base();
    let files = d.files();
    let content = text_of(&files, "content.xml")
        .replacen("<office:document-content ", "<office:document-content xmlns:p0=\"urn:root\" ", 1)
        .replace("</office:document-content>", "<p0:k xmlns:p0=\"urn:own\"/></office:document-content>");
    let pkg = package(DRAWING, replace(files, "content.xml", content.into_bytes()));
    let mut f = Flat::of(&d);
    f.tail = "<p0:k xmlns:p0=\"urn:own\"/>".to_owned();
    crate::canon::same(&f.bytes(), &pkg);
    // The same child in the other namespace is another element.
    let mut other = Flat::of(&d);
    other.tail = "<p0:k xmlns:p0=\"urn:root\"/>".to_owned();
    let diff = crate::canon::differs(&other.bytes(), &pkg);
    assert_eq!(diff.shown.first().map(|x| x.found), Some(Found::Element), "{:?}", diff.shown);
}

/// The least cap on a part with which the flat file of `declaring(1, children, address, 1)` compares with its package, found by halving. What one side may add
/// is a quarter of the cap (rounded down), so the least cap is four times what the copies charge, to the byte.
fn least_cap(children: usize, address: &str) -> u64 {
    let (flat, pkg) = declaring(1, children, address, 1);
    let fits = |cap: u64| {
        let mut limits = Limits::default();
        limits.container.max_part_bytes = cap;
        compare_content(&flat, &pkg, &limits).is_ok()
    };
    let (mut low, mut high) = (0u64, 8 << 20);
    assert!(fits(high) && !fits(low));
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

#[test]
fn a_copy_of_a_declaration_is_charged_its_key_its_address_and_sixty_four_bytes() {
    let address = "x".repeat(2_000);
    let (five, six) = (least_cap(5, &address), least_cap(6, &address));
    // The sixth child's copy of `xmlns:p0="{address}0"`: what one side may add grows by exactly it, a quarter of the cap at a time.
    assert_eq!(six - five, 4 * ("xmlns:p0".len() + address.len() + "0".len() + 64) as u64);
}

#[test]
fn the_children_of_a_later_container_in_the_same_context_are_given_no_declarations() {
    // Both parts bind `p` to one long address and both containers of automatic styles use it: a flat file binds it once, so the many children of the second are
    // not given a copy each (200 copies of 5,000 bytes are a million, where one side may add 64 KiB).
    let mut d = base();
    d.ns = format!("{} xmlns:p=\"urn:{}\"", d.ns, "x".repeat(5_000));
    d.styles_auto = format!("{}<p:k/>", d.styles_auto);
    d.auto = "<p:k/>".repeat(200);
    let mut limits = Limits::default();
    limits.container.max_part_bytes = 256 * 1024;
    let diff = compare_content(&Flat::of(&d).bytes(), &d.bytes(), &limits).expect("compares");
    assert!(diff.same(), "{:?}", diff.shown);
}
