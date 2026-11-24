//! The element-level comparison (design, "Order of work" step 3): what it finds, by path, and what it tells apart. An echo passes every
//! test of an unedited save, so each test here plants one change of a known kind and asks for exactly that change and nothing else.

use diacraft_odg::{DiffCounts, ElementChange, Limits, PackageDiff, PartDiff, diff_packages};

use crate::build::{DRAWING, content, drawing, files, package, page, replace, shape, shapes, with_manifest, write};

const PAGE2: &str = "/office:document-content/office:body/office:drawing/draw:page[2]";

fn diff(a: &[u8], b: &[u8]) -> PackageDiff {
    diff_packages(a, b, &Limits::default()).unwrap()
}

/// Two pages, the second of `n` shapes.
fn body(n: usize) -> String {
    page("p1", &shapes(2)) + &page("p2", &shapes(n))
}

/// The one part that differs, with nothing differing outside it.
fn only_part(d: &PackageDiff) -> &PartDiff {
    assert_eq!(d.parts.len(), 1, "{d:#?}");
    assert!(d.bytes_only.is_empty() && d.unparsed.is_empty() && d.added.is_empty() && d.removed.is_empty(), "{d:#?}");
    assert!(d.methods_changed.is_empty() && !d.order_changed, "{d:#?}");
    &d.parts[0]
}

#[test]
fn diff_of_identical_packages_is_empty() {
    let a = drawing(&body(8));
    let d = diff(&a, &a);
    assert!(d.is_empty(), "{d:#?}");
    assert_eq!(d.identical, 6);
    // Two packages built apart from the same parts are the same bytes, and the same again.
    assert!(diff(&a, &drawing(&body(8))).is_empty());
}

#[test]
fn diff_names_the_changed_shape_by_path() {
    // Two hand-built packages that differ in one `svg:x`: the diff holds exactly that element's path and attribute.
    let a = drawing(&body(8));
    let edited = body(8).replace(r#"draw:name="s7" svg:x="7cm""#, r#"draw:name="s7" svg:x="7.5cm""#);
    assert_ne!(edited, body(8), "the edit is made");
    let b = drawing(&edited);
    let d = diff(&a, &b);
    assert_eq!(d.identical, 5, "every part but content.xml");
    let part = only_part(&d);
    assert_eq!(part.part, "content.xml");
    assert_eq!(
        part.changed,
        [ElementChange {
            path: format!("{PAGE2}/draw:custom-shape[7]"),
            label: format!("{PAGE2}/draw:custom-shape[7]"),
            attributes: vec!["svg:x".to_owned()],
            attribute_labels: vec!["svg:x"],
            attribute_count: 1,
            reordered: false,
            text: false,
        }]
    );
    assert!(part.added.is_empty() && part.removed.is_empty() && part.moved.is_empty() && part.whitespace.is_empty(), "{part:#?}");
    assert_eq!(part.counts, DiffCounts { changed: 1, ..DiffCounts::default() });
    // The same edit on the first page's only shape of its name is named without a place where the name is not shared.
    let one = |n: usize| page("p1", &shape("only", &format!("{n}cm")));
    let part = diff(&drawing(&one(1)), &drawing(&one(2))).parts.remove(0);
    assert_eq!(part.changed[0].path, "/office:document-content/office:body/office:drawing/draw:page/draw:custom-shape");
}

#[test]
fn diff_tells_whitespace_only_changes_apart() {
    // The same drawing, indented with tabs: white space alone, listed apart, and nothing changed.
    let a = drawing(&body(8));
    let tabs = body(8).replace("\n    <draw:custom-shape", "\n\t\t<draw:custom-shape");
    let d = diff(&a, &drawing(&tabs));
    let part = only_part(&d);
    assert!(part.changed.is_empty() && part.added.is_empty() && part.removed.is_empty() && part.moved.is_empty(), "{part:#?}");
    assert_eq!(part.whitespace, ["/office:document-content/office:body/office:drawing/draw:page[1]".to_owned(), PAGE2.to_owned()]);
    // White space and one real change together: the change is the one change, and the white space is listed on its own.
    let both = tabs.replace(r#"svg:x="3cm""#, r#"svg:x="3.25cm""#);
    let part = diff(&a, &drawing(&both)).parts.remove(0);
    assert_eq!(part.changed.len(), 1, "{part:#?}");
    assert_eq!(
        (part.changed[0].path.as_str(), part.changed[0].attributes.as_slice()),
        (&*format!("{PAGE2}/draw:custom-shape[3]"), &["svg:x".to_owned()][..])
    );
    assert_eq!(part.whitespace.len(), 2);
    // White space inside text that holds more than white space is a change of text, not white space.
    let spaced = body(8).replacen("<text:p/>", "<text:p>a b</text:p>", 1);
    let wider = body(8).replacen("<text:p/>", "<text:p>a  b</text:p>", 1);
    let part = diff(&drawing(&spaced), &drawing(&wider)).parts.remove(0);
    assert_eq!(part.changed.len(), 1);
    assert!(part.changed[0].text && part.changed[0].path.ends_with("draw:page[1]/draw:custom-shape[1]/text:p"), "{part:#?}");
    assert!(part.whitespace.is_empty());
    // The same white space moved from before the first of two equal children to between them: white space alone, though its text is the
    // same; nothing changed.
    let rect = "<draw:rect svg:x=\"1cm\"/>";
    let before = drawing(&page("p1", &format!("  {rect}{rect}")));
    let between = drawing(&page("p1", &format!("{rect}  {rect}")));
    let part = diff(&before, &between).parts.remove(0);
    assert_eq!(part.counts, DiffCounts { whitespace: 1, ..DiffCounts::default() }, "{part:#?}");
    assert_eq!(part.whitespace, ["/office:document-content/office:body/office:drawing/draw:page".to_owned()]);
}

#[test]
fn a_moved_shape_is_named_with_both_its_paths() {
    // The second shape moved after the fifth: one move, from place 2 to place 5, and the shapes it passed are not moved.
    let n = 8;
    let all: Vec<String> = (1..=n).map(|k| shape(&format!("s{k}"), &format!("{k}cm"))).collect();
    let mut moved = all.clone();
    let second = moved.remove(1);
    moved.insert(4, second);
    let a = drawing(&(page("p1", "") + &page("p2", &all.concat())));
    let b = drawing(&(page("p1", "") + &page("p2", &moved.concat())));
    let part = only_part(&diff(&a, &b)).clone();
    assert_eq!(part.moved.len(), 1);
    assert_eq!(part.moved[0].from, format!("{PAGE2}/draw:custom-shape[2]"));
    assert_eq!(part.moved[0].to, format!("{PAGE2}/draw:custom-shape[5]"));
    assert_eq!(part.moved[0].to.label, format!("{PAGE2}/draw:custom-shape[5]"), "every name of the path is the format's");
    assert!(part.changed.is_empty() && part.added.is_empty() && part.removed.is_empty() && part.whitespace.is_empty(), "{part:#?}");
    // A shape moved past one of another name keeps its own place among its own name, and is still a move: the order of the page changed.
    let rect = "\n    <draw:rect draw:name=\"r\" svg:x=\"0cm\" svg:y=\"0cm\" svg:width=\"1cm\" svg:height=\"1cm\"/>";
    let a = drawing(&page("p1", &(shape("s", "1cm") + rect)));
    let b = drawing(&page("p1", &(rect.to_owned() + &shape("s", "1cm"))));
    let part = diff(&a, &b).parts.remove(0);
    assert_eq!(part.counts.moved, 1, "{part:#?}");
    assert_eq!(part.counts.changed + part.counts.whitespace + part.counts.added + part.counts.removed, 0, "{part:#?}");
}

#[test]
fn added_and_removed_shapes_are_named_once_with_all_they_hold() {
    let a = drawing(&page("p1", &shapes(20)));
    let mut more: Vec<String> = (1..=20).map(|k| shape(&format!("s{k}"), &format!("{k}cm"))).collect();
    more.insert(10, shape("new", "99cm"));
    let b = drawing(&page("p1", &more.concat()));
    let page1 = "/office:document-content/office:body/office:drawing/draw:page";
    let part = diff(&a, &b).parts.remove(0);
    assert_eq!(part.added, [format!("{page1}/draw:custom-shape[11]")], "its text and geometry are not listed apart");
    assert!(part.removed.is_empty() && part.changed.is_empty() && part.moved.is_empty(), "{part:#?}");
    assert_eq!(part.whitespace, [page1.to_owned()], "the indentation that came with it");
    let part = diff(&b, &a).parts.remove(0);
    assert_eq!(part.removed, [format!("{page1}/draw:custom-shape[11]")]);
    assert!(part.added.is_empty() && part.changed.is_empty());
    // A copy of a shape added next to it: the equal runs at the front and at the back may not share the shape, and one is added.
    let one = drawing(&page("p1", &shape("s", "1cm")));
    let two = drawing(&page("p1", &(shape("s", "1cm") + &shape("s", "1cm"))));
    let part = diff(&one, &two).parts.remove(0);
    assert_eq!(part.counts.added, 1, "{part:#?}");
    assert_eq!(part.added, [format!("{page1}/draw:custom-shape[2]")]);
    let part = diff(&two, &one).parts.remove(0);
    assert_eq!((part.counts.removed, part.counts.added), (1, 0), "{part:#?}");
}

#[test]
fn text_reordered_attributes_and_roots_are_told() {
    // Text inside a shape: the element that holds it changed, with `text`.
    let a = drawing(&page("p1", &shapes(1)));
    let b = drawing(&page("p1", &shapes(1).replace("<text:p/>", "<text:p>words</text:p>")));
    let part = diff(&a, &b).parts.remove(0);
    assert_eq!(part.changed.len(), 1);
    assert!(part.changed[0].path.ends_with("/draw:custom-shape/text:p") && part.changed[0].text, "{part:#?}");
    // Its own text and a child's attribute both changed: two changes, the text named on the paragraph and the attribute on the span.
    let para = |word: &str, size: &str| page("p1", &shapes(1).replace("<text:p/>", &format!("<text:p>{word}<text:span fo:x=\"{size}\"/></text:p>")));
    let part = diff(&drawing(&para("one", "1")), &drawing(&para("two", "2"))).parts.remove(0);
    assert_eq!(part.changed.len(), 2, "{part:#?}");
    assert!(part.changed[0].text && part.changed[0].path.ends_with("/text:p") && part.changed[0].attributes.is_empty());
    assert!(!part.changed[1].text && part.changed[1].path.ends_with("/text:p/text:span") && part.changed[1].attributes == ["fo:x"]);
    // A word and white space that trade places around a child: the content is interleaved otherwise, which is a change of text and not
    // white space.
    let around = |inner: &str| page("p1", &shapes(1).replace("<text:p/>", &format!("<text:p>{inner}</text:p>")));
    let part = diff(&drawing(&around("\n<text:span/>word")), &drawing(&around("word<text:span/>\n"))).parts.remove(0);
    assert_eq!(part.counts, DiffCounts { changed: 1, ..DiffCounts::default() }, "{part:#?}");
    assert!(part.changed[0].text);
    // More than 32 attributes changed: 32 are named, and all are counted.
    let many = |v: &str| page("p1", &format!("<draw:rect {}/>", (0..40).map(|i| format!("svg:a{i}=\"{v}\"")).collect::<Vec<_>>().join(" ")));
    let part = diff(&drawing(&many("1")), &drawing(&many("2"))).parts.remove(0);
    assert_eq!((part.changed[0].attributes.len(), part.changed[0].attribute_count), (32, 40));
    assert_eq!(part.changed[0].attributes[0], "svg:a0");
    // The same attributes in another order.
    let b = drawing(&page("p1", &shapes(1).replace(r#"svg:x="1cm" svg:y="1cm""#, r#"svg:y="1cm" svg:x="1cm""#)));
    let part = diff(&a, &b).parts.remove(0);
    assert!(part.changed[0].reordered && part.changed[0].attributes.is_empty() && part.changed[0].attribute_count == 0, "{part:#?}");
    // A comment moved from before a child to after it: the content is the same, interleaved otherwise, which is not white space.
    let b = drawing(&page("p1", &shapes(1).replace("<text:p/>", "<!--c--><text:p/>")));
    let c = drawing(&page("p1", &shapes(1).replace("<text:p/>", "<text:p/><!--c-->")));
    let part = diff(&b, &c).parts.remove(0);
    assert_eq!(part.counts, DiffCounts { changed: 1, ..DiffCounts::default() }, "{part:#?}");
    assert!(part.changed[0].text);
    // Another root: the root removed and another added.
    let styles_root = crate::build::styles();
    let b = write(Some(DRAWING), &with_manifest(DRAWING, replace(files(""), "content.xml", styles_root.into_bytes())));
    let part = diff(&drawing(""), &b).parts.remove(0);
    assert_eq!((part.removed[0].path.as_str(), part.added[0].path.as_str()), ("/office:document-content", "/office:document-styles"));
    assert_eq!((part.removed.len(), part.added.len()), (1, 1));
    assert_eq!(part.counts, DiffCounts { added: 1, removed: 1, ..DiffCounts::default() });
    // With no room left to list, the two are still counted.
    let limits = Limits { max_diff_entries: 0, ..Limits::default() };
    let part = diff_packages(&drawing(""), &b, &limits).unwrap().parts.remove(0);
    assert!(part.removed.is_empty() && part.added.is_empty());
    assert_eq!(part.counts, DiffCounts { added: 1, removed: 1, ..DiffCounts::default() });
}

#[test]
fn parts_that_differ_in_bytes_only_and_in_the_archive_are_told() {
    // The same tree written with single quotes: bytes only.
    let a = drawing(&page("p1", &shapes(2)));
    let quoted = content(&page("p1", &shapes(2))).replace('"', "'");
    let b = package(DRAWING, replace(files(&page("p1", &shapes(2))), "content.xml", quoted.into_bytes()));
    let d = diff(&a, &b);
    assert_eq!(d.bytes_only, ["content.xml"]);
    assert!(d.parts.is_empty());
    // A picture changed is bytes only; one added is added, and the manifest that lists it changed.
    let with_png = |png: &[u8]| {
        let mut entries = files("");
        entries.push(("Pictures/a.png".to_owned(), png.to_vec()));
        package(DRAWING, entries)
    };
    let d = diff(&with_png(b"\x89PNG 1"), &with_png(b"\x89PNG 2"));
    assert_eq!((d.bytes_only.as_slice(), d.identical), (&["Pictures/a.png".to_owned()][..], 6));
    let d = diff(&drawing(""), &with_png(b"\x89PNG"));
    assert_eq!(d.added, ["Pictures/a.png"]);
    assert_eq!(d.parts.len(), 1);
    assert_eq!(d.parts[0].part, "META-INF/manifest.xml");
    assert_eq!(d.parts[0].added, ["/manifest:manifest/manifest:file-entry[6]"]);
    let d = diff(&with_png(b"\x89PNG"), &drawing(""));
    assert_eq!(d.removed, ["Pictures/a.png"]);
    // The same bytes under another method, and the same entries in another order.
    let entries = with_manifest(DRAWING, files(""));
    let stored = {
        let mut w = diacraft_container::zip::ArchiveWriter::new();
        w.add("mimetype", DRAWING.as_bytes(), diacraft_container::zip::Method::Stored).unwrap();
        for (name, data) in &entries {
            w.add(name, data, diacraft_container::zip::Method::Stored).unwrap();
        }
        w.finish().unwrap()
    };
    let d = diff(&drawing(""), &stored);
    assert_eq!(d.methods_changed.len(), 5);
    assert_eq!(d.identical, 6);
    let mut reversed = entries.clone();
    reversed.reverse();
    let d = diff(&drawing(""), &write(Some(DRAWING), &reversed));
    assert!(d.order_changed && d.identical == 6 && d.parts.is_empty());
    // A part that no longer parses.
    let b = package(DRAWING, replace(files(""), "content.xml", b"<office:document-content".to_vec()));
    let d = diff(&drawing(""), &b);
    assert_eq!((d.unparsed.len(), d.unparsed[0].part.as_str(), d.unparsed[0].class), (1, "content.xml", "xml_malformed"));
}

#[test]
fn what_is_listed_is_capped_and_counted() {
    let a = drawing(&page("p1", &shapes(10)));
    let b = drawing(&page("p1", &shapes(10).replace("svg:y=\"1cm\"", "svg:y=\"2cm\"")));
    let limits = Limits { max_diff_entries: 3, ..Limits::default() };
    let part = diff_packages(&a, &b, &limits).unwrap().parts.remove(0);
    assert_eq!(part.changed.len(), 3);
    assert_eq!(part.counts.changed, 10);
    let all = diff(&a, &b).parts.remove(0);
    assert_eq!(all.changed.len(), 10);
    assert_eq!(&all.changed[..3], &part.changed[..], "the first three, in document order");
    // A path deeper than the cap is cut, and a name longer than its cap too.
    let long = "x".repeat(300);
    let (open, close) = (format!("<x:{long}>"), format!("</x:{long}>"));
    let deep = format!("<draw:g xmlns:x=\"urn:example:x\">{}{}</draw:g>", open.repeat(40), close.repeat(40));
    let a = drawing(&page("p1", &deep));
    let b = drawing(&page("p1", &deep.replacen(&format!("{close}{close}"), &format!("<draw:rect/>{close}{close}"), 1)));
    let part = diff(&a, &b).parts.remove(0);
    assert_eq!(part.added.len(), 1);
    let added = &part.added[0];
    assert!(added.path.len() <= 4096 + 4 && added.path.ends_with("/…"), "{}", added.path);
    assert!(added.label.ends_with("/(unlisted)/draw:rect") && added.label.len() < 1000, "the label is short and whole: {}", added.label);
    let long_name = format!("<x:{long} xmlns:x=\"urn:example:x\"/>");
    let part = diff(&drawing(&page("p1", "")), &drawing(&page("p1", &long_name))).parts.remove(0);
    assert!(part.added[0].path.ends_with('…') && part.added[0].path.len() < 220, "{}", part.added[0].path);
}

#[test]
fn labels_name_elements_by_their_namespace_and_only_from_the_vocabulary() {
    // A name of the drawing namespace that no spec records is `(unlisted)`; a recorded name under another prefix bound to the
    // drawing namespace is its own name; a recorded name under the usual prefix bound to another namespace is `(unlisted)`.
    let page1 = "/office:document-content/office:body/office:drawing/draw:page";
    let added = |inner: &str| diff(&drawing(&page("p1", "")), &drawing(&page("p1", inner))).parts.remove(0).added.remove(0);
    let password = added("<draw:MyPassword/>");
    assert_eq!((password.path.as_str(), password.label.as_str()), (&*format!("{page1}/draw:MyPassword"), &*format!("{page1}/(unlisted)")));
    // An element path is equal to its path as written, and to nothing else.
    assert!(password == format!("{page1}/draw:MyPassword") && password == &*format!("{page1}/draw:MyPassword"));
    assert!(password != format!("{page1}/(unlisted)") && password != "/elsewhere");
    let odd_prefix = added(r#"<d:rect xmlns:d="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"/>"#);
    assert_eq!((odd_prefix.path.as_str(), odd_prefix.label.as_str()), (&*format!("{page1}/d:rect"), &*format!("{page1}/draw:rect")));
    let rebound = added(r#"<draw:rect xmlns:draw="urn:example:x"/>"#);
    assert_eq!(rebound.label, format!("{page1}/(unlisted)"));
    // Attributes too: `s:x` with `s` bound to the SVG namespace is `svg:x`; a namespace declaration, and an attribute in no namespace,
    // are `(unlisted)`.
    let rect =
        |x: &str| page("p1", &format!(r#"<draw:rect xmlns:s="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" s:x="{x}" note="{x}"/>"#));
    let change = diff(&drawing(&rect("1cm")), &drawing(&rect("2cm"))).parts.remove(0).changed.remove(0);
    assert_eq!(change.attributes, ["s:x", "note"]);
    assert_eq!(change.attribute_labels, ["svg:x", "(unlisted)"]);
    assert_eq!(change.label, format!("{page1}/draw:rect"));
    let declared = |uri: &str| page("p1", &format!(r#"<draw:rect xmlns:mine="{uri}"/>"#));
    let change = diff(&drawing(&declared("urn:a")), &drawing(&declared("urn:b"))).parts.remove(0).changed.remove(0);
    assert_eq!(change.attribute_labels, ["(unlisted)"]);
}

#[test]
fn a_diff_is_empty_only_when_nothing_differs() {
    assert!(PackageDiff::default().is_empty());
    assert!(PackageDiff { identical: 9, ..PackageDiff::default() }.is_empty(), "parts that are the same are no difference");
    let one = || vec!["content.xml".to_owned()];
    let unparsed = vec![diacraft_odg::Unparsed { part: "a.xml".to_owned(), class: "xml_malformed" }];
    for (what, d) in [
        ("parts", PackageDiff { parts: vec![PartDiff::default()], ..PackageDiff::default() }),
        ("bytes_only", PackageDiff { bytes_only: one(), ..PackageDiff::default() }),
        ("unparsed", PackageDiff { unparsed, ..PackageDiff::default() }),
        ("added", PackageDiff { added: one(), ..PackageDiff::default() }),
        ("removed", PackageDiff { removed: one(), ..PackageDiff::default() }),
        ("methods_changed", PackageDiff { methods_changed: one(), ..PackageDiff::default() }),
        ("order_changed", PackageDiff { order_changed: true, ..PackageDiff::default() }),
        (
            "headers_changed",
            PackageDiff {
                headers_changed: vec![diacraft_odg::HeaderChange { name: "a".to_owned(), fields: vec!["time"] }],
                ..PackageDiff::default()
            },
        ),
    ] {
        assert!(!d.is_empty(), "{what}");
    }
    // And a comparison that found a change says it is not empty.
    assert!(!diff(&drawing(&body(2)), &drawing(&body(2).replace("\"2cm\"", "\"3cm\""))).is_empty());
}

#[test]
fn entries_whose_headers_changed_are_listed_with_what_changed() {
    // The same bytes in both, under headers that say other things: a local extra field added to one entry, and a flag set on another. R26
    // asks an edited save to keep these, and a comparison that looked at bytes alone would not see them go.
    use crate::zipbuild::{Item, build};
    let parts = with_manifest(DRAWING, files(&page("p1", &shapes(1))));
    let items = |stamp: bool| -> Vec<Item> {
        let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
        for (name, data) in &parts {
            let item = Item::deflated(name, data);
            items.push(match name.as_str() {
                "styles.xml" if stamp => item.with_local_extra(&[0x55, 0x54, 1, 0, 1]),
                "meta.xml" if stamp => item.with_flags(0x0800),
                "settings.xml" if stamp => Item { extra: vec![0x55, 0x54, 1, 0, 1], ..item },
                "content.xml" if stamp => Item { comment: b"a row's comment".to_vec(), ..item },
                _ => item,
            });
        }
        items
    };
    let (a, b) = (build(&items(false)), build(&items(true)));
    let d = diff(&a, &b);
    assert_eq!(d.identical, 6, "every part's bytes are the same");
    assert!(d.parts.is_empty() && d.bytes_only.is_empty() && d.methods_changed.is_empty());
    let changed: Vec<(&str, &[&str])> = d.headers_changed.iter().map(|h| (h.name.as_str(), h.fields.as_slice())).collect();
    let want: [(&str, &[&str]); 4] =
        [("content.xml", &["comment"]), ("styles.xml", &["local_extra"]), ("meta.xml", &["flags"]), ("settings.xml", &["extra"])];
    assert_eq!(changed, want);
    assert!(!d.is_empty());
    // A package written by another writer of the same parts: versions and dates as that writer writes them.
    let other = write(Some(DRAWING), &parts);
    let d = diff(&a, &other);
    assert!(d.headers_changed.iter().all(|h| h.fields.contains(&"version_made_by") || h.fields.contains(&"version_needed")), "{d:?}");
    assert!(diff(&a, &a).headers_changed.is_empty());
}

#[test]
fn the_data_descriptor_bit_is_the_saves_own_on_an_entry_whose_bytes_differ_and_a_change_of_header_on_one_whose_bytes_do_not() {
    // Census: a save that changes an entry writes it under headers that state its sizes, with no data descriptor, so on an entry
    // whose bytes differ the descriptor's bit (bit 3) is not a change of header; on an entry whose bytes are the same it is (a copy that gave a descriptor
    // up has rewritten the header, which is what `headers_changed` is for), and every other flag is compared on both.
    use crate::zipbuild::{Item, build};
    let parts = with_manifest(DRAWING, files(&page("p1", &shapes(1))));
    // The entry `meta.xml` is flagged with a descriptor in the first package, and not in the second, which may hold other bytes, and other flags.
    let items = |descriptor: bool, flags: u16, meta: &[u8]| -> Vec<Item> {
        let mut items = vec![Item::stored("mimetype", DRAWING.as_bytes())];
        for (name, data) in &parts {
            let data: &[u8] = if name == "meta.xml" { meta } else { data };
            let item = Item::deflated(name, data);
            items.push(match name.as_str() {
                "meta.xml" if descriptor => {
                    let mut after = 0x0807_4B50u32.to_le_bytes().to_vec();
                    after.extend_from_slice(&item.crc.to_le_bytes());
                    after.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
                    after.extend_from_slice(&(item.size as u32).to_le_bytes());
                    Item { after_data: after, ..item.with_flags(0x0008 | flags) }
                }
                "meta.xml" => item.with_flags(flags),
                _ => item,
            });
        }
        items
    };
    let old = br#"<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"/>"#.as_slice();
    let new = br#"<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"></office:document-meta>"#.as_slice();
    let header = |d: &PackageDiff| d.headers_changed.iter().map(|h| (h.name.clone(), h.fields.clone())).collect::<Vec<_>>();
    let with = build(&items(true, 0, old));
    // The same bytes without the descriptor: the header changed, and the diff says the flags.
    assert_eq!(header(&diff(&with, &build(&items(false, 0, old)))), [("meta.xml".to_owned(), vec!["flags"])]);
    // Other bytes without the descriptor: the entry was written anew by a save, and its descriptor went with its old bytes.
    let rewritten = diff(&with, &build(&items(false, 0, new)));
    assert!(rewritten.headers_changed.is_empty(), "{rewritten:?}");
    assert_eq!(rewritten.identical, 5, "every other part is the same bytes");
    assert!(rewritten.methods_changed.is_empty() && !rewritten.order_changed, "{rewritten:?}");
    // Another flag that differs on such an entry is still a change of header (here: the language encoding flag, 0x0800), and so is the bit when the
    // descriptor is the other way round: it is the bit, on either side, that is the save's.
    assert_eq!(header(&diff(&with, &build(&items(false, 0x0800, new)))), [("meta.xml".to_owned(), vec!["flags"])]);
    let gained = diff(&build(&items(false, 0, old)), &build(&items(true, 0, new)));
    assert!(gained.headers_changed.is_empty(), "{gained:?}");
    // With the descriptor on both sides and other bytes, nothing about flags changes either.
    assert!(diff(&with, &build(&items(true, 0, new))).headers_changed.is_empty());
}

#[test]
fn the_xml_compared_on_each_side_is_held_to_its_cap() {
    // Two drawings whose `content.xml` differ by one character and are as long: each side parses that one part. A cap of exactly its
    // length compares them; one byte less is `too_much_xml`.
    let edited = body(3).replace("\"3cm\"", "\"4cm\"");
    let (a, b) = (drawing(&body(3)), drawing(&edited));
    let length = content(&body(3)).len() as u64;
    assert_eq!(content(&edited).len() as u64, length);
    let at = Limits { max_xml_bytes: length, ..Limits::default() };
    assert_eq!(diff_packages(&a, &b, &at).unwrap().parts.len(), 1);
    let under = Limits { max_xml_bytes: length - 1, ..Limits::default() };
    let error = diff_packages(&a, &b, &under).unwrap_err();
    assert_eq!(error.class(), "too_much_xml", "{error}");
    // The read of a package looks through `content.xml` and `styles.xml` for macros under the same cap.
    let both = length + crate::build::styles().len() as u64;
    assert!(diacraft_odg::Package::read(&a, &Limits { max_xml_bytes: both, ..Limits::default() }).is_ok());
    let error = diacraft_odg::Package::read(&a, &Limits { max_xml_bytes: both - 1, ..Limits::default() }).unwrap_err();
    assert_eq!(error.class(), "too_much_xml");
}

#[test]
fn a_path_of_exactly_the_cap_is_whole_and_one_byte_more_is_cut() {
    // The page's path, then steps of `/x:` and a name of 128 bytes (the most a step keeps whole), then the element added, whose name is
    // sized so that its path is exactly the cap; and one byte longer.
    let most = diacraft_odg::limits::MAX_PATH_BYTES;
    let base = "/office:document-content/office:body/office:drawing/draw:page";
    let full = "a".repeat(126);
    let step = full.len() + 3;
    let steps = (most - base.len()) / step;
    let open: String = (0..steps).map(|i| if i == 0 { format!("<x:{full} xmlns:x=\"urn:example:x\">") } else { format!("<x:{full}>") }).collect();
    let close: String = (0..steps).map(|_| format!("</x:{full}>")).collect();
    let a = drawing(&page("p1", &format!("{open}{close}")));
    for extra in [0usize, 1] {
        let last = most - base.len() - steps * step - 3 + extra;
        let b = drawing(&page("p1", &format!("{open}<x:{}/>{close}", "b".repeat(last))));
        let path = diff(&a, &b).parts.remove(0).added.remove(0).path;
        if extra == 0 {
            assert_eq!(path.len(), most, "{path}");
            assert!(!path.contains('…'));
        } else {
            assert!(path.ends_with("/…") && path.len() < most, "{} bytes", path.len());
        }
    }
}
