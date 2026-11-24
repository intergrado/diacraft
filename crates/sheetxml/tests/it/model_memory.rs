//! What the decoder charges for the model it builds is at least what the model costs, for every kind of element that can repeat in a
//! part. An element that costs one item but builds a few hundred bytes of model (an empty `<shape/>` built 338) let a file of eight pages of a million of them
//! pass the open budget and abort the process at an allocation, which no test of text bytes or blob sizes could see.
//!
//! Each kind is decoded in a process of its own and measured two ways against the bytes the budget was charged for the same decode:
//!
//! * **Exactly, which is the law.** The binary's allocator counts what is asked for and given back (`GLOBAL` in `main.rs`), so the bytes and the allocations the
//!   model keeps once the decode has returned are known to the byte, the same on every platform; the charge covers them, an eighth of them for the size classes
//!   of the app's allocator (mimalloc, [`CLASS_SHARE`]) and, for each allocation, the most glibc adds to one ([`BLOCK_BYTES`]): [`law_need`].
//! * **As the process sees it, within a margin.** The growth of the peak resident size, reset after the trees are built so that what grows is the decode's own,
//!   stays within twice the charge ([`RSS_MARGIN`]). It is no measure of the model: it misses a vector's room that nothing has touched, and holds the
//!   decode's transients and the placements of the platform's allocator. A count of 20,000 connects kept 118 bytes each, of which the process showed 52 to 71
//!   on Fedora (glibc 2.44), 61 to 88 on Ubuntu 24.04 (glibc 2.39) and 98 on GitHub's runner, against a charge of 96: the room of their vector, which the
//!   resident size never showed, was the shortfall.
//!
//! A kind is measured twice: with only the attributes it must have, and with every attribute the table lists (the worst case for the strings it keeps), and the
//! kinds whose units need a name of their own get one.
#[cfg(target_os = "linux")]
use std::process::Command;

use diacraft_container::xml::El;
use diacraft_sheetxml::model::vocab;
use diacraft_sheetxml::model::{Budget, decode};
use stats_alloc::Region;

use super::model_table::{OneBlob, Root, context, dx, prepare, run, sample};

const KIND: &str = "SHEETXML_MODEL_MEMORY_KIND";
const HUGE: u64 = 1 << 40;

/// What a unit of a kind needs to be its own: a value of its own for an attribute that names it.
fn vary(element: &str, attr: &str, i: usize) -> Option<String> {
    let text = |prefix: &str| Some(format!("{prefix}{i}"));
    match (element, attr) {
        ("Cell", "N") | ("Section", "N") | ("Row", "N") => text("C"),
        ("sheet", "def") | ("Section", "def") => text("d"),
        ("font", "name-u") => text("f"),
        ("part", "name") => text("/p"),
        ("rel", "id") | ("rel", "source") => text("r"),
        ("custom", "name") => text("n"),
        ("comment-author" | "comment-entry", "id") => Some(i.to_string()),
        ("rest", "key") => text("k"),
        ("root", "name") | ("manifest-row", "name") | ("definition", "name") | ("kept-style", "name") => text("m"),
        ("content-type", "extension") => text("e"),
        ("shape" | "style" | "color", "id" | "ix") | ("blob" | "page-ref" | "master-ref", "id") => Some(i.to_string()),
        _ => None,
    }
}

/// The attributes of the `i`th unit: the ones the element must have, or every one the table lists for it, each with its sample or its own value.
fn unit_attrs(element: &str, i: usize, full: bool) -> String {
    let def = vocab::element(element).expect("an element of the table");
    def.attrs
        .iter()
        .filter(|a| (full || a.required || vary(element, a.name, i).is_some()) && !(element == "content-type" && a.name == "part"))
        // A name that a unit gives itself is given once in a part: each unit a name of its own, and only when every attribute is asked for.
        .filter(|a| a.name != "def" || full)
        .map(|a| format!("{}=\"{}\"", a.name, vary(element, a.name, i).unwrap_or_else(|| sample(element, a))))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where the element `name` starts and ends in `text`.
fn span(text: &str, name: &str) -> (usize, usize) {
    let open = format!("<diacraft:{name} ");
    let start = text.find(&open).expect("the element is in its context");
    let tag_end = start + text[start..].find('>').expect("a tag") + 1;
    if text[..tag_end].ends_with("/>") {
        return (start, tag_end);
    }
    let close = format!("</diacraft:{name}>");
    (start, tag_end + text[tag_end..].find(&close).expect("a closing tag") + close.len())
}

/// What a shape holds, for the kinds that are a shape with something in it: the picture it shows, an empty sheet of links, a text, a slot of the other format.
fn shape_holding(what: &str, i: usize) -> String {
    let inner = match what {
        "picture" => "<QD:foreign rel-id=\"r\" text=\"t\"><QD:picture part=\"/media/p.png\" type=\"image/png\" blob=\"0\"/></QD:foreign>",
        "sheet" => "<QD:sheet LineStyle=\"1\" FillStyle=\"2\" TextStyle=\"3\"/>",
        "text" => "<QD:text><QD:t>YQ==</QD:t></QD:text>",
        "ext" => "<QD:ext><QD:odf style-name=\"s\"/></QD:ext>",
        "units" => "<QD:ext><QD:odf><QD:units><QD:attr name=\"a\" value=\"b\"/></QD:units></QD:odf></QD:ext>",
        other => panic!("no shape holding {other}"),
    };
    format!("<QD:shape id=\"{i}\">{inner}</QD:shape>")
}

/// A page with one shape that holds `n` units of what `tree` says: the kinds whose unit is not an element of the table but a part of what a kept or unknown
/// element holds (the nodes and attributes of a kept tree, the unknown elements, comments and instructions of a newer build).
fn shape_with(tree: &str, n: usize) -> String {
    let units: String = (0..n)
        .map(|i| match tree {
            "kept+el" => format!("<QD:el name=\"c{i}\"/>"),
            "kept+txt" => "<QD:txt>x</QD:txt>".to_owned(),
            "kept+comment" => "<QD:comment>x</QD:comment>".to_owned(),
            "kept+pi" => "<QD:pi target=\"t\">d</QD:pi>".to_owned(),
            "kept+attr" => format!("<QD:attr name=\"a{i}\" value=\"v\"/>"),
            "native+element" => format!("<QD:future{i}/>"),
            "native+comment" => "<!--x-->".to_owned(),
            "native+pi" => "<?t d?>".to_owned(),
            other => panic!("no tree {other}"),
        })
        .collect();
    let inner = match tree {
        t if t.starts_with("kept+") => format!("<QD:kept><QD:el name=\"p\">{units}</QD:el></QD:kept>"),
        "native+element" => units,
        _ => format!("<QD:future>{units}</QD:future>"),
    };
    format!("<QD:page xmlns:QD=\"{}\"><QD:shape id=\"1\">{inner}</QD:shape></QD:page>", diacraft_sheetxml::model::URN)
}

/// The cells of a container: `k` of them, each with a name of its own.
fn cells_of(k: usize) -> String {
    (0..k).map(|c| format!("<QD:Cell N=\"c{c}\"/>")).collect()
}

/// A shape that holds `inner`, with the number `i` for its ID.
fn shape_of(i: usize, inner: &str) -> String {
    format!("<QD:shape id=\"{i}\">{inner}</QD:shape>")
}

/// The parts of a model in which every container holds one thing, or few: what a decode of a part of **sparse** containers builds, which a part of dense ones
/// does not show. A `BTreeMap` of cells allocates a leaf for eleven at its first cell (1,160 bytes), a vector allocates room for four at its first element, and
/// a kind measured in a place where a thousand of it stand together (the tests above) pays a thousandth of that. Each unit here is a whole shape with the
/// kind in it alone (the reviewer's sparse shapes, rows, sections and mixes, at twice what the charges were).
fn sparse_part(which: &str, n: usize) -> (Root, String) {
    let sheet_of = |inner: String| format!("<QD:sheet>{inner}</QD:sheet>");
    let section_of = |inner: String| format!("<QD:sheet><QD:Section N=\"User\">{inner}</QD:Section></QD:sheet>");
    let unit = |i: usize| -> String {
        match which {
            "sheet-cell1" => shape_of(i, &sheet_of(cells_of(1))),
            "sheet-cells2" => shape_of(i, &sheet_of(cells_of(2))),
            "sheet-cells5" => shape_of(i, &sheet_of(cells_of(5))),
            "sheet-cells11" => shape_of(i, &sheet_of(cells_of(11))),
            "sheet-cells12" => shape_of(i, &sheet_of(cells_of(12))),
            "section" => shape_of(i, "<QD:sheet><QD:Section N=\"User\"/></QD:sheet>"),
            "section-cell1" => shape_of(i, &section_of(cells_of(1))),
            "row" => shape_of(i, &section_of("<QD:Row N=\"r\"/>".to_owned())),
            "row-cell1" => shape_of(i, &section_of(format!("<QD:Row N=\"r\">{}</QD:Row>", cells_of(1)))),
            "row-cells2" => shape_of(i, &section_of(format!("<QD:Row N=\"r\">{}</QD:Row>", cells_of(2)))),
            "row-cells4" => shape_of(i, &section_of(format!("<QD:Row N=\"r\">{}</QD:Row>", cells_of(4)))),
            "row-cells12" => shape_of(i, &section_of(format!("<QD:Row N=\"r\">{}</QD:Row>", cells_of(12)))),
            "sections8" => {
                let sections: String = (0..8).map(|s| format!("<QD:Section N=\"s{s}\"><QD:Cell N=\"c\"/></QD:Section>")).collect();
                shape_of(i, &sheet_of(sections))
            }
            "paragraph-run" => shape_of(i, "<QD:text><QD:ext><QD:odf><QD:paragraph><QD:run/></QD:paragraph></QD:odf></QD:ext></QD:text>"),
            "held" => shape_of(i, "<QD:ext><QD:odf><QD:held group=\"fill\"/></QD:odf></QD:ext>"),
            "kept" => shape_of(i, "<QD:kept><QD:el name=\"a\"><QD:el name=\"b\"/></QD:el></QD:kept>"),
            "unknown" => format!("<QD:shape id=\"{i}\" future=\"1\"><zz><yy/></zz></QD:shape>"),
            // Kept trees with a few children, the first child of each being what makes a vector of room for four.
            "kept-children4" | "kept-children5" | "kept-children9" => {
                let k = which.strip_prefix("kept-children").and_then(|k| k.parse::<usize>().ok()).unwrap_or(1);
                let children: String = (0..k).map(|c| format!("<QD:el name=\"c{c}\"/>")).collect();
                shape_of(i, &format!("<QD:kept><QD:el name=\"p\">{children}</QD:el></QD:kept>"))
            }
            "kept-chain" => shape_of(
                i,
                "<QD:kept><QD:el name=\"a\"><QD:el name=\"b\"><QD:el name=\"c\"><QD:el name=\"d\"><QD:el name=\"e\"/></QD:el></QD:el></QD:el></QD:el></QD:kept>",
            ),
            "kept-text" => {
                shape_of(i, "<QD:kept><QD:el name=\"a\"><QD:txt>x</QD:txt><QD:comment>y</QD:comment><QD:pi target=\"t\">d</QD:pi></QD:el></QD:kept>")
            }
            "kept-two" => shape_of(i, "<QD:kept><QD:el name=\"a\"/></QD:kept><QD:kept><QD:el name=\"b\"/></QD:kept>"),
            "attrs1" => shape_of(i, "<QD:attr name=\"a\" value=\"v\"/>"),
            "attrs5" => shape_of(i, &(0..5).map(|a| format!("<QD:attr name=\"a{a}\" value=\"v\"/>")).collect::<String>()),
            // What a newer build wrote: unknown attributes and elements, with names that need their namespace and children that are text.
            "future-attrs4" => format!("<QD:shape id=\"{i}\" fa=\"1\" fb=\"2\" fc=\"3\" fd=\"4\"/>"),
            "future-attrs5" => format!("<QD:shape id=\"{i}\" fa=\"1\" fb=\"2\" fc=\"3\" fd=\"4\" fe=\"5\"/>"),
            "future-children5" => shape_of(i, "<zz><y1/><y2/><y3/><y4/><y5/></zz>"),
            "future-text" => shape_of(i, "<zz a=\"1\">text<!--c--><?t d?></zz>"),
            "future-ns" => shape_of(i, "<x:zz xmlns:x=\"urn:x\" x:a=\"1\"><x:yy/></x:zz>"),
            "geometry" => shape_of(
                i,
                "<QD:sheet><QD:Section N=\"Geometry\" IX=\"0\"><QD:Row N=\"r\" T=\"MoveTo\"><QD:Cell N=\"X\" V=\"1\" F=\"2*3+Width\" U=\"IN\"/></QD:Row></QD:Section></QD:sheet>",
            ),
            "text-markers" => shape_of(i, "<QD:text><QD:t>aGVsbG8=</QD:t><QD:cp/><QD:pp/><QD:tp/></QD:text>"),
            "mix" => shape_of(
                i,
                &format!(
                    "{}<QD:text><QD:ext><QD:odf><QD:paragraph><QD:run/></QD:paragraph><QD:held group=\"fill\"/></QD:odf></QD:ext></QD:text>",
                    section_of(format!("<QD:Row N=\"r\">{}</QD:Row>", cells_of(1)))
                ),
            ),
            other => panic!("no sparse unit {other}"),
        }
    };
    let inner = match which {
        // Rows of one cell each, in one section: the container of the rows is dense, and the container of each row's cells is not.
        "rows-of-one-cell" | "rows-of-two-cells" | "rows-of-four-cells" | "rows-of-twelve-cells" => {
            let k = match which {
                "rows-of-one-cell" => 1,
                "rows-of-two-cells" => 2,
                "rows-of-four-cells" => 4,
                _ => 12,
            };
            let rows: String = (0..n).map(|i| format!("<QD:Row N=\"r{i}\">{}</QD:Row>", cells_of(k))).collect();
            shape_of(1, &format!("<QD:sheet><QD:Section N=\"User\">{rows}</QD:Section></QD:sheet>"))
        }
        // Sections of one cell each, in one sheet.
        "sections-of-one-cell" => {
            let sections: String = (0..n).map(|s| format!("<QD:Section N=\"s{s}\"><QD:Cell N=\"c\"/></QD:Section>")).collect();
            shape_of(1, &sheet_of(sections))
        }
        _ => (0..n).map(unit).collect(),
    };
    (Root::Page, dx(&format!("<QD:page xmlns:QD=\"{}\">{inner}</QD:page>", diacraft_sheetxml::model::URN)))
}

/// The bytes of the formula of a `long+formula` unit: 10 KiB, its two quotes included.
const LONG_FORMULA_BYTES: usize = 10 * 1024;
/// The bytes of the run of characters of a `long+run` unit: 64 KiB.
const LONG_RUN_BYTES: usize = 64 * 1024;
/// The bytes of the kept text of a `long+kept` unit: 1 MiB.
const LONG_KEPT_BYTES: usize = 1024 * 1024;

/// A text of `len` bytes of the `i`th unit's own: its number, then letters (nothing XML or a formula's string would escape).
fn long_text(len: usize, i: usize) -> String {
    let head = format!("u{i}x");
    let fill = len.saturating_sub(head.len());
    head + &"a".repeat(fill)
}

/// The parts of the kinds **made mostly of long text**: what a hostile file holds the most of for the fewest elements, where the model keeps
/// the text and little beside it, so that what a charge of the text by its length leaves out (the eighth the law holds for the size classes of the app's allocator)
/// is all there is to see. A cell's formula of 10 KiB that is one string (its tree holds the string again beside the source), a text of one run of 64 KiB in a shape
/// of its own, and a kept tree whose text is 1 MiB in a shape of its own.
fn long_part(which: &str, n: usize) -> (Root, String) {
    let inner: String = match which {
        "formula" => {
            let cells: String = (0..n).map(|i| format!("<QD:Cell N=\"C{i}\" F=\"&quot;{}&quot;\"/>", long_text(LONG_FORMULA_BYTES - 2, i))).collect();
            shape_of(1, &format!("<QD:sheet>{cells}</QD:sheet>"))
        }
        "run" => (0..n).map(|i| shape_of(i, &format!("<QD:text><QD:t>{}</QD:t></QD:text>", long_text(LONG_RUN_BYTES, i)))).collect(),
        "kept" => (0..n)
            .map(|i| shape_of(i, &format!("<QD:kept><QD:el name=\"p\"><QD:txt>{}</QD:txt></QD:el></QD:kept>", long_text(LONG_KEPT_BYTES, i))))
            .collect(),
        other => panic!("no long unit {other}"),
    };
    (Root::Page, dx(&format!("<QD:page xmlns:QD=\"{}\">{inner}</QD:page>", diacraft_sheetxml::model::URN)))
}

/// The part with `n` units of the element `name` in the place where it stands.
fn part_of(name: &str, n: usize, full: bool) -> (Root, String) {
    if let Some(which) = name.strip_prefix("sparse+") {
        return sparse_part(which, n);
    }
    if let Some(which) = name.strip_prefix("long+") {
        return long_part(which, n);
    }
    if let Some(what) = name.strip_prefix("shape+") {
        let shapes: String = (0..n).map(|i| shape_holding(what, i)).collect();
        return (Root::Page, dx(&format!("<QD:page xmlns:QD=\"{}\">{shapes}</QD:page>", diacraft_sheetxml::model::URN)));
    }
    if name.starts_with("kept+") || name.starts_with("native+") {
        return (Root::Page, dx(&shape_with(name, n)));
    }
    if name == "cell+extra" {
        let cells: String = (0..n).map(|i| format!("<QD:Cell N=\"C{i}\"><QD:extra text=\"t\"/></QD:Cell>")).collect();
        return (Root::Page, dx(&format!("<QD:page xmlns:QD=\"{}\"><QD:sheet>{cells}</QD:sheet></QD:page>", diacraft_sheetxml::model::URN)));
    }
    if name == "paragraph+style" {
        let group = diacraft_doc::TextGroup::ALL[0].id();
        let style = format!("<QD:text-style name=\"s\" stamp=\"0123456789abcdef\"><QD:held-style group=\"{group}\"/></QD:text-style>");
        let paragraphs: String = (0..n).map(|_| format!("<QD:paragraph>{style}</QD:paragraph>")).collect();
        return (
            Root::Page,
            dx(&format!(
                "<QD:page xmlns:QD=\"{}\"><QD:shape id=\"1\"><QD:text><QD:ext><QD:odf>{paragraphs}</QD:odf></QD:ext></QD:text></QD:shape></QD:page>",
                diacraft_sheetxml::model::URN
            )),
        );
    }
    // The lists round a paragraph, and the element of a paragraph or of a run.
    if matches!(name, "paragraph+level" | "paragraph+element" | "run+element") {
        let inner = match name {
            "paragraph+level" => {
                "<QD:level><QD:list><QD:el name=\"text:list\"/></QD:list><QD:item><QD:el name=\"text:list-item\"/></QD:item></QD:level>"
            }
            "paragraph+element" => "<QD:element><QD:el name=\"text:h\"/></QD:element>",
            _ => "<QD:run><QD:element><QD:el name=\"text:a\"/></QD:element></QD:run>",
        };
        let paragraphs: String = (0..n).map(|_| format!("<QD:paragraph>{inner}</QD:paragraph>")).collect();
        return (
            Root::Page,
            dx(&format!(
                "<QD:page xmlns:QD=\"{}\"><QD:shape id=\"1\"><QD:text><QD:ext><QD:odf>{paragraphs}</QD:odf></QD:ext></QD:text></QD:shape></QD:page>",
                diacraft_sheetxml::model::URN
            )),
        );
    }
    if name == "native-attrs" {
        let attrs: String = (0..n).map(|i| format!(" future{i}=\"v\"")).collect();
        return (Root::Page, dx(&format!("<QD:page xmlns:QD=\"{}\"><QD:shape id=\"1\"{attrs}/></QD:page>", diacraft_sheetxml::model::URN)));
    }
    let (root, first) = context(name, &unit_attrs(name, 0, full)).expect("a context for the kind");
    let (start, end) = span(&first, name);
    let mut units = String::new();
    for i in 0..n {
        let (_, text) = context(name, &unit_attrs(name, i, full)).expect("a context");
        let (s, e) = span(&text, name);
        units.push_str(&text[s..e]);
    }
    (root, format!("{}{units}{}", &first[..start], &first[end..]))
}

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

#[test]
#[ignore = "run by the tests below in a process of its own; alone it needs the kind in the environment"]
fn memory_child() {
    let spec = std::env::var(KIND).unwrap_or_default();
    let mut parts = spec.split(':');
    let (name, n, full) = (parts.next().expect("a kind"), parts.next().expect("a count"), parts.next() == Some("full"));
    let n: usize = n.parse().expect("a count");
    // The code a decode runs is mapped into the process the first time it runs, and the pages of the executable count as resident memory (a few hundred
    // kibibytes, more or less, from one run to the next): a small decode first, so that what the large one grows by is the model.
    {
        let (root, text) = part_of(name, n.min(50), full);
        let prepared = prepare(root, &text).expect("the trees");
        drop(text);
        // What each kind costs is measured unit by unit: nothing is shared (a thousand equal sheets would be one).
        let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
        run(&prepared, &mut budget).expect("the warm-up decodes");
    }
    let (root, text) = part_of(name, n, full);
    let prepared = prepare(root, &text).expect("the trees");
    drop(text);
    let (pages, masters): (Vec<&El>, Vec<&El>) = (prepared.pages.iter().collect(), prepared.masters.iter().collect());
    let mut notes = Vec::new();
    // The count is the binary's allocator's: a vector of a known size, made and let go, is seen, or a binary whose allocator counts nothing would pass every kind.
    let probe = Region::new(crate::GLOBAL);
    drop(std::hint::black_box(Vec::<u8>::with_capacity(4096)));
    let counting = probe.change().bytes_allocated >= 4096;
    std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
    let start = status_kib("VmHWM");
    let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
    // What the decode keeps is what it asked for and did not give back, the model held until it is counted (the warm-up made what a first decode makes once).
    let region = Region::new(crate::GLOBAL);
    let model = decode(&prepared.document, &pages, &masters, &OneBlob, &mut budget, &mut notes);
    let kept = region.change();
    let grew = status_kib("VmHWM").saturating_sub(start).saturating_mul(1024);
    let signed = |n: usize| i128::try_from(n).unwrap_or(i128::MAX);
    // A reallocation's growth is in the bytes allocated and its shrinking in the bytes given back (`bytes_reallocated` is the two again, as one sum).
    let bytes = signed(kept.bytes_allocated) - signed(kept.bytes_deallocated);
    let blocks = signed(kept.allocations) - signed(kept.deallocations);
    println!(
        "MEMORY ok={} counting={counting} n={n} grew={grew} charged={} items={} kept={bytes} blocks={blocks}",
        model.is_ok(),
        HUGE - budget.bytes,
        HUGE - budget.items
    );
    drop(model);
}

/// What the child measured of the decode of a kind.
#[cfg(target_os = "linux")]
struct Measured {
    /// The growth of the peak resident size of the process, in bytes.
    grew: u64,
    /// What the budget was charged, in bytes and in items.
    charged: u64,
    items: u64,
    /// The bytes the model keeps once the decode has returned, and the allocations that hold them, as the allocator counted them.
    kept: u64,
    blocks: u64,
}

/// The decode of `n` units of `name`, measured by the child in a process of its own.
#[cfg(target_os = "linux")]
fn measure(name: &str, n: usize, full: bool) -> Measured {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(["--exact", "model_memory::memory_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(KIND, format!("{name}:{n}:{}", if full { "full" } else { "bare" }))
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line =
        text.find("MEMORY ok=").map(|at| text[at..].lines().next().unwrap_or_default()).unwrap_or_else(|| panic!("{name}: no measure in {text}"));
    assert!(line.contains("ok=true"), "{name}: the decode of {n} units failed: {line}");
    assert!(line.contains("counting=true"), "{name}: the binary's allocator does not count what is allocated: {line}");
    let field = |key: &str| -> i128 { line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number") };
    // A decode gives back nothing it did not make, so what it keeps is never less than nothing.
    let unsigned = |key: &str| u64::try_from(field(key)).unwrap_or_else(|_| panic!("{name}: {key} is not a count of bytes: {line}"));
    Measured {
        grew: unsigned("grew="),
        charged: unsigned("charged="),
        items: unsigned("items="),
        kept: unsigned("kept="),
        blocks: unsigned("blocks="),
    }
}

/// The most the system's allocator (glibc's) adds to an allocation of the model, in bytes: a request of `n` bytes is a chunk of `n` and 8, rounded up to 16 and never
/// under 32 ([`diacraft_sheetxml::model::cost::chunk`]), so at most 24 bytes more for a request of 8 bytes or more. A vector of hundreds of kibibytes is mapped on
/// its own and rounded up to a page, at most 4 KiB once for the vector, which the charges of the thousands of units it holds cover. Measured exactly (glibc's own
/// count of the bytes in use, `malloc_stats`, read under a debugger before and after a decode of each kind), glibc added 8 to 19 bytes to the
/// allocations of every kind, and the charge of every kind covered the bytes it counted.
#[cfg(target_os = "linux")]
const BLOCK_BYTES: u64 = 24;

/// The share of the bytes kept that the allocator of the shipped app and command line (mimalloc) may add by rounding a request up to one of its size classes, as
/// the denominator of a fraction: an eighth. mimalloc has no header, but it rounds a request to one of four classes for each doubling, so a
/// block of 1,160 bytes (a map of cells' first leaf) is 1,280; counted its usable size at up to 10.7 percent over the bytes asked for, on the kinds
/// measured, and glibc's chunk is the [`BLOCK_BYTES`] beside it.
#[cfg(target_os = "linux")]
const CLASS_SHARE: u64 = 8;

/// **The law** (widened by to the allocator the app ships with): what a model that keeps `kept` bytes in `blocks` allocations needs of the
/// allocator, its bytes, an eighth of them for mimalloc's size classes and, for each allocation, the most glibc adds to one. A charge is at least this.
#[cfg(target_os = "linux")]
fn law_need(kept: u64, blocks: u64) -> u64 {
    kept.saturating_add(kept / CLASS_SHARE).saturating_add(blocks.saturating_mul(BLOCK_BYTES))
}

/// How far the growth of the process's peak resident size may pass the charge: to twice it (2/1, as a numerator and a denominator). The peak holds what the
/// decode keeps and has touched, and besides it the decode's transients: a vector that doubles in the heap holds its old room beside the new while it is copied,
/// and a table the decode makes and lets go (the blob records); it holds less where a decode reuses pages a text let go left resident, and none of a vector's room
/// that is never touched. How much of each depends on the platform's allocator, its version and where it puts a large vector (mapped on its own or in the heap,
/// which moves with what was freed before). It is the check that the exact count sees what the process holds, not the law. Measured at the counts below
/// (four runs on Fedora 45 with glibc 2.44 and three on Ubuntu 24.04 with glibc 2.39), the growth was at most 1.01 of the charge on the one and
/// 1.09 on the other (a held value of a style), and the decode's peak, counted with every reallocation a copy (what an allocator that cannot grow a block in
/// place holds), reached 1.96 (the blob records), so that a margin under two could be passed by an allocator that copies. The law the test held before (the growth
/// within the charge) failed a connect on GitHub's runner at 1.02.
#[cfg(target_os = "linux")]
const RSS_MARGIN: (u64, u64) = (2, 1);

/// A kind is measured with only the attributes it must have, and with all of them.
#[cfg(target_os = "linux")]
fn check(name: &str, n: usize) {
    let mut problems = Vec::new();
    let cases: &[bool] = if name.starts_with("sparse+") || name.starts_with("long+") { &[false] } else { &[false, true] };
    for &full in cases {
        let Measured { grew, charged, items, kept, blocks } = measure(name, n, full);
        let per = |x: u64| x as f64 / n as f64;
        let which = if full { "full" } else { "bare" };
        let need = law_need(kept, blocks);
        eprintln!(
            "{name:<15} {which:<5} {n:>6} units: model kept {:>7.1} bytes in {:.2} allocations each ({:>7.1} with the allocator's), the process grew {:>7.1}, charged {:>7.1} bytes and {:.1} items each: {:.3} of the need",
            per(kept),
            per(blocks),
            per(need),
            per(grew),
            per(charged),
            per(items),
            charged as f64 / need.max(1) as f64
        );
        if charged == 0 {
            problems.push(format!("{name} ({which}): a kind that builds model charges bytes"));
        }
        if charged < need {
            problems.push(format!(
                "{name} ({which}): the decode of {n} units keeps {kept} bytes in {blocks} allocations ({need} with what the allocators add) and the budget was charged {charged}"
            ));
        }
        if grew.saturating_mul(RSS_MARGIN.1) > charged.saturating_mul(RSS_MARGIN.0) {
            problems.push(format!(
                "{name} ({which}): the decode of {n} units grew the process by {grew} bytes, more than {}/{} of the {charged} the budget was charged",
                RSS_MARGIN.0, RSS_MARGIN.1
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

macro_rules! kinds {
    ($($test:ident: $name:literal, $count:expr, [$($covers:literal),*];)*) => {
        $(
            #[test]
            #[cfg(target_os = "linux")]
            fn $test() {
                check($name, $count);
            }
        )*

        /// The elements of the vocabulary that the kinds above measure: each kind covers the elements it is made of.
        const COVERED: &[&str] = &[$($($covers,)*)*];
    };
}

// Each count is one more than a power of two: a vector that grows by doubling then has room for twice what it holds, the most a decode leaves it, which a
// count of 20,000 (room for 32,768) did not show. It is 16,385 where the model's cap allows it, and under the cap where it does not (4,096 cells in
// a sheet, 4,096 colours, 1,000 pages); the kinds that held a hundred thousand rows or eight thousand sections hold 65,537 and 4,097.
kinds! {
    a_shape_is_charged_what_it_costs: "shape", 16_385, ["shape"];
    a_shape_with_a_picture_is_charged_what_it_costs: "shape+picture", 16_385, ["foreign", "picture"];
    a_shape_with_a_sheet_is_charged_what_it_costs: "shape+sheet", 16_385, ["sheet"];
    a_shape_with_a_text_is_charged_what_it_costs: "shape+text", 16_385, ["text"];
    a_shape_with_a_slot_is_charged_what_it_costs: "shape+ext", 16_385, ["ext", "odf"];
    a_shape_with_a_bag_is_charged_what_it_costs: "shape+units", 16_385, ["units"];
    a_cell_is_charged_what_it_costs: "Cell", 2_049, ["Cell"];
    a_cell_with_an_extra_is_charged_what_it_costs: "cell+extra", 2_049, ["extra"];
    a_section_is_charged_what_it_costs: "Section", 4_097, ["Section"];
    a_row_is_charged_what_it_costs: "Row", 16_385, ["Row"];
    a_connect_is_charged_what_it_costs: "connect", 16_385, ["connect"];
    an_attr_is_charged_what_it_costs: "attr", 16_385, ["attr"];
    a_kept_node_is_charged_what_it_costs: "kept", 16_385, ["kept"];
    a_kept_element_in_a_tree_is_charged_what_it_costs: "kept+el", 16_385, ["el"];
    a_kept_text_in_a_tree_is_charged_what_it_costs: "kept+txt", 16_385, ["txt"];
    a_kept_comment_in_a_tree_is_charged_what_it_costs: "kept+comment", 16_385, ["comment"];
    a_kept_instruction_in_a_tree_is_charged_what_it_costs: "kept+pi", 16_385, ["pi"];
    a_kept_attribute_in_a_tree_is_charged_what_it_costs: "kept+attr", 16_385, [];
    an_unknown_element_is_charged_what_it_costs: "native+element", 16_385, [];
    an_unknown_attribute_is_charged_what_it_costs: "native-attrs", 16_385, [];
    an_unknown_comment_is_charged_what_it_costs: "native+comment", 16_385, [];
    an_unknown_instruction_is_charged_what_it_costs: "native+pi", 16_385, [];
    a_text_run_is_charged_what_it_costs: "t", 16_385, ["t"];
    a_character_marker_is_charged_what_it_costs: "cp", 16_385, ["cp"];
    a_paragraph_marker_is_charged_what_it_costs: "pp", 16_385, ["pp"];
    a_text_marker_is_charged_what_it_costs: "tp", 16_385, ["tp"];
    a_field_is_charged_what_it_costs: "fld", 16_385, ["fld"];
    a_wrapped_node_of_the_other_format_is_charged_what_it_costs: "odf-raw", 16_385, ["odf-raw"];
    a_held_value_is_charged_what_it_costs: "held", 16_385, ["held"];
    a_paragraph_of_a_texts_data_is_charged_what_it_costs: "paragraph", 16_385, ["paragraph"];
    a_paragraph_with_its_style_is_charged_what_it_costs: "paragraph+style", 16_385, ["text-style", "held-style"];
    a_run_of_a_texts_data_is_charged_what_it_costs: "run", 16_385, ["run"];
    a_level_of_a_paragraph_is_charged_what_it_costs: "paragraph+level", 16_385, ["level", "list", "item"];
    the_element_of_a_paragraph_is_charged_what_it_costs: "paragraph+element", 16_385, ["element"];
    the_element_of_a_run_is_charged_what_it_costs: "run+element", 16_385, [];
    a_held_style_value_is_charged_what_it_costs: "held-style", 16_385, ["held-style"];
    a_colour_is_charged_what_it_costs: "color", 2_049, ["color"];
    a_font_is_charged_what_it_costs: "font", 16_385, ["font"];
    a_style_is_charged_what_it_costs: "style", 16_385, ["style"];
    a_theme_is_charged_what_it_costs: "theme", 16_385, ["theme"];
    a_part_is_charged_what_it_costs: "part", 16_385, ["part"];
    a_relationship_is_charged_what_it_costs: "rel", 16_385, ["rel"];
    a_content_type_row_is_charged_what_it_costs: "content-type", 16_385, ["content-type"];
    a_custom_property_is_charged_what_it_costs: "custom", 16_385, ["custom"];
    a_comment_is_charged_what_it_costs: "comment-entry", 16_385, ["comment-entry"];
    a_comment_author_is_charged_what_it_costs: "comment-author", 8_193, ["comment-author"];
    a_rest_entry_is_charged_what_it_costs: "rest", 16_385, ["rest"];
    an_odf_root_is_charged_what_it_costs: "root", 16_385, ["root"];
    a_manifest_row_is_charged_what_it_costs: "manifest-row", 16_385, ["manifest-row"];
    a_definition_is_charged_what_it_costs: "definition", 16_385, ["definition"];
    a_kept_style_is_charged_what_it_costs: "kept-style", 16_385, ["kept-style"];
    a_layer_is_charged_what_it_costs: "layer", 16_385, ["layer"];
    a_page_reference_is_charged_what_it_costs: "page-ref", 513, ["page-ref", "page"];
    a_master_reference_is_charged_what_it_costs: "master-ref", 16_385, ["master-ref", "master"];
    a_blob_record_is_charged_what_it_costs: "blob", 16_385, ["blob"];
    a_shape_with_a_sheet_of_one_cell_is_charged_what_it_costs: "sparse+sheet-cell1", 16_385, [];
    a_shape_with_a_sheet_of_two_cells_is_charged_what_it_costs: "sparse+sheet-cells2", 16_385, [];
    a_shape_with_a_sheet_of_five_cells_is_charged_what_it_costs: "sparse+sheet-cells5", 16_385, [];
    a_shape_with_a_sheet_of_eleven_cells_is_charged_what_it_costs: "sparse+sheet-cells11", 16_385, [];
    a_shape_with_a_sheet_of_twelve_cells_is_charged_what_it_costs: "sparse+sheet-cells12", 16_385, [];
    a_shape_with_a_section_alone_is_charged_what_it_costs: "sparse+section", 16_385, [];
    a_shape_with_a_section_of_one_cell_is_charged_what_it_costs: "sparse+section-cell1", 16_385, [];
    a_shape_with_a_row_alone_is_charged_what_it_costs: "sparse+row", 16_385, [];
    a_shape_with_a_row_of_one_cell_is_charged_what_it_costs: "sparse+row-cell1", 16_385, [];
    a_shape_with_a_row_of_two_cells_is_charged_what_it_costs: "sparse+row-cells2", 16_385, [];
    a_shape_with_a_row_of_four_cells_is_charged_what_it_costs: "sparse+row-cells4", 16_385, [];
    a_shape_with_a_row_of_twelve_cells_is_charged_what_it_costs: "sparse+row-cells12", 16_385, [];
    a_shape_with_eight_sections_of_one_cell_is_charged_what_it_costs: "sparse+sections8", 16_385, [];
    a_shape_with_a_paragraph_and_a_run_is_charged_what_it_costs: "sparse+paragraph-run", 16_385, [];
    a_shape_with_a_held_value_is_charged_what_it_costs: "sparse+held", 16_385, [];
    a_shape_with_a_kept_tree_is_charged_what_it_costs: "sparse+kept", 16_385, [];
    a_shape_with_unknown_content_is_charged_what_it_costs: "sparse+unknown", 16_385, [];
    a_shape_with_a_kept_tree_of_four_children_is_charged_what_it_costs: "sparse+kept-children4", 16_385, [];
    a_shape_with_a_kept_tree_of_five_children_is_charged_what_it_costs: "sparse+kept-children5", 16_385, [];
    a_shape_with_a_kept_tree_of_nine_children_is_charged_what_it_costs: "sparse+kept-children9", 16_385, [];
    a_shape_with_a_kept_chain_is_charged_what_it_costs: "sparse+kept-chain", 16_385, [];
    a_shape_with_a_kept_tree_of_text_comment_and_instruction_is_charged_what_it_costs: "sparse+kept-text", 16_385, [];
    a_shape_with_two_kept_nodes_is_charged_what_it_costs: "sparse+kept-two", 16_385, [];
    a_shape_with_one_kept_attribute_is_charged_what_it_costs: "sparse+attrs1", 16_385, [];
    a_shape_with_five_kept_attributes_is_charged_what_it_costs: "sparse+attrs5", 16_385, [];
    a_shape_with_four_unknown_attributes_is_charged_what_it_costs: "sparse+future-attrs4", 16_385, [];
    a_shape_with_five_unknown_attributes_is_charged_what_it_costs: "sparse+future-attrs5", 16_385, [];
    a_shape_with_an_unknown_element_of_five_children_is_charged_what_it_costs: "sparse+future-children5", 16_385, [];
    a_shape_with_an_unknown_element_of_text_is_charged_what_it_costs: "sparse+future-text", 16_385, [];
    a_shape_with_an_unknown_element_in_a_namespace_is_charged_what_it_costs: "sparse+future-ns", 16_385, [];
    a_shape_with_a_geometry_row_is_charged_what_it_costs: "sparse+geometry", 16_385, [];
    a_shape_with_text_markers_is_charged_what_it_costs: "sparse+text-markers", 16_385, [];
    a_shape_with_a_little_of_everything_is_charged_what_it_costs: "sparse+mix", 16_385, [];
    rows_of_one_cell_are_charged_what_they_cost: "sparse+rows-of-one-cell", 65_537, [];
    rows_of_two_cells_are_charged_what_they_cost: "sparse+rows-of-two-cells", 65_537, [];
    rows_of_four_cells_are_charged_what_they_cost: "sparse+rows-of-four-cells", 65_537, [];
    rows_of_twelve_cells_are_charged_what_they_cost: "sparse+rows-of-twelve-cells", 32_769, [];
    sections_of_one_cell_are_charged_what_they_cost: "sparse+sections-of-one-cell", 4_097, [];
    // Kinds made mostly of long text, at a count one more than a power of two (the cells under the cap of one place, the text tens of MB).
    a_cell_of_a_long_formula_is_charged_what_it_costs: "long+formula", 2_049, [];
    a_long_text_run_is_charged_what_it_costs: "long+run", 257, [];
    a_long_kept_text_is_charged_what_it_costs: "long+kept", 17, [];
}

/// The elements that no kind above measures, and why: each is once in its owner (so that a million of them is a million owners, which are measured), or has
/// nothing but text, which is charged by its length.
const NOT_REPEATED: &[(&str, &str)] = &[
    ("document", "one in a part"),
    ("props", "one in a document"),
    ("comment-list", "one in a document"),
    ("same", "a reference: charged by the bytes of the name and a pointer, and the model of the unit is the first one's"),
    ("solution", "one in a document"),
    ("s", "a field of text, charged by its length"),
    ("vsdx", "one in a slot"),
    ("manifest", "one in a part"),
    ("vsdx-held", "one in a document"),
    ("props-rest", "one in the VSDX records"),
    ("windows", "one in the VSDX records"),
    ("meta", "one in the ODF data of a document"),
    ("settings", "one in the ODF data of a document"),
    ("master-page-attrs", "one in the ODF data of a page"),
    ("layout-props", "one in the ODF data of a page"),
    ("drawing-page-style", "one in the ODF data of a page"),
    ("unmapped", "one in the ODF data of a style"),
];

#[test]
fn every_element_of_the_vocabulary_is_measured_or_says_why_it_is_not() {
    let mut missing = Vec::new();
    for def in vocab::ELEMENTS {
        let measured = COVERED.contains(&def.name);
        let excused = NOT_REPEATED.iter().any(|(name, _)| *name == def.name);
        if measured == excused {
            missing.push(format!("{} ({})", def.name, if measured { "both measured and excused" } else { "neither" }));
        }
    }
    assert!(missing.is_empty(), "elements to measure, or to say why not: {missing:?}");
    // And everything that is covered or excused is in the table.
    for name in COVERED.iter().copied().chain(NOT_REPEATED.iter().map(|(n, _)| *n)) {
        assert!(vocab::element(name).is_some(), "{name} is not an element of the vocabulary");
    }
}

/// What each kind was **measured** to cost beyond 32 bytes for each attribute and the bytes of its text (the largest of the two cases of the kind, an element
/// with the attributes it must have and one with all of them, as the `model_memory` tests print them), by the elements it is made of: the table's figures are a
/// quarter above, and a figure that falls below one of these is a table that would let a file through that the process cannot hold. The figures of the connect,
/// the run and the markers are **counted** (what the model keeps, with the most the allocator adds to each allocation, at a count that leaves a
/// vector at its emptiest; the run's is a run alone in its paragraph); the figures of the style, the paragraph, the text run and the field, the part, the
/// relationship, the custom property, the font, the entry of the VSDX records, the wrapped node, the definition and the content-type row are **counted by the law
/// of ** (an eighth of the bytes kept besides, [`law_need`]), the largest of the two cases; the others are the growth of the process they were
/// set from, which they still cover.
const MEASURED_NEEDS: &[(&[&str], u64)] = &[
    (&["shape"], 353),
    (&["text"], 200),
    (&["ext", "odf"], 165),
    (&["foreign", "picture"], 142),
    (&["Cell"], 50),
    (&["Row"], 167),
    (&["Section"], 222),
    (&["attr"], 99),
    (&["color"], 132),
    (&["connect"], 144),
    (&["content-type"], 146),
    (&["cp"], 96),
    (&["pp"], 96),
    (&["tp"], 96),
    (&["custom"], 206),
    (&["definition"], 118),
    (&["fld"], 155),
    (&["held"], 74),
    (&["held-style"], 64),
    (&["kept"], 148),
    (&["kept-style"], 155),
    (&["manifest-row"], 63),
    (&["master-ref", "master"], 324),
    (&["page-ref", "page"], 708),
    (&["paragraph"], 288),
    (&["element", "el"], 167),
    (&["level", "list", "item", "el", "el"], 372),
    (&["part"], 148),
    (&["rel"], 274),
    (&["root"], 65),
    (&["run"], 218),
    (&["style"], 284),
    (&["t"], 155),
    (&["theme"], 714),
    (&["el"], 62),
    (&["txt"], 70),
    (&["comment"], 95),
    (&["pi"], 76),
    (&["font"], 86),
    (&["rest"], 86),
    (&["odf-raw"], 137),
];

#[test]
fn the_table_of_what_the_model_costs_is_at_least_what_was_measured() {
    use diacraft_sheetxml::model::cost::{
        ATTR_BYTES, TEXT_PART_BYTES, UNKNOWN_ATTR_BYTES, UNKNOWN_COMMENT_BYTES, UNKNOWN_ELEMENT_BYTES, UNKNOWN_PI_BYTES, UNKNOWN_TEXT_BYTES,
        element_bytes,
    };
    for (kinds, need) in MEASURED_NEEDS {
        let price: u64 = kinds.iter().map(|k| element_bytes(k)).sum();
        assert!(price >= *need, "{kinds:?} cost {need} bytes when measured and the table says {price}");
        // And it is not the default for an element the table names: the default is for the ones that build almost nothing.
        assert!(price > 64 || *need <= 64, "{kinds:?}");
    }
    // What a newer build's content costs, kept: an element, an attribute (of the 208 in all, 32 are for every attribute), and a text, a comment and an instruction.
    const { assert!(UNKNOWN_ELEMENT_BYTES >= 385 && UNKNOWN_ATTR_BYTES + ATTR_BYTES >= 160) };
    const { assert!(UNKNOWN_TEXT_BYTES >= 70 && UNKNOWN_COMMENT_BYTES >= 77 && UNKNOWN_PI_BYTES >= 114 && TEXT_PART_BYTES >= 60) };
}

/// The price of each kind as the table has it, by the name of the constant of the vocabulary: the table is data, and a kind that falls out of it is priced at
/// the default of 64 (which is the figure of no kind that builds anything), where the memory tests held it to what it was measured to need.
#[test]
fn every_kind_the_table_names_is_priced_as_the_table_says_and_not_at_the_default() {
    use diacraft_sheetxml::model::cost::element_bytes;
    use diacraft_sheetxml::model::vocab::el;
    let pinned: &[(&str, u64)] = &[
        (el::PAGE, 704),
        (el::MASTER, 288),
        (el::SHAPE, 448),
        (el::THEME, 896),
        (el::TEXT, 256),
        (el::CELL, 80),
        (el::SECTION, 288),
        (el::ROW, 224),
        (el::STYLE, 368),
        (el::PARAGRAPH, 368),
        (el::KEPT_STYLE, 208),
        (el::EXT, 208),
        (el::PAGE_REF, 192),
        (el::REL, 352),
        (el::KEPT, 192),
        (el::COLOR, 176),
        (el::CHARS, 208),
        (el::FLD, 208),
        (el::CUSTOM, 272),
        (el::TEXT_STYLE, 128),
        (el::MASTER_REF, 128),
        (el::EXTRA, 128),
        (el::CONTENT_TYPE, 192),
        (el::ATTR, 128),
        (el::PROPS, 128),
        (el::VSDX_HELD, 128),
        (el::ODF_RAW, 176),
        (el::KEPT_COMMENT, 144),
        (el::KEPT_PI, 144),
        (el::RUN, 256),
        (el::LEVEL, 96),
        (el::ELEMENT, 96),
        (el::LIST, 96),
        (el::ITEM, 96),
        (el::PART, 192),
        (el::HELD, 112),
        (el::DEFINITION, 160),
        (el::FOREIGN, 112),
        (el::PICTURE, 96),
        (el::CP, 128),
        (el::PP, 128),
        (el::TP, 128),
        (el::ODF_ROOT, 96),
        (el::MANIFEST_ROW, 96),
        (el::HELD_STYLE, 96),
        (el::CONNECT, 192),
        (el::KEPT_TEXT, 96),
        (el::BLOB, 96),
        (el::KEPT_EL, 80),
        (el::FONT, 112),
        (el::REST, 112),
        (el::SHEET, 32),
        (el::ODF, 32),
        (el::LONG, 32),
    ];
    for (kind, price) in pinned {
        assert_eq!(element_bytes(kind), *price, "{kind}");
        assert_ne!(*price, 64, "{kind}: the default is not a figure of the table");
    }
    assert_eq!(element_bytes("a-kind-no-version-of-the-format-has"), 64);
}
