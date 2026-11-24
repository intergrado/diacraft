//! The tables a namespace declaration makes. A frame of the scope of a part holds the prefixes its element declares, and the table of
//! open frames holds each of them again; the first version of the prefix lookup built **337 bytes for each prefix declared before any charge** (a root of
//! 5.8 million prefixes and a part of 5 million shapes was 12 million items, and aborted the process). The decoder now refuses an element that declares more than
//! [`MAX_PREFIXES_PER_ELEMENT`], charges what the frame costs before it makes it, and a frame keeps a bounded number of answers; these tests measure the
//! figures it charges with and hold every place that makes a frame to the charge.
#[cfg(target_os = "linux")]
use std::process::Command;

use diacraft_container::xml::El;
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, URN, decode_page};
use diacraft_sheetxml::ns::{FRAME_BYTES, MAX_PREFIXES_PER_ELEMENT, PREFIX_BYTES, Scope, Vocabulary, frame_bytes};

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

#[derive(Clone, Copy, Default)]
struct Any;

impl Vocabulary for Any {
    fn reads(self, _uri: Option<&str>) -> bool {
        true
    }
}

fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.trim_start_matches(':').split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0)
}

fn declaring(prefixes: &[(String, String)]) -> El {
    El { name: "x:e".to_owned(), attrs: prefixes.iter().map(|(p, u)| (format!("xmlns:{p}"), u.clone())).collect(), children: Vec::new() }
}

// ------------------------------------------------------------------------------------------------------------------------------------- measured

const CHILD: &str = "SHEETXML_SCOPES_CHILD";

/// Frames that are all open at once, each declaring `k` prefixes of names of its own, each asked about sixteen prefixes of the root (a frame keeps the first eight
/// answers): what the process grows by for each, against what the decoder charges for it.
#[test]
#[ignore = "run by the test below in a process of its own"]
fn scopes_child() {
    let spec = std::env::var(CHILD).unwrap_or_default();
    let (k, same): (usize, bool) = {
        let (k, mode) = spec.split_once(':').expect("k:mode");
        (k.parse().expect("a count"), mode == "same")
    };
    let root_prefixes: Vec<(String, String)> = (0..16).map(|j| (format!("q{j}"), format!("urn:q{j}"))).collect();
    let root = Scope::<Any>::empty().enter(&declaring(&root_prefixes));
    let per_frame = frame_bytes(k, (0..k).map(|j| 2 * format!("p{j:04}").len() as u64 + format!("urn:u{j:04}").len() as u64).sum());
    let frames = (40_000_000 / per_frame.max(1)).clamp(50, 100_000) as usize;
    let build = |m: usize| -> Vec<Scope<Any>> {
        (0..m)
            .map(|i| {
                let prefixes: Vec<(String, String)> =
                    (0..k).map(|j| (if same { format!("p{j:04}") } else { format!("p{i:06}_{j:04}") }, format!("urn:u{j:04}"))).collect();
                let sc = root.enter(&declaring(&prefixes));
                for j in 0..16 {
                    assert!(sc.uri_of_element(&El { name: format!("q{j}:x"), attrs: Vec::new(), children: Vec::new() }).is_some());
                }
                sc
            })
            .collect()
    };
    drop(build(frames.min(50)));
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak");
    let before = status_kib("VmRSS");
    let held = build(frames);
    let grew = status_kib("VmRSS").saturating_sub(before) * 1024;
    assert_eq!(held.len(), frames);
    // The names of the distinct case are longer (`p000001_0001`): charged as they are, at twice their length each.
    let charged = if same {
        per_frame
    } else {
        frame_bytes(k, (0..k).map(|j| 2 * format!("p{:06}_{j:04}", frames / 2).len() as u64 + format!("urn:u{j:04}").len() as u64).sum())
    };
    println!("SCOPES k={k} same={same} frames={frames} grew_each={} charged_each={charged}", grew / frames as u64);
}

#[test]
#[cfg(target_os = "linux")]
fn a_frame_costs_no_more_than_it_is_charged_for_the_prefixes_it_declares_and_the_answers_it_keeps() {
    let exe = std::env::current_exe().unwrap();
    let mut worst = (0.0f64, String::new());
    for k in [0usize, 1, 2, 4, 8, 64, 512, 1024] {
        for mode in ["distinct", "same"] {
            if k == 0 && mode == "same" {
                continue;
            }
            let out = Command::new(&exe)
                .args(["--exact", "model_scopes::scopes_child", "--ignored", "--nocapture", "--test-threads=1"])
                .env(CHILD, format!("{k}:{mode}"))
                .output()
                .unwrap();
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let line = text
                .lines()
                .find_map(|l| l.find("SCOPES k=").map(|at| l[at..].to_owned()))
                .unwrap_or_else(|| panic!("no measure for {k} {mode}: {text}"));
            let field =
                |key: &str| -> u64 { line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number") };
            let (grew, charged) = (field("grew_each="), field("charged_each="));
            eprintln!("{k:>5} prefixes, {mode:<8}: a frame grew by {grew} bytes, charged {charged}");
            assert!(grew <= charged, "a frame of {k} prefixes ({mode}) grew by {grew} bytes and is charged {charged}");
            let ratio = grew as f64 / charged as f64;
            if ratio > worst.0 {
                worst = (ratio, format!("{k} {mode}"));
            }
        }
    }
    // The figures are what the frames cost, with the margin a quarter: the worst case is not far below them.
    assert!(worst.0 > 0.6, "the closest a frame comes to its charge is {:.2} ({}): the figures are not measured", worst.0, worst.1);
}

// ------------------------------------------------------------------------------------------------------------------------------------- charged

fn page(inner: &str, root_decls: &str) -> El {
    let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\"{root_decls}>{inner}</diacraft:page>");
    diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

fn charged(el: &El) -> Result<u64, CodecError> {
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    decode_page(el, &MemBlobs::new(), &mut budget, &mut Vec::new())?;
    Ok(HUGE - budget.bytes)
}

fn decls(k: usize) -> String {
    (0..k).map(|j| format!(" xmlns:p{j:03}=\"urn:u{j:03}\"")).collect()
}

#[test]
fn an_element_that_declares_prefixes_is_charged_for_its_frame_and_for_each_prefix() {
    // The root: each prefix it declares is one more item (an attribute), its bytes at the figure of an attribute, and its frame's share.
    let base = charged(&page("", "")).unwrap();
    let one = charged(&page("", &decls(1))).unwrap();
    let (prefix_text, uri_text) = ("p000".len() as u64, "urn:u000".len() as u64);
    assert_eq!(
        one - base,
        32 + PREFIX_BYTES + 2 * prefix_text + uri_text,
        "a root that declares one prefix more is charged the attribute, the prefix and its bytes"
    );
    for k in 1..20 {
        let (a, b) = (charged(&page("", &decls(k))).unwrap(), charged(&page("", &decls(k + 1))).unwrap());
        assert_eq!(b - a, 32 + PREFIX_BYTES + 2 * prefix_text + uri_text, "the {}th prefix of one element", k + 1);
    }
    // A shape that declares one is a frame of its own: the frame, the prefix and the bytes, beside the shape's own.
    let plain = charged(&page("<diacraft:shape id=\"1\"/>", "")).unwrap();
    let declaring_one = charged(&page("<diacraft:shape id=\"1\" xmlns:p=\"urn:u\"/>", "")).unwrap();
    assert_eq!(declaring_one - plain, 32 + frame_bytes(1, 2 + 5));
}

#[test]
fn an_element_that_declares_only_the_default_namespace_is_charged_for_a_frame_and_no_prefix() {
    let plain = charged(&page("<diacraft:shape id=\"1\"/>", "")).unwrap();
    let default = charged(&page("<diacraft:shape id=\"1\" xmlns=\"urn:d\"/>", "")).unwrap();
    assert_eq!(default - plain, 32 + frame_bytes(0, "urn:d".len() as u64));
}

#[test]
fn an_element_that_declares_nothing_is_charged_no_frame() {
    let one = charged(&page("<diacraft:shape id=\"1\"/>", "")).unwrap();
    let two = charged(&page("<diacraft:shape id=\"1\"/><diacraft:shape id=\"2\"/>", "")).unwrap();
    let three = charged(&page("<diacraft:shape id=\"1\"/><diacraft:shape id=\"2\"/><diacraft:shape id=\"3\"/>", "")).unwrap();
    assert_eq!(three - two, two - one, "each shape the same");
    assert!(two - one < FRAME_BYTES, "and none a frame: {}", two - one);
}

#[test]
fn a_frame_nobody_can_pay_for_is_refused_in_words_by_the_budget() {
    let el = page("<diacraft:shape id=\"1\" xmlns:p=\"urn:u\"/>", "");
    let needed = charged(&el).unwrap();
    for short in [1u64, 100, FRAME_BYTES] {
        let mut budget = Budget::new(HUGE, needed - short, HUGE);
        let out = decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new());
        assert!(matches!(out, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "{short} short: {:?}", out.err());
    }
    let mut budget = Budget::new(HUGE, needed, HUGE);
    assert!(decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).is_ok(), "and exactly what it costs is enough");
}

// ------------------------------------------------------------------------------------------------------------------------------------- the cap

/// The place of an element in a part, `k` declarations on it.
fn with_declarations_at(place: &str, k: usize) -> El {
    let d = decls(k);
    match place {
        // The root declares the codec's own prefix besides.
        "root" => page("", &decls(k - 1)),
        "shape" => page(&format!("<diacraft:shape id=\"1\"{d}/>"), ""),
        "a child of a shape no one reads" => {
            page(&format!("<diacraft:shape id=\"1\"><diacraft:sheet><zz{d}/></diacraft:sheet></diacraft:shape>"), "")
        }
        "a kept element" => page(&format!("<diacraft:shape id=\"1\"><future xmlns=\"urn:f\"{d}/></diacraft:shape>"), ""),
        "an element of the data form" => {
            page(&format!("<diacraft:shape id=\"1\"><diacraft:kept><diacraft:el name=\"a\"{d}/></diacraft:kept></diacraft:shape>"), "")
        }
        "a long field" => page(&format!("<diacraft:shape id=\"1\"><diacraft:s{d} n=\"name\">x</diacraft:s></diacraft:shape>"), ""),
        other => panic!("no place {other}"),
    }
}

#[test]
fn an_element_may_declare_the_most_prefixes_and_not_one_more_in_every_place_that_makes_a_frame() {
    for place in ["root", "shape", "a child of a shape no one reads", "a kept element", "an element of the data form", "a long field"] {
        let ok = charged(&with_declarations_at(place, MAX_PREFIXES_PER_ELEMENT));
        assert!(ok.is_ok(), "{place}: {:?}", ok.err());
        let refused = charged(&with_declarations_at(place, MAX_PREFIXES_PER_ELEMENT + 1));
        assert!(
            matches!(&refused, Err(CodecError::Limit { what: "namespace prefixes declared by one element", max }) if *max == MAX_PREFIXES_PER_ELEMENT as u64),
            "{place}: {:?}",
            refused.err()
        );
    }
}

#[test]
fn the_document_part_and_the_lists_of_a_package_are_held_to_the_cap_too() {
    let xml = format!("<diacraft:document xmlns:diacraft=\"{URN}\"{}><diacraft:page-ref/></diacraft:document>", decls(MAX_PREFIXES_PER_ELEMENT + 1));
    let document = diacraft_container::xml::parse_with(xml.as_bytes(), &native_limits()).unwrap();
    let out = diacraft_sheetxml::model::part_lists(&document);
    assert!(matches!(out, Err(CodecError::Limit { what: "namespace prefixes declared by one element", .. })), "{:?}", out.err());
    let page_ref =
        format!("<diacraft:document xmlns:diacraft=\"{URN}\"><diacraft:page-ref{}/></diacraft:document>", decls(MAX_PREFIXES_PER_ELEMENT + 1));
    let document = diacraft_container::xml::parse_with(page_ref.as_bytes(), &native_limits()).unwrap();
    let out = diacraft_sheetxml::model::part_lists(&document);
    assert!(matches!(out, Err(CodecError::Limit { what: "namespace prefixes declared by one element", .. })), "{:?}", out.err());
}

// ------------------------------------------------------------------------------------------------------------------------------------- end to end

const SCENARIO: &str = "SHEETXML_SCOPES_SCENARIO";

#[test]
#[ignore = "run by the test below under `ulimit -v`; alone it needs the scenario in the environment"]
fn declarations_child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    let limit = diacraft_sheetxml::model::OPEN_ITEMS as usize;
    let (document, trees): (El, Vec<El>) = match scenario.as_str() {
        // The reviewer's case: a root that declares 5.8 million prefixes and a part of 5 million shapes, 10.8 million items that the decoder built tables for
        // before it charged any (337 bytes for each prefix), and aborted the process.
        "a_root_of_millions_of_prefixes_and_a_part_of_shapes" => {
            let n = 5_800_000;
            let mut a = format!("<page xmlns=\"{URN}\"");
            for i in 0..n {
                a.push_str(&format!(" xmlns:p{i}=\"u\""));
            }
            a.push_str("><shape/></page>");
            let ta = super::model_table::parse(&a);
            drop(a);
            let tb = super::model_table::parse(&format!("<page xmlns=\"{URN}\">{}</page>", "<shape/>".repeat(5_000_000)));
            (super::model_table::parse(&format!("<document xmlns=\"{URN}\"><page-ref/><page-ref/></document>")), vec![ta, tb])
        }
        // Shapes that each declare as many as an element may, one after another: a frame for each, and none of them given back.
        "shapes_that_each_declare_the_most_prefixes" => {
            let k = MAX_PREFIXES_PER_ELEMENT;
            let shapes: String = (0..(limit / (k + 2)).min(3_000)).map(|i| format!("<shape id=\"{i}\"{}/>", decls(k))).collect();
            // A page is as many shapes as a tree of an open may have items for (2 million), and the pool of the model (1,344 MiB) is spent by four of them.
            let page = super::model_table::parse(&format!("<page xmlns=\"{URN}\">{shapes}</page>"));
            (
                super::model_table::parse(&format!("<document xmlns=\"{URN}\">{}</document>", "<page-ref/>".repeat(4))),
                vec![page.clone(), page.clone(), page.clone(), page],
            )
        }
        // Shapes nested to the depth the reader allows, each declaring as many as an element may: the frames are all open at once.
        "nested_shapes_that_each_declare_the_most_prefixes" => {
            let (k, depth) = (MAX_PREFIXES_PER_ELEMENT, 190usize);
            let open: String = (0..depth).map(|i| format!("<shape id=\"{i}\"{}>", decls(k))).collect();
            let close = "</shape>".repeat(depth);
            (
                super::model_table::parse(&format!("<document xmlns=\"{URN}\"><page-ref/></document>")),
                vec![super::model_table::parse(&format!("<page xmlns=\"{URN}\">{open}{close}</page>"))],
            )
        }
        other => panic!("no scenario {other:?}"),
    };
    let refs: Vec<&El> = trees.iter().collect();
    let mut budget = Budget::for_open(&diacraft_sheetxml::model::open_limits());
    let out = diacraft_sheetxml::model::decode(&document, &refs, &[], &MemBlobs::new(), &mut budget, &mut Vec::new());
    println!(
        "DECL {scenario}: VmPeak {} MiB, {}",
        status_kib("VmPeak") >> 10,
        out.as_ref().err().map_or_else(|| "decoded".to_owned(), |e| format!("refused: {e}"))
    );
    match out {
        Ok(_) | Err(CodecError::Limit { .. }) => {}
        Err(other) => panic!("a refusal that is not the budget's: {other}"),
    }
    println!("declarations: no abort");
}

#[cfg(target_os = "linux")]
fn run_declarations(scenario: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_scopes::declarations_child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("declarations: no abort"), "{scenario}: {text}");
    text
}

#[test]
#[cfg(target_os = "linux")]
fn a_root_of_millions_of_prefixes_and_a_part_of_shapes_are_refused_in_words_and_do_not_abort_under_an_address_limit() {
    let text = run_declarations("a_root_of_millions_of_prefixes_and_a_part_of_shapes");
    assert!(text.contains("refused: the drawing holds more than 1024 namespace prefixes declared by one element"), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn shapes_that_each_declare_the_most_prefixes_are_refused_in_words_by_the_model_pool_and_do_not_abort_under_an_address_limit() {
    let text = run_declarations("shapes_that_each_declare_the_most_prefixes");
    assert!(
        text.contains(&format!("refused: the drawing holds more than {} bytes of the model in memory", diacraft_sheetxml::model::OPEN_POOL_BYTES)),
        "{text}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn nested_shapes_that_each_declare_the_most_prefixes_are_all_open_at_once_and_do_not_abort_under_an_address_limit() {
    run_declarations("nested_shapes_that_each_declare_the_most_prefixes");
}

#[test]
fn the_lists_of_a_part_refuse_a_root_that_is_not_a_document() {
    // The page of a package is not the document of it: the lists are read from the document part, and a part with another root is refused where it is read.
    let a_page = page("", "");
    let out = diacraft_sheetxml::model::part_lists(&a_page);
    assert!(matches!(&out, Err(CodecError::Malformed { part, why }) if part == "document" && why.contains("root element")), "{:?}", out.err());
    let other = diacraft_container::xml::parse_with(b"<d:document xmlns:d=\"urn:other\"/>", &native_limits()).unwrap();
    assert!(diacraft_sheetxml::model::part_lists(&other).is_err(), "a document of another namespace");
}
