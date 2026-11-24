//! Namespaces in force (`ns.rs`): what an element's prefix and default namespace mean, how a declaration shadows and ends, and that a file that
//! declares ten thousand prefixes and ten thousand shapes that each declare one costs milliseconds and not minutes.
//!
//! The first version copied every prefix in force at every element that declared one: a root with K prefixes and M shapes that each declared
//! one was 2.74 s at 10,000 each (a 51 KB file), 47 s at 40,000 (190 KB), and some eight hours at the item cap. The scope is a chain of frames
//! now, each holding its own declarations, with the answer to a lookup kept in the frame it was asked of.

use std::time::{Duration, Instant};

use diacraft_testkit::package::{NS_2011, NS_2012, NS_R, PageSpec, VsdxBuilder};
use diacraft_vsdx::{Limits, ReadReport};

use crate::{first_shape, read_ok, read_with};

/// A hang guard and no measure of cost (a cost is bounded by the reader's budgets and caps, which these tests assert): the slowest read here
/// takes a few seconds at load 50, so a minute tells a hang from a slow machine.
const BOUND: Duration = Duration::from_secs(60);

fn page(root_attrs: &str, inner: &str) -> Vec<u8> {
    let part = format!("<PageContents xmlns=\"{NS_2012}\" xmlns:r=\"{NS_R}\"{root_attrs}>{inner}</PageContents>");
    VsdxBuilder::new().page(PageSpec::new(1, "P").part(&part)).build()
}

fn ids(doc: &diacraft_doc::Document) -> Vec<u32> {
    doc.pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect()
}

fn report_of(bytes: &[u8]) -> ReadReport {
    read_ok(bytes).1
}

#[test]
fn a_default_namespace_of_another_vocabulary_ends_with_the_element_that_declares_it() {
    // The first shape is in `urn:other` (not read as a shape); its sibling is back in the VSDX namespace.
    let bytes = page("", r#"<Shapes><Shape ID="1" xmlns="urn:other"/><Shape ID="2"/></Shapes>"#);
    let (doc, report) = read_ok(&bytes);
    assert_eq!(ids(&doc), [2]);
    // The element that is in another vocabulary is held on the page's `Shapes`, where it was.
    assert_eq!(report.skipped_counts.len(), 0);
    let held = crate::held::dump(crate::held::rest(&doc.pages.get(0).unwrap().ext.get().expect("the page's slot").vsdx, "Shapes")[0]);
    assert_eq!(held, " [after 0] <n><Shape ID=\"1\" xmlns=\"urn:other\"/></n>");
}

#[test]
fn a_prefix_is_bound_by_the_nearest_declaration_above_and_not_by_one_beside() {
    // `v` is the VSDX namespace on `Shapes`, and `urn:other` on the second shape's own element: inside it, `v:Shape` is another vocabulary's.
    let inner = format!(
        r#"<Shapes xmlns:v="{NS_2012}"><v:Shape ID="1"/><Shape ID="2" xmlns:v="urn:other"><v:Shape ID="3"/></Shape><v:Shape ID="4"/></Shapes>"#
    );
    let (doc, _) = read_ok(&page("", &inner));
    assert_eq!(ids(&doc), [1, 2, 4]);
    assert_eq!(first_shape_children(&doc), 0, "the `v:Shape` inside the second shape is another vocabulary's, and it is no shape");
}

fn first_shape_children(doc: &diacraft_doc::Document) -> usize {
    doc.pages.get(0).unwrap().shapes.get(1).unwrap().children.len()
}

#[test]
fn a_prefix_nobody_bound_is_no_vocabulary_we_read_and_the_2011_namespace_is() {
    let inner = format!(r#"<Shapes><u:Shape ID="1"/><w:Shape ID="2" xmlns:w="{NS_2011}"/></Shapes>"#);
    let (doc, _) = read_ok(&page("", &inner));
    assert_eq!(ids(&doc), [2]);
}

#[test]
fn a_prefix_for_the_relationships_namespace_may_be_any_and_r_may_be_something_else() {
    // `q:id` is the relationship id when `q` is bound to the relationships namespace; `r:id` is not when `r` is bound to something else.
    let pages = format!(
        "<Pages xmlns=\"{NS_2012}\" xmlns:q=\"{NS_R}\" xmlns:r=\"urn:not-relationships\"><Page ID=\"1\" Name=\"A\"><Rel r:id=\"rId1\"/></Page><Page ID=\"2\" Name=\"B\"><Rel q:id=\"rId1\"/></Page></Pages>"
    );
    let page_part = format!("<PageContents xmlns=\"{NS_2012}\"><Shapes><Shape ID=\"5\"/></Shapes></PageContents>");
    let pkg = diacraft_testkit::package::Pkg::new()
        .part("/visio/pages/pages.xml", diacraft_testkit::package::CT_PAGES, pages)
        .rel("/visio/document.xml", "rId50", &format!("{}pages", diacraft_testkit::package::VIS), "pages/pages.xml")
        .part("/visio/pages/page1.xml", diacraft_testkit::package::CT_PAGE, page_part)
        .rel("/visio/pages/pages.xml", "rId1", &format!("{}page", diacraft_testkit::package::VIS), "page1.xml");
    let (doc, _) = read_ok(&VsdxBuilder::new().without_pages_part().with(pkg).build());
    assert_eq!(doc.pages.get(0).unwrap().shapes.len(), 0, "r:id with r bound elsewhere names no part");
    assert_eq!(doc.pages.get(1).unwrap().shapes.len(), 1, "q:id with q bound to the relationships namespace does");
}

#[test]
fn a_root_with_forty_thousand_prefixes_and_forty_thousand_shapes_that_each_declare_one_costs_milliseconds() {
    let root: String = (0..40_000).map(|i| format!(" xmlns:p{i}=\"urn:p{i}\"")).collect();
    let shapes: String = (1..=40_000).map(|i| format!("<Shape ID=\"{i}\" xmlns:q=\"urn:q\"><p{}:note/></Shape>", i % 40_000)).collect();
    let bytes = page(&root, &format!("<Shapes>{shapes}</Shapes>"));
    let started = Instant::now();
    let (doc, report) = read_with(&bytes, &Limits::default());
    let took = started.elapsed();
    eprintln!("[timing] 40,000 prefixes on the root, 40,000 shapes that declare one each ({} bytes): {took:?}", bytes.len());
    assert_eq!(ids(&doc).len(), 40_000);
    // The declarations on the root that are not the standard two are held on the page; those on a shape are its `raw`.
    assert_eq!(report.skipped_counts.len(), 0);
    let root = crate::held::rest(&doc.pages.get(0).unwrap().ext.get().expect("the page's slot").vsdx, "PageContents")[0];
    assert_eq!(root.raw.len(), 40_000);
    assert_eq!(first_shape(&doc).raw.get("xmlns:q"), Some("urn:q"));
    assert!(took < BOUND, "{took:?}");
}

#[test]
fn a_lookup_of_a_prefix_declared_far_above_is_answered_once_for_a_frame_and_not_for_each_element() {
    // A chain of 90 shapes, each declaring a prefix, and under the last 100,000 elements that use a prefix of the root: each lookup walks at most
    // the frames above, and the answer is kept, so it is not 90 steps for each of 100,000.
    let nest = |depth: usize, leaf: &str| {
        format!(
            "{}{leaf}{}",
            (0..depth).map(|d| format!("<Shape ID=\"{}\" xmlns:d{d}=\"urn:d{d}\"><Shapes>", d + 1)).collect::<String>(),
            "</Shapes></Shape>".repeat(depth)
        )
    };
    let leaf: String = "<root:note/>".repeat(100_000);
    let bytes = page(" xmlns:root=\"urn:root\"", &format!("<Shapes>{}</Shapes>", nest(90, &leaf)));
    let started = Instant::now();
    let report = report_of(&bytes);
    let took = started.elapsed();
    eprintln!("[timing] 100,000 elements under 90 nested declarations ({} bytes): {took:?}", bytes.len());
    assert_eq!(report.stats.max_shape_depth, 90);
    assert!(took < BOUND, "{took:?}");
}

#[test]
fn a_default_namespace_is_inherited_through_an_element_that_declares_only_a_prefix() {
    // The inner `Shapes` and `Shape` are in the namespace the root declared, though the element between them declared a prefix and nothing else.
    let inner = r#"<Shapes><Shape ID="1" xmlns:q="urn:q"><Shapes><Shape ID="2"/></Shapes></Shape></Shapes>"#;
    let (doc, _) = read_ok(&page("", inner));
    assert_eq!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().children.len(), 1);
}

#[test]
fn a_prefix_the_root_bound_to_the_vsdx_namespace_is_found_below_an_element_that_declared_another() {
    // Not a prefix of another vocabulary, whose absence reads the same: one whose presence reads a shape.
    let inner = r#"<Shapes><Shape ID="1" xmlns:q="urn:q"><Shapes><v:Shape ID="2"/></Shapes></Shape></Shapes>"#;
    let (doc, _) = read_ok(&page(&format!(" xmlns:v=\"{NS_2012}\""), inner));
    assert_eq!(doc.pages.get(0).unwrap().shapes.get(0).unwrap().children.len(), 1);
}
