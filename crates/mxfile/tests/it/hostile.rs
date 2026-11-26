//! Hostile pages, counted: each cap of the compressed page and of the frame gets an input built in code that sits at it and one byte past it, and the work the
//! read did is counted in the units each loop walks ([`ReadWork`]), because a fuzzer finds crashes and misses hangs. Nothing here asserts
//! a time: the `#[ignore]`d twins at the end run the same inputs beside a yardstick, by hand, on a quiet machine (`cargo test -p diacraft-mxfile -- --ignored`).

use std::time::{Duration, Instant};

use diacraft_container::inflate::CHUNK;
use diacraft_mxfile::cells::{Graph, Role};
use diacraft_mxfile::decode::{self, Fail, Step};
use diacraft_mxfile::file::{self, PageSource};
use diacraft_mxfile::limits::{MAX_FILE_BYTES, MAX_FILE_TEXT, MAX_PAGE_TEXT};
use diacraft_mxfile::{DrawioError, ErrorKind, Limits, ReadWork, Reading, Report, read, read_counted};
use diacraft_testkit::mxfile as enc;

use crate::limits;

const MIB: usize = 1 << 20;

/// A pseudo-random stream of bytes: the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// `len` bytes of ASCII that deflate at a ratio of about 8 (numbered cells, one after another), cut to exactly `len`.
fn lines(len: usize) -> Vec<u8> {
    let mut rng = Lcg(7);
    let mut out = Vec::with_capacity(len + 200);
    let mut i = 0u64;
    while out.len() < len {
        out.extend(
            format!("<mxCell id=\"c{i}\" value=\"cell {}\" style=\"rounded=1;html=1;\" vertex=\"1\" parent=\"1\"/>\n", rng.next() % 100_003).bytes(),
        );
        i += 1;
    }
    out.truncate(len);
    out
}

/// A compressed page's text for the inflated text `bytes`: raw DEFLATE at level 1 (fast: these are tens of megabytes) and Base64.
fn packed(bytes: &[u8]) -> String {
    enc::base64(&enc::deflate_raw(bytes, 1))
}

/// A file of one compressed page named `name` with the text `text`.
fn file_of(name: &str, text: &str) -> String {
    format!("<mxfile><diagram name=\"{name}\">{text}</diagram></mxfile>")
}

/// What a read of `bytes` gave: its class (or `"read"`) and the work it counted.
fn counted(bytes: &[u8], limits: &Limits) -> (&'static str, ReadWork) {
    let mut work = ReadWork::default();
    let class = match read_counted(bytes, limits, &mut work) {
        Ok(_) => "read",
        Err(e) => e.class,
    };
    (class, work)
}

#[test]
fn a_bomb_is_stopped_at_its_ratio_and_the_work_is_the_bytes_made_up_to_then() {
    // V7 of the spec at the file level: 100 MiB of one letter is a ratio of a thousand; the ratio stops it at about 20 MiB, and nothing after the first chunk past that is made.
    let stream = enc::bomb(b'A', 100 * MIB as u64);
    let file = file_of("Bomb", &enc::base64(&stream));
    let (class, work) = counted(file.as_bytes(), limits());
    assert_eq!(class, "drawio_ratio_exceeded");
    let at = 200 * stream.len() as u64;
    assert!(work.inflated > at && work.inflated <= at + CHUNK as u64, "{} against {at}", work.inflated);
    assert_eq!((work.uri_bytes, work.clean_bytes), (0, 0), "the steps after the inflate never ran");
    // The same stream at a ratio the limits allow (no ratio), runs to the cap of the page and says too large, having made one byte past it and no more.
    let no_ratio = Limits { max_ratio: u64::MAX, ..Limits::default() };
    let (class, work) = counted(file.as_bytes(), &no_ratio);
    assert_eq!(class, "drawio_too_large");
    assert_eq!(work.inflated, MAX_PAGE_TEXT + 1, "the byte past the cap is the last one made");
}

#[test]
fn a_page_of_64_mib_of_text_is_read_to_the_end_and_one_byte_more_is_too_large() {
    let at = lines(MAX_PAGE_TEXT as usize);
    let text = packed(&at);
    let mut reading = Reading::new(limits());
    // At the cap the page decodes (it is not an XML page, so it fails at the parse, step 5, which is a warning and not a refusal).
    let stage = decode::compressed_text(&text, &mut reading).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(stage.text.len(), MAX_PAGE_TEXT as usize);
    assert!(stage.not_uri_encoded, "it begins with `<`");
    assert_eq!(reading.report.work.inflated, MAX_PAGE_TEXT);
    assert_eq!(reading.report.work.base64_chars, text.len() as u64);
    assert_eq!(reading.report.work.clean_bytes, MAX_PAGE_TEXT);
    drop((stage, at));
    // One byte more: refused as too large, with the byte past the cap the last one made.
    let past = lines(MAX_PAGE_TEXT as usize + 1);
    let mut reading = Reading::new(limits());
    match decode::compressed_text(&packed(&past), &mut reading) {
        Err(Fail::Refused(e)) => assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large")),
        other => panic!("{other:?}"),
    }
    assert_eq!(reading.report.work.inflated, MAX_PAGE_TEXT + 1);
    assert_eq!((reading.report.work.uri_bytes, reading.report.work.clean_bytes), (0, 0));
}

#[test]
fn a_plain_page_of_64_mib_is_read_and_one_byte_more_is_too_large() {
    // The page is three cells with a very long value each: few items, 64 MiB of text, so the cap is held by its size and not by its item count.
    let page = |extra: usize| {
        let head = "<diagram name=\"Big\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>";
        let (mid, tail) = ("<mxCell id=\"2\" parent=\"1\" value=\"", "\"/></root></mxGraphModel></diagram>");
        let fixed = head.len() + 3 * mid.len() + tail.len() + 2 * "\"/>".len();
        let fill = MAX_PAGE_TEXT as usize + extra - fixed;
        let (a, b) = (fill / 3, fill / 3);
        let c = fill - a - b;
        format!("{head}{mid}{}\"/>{mid}{}\"/>{mid}{}{tail}", "a".repeat(a), "b".repeat(b), "c".repeat(c))
    };
    let exact = page(0);
    assert_eq!(exact.len(), MAX_PAGE_TEXT as usize);
    let file = format!("<mxfile>{exact}</mxfile>");
    let (class, work) = counted(file.as_bytes(), limits());
    assert_eq!(class, "read");
    assert_eq!(work.pages, 1);
    drop((exact, file));
    let past = format!("<mxfile>{}</mxfile>", page(1));
    let (class, _) = counted(past.as_bytes(), limits());
    assert_eq!(class, "drawio_too_large");
}

#[test]
fn base64_of_64_mib_of_white_space_is_one_pass_and_no_allocation_in_proportion_to_it() {
    // `A`, 64 MiB of spaces and line feeds, `A`: the text is not blank, so it is a compressed page; the white space is removed as it is read, and two characters are all there is.
    let ws: String = (0..MAX_PAGE_TEXT as usize).map(|i| if i % 77 == 76 { '\n' } else { ' ' }).collect();
    let text = format!("A{ws}A");
    drop(ws);
    let mut reading = Reading::new(limits());
    match decode::compressed_text(&text, &mut reading) {
        Err(Fail::Undecodable { step: Step::Inflate, .. }) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(reading.report.work.base64_chars, text.len() as u64, "each character was charged once, before the pass");
    assert_eq!(reading.report.work.inflated, 0);
    // In a file: the page is empty, undecodable at step 2, and the file is read.
    let file = file_of("W", &text);
    drop(text);
    let mut work = ReadWork::default();
    let file = read_counted(file.as_bytes(), limits(), &mut work).unwrap();
    assert_eq!(file.report.warnings.get("drawio_page_undecodable").map(|n| n.cases.clone()), Some(vec!["W: step 2 (inflate)".to_owned()]));
    assert_eq!(work.base64_chars, MAX_PAGE_TEXT + 2);
}

#[test]
fn a_giant_base64_text_of_valid_characters_is_decoded_once_and_fails_at_the_inflate() {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rng = Lcg(99);
    let text: String = (0..48 * MIB).map(|_| char::from(ALPHABET[(rng.next() % 64) as usize])).collect();
    let mut reading = Reading::new(limits());
    let r = decode::compressed_text(&text, &mut reading);
    assert!(matches!(r, Err(Fail::Undecodable { step: Step::Inflate, .. })), "{r:?}");
    assert_eq!(reading.report.work.base64_chars, text.len() as u64);
    assert!(reading.report.work.inflated <= 64 * 1024 + 1, "noise is no stream: at most a chunk was made of it, {}", reading.report.work.inflated);
}

#[test]
fn a_uri_text_of_64_mib_of_percent_41_is_one_pass_and_a_page_of_nothing_but_fffe_is_cleaned_to_nothing() {
    // `%41` is `A`: 22,369,621 of them are 64 MiB less one byte.
    let n = (MAX_PAGE_TEXT as usize - 1) / 3;
    let inflated = "%41".repeat(n);
    assert_eq!(inflated.len(), MAX_PAGE_TEXT as usize - 1);
    let mut reading = Reading::new(limits());
    let stage = decode::decode_inflated(inflated.as_bytes(), &mut reading).unwrap();
    assert_eq!(stage.text.len(), n);
    assert!(stage.text.bytes().all(|b| b == b'A'));
    assert_eq!((reading.report.work.uri_bytes, reading.report.work.clean_bytes), (inflated.len() as u64, n as u64));
    drop((inflated, stage));
    // U+FFFE is `%EF%BF%BE`: 7 million of them are 21 MB of text that the clean takes to nothing, and the page then has no root (step 5, a warning).
    let n = 7_000_000;
    let inflated = "%EF%BF%BE".repeat(n);
    let mut reading = Reading::new(limits());
    let stage = decode::decode_inflated(inflated.as_bytes(), &mut reading).unwrap();
    assert_eq!(stage.text, "");
    assert_eq!((reading.report.work.uri_bytes, reading.report.work.clean_bytes), (inflated.len() as u64, 3 * n as u64));
    // Through the whole pipeline, at a size under the floor of the ratio (a stream of one repeated sequence passes none above it): the page is empty at step 5.
    let small = "%EF%BF%BE".repeat(100_000);
    let page = enc::base64(&enc::deflate_raw(small.as_bytes(), 9));
    let file = read(file_of("F", &page).as_bytes(), limits()).unwrap();
    assert_eq!(file.report.warnings.get("drawio_page_undecodable").map(|n| n.cases.clone()), Some(vec!["F: step 5 (XML)".to_owned()]));
}

#[test]
fn nesting_a_million_deep_is_refused_at_the_depth_cap_by_a_parse_that_made_a_few_dozen_events() {
    let deep = format!("<mxfile>{}</mxfile>", "<a>".repeat(1_000_000));
    let (class, work) = counted(deep.as_bytes(), limits());
    assert_eq!(class, "drawio_too_deep");
    assert!(work.xml_events <= 100, "{}", work.xml_events);
    // The same in a compressed page, whose elements have different names so that the stream is no bomb: 10 MB of text, refused at the 65th element.
    let names: String = (0..1_000_000).map(|i| format!("<e{i}>")).collect();
    let page = format!("<mxGraphModel>{names}</mxGraphModel>");
    let text = packed(page.as_bytes());
    let (class, work) = counted(file_of("Deep", &text).as_bytes(), limits());
    assert_eq!(class, "drawio_too_deep");
    assert!(work.xml_events <= 100, "{}", work.xml_events);
    // And one that is deep and never closed, and one whose names are long.
    let open = format!("<mxfile>{}", "<a>".repeat(200_000));
    assert_eq!(counted(open.as_bytes(), limits()).0, "drawio_too_deep");
}

#[test]
fn a_file_past_the_cap_is_refused_before_a_byte_of_it_is_looked_at() {
    let big = vec![0u8; MAX_FILE_BYTES as usize + 1];
    let (class, work) = counted(&big, limits());
    assert_eq!(class, "drawio_too_large");
    assert_eq!(work, ReadWork::default(), "no work at all");
    // A file of exactly the cap is looked at (the sniff, 64 KiB at most) and is not a drawing.
    let (class, work) = counted(&big[..MAX_FILE_BYTES as usize], limits());
    assert_eq!(class, "drawio_not_a_drawing");
    assert!(work.sniff_bytes <= 64 * 1024);
}

#[test]
fn pages_that_are_each_under_the_floor_of_the_ratio_are_held_to_the_sum_of_all_of_them() {
    // 1 MiB of one repeated letter per page is a ratio of a thousand and under the floor of the ratio, so no page is refused for it. A file of a thousand of them is a file of a
    // gigabyte of text in 40 KB: the sum of the pages is what is capped (256 MiB), and the 257th page is the one refused. Per-page caps alone would let it all through.
    let xml_of = |len: usize| {
        let (head, tail) = ("<mxGraphModel><root><mxCell id=\"0\" value=\"", "\"/></root></mxGraphModel>");
        let overhead = enc::uri_encode(&format!("{head}{tail}")).len();
        format!("{head}{}{tail}", "A".repeat(len - overhead))
    };
    let xml = xml_of(MIB);
    let inflated = enc::uri_encode(&xml);
    assert_eq!(inflated.len(), MIB, "each page inflates to exactly 1 MiB");
    let page = format!("<diagram name=\"Same\">{}</diagram>", enc::base64(&enc::deflate_raw(inflated.as_bytes(), 9)));
    assert!(page.len() < 3_000, "a page is {} bytes", page.len());
    let file = |n: usize| format!("<mxfile>{}</mxfile>", page.repeat(n));
    let pages = (MAX_FILE_TEXT as usize) / MIB;
    // The caps are held to the byte in small: 3 pages under a file cap of 3 MiB, and the fourth is the one refused, with the byte past the cap the last one made.
    let small = Limits { max_file_text: 3 * MIB as u64, ..Limits::default() };
    assert_eq!(counted(file(3).as_bytes(), &small).0, "read");
    let (class, work) = counted(file(4).as_bytes(), &small);
    assert_eq!(class, "drawio_too_large");
    assert_eq!(work.inflated, 3 * MIB as u64 + 1, "the fourth page had no room: one byte of it was made, and refused");
    // And at the real cap: 256 pages are read, and the 257th is refused. The budget of work is raised for this one: a file at the cap of its text costs about 4 steps a byte (the
    // Base64, the inflate, the URI step, the clean and the parse each walk it once), a little over a billion, and the default budget of a billion refuses it a few pages short of the
    // text cap, in words and with the work it did counted. Sets that number from the measures, and this test with it.
    let (class, work) = counted(file(pages).as_bytes(), limits());
    assert_eq!(class, "drawio_too_complex", "the default budget of work is the nearer cap of the two, for now");
    assert!(work.total() <= limits().max_read_work && work.pages < pages as u64, "{work:?}");
    let wide = Limits { max_read_work: 20_000_000_000, ..Limits::default() };
    let (class, work) = counted(file(pages).as_bytes(), &wide);
    assert_eq!((class, work.pages), ("read", pages as u64));
    assert!(work.total() > limits().max_read_work && work.total() < 5 * MAX_FILE_TEXT, "about 4 steps a byte: {}", work.total());
    let mut work = ReadWork::default();
    let e = read_counted(file(pages + 1).as_bytes(), &wide, &mut work).expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_large"));
    assert_eq!(work.inflated, MAX_FILE_TEXT + 1);
    assert!(e.words.contains("256 MiB") && e.words.contains("together"), "{}", e.words);
}

#[test]
fn a_file_of_a_thousand_pages_costs_the_sum_of_its_pages_and_no_more() {
    let page = enc::compressed_page("P", "<mxCell id=\"2\" value=\"x\" vertex=\"1\" parent=\"1\"/>");
    let one = counted(enc::mxfile(std::slice::from_ref(&page)).as_bytes(), limits()).1;
    let many = counted(enc::mxfile(&vec![page; 1_000]).as_bytes(), limits());
    assert_eq!(many.0, "read");
    assert_eq!(many.1.pages, 1_000);
    // Each page costs what the first did, in every unit that a page walks; the file's own parse is added once.
    assert_eq!(many.1.base64_chars, 1_000 * one.base64_chars);
    assert_eq!(many.1.inflated, 1_000 * one.inflated);
    assert_eq!(many.1.uri_bytes, 1_000 * one.uri_bytes);
    assert_eq!(many.1.clean_bytes, 1_000 * one.clean_bytes);
}

#[test]
fn the_items_the_depth_and_an_attribute_of_the_xml_are_each_held_to_the_cap_and_one_past_it() {
    // Items: a plain page of `k` empty elements is `k + 4` items (its `diagram` and its `name`, `mxGraphModel` and `root`).
    let page = |k: usize| format!("<mxfile><diagram name=\"p\"><mxGraphModel><root>{}</root></mxGraphModel></diagram></mxfile>", "<a/>".repeat(k));
    let at = Limits { max_page_items: 104, ..Limits::default() };
    assert_eq!(counted(page(100).as_bytes(), &at).0, "read");
    assert_eq!(counted(page(101).as_bytes(), &at).0, "drawio_too_complex");
    // The same page compressed: the cap is held to the decoded page, whose items are `k + 2` (`mxGraphModel`, `root` and the elements: no `diagram`).
    let packed = |k: usize| {
        enc::mxfile(&[format!(
            "<diagram name=\"p\">{}</diagram>",
            enc::encode(&format!("<mxGraphModel><root>{}</root></mxGraphModel>", "<a/>".repeat(k)))
        )])
    };
    let at = Limits { max_page_items: 102, ..Limits::default() };
    assert_eq!(counted(packed(100).as_bytes(), &at).0, "read");
    assert_eq!(counted(packed(101).as_bytes(), &at).0, "drawio_too_complex");
    // Depth: the root's level is the first, so `mxfile`, `diagram`, `mxGraphModel`, `root` and the elements below are the levels.
    let deep = |levels: usize| {
        format!("<mxfile><diagram><mxGraphModel><root>{}{}</root></mxGraphModel></diagram></mxfile>", "<a>".repeat(levels), "</a>".repeat(levels))
    };
    let at = Limits { max_xml_depth: 14, ..Limits::default() };
    assert_eq!(counted(deep(10).as_bytes(), &at).0, "read");
    assert_eq!(counted(deep(11).as_bytes(), &at).0, "drawio_too_deep");
    // An attribute value, at the cap and one byte past it, in a plain page and in a page that was compressed, at the real cap of 32 MiB and at a small one.
    let value = |n: usize| {
        format!(
            "<mxfile><diagram name=\"p\"><mxGraphModel><root><mxCell id=\"2\" value=\"{}\"/></root></mxGraphModel></diagram></mxfile>",
            "v".repeat(n)
        )
    };
    let at = Limits { max_attr_bytes: 1_000, ..Limits::default() };
    assert_eq!(counted(value(1_000).as_bytes(), &at).0, "read");
    assert_eq!(counted(value(1_001).as_bytes(), &at).0, "drawio_too_large");
    let long = 32 * MIB;
    assert_eq!(counted(value(long).as_bytes(), limits()).0, "read");
    assert_eq!(counted(value(long + 1).as_bytes(), limits()).0, "drawio_too_large");
}

// ------------------------------------------------------------------------------------------------------------------------------------ the cells
//
// Each cap of the file, the pages and the cells gets an input built in code that sits at it and one past it, and what the read did is counted in the units each loop walks. The
// cells of a million are the case that hung before (a walk of every cell's parents with a step cap of the page's cells is 10^12 steps on a chain): here a read of any of them
// makes `5 n` steps of cells, the passes the walk is made of, and no more.

/// A file of one plain page whose `root` holds `cells`, with no `diagram` text of its own.
fn page_of(cells: &str) -> String {
    format!("<mxfile><diagram name=\"P\"><mxGraphModel><root>{cells}</root></mxGraphModel></diagram></mxfile>")
}

/// The root cell `0` and the layer `1`, then `n - 2` shapes (so that the page has `n` cells), the id and the parent of shape `i` each made by `make`.
fn page_of_cells(n: usize, make: impl Fn(usize) -> String) -> String {
    let mut out = String::from("<mxfile><diagram name=\"P\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>");
    for i in 2..n {
        out.push_str(&make(i));
    }
    out.push_str("</root></mxGraphModel></diagram></mxfile>");
    out
}

/// The pages of `file` as the walk gives them, the report of the read and what it counted; the pages are `Err` for a refusal.
fn read_pages(file: &str, limits: &Limits) -> (Result<Vec<PageSource>, DrawioError>, Report) {
    let mut reading = Reading::new(limits);
    let mut pages = Vec::new();
    let result = file::read_sources(file.as_bytes(), &mut reading, |_, page| {
        pages.push(page);
        Ok(())
    });
    (result.map(|_| pages), reading.report)
}

/// The count of a warning class in a report.
fn count_of(report: &Report, class: &str) -> u64 {
    report.warnings.get(class).map_or(0, |n| n.count)
}

/// The ids of the children of the layer `1` of the page's graph.
fn layer_children(g: &Graph) -> usize {
    g.by_id("1").map_or(0, |layer| g.children(layer).len())
}

/// A graph holds the invariants of the tree (the fuzz target's law), and every cell is reached or left out.
fn sound(g: &Graph, limits: &Limits) {
    if let Err(why) = crate::fuzz::law::read::tree_holds(g, limits) {
        panic!("the tree is broken: {why}");
    }
}

#[test]
fn cells_a_chain_of_a_million_each_the_parent_of_the_next_is_walked_once_and_cut_at_the_depth_cap() {
    let n = 1_000_000;
    let file = page_of_cells(n, |i| {
        let parent = if i == 2 { "1".to_owned() } else { format!("c{}", i - 1) };
        format!("<mxCell id=\"c{i}\" parent=\"{parent}\" vertex=\"1\"/>")
    });
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    assert_eq!(g.len(), n);
    // The walk makes five steps of cells for each: the ids, the cycles and the root, the levels, and the lists and the places (twice).
    assert_eq!(report.work.cells, 5 * n as u64, "each pass visited each cell once, and nothing walked a cell's parents");
    // 999,998 shapes in a chain: each 200 deep, then the next begins again at the first layer.
    let shapes = n - 2;
    let cuts = (shapes - 1) / 200;
    assert_eq!(count_of(&report, "drawio_too_deep_tree"), cuts as u64);
    assert_eq!(layer_children(g), 1 + cuts);
    assert!(g.cells.iter().all(|c| c.depth <= 201));
    assert!(g.cells.iter().all(|c| c.role != Role::Dropped));
    sound(g, limits());
}

#[test]
fn cells_a_cycle_of_a_million_is_cut_at_every_cell_and_moved_to_the_layer() {
    let n = 1_000_000;
    let shapes = n - 2;
    // Shape `i` hangs from shape `i + 1`, and the last from the first.
    let file = page_of_cells(n, |i| {
        let next = if i + 1 < n { i + 1 } else { 2 };
        format!("<mxCell id=\"c{i}\" parent=\"c{next}\" vertex=\"1\"/>")
    });
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    assert_eq!(report.work.cells, 5 * n as u64);
    assert_eq!(count_of(&report, "drawio_parent_cycle"), shapes as u64, "every cell of the cycle is cut, and counted once");
    assert_eq!(count_of(&report, "drawio_parent_missing"), 0);
    assert_eq!(layer_children(g), shapes, "and every one is in the first layer, none below another");
    assert!(g.cells.iter().skip(2).all(|c| c.depth == 2));
    sound(g, limits());
}

#[test]
fn cells_a_million_cells_with_one_id_keep_the_first_and_give_every_other_an_id_of_the_readers_own_in_linear_work() {
    let n = 1_000_000;
    let file = page_of_cells(n, |_| "<mxCell id=\"x\" parent=\"1\" vertex=\"1\"/>".to_owned());
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    assert_eq!(report.work.cells, 5 * n as u64);
    assert_eq!(count_of(&report, "drawio_duplicate_id"), (n - 3) as u64, "every shape but the first repeats the id");
    assert_eq!(g.cells.iter().filter(|c| c.renamed).count(), n - 3);
    assert_eq!(g.by_id("x").map(|c| c.index()), Some(2), "the first owns it");
    assert_eq!(g.by_id(&format!("x~{}", n - 2)).map(|c| c.index()), Some(n - 1), "and the last has the last number");
    sound(g, limits());
}

#[test]
fn cells_a_million_ids_made_to_collide_with_the_renamed_ones_are_probed_past_once_each() {
    // The file has `x~2` to `x~499001` of its own, and then 499,000 more cells named `x`, whose new ids must probe past all of them: with a counter that goes on, a probe is made once
    // for each id there is, 499,000 and not 499,000 times 499,000.
    let (own, repeats) = (499_000usize, 499_000usize);
    let mut cells = String::new();
    cells.push_str("<mxCell id=\"x\" parent=\"1\" vertex=\"1\"/>");
    for k in 2..own + 2 {
        cells.push_str(&format!("<mxCell id=\"x~{k}\" parent=\"1\" vertex=\"1\"/>"));
    }
    for _ in 0..repeats {
        cells.push_str("<mxCell id=\"x\" parent=\"1\" vertex=\"1\"/>");
    }
    let file = format!(
        "<mxfile><diagram name=\"P\"><mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>{cells}</root></mxGraphModel></diagram></mxfile>"
    );
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    assert_eq!(count_of(&report, "drawio_duplicate_id"), repeats as u64);
    assert_eq!(report.work.cells, 5 * g.len() as u64);
    sound(g, limits());
}

#[test]
fn cells_a_page_of_two_million_and_one_elements_is_refused_in_words_before_a_cell_is_read() {
    // The model, the root and 1,999,999 empty elements: one past the cap of the elements of a page. The cells of the root are counted after the elements, so this is what refuses it.
    let file = page_of(&"<a/>".repeat(1_999_999));
    let (result, report) = read_pages(&file, limits());
    let e = result.expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert!(e.words.contains("2000000 elements") && e.words.contains("\"P\""), "{}", e.words);
    assert_eq!(report.work.cells, 0, "no cell was read");
    // At the cap it is the cells that are too many: 1,999,998 of them against the million of a page.
    let at = page_of(&"<a/>".repeat(1_999_998));
    let e = read_pages(&at, limits()).0.expect_err("refused");
    assert_eq!(e.class, "drawio_too_complex");
    assert!(e.words.contains("1999998 cells") && e.words.contains("1000000"), "{}", e.words);
}

#[test]
fn cells_the_caps_of_the_elements_and_of_the_cells_are_held_exactly_with_small_limits() {
    // Elements: the model, the root and `k` cells.
    let few = Limits { max_page_elements: 10, ..Limits::default() };
    assert!(read_pages(&page_of(&"<mxCell/>".repeat(8)), &few).0.is_ok());
    let e = read_pages(&page_of(&"<mxCell/>".repeat(9)), &few).0.expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert!(e.words.contains("10 elements"), "{}", e.words);
    // Cells: every element of the root.
    let five = Limits { max_shapes_per_page: 5, ..Limits::default() };
    assert!(read_pages(&page_of(&"<mxCell/>".repeat(5)), &five).0.is_ok());
    let e = read_pages(&page_of(&"<mxCell/>".repeat(6)), &five).0.expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert!(e.words.contains("6 cells") && e.words.contains('5'), "{}", e.words);
    // The refusal is the whole open's, and the page is named.
    assert!(e.words.contains("\"P\""), "{}", e.words);
    // A model's own elements count: a wrapper with its inner cell and a geometry is three.
    let nested = page_of("<object id=\"a\"><mxCell vertex=\"1\" parent=\"1\"><mxGeometry as=\"geometry\"/></mxCell></object>");
    assert!(read_pages(&nested, &Limits { max_page_elements: 5, ..Limits::default() }).0.is_ok());
    assert!(read_pages(&nested, &Limits { max_page_elements: 4, ..Limits::default() }).0.is_err());
}

#[test]
fn cells_a_million_cells_exactly_are_read_and_one_more_is_refused_before_any_is() {
    let n = 1_000_000;
    let ok = page_of_cells(n, |i| format!("<mxCell id=\"c{i}\" parent=\"1\"/>"));
    let (result, report) = read_pages(&ok, limits());
    assert_eq!(result.unwrap()[0].graph.len(), n);
    assert_eq!(report.work.cells, 5 * n as u64);
    let past = page_of_cells(n + 1, |i| format!("<mxCell id=\"c{i}\" parent=\"1\"/>"));
    let (result, report) = read_pages(&past, limits());
    let e = result.expect_err("refused");
    assert_eq!((e.kind, e.class), (ErrorKind::TooBig, "drawio_too_complex"));
    assert_eq!(report.work.cells, 0, "refused by the count of the root's children, before a cell was read");
}

#[test]
fn cells_a_hundred_connectors_of_ten_thousand_and_one_waypoints_keep_the_cap_and_count_what_they_drop() {
    let points = "<mxPoint x=\"1\" y=\"2\"/>".repeat(10_001);
    let edge = |i: usize| {
        format!(
            "<mxCell id=\"e{i}\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\">{points}</Array></mxGeometry></mxCell>"
        )
    };
    let file = page_of_cells(102, edge);
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    let kept: usize = g.cells.iter().filter_map(|c| c.geometry.as_deref()).map(|geo| geo.points.len()).sum();
    assert_eq!(kept, 100 * 10_000);
    assert_eq!(count_of(&report, "drawio_too_many_points"), 100);
    sound(g, limits());
}

#[test]
fn cells_what_is_wrong_with_every_cell_is_counted_once_and_kept_to_two_hundred_cases() {
    // 300,000 cells, each with an attribute that the table does not list, a child element that is not read and four numbers that are no numbers.
    let n = 300_000;
    let file = page_of_cells(n, |i| {
        format!(
            "<mxCell id=\"c{i}\" parent=\"1\" vertex=\"1\" z{}=\"1\"><mxGeometry x=\"a\" y=\"b\" width=\"c\" height=\"d\" as=\"geometry\"/><Odd/></mxCell>",
            i % 11
        )
    });
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    sound(&pages[0].graph, limits());
    let odd = report.warnings.get("drawio_unknown_element").unwrap();
    assert_eq!((odd.count, odd.cases.len()), ((n - 2) as u64, 200), "a count for each, and the first two hundred named");
    let unknown = report.warnings.get("drawio_unknown_attribute").unwrap();
    assert_eq!(unknown.count, 1, "one warning for the page");
    assert_eq!(unknown.cases, ["P: z2, z3, z4, z5, z6, z7, z8, z9, \u{2026}"], "the first names, each once, and that there are more");
    let numbers = report.warnings.get("drawio_bad_number").unwrap();
    assert_eq!((numbers.count, numbers.cases.clone()), (1, vec![format!("P: {} numbers", 4 * (n - 2))]));
}

#[test]
fn cells_a_json_attribute_nested_a_million_deep_or_a_million_members_long_is_refused_or_capped_in_one_pass() {
    // `vars` nested a million arrays deep: refused at the 65th level, with the bytes of the text charged and nothing else walked.
    let deep = format!("{{\"a\":{}", "[".repeat(1_000_000));
    let file = format!("<mxfile vars=\"{}\"><diagram name=\"P\"/></mxfile>", enc::escape_attr(&deep));
    let (result, report) = read_pages(&file, limits());
    assert!(result.is_ok(), "the file is read, and its vars are not");
    assert_eq!(count_of(&report, "drawio_attribute_unreadable"), 1);
    assert_eq!(report.work.json_bytes, deep.len() as u64);
    // And 1,000,000 members: 320,000 are kept, the rest counted once.
    let members: String = (0..1_000_000).map(|i| format!("\"k{i}\":\"v\"")).collect::<Vec<_>>().join(",");
    // (The attribute is written in single quotes, so that the text needs no escapes: six times the bytes would pass the cap of an attribute before it passed the cap of the rows.)
    let file = format!("<mxfile vars='{{{members}}}'><diagram name=\"P\"/></mxfile>");
    let mut reading = Reading::new(limits());
    let mx = file::read_sources(file.as_bytes(), &mut reading, |_, _| Ok(())).unwrap();
    assert_eq!(mx.vars.len(), diacraft_doc::MAX_ROWS_PER_SECTION);
    assert_eq!(count_of(&reading.report, "drawio_too_many_properties"), 1);
    // Text of a string that never ends is one pass.
    let open = format!("<mxfile vars=\"{{&quot;a&quot;:&quot;{}\"><diagram name=\"P\"/></mxfile>", "x".repeat(20 << 20));
    let (result, report) = read_pages(&open, limits());
    assert!(result.is_ok());
    assert_eq!(count_of(&report, "drawio_attribute_unreadable"), 1);
}

#[test]
fn cells_a_number_that_is_a_megabyte_of_digits_is_no_number_and_costs_nothing_to_say_so() {
    let digits = "9".repeat(1 << 20);
    let file = page_of(&format!(
        "<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/><mxCell id=\"2\" parent=\"1\" vertex=\"1\"><mxGeometry x=\"{digits}\" as=\"geometry\"/></mxCell>"
    ));
    let (result, report) = read_pages(&file, limits());
    let pages = result.unwrap();
    let g = &pages[0].graph;
    assert_eq!(g.cell(g.by_id("2").unwrap()).unwrap().rect.map(|r| r.x), Some(0.0));
    assert_eq!(count_of(&report, "drawio_bad_number"), 1);
}

#[test]
fn cells_a_deep_chain_of_wrappers_and_of_xml_is_refused_or_read_without_recursing() {
    // 64 levels of XML are read and 65 are not (the container's cap), and the cells' own chain is the cap of 200, not the stack's.
    let chain: String = (2..400)
        .map(|i| {
            format!("<object id=\"w{i}\"><mxCell parent=\"{}\" vertex=\"1\"/></object>", if i == 2 { "1".to_owned() } else { format!("w{}", i - 1) })
        })
        .collect();
    let (result, report) = read_pages(&page_of(&format!("<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>{chain}")), limits());
    let pages = result.unwrap();
    sound(&pages[0].graph, limits());
    assert_eq!(count_of(&report, "drawio_too_deep_tree"), 1, "398 wrappers in a chain: 200 stay and the 201st begins again");
}

// ------------------------------------------------------------------------------------------------------------------------------------ JSON read within its caps, not after them

/// The scenario the child below runs, which its parent puts in its environment.
const JSON_SCENARIO: &str = "MXFILE_JSON_SCENARIO";

/// Members of an object that are as small as JSON lets one be: 5,000,000 of them are 30,000,000 bytes, which an attribute of 32 MiB holds.
const TINY_MEMBERS: usize = 5_000_000;

/// The peak of the address space and of the resident memory of this process, from the system when it says (Linux), for the reader of the test's output.
fn peaks() -> String {
    std::fs::read_to_string("/proc/self/status")
        .map(|s| s.lines().filter(|l| l.starts_with("VmPeak") || l.starts_with("VmHWM")).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn cells_json_child() {
    let scenario = std::env::var(JSON_SCENARIO).expect("the scenario in the environment");
    // The attribute of a `backgroundImage` also holds a `src` of 4 MiB, so it has fewer members: the attribute is 32 MiB at the most.
    let members = "\"a\":1,".repeat(if scenario == "background" { 4_000_000 } else { TINY_MEMBERS });
    match scenario.as_str() {
        // `vars` of five million members all named `a`, and one named `b` at the end: a name that is past no cap, found after all the others.
        "vars" => {
            let file = format!("<mxfile vars='{{{members}\"b\":\"last\"}}'><diagram name=\"P\"/></mxfile>");
            drop(members);
            let mut reading = Reading::new(limits());
            let mx = file::read_sources(file.as_bytes(), &mut reading, |_, _| Ok(())).unwrap();
            println!("RESULT vars {:?}", mx.vars);
            println!("RESULT warnings {}", reading.report.warnings.len());
            println!("RESULT json_bytes {}", reading.report.work.json_bytes);
        }
        // `backgroundImage` of four million members and then a `src` of 4 MiB: only the `src` is kept.
        "background" => {
            let src = "s".repeat(4 << 20);
            let file = format!(
                "<mxfile><diagram name=\"P\"><mxGraphModel backgroundImage='{{{members}\"src\":\"{src}\"}}'><root/></mxGraphModel></diagram></mxfile>"
            );
            drop(members);
            let (result, report) = read_pages(&file, limits());
            let pages = result.unwrap();
            println!("RESULT src {}", pages[0].settings.background_image_src.as_deref().map_or(0, str::len));
            println!("RESULT warnings {}", report.warnings.len());
        }
        other => panic!("no scenario {other}"),
    }
    println!("PEAKS {}", peaks());
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_json_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact hostile::cells_json_child --ignored --nocapture --test-threads=1");
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(JSON_SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// What the child printed after `RESULT <what> `. (The first line it prints follows the name of the test on the same line.)
fn result_of(stdout: &str, what: &str) -> String {
    let prefix = format!("RESULT {what} ");
    stdout.lines().find_map(|l| l.split_once(prefix.as_str()).map(|(_, rest)| rest)).unwrap_or("none").to_owned()
}

/// The address space a child of the scenarios below may use, in KiB: 342 MB. **Measured on beast, 2026-11-26** (`VmPeak`): a parser that held every member of the object before it
/// applied a cap (a member of 6 bytes is about 150 held) needed 986 MB for the `vars` of five million members and 646 MB for the `backgroundImage` of four million, and one that
/// keeps what it needs as the members go by needs 164 MB and 154 MB (the text of 30 MB three times over, and the few names kept). The limit sits between, at twice what the
/// parser needs, so that a platform whose process starts larger is no reason to loosen it.
const JSON_CHILD_LIMIT_KIB: u64 = 350_000;

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn cells_vars_of_five_million_members_are_capped_as_they_are_read_and_not_held_whole() {
    let (ended_well, stdout) = run_json_child("vars", JSON_CHILD_LIMIT_KIB);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_of(&stdout, "vars"), "[(\"a\", \"1\"), (\"b\", \"last\")]", "{stdout}");
    assert_eq!(result_of(&stdout, "warnings"), "0", "{stdout}");
    assert_eq!(
        result_of(&stdout, "json_bytes"),
        (6 * TINY_MEMBERS + "{\"b\":\"last\"}".len()).to_string(),
        "the bytes of the text, charged once: {stdout}"
    );
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn cells_a_background_image_of_four_million_members_keeps_the_src_and_holds_nothing_else() {
    let (ended_well, stdout) = run_json_child("background", JSON_CHILD_LIMIT_KIB);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_of(&stdout, "src"), (4usize << 20).to_string(), "{stdout}");
    assert_eq!(result_of(&stdout, "warnings"), "0", "{stdout}");
}

// Timed twins of the cells' cases, by hand, beside the yardstick.

#[test]
#[ignore = "timed, and about a gigabyte: run by hand on a quiet machine beside its yardstick"]
fn timed_cells_a_chain_a_cycle_and_one_id_of_a_million_cells() {
    let yard = yardstick();
    let n = 1_000_000;
    let chain = page_of_cells(n, |i| {
        let parent = if i == 2 { "1".to_owned() } else { format!("c{}", i - 1) };
        format!("<mxCell id=\"c{i}\" parent=\"{parent}\" vertex=\"1\"/>")
    });
    let cycle = page_of_cells(n, |i| format!("<mxCell id=\"c{i}\" parent=\"c{}\" vertex=\"1\"/>", if i + 1 < n { i + 1 } else { 2 }));
    let same = page_of_cells(n, |_| "<mxCell id=\"x\" parent=\"1\" vertex=\"1\"/>".to_owned());
    for (case, file) in [("a chain of a million", chain), ("a cycle of a million", cycle), ("a million of one id", same)] {
        let start = Instant::now();
        let (result, report) = read_pages(&file, limits());
        assert!(result.is_ok());
        within(case, start.elapsed(), report.work.total().max(file.len() as u64), yard);
    }
}

#[test]
#[ignore = "timed, and about a gigabyte: run by hand on a quiet machine beside its yardstick"]
fn timed_cells_two_million_elements_and_a_million_waypoints() {
    let yard = yardstick();
    let file = page_of(&"<a/>".repeat(1_999_999));
    let start = Instant::now();
    let (result, report) = read_pages(&file, limits());
    assert!(result.is_err());
    within("two million elements", start.elapsed(), report.work.total().max(file.len() as u64), yard);
    let points = "<mxPoint x=\"1\" y=\"2\"/>".repeat(10_001);
    let edge = |i: usize| {
        format!(
            "<mxCell id=\"e{i}\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><Array as=\"points\">{points}</Array></mxGeometry></mxCell>"
        )
    };
    let file = page_of_cells(102, edge);
    let start = Instant::now();
    let (result, report) = read_pages(&file, limits());
    assert!(result.is_ok());
    within("a hundred connectors of ten thousand points", start.elapsed(), report.work.total().max(file.len() as u64), yard);
}

// ------------------------------------------------------------------------------------------------------------------------------------ timed twins, by hand

/// The yardstick: a real-looking page of about 2 MiB, compressed, read whole, ten times. Gives the time per byte of text.
fn yardstick() -> f64 {
    let cells: String = (0..16_000)
        .map(|i| {
            format!(
                "<mxCell id=\"c{i}\" value=\"cell number {}\" style=\"rounded=1;whiteSpace=wrap;html=1;\" vertex=\"1\" parent=\"1\"/>",
                i * 7919 % 10_007
            )
        })
        .collect();
    let file = enc::mxfile(&[enc::compressed_page("Real", &cells)]);
    let text = enc::uri_encode(&enc::bare_model(&cells)).len() as f64;
    let start = Instant::now();
    for _ in 0..10 {
        read(file.as_bytes(), limits()).unwrap();
    }
    start.elapsed().as_secs_f64() / (10.0 * text)
}

/// A hostile input that makes `bytes` of work (the units its loops walk) is at most ten times as slow per unit as the yardstick, which also parses what it reads.
fn within(case: &str, time: Duration, bytes: u64, yard: f64) {
    let per = time.as_secs_f64() / bytes as f64;
    eprintln!("{case}: {time:?} for {bytes} units, {:.2} times the yardstick's {:.1} ns", per / yard, yard * 1e9);
    assert!(per <= 10.0 * yard, "{case}: {:.2} times the yardstick per unit", per / yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_a_bomb_a_page_at_the_cap_and_a_page_past_it() {
    let yard = yardstick();
    let bomb = enc::base64(&enc::bomb(b'A', 100 * MIB as u64));
    let start = Instant::now();
    let (class, work) = counted(file_of("Bomb", &bomb).as_bytes(), limits());
    within("a bomb stopped at its ratio", start.elapsed(), work.total(), yard);
    assert_eq!(class, "drawio_ratio_exceeded");
    let text = packed(&lines(MAX_PAGE_TEXT as usize));
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    decode::compressed_text(&text, &mut reading).unwrap();
    within("a page of 64 MiB", start.elapsed(), reading.report.work.total(), yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_white_space_noise_uri_and_fffe() {
    let yard = yardstick();
    let ws: String = (0..MAX_PAGE_TEXT as usize).map(|i| if i % 77 == 76 { '\n' } else { ' ' }).collect();
    let text = format!("A{ws}A");
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    let _ = decode::compressed_text(&text, &mut reading);
    within("64 MiB of white space", start.elapsed(), reading.report.work.total(), yard);
    let inflated = "%41".repeat((MAX_PAGE_TEXT as usize - 1) / 3);
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    decode::decode_inflated(inflated.as_bytes(), &mut reading).unwrap();
    within("64 MiB of %41", start.elapsed(), reading.report.work.total(), yard);
    let fffe = "%EF%BF%BE".repeat(7_000_000);
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    decode::decode_inflated(fffe.as_bytes(), &mut reading).unwrap();
    within("a page of U+FFFE", start.elapsed(), reading.report.work.total(), yard);
}

#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_deep_nesting_and_the_sum_of_many_pages() {
    let yard = yardstick();
    let deep = format!("<mxfile>{}</mxfile>", "<a>".repeat(1_000_000));
    let start = Instant::now();
    let (_, work) = counted(deep.as_bytes(), limits());
    within("a million levels", start.elapsed(), work.total().max(deep.len() as u64), yard);
    let page = enc::compressed_page("P", "<mxCell id=\"2\" value=\"x\" vertex=\"1\" parent=\"1\"/>");
    let file = enc::mxfile(&vec![page; 1_000]);
    let start = Instant::now();
    let (_, work) = counted(file.as_bytes(), limits());
    within("a thousand pages", start.elapsed(), work.total(), yard);
}

#[test]
#[ignore = "timed, and about a gigabyte: a plain page of six million items, run by hand with /usr/bin/time"]
fn timed_a_page_at_its_item_cap_and_one_past_it() {
    // 6,000,000 items: the `diagram` and its `name`, `mxGraphModel` and `root` are four, and the rest are empty elements.
    let page = |k: usize| format!("<mxfile><diagram name=\"p\"><mxGraphModel><root>{}</root></mxGraphModel></diagram></mxfile>", "<a/>".repeat(k));
    let yard = yardstick();
    for (k, want) in [(5_999_996, "read"), (5_999_997, "drawio_too_complex")] {
        let file = page(k);
        let start = Instant::now();
        let (class, work) = counted(file.as_bytes(), limits());
        within(&format!("{k} elements"), start.elapsed(), work.total().max(file.len() as u64), yard);
        assert_eq!(class, want);
    }
}

#[test]
#[ignore = "timed, and about 100 MB: run by hand on a quiet machine beside its yardstick"]
fn timed_cells_json_of_millions_of_tiny_members_in_vars_and_in_a_background_image() {
    let yard = yardstick();
    // `vars` of five million members all of one name, and then 1,500,000 distinct names (past the cap of the rows: each is checked, skipped and counted).
    let members = "\"a\":1,".repeat(TINY_MEMBERS);
    let file = format!("<mxfile vars='{{{members}\"b\":\"last\"}}'><diagram name=\"P\"/></mxfile>");
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    file::read_sources(file.as_bytes(), &mut reading, |_, _| Ok(())).unwrap();
    within("vars of five million members", start.elapsed(), reading.report.work.total().max(file.len() as u64), yard);
    let distinct: String = (0..1_500_000).map(|i| format!("\"{i}\":1,")).collect();
    let file = format!("<mxfile vars='{{{distinct}\"z\":1}}'><diagram name=\"P\"/></mxfile>");
    let start = Instant::now();
    let mut reading = Reading::new(limits());
    let mx = file::read_sources(file.as_bytes(), &mut reading, |_, _| Ok(())).unwrap();
    assert_eq!(mx.vars.len(), diacraft_doc::MAX_ROWS_PER_SECTION);
    within("vars of 1,500,000 distinct names", start.elapsed(), reading.report.work.total().max(file.len() as u64), yard);
    // `backgroundImage` of four million members and a `src` of 4 MiB.
    let members = "\"a\":1,".repeat(4_000_000);
    let src = "s".repeat(4 << 20);
    let file = format!(
        "<mxfile><diagram name=\"P\"><mxGraphModel backgroundImage='{{{members}\"src\":\"{src}\"}}'><root/></mxGraphModel></diagram></mxfile>"
    );
    let start = Instant::now();
    let (result, report) = read_pages(&file, limits());
    assert_eq!(result.unwrap()[0].settings.background_image_src.as_deref().map(str::len), Some(4 << 20));
    within("a background image of four million members", start.elapsed(), report.work.total().max(file.len() as u64), yard);
}
