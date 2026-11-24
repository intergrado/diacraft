//! What a read of an `.odg` drawing says its model holds is at least what the model keeps, **counted**, for each kind of thing a
//! drawing can repeat.
//!
//! The figure is [`diacraft_odg::ReadCost::model_bytes`]: the items the read charged (`ReadWork::items`) at the bytes an item stands for (`index::ITEM_BYTES`), and
//! the pictures' bytes. It is the engine's hold of an open drawing in the process's ledger (`OpenFile::with_model_bytes`) and, twice over, the `.odg` save's memory
//! gate (`ReadCost::save_bytes`: the caller's model and the model of the file read again), so a figure under what the model keeps lets the engine hold more than
//! it says and a save start that the process cannot finish. It was set by the growth of the process's resident size (40 to 54 bytes an item), which misses a
//! vector's room that nothing has touched; counted, a connector needed 57.4 bytes an item, a common style 56.6 and a kept hyperlink wrapper 63.6
//! Over the 54 it was.
//!
//! Here **the binary's allocator counts** (`GLOBAL` in `main.rs`): a child reads a drawing of `n` units of a kind and the same drawing with none, keeps each
//! document and its index until it is counted (the rest of the report is let go first; the engine holds the index beside the document), and prints the bytes and
//! the allocations they hold as the allocator was asked for them, the items the read charged and its figure. **The law** ([`need`]) is that the
//! figure for the units covers the bytes they keep, an eighth of them for the size classes of the app's allocator ([`CLASS_SHARE`], mimalloc) and, for each
//! allocation, the most glibc adds to one ([`BLOCK_BYTES`]): the difference of the two drawings, so that what both hold (the document, the page, the default
//! styles) is not in it. The whole of each drawing is held to the same law too.
//!
//! Each count is one more than a power of two: a vector that grows by doubling then has room for twice what it holds, the most a read leaves it. The count is
//! 16,385 where the model's caps allow it, and under them where they do not (1,000 pages) or where a unit is a part of the package (2,049 pictures, 2,049 entries,
//! 513 objects). Each unit is its own (a size, a name or a value of its own), so that nothing is shared that a drawing of many different things would not share.
//! The kinds are the shapes, texts, styles and definitions a LibreOffice drawing holds, the constructs of `model_measure.rs`' cases, and what the reader keeps
//! without modelling it.

#![cfg(target_os = "linux")]

use std::process::Command;

use diacraft_odg::{Limits, ReadReport, read_partial};
use stats_alloc::Region;

use crate::drawing::Drawing;

/// The kind and the count a child reads, as `kind:n`.
const KIND: &str = "DIACRAFT_ODG_COUNTED_KIND";

/// The most the system's allocator (glibc's) adds to an allocation, in bytes: a request of `n` bytes is a chunk of `n` and 8, rounded up to 16 and never under 32,
/// so at most 24 bytes more for a request of 8 bytes or more (the figure). A large allocation is mapped on its own and rounded up to a page, at most
/// 4 KiB once, which the thousands of units it holds cover.
const BLOCK_BYTES: u64 = 24;

/// The share of the bytes kept that the allocator of the shipped app and command line (mimalloc) may add by rounding a request up to one of its size classes, as
/// the denominator of a fraction: an eighth. mimalloc has no header, but it rounds a request to one of four classes for each doubling; its usable size
/// was counted at up to 10.7 percent over the bytes asked for, and glibc's chunk is the [`BLOCK_BYTES`] beside it.
const CLASS_SHARE: u64 = 8;

/// **The law**: what the allocators give a model that keeps `kept` bytes in `blocks` allocations: its bytes, an eighth of them and, for each allocation, the most
/// glibc adds to one.
fn need(kept: u64, blocks: u64) -> u64 {
    kept.saturating_add(kept / CLASS_SHARE).saturating_add(blocks.saturating_mul(BLOCK_BYTES))
}

/// Eight bytes of a PNG's signature and four of its own, so that no two pictures are equal.
fn png(k: usize) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&u32::try_from(k).unwrap_or(u32::MAX).to_be_bytes());
    bytes
}

fn many(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(f).collect()
}

/// A text of `len` bytes of the `k`th unit's own: its number, then letters (nothing XML or a formula's string would escape). The kinds made mostly of long text
/// hold it: a cell's formula of 10 KiB that is one string, carried in DiaCraft's data (the one place an `.odg` holds a formula as written),
/// a paragraph of 64 KiB, and a kept element whose text is 1 MiB.
fn long_text(len: usize, k: usize) -> String {
    let head = format!("u{k}x");
    let fill = len.saturating_sub(head.len());
    head + &"a".repeat(fill)
}

/// `xml` as the value of an attribute: the markup and the quotes escaped.
fn attr_value(xml: &str) -> String {
    xml.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A place and a size of the `k`th unit's own (no two of the first 997,000 alike): equal shapes would share their sheet.
fn size(k: usize) -> String {
    format!(r#"svg:x="{}mm" svg:y="10mm" svg:width="{}mm" svg:height="10mm""#, k % 1000, 1 + k % 997)
}

/// The two ends of the `k`th line of its own.
fn ends(k: usize) -> String {
    format!(r#"svg:x1="{k}mm" svg:y1="1cm" svg:x2="{}mm" svg:y2="2cm""#, k + 5)
}

/// A drawing of one page that holds `shapes`.
fn page_of(shapes: &str) -> Drawing {
    Drawing::a4().page(shapes)
}

/// A drawing of one page whose one text box holds `inner` (paragraphs), with `auto` among its automatic styles.
fn text_box(auto: &str, inner: &str) -> Drawing {
    Drawing::a4().auto(auto).page(&format!(r#"<draw:frame {}><draw:text-box>{inner}</draw:text-box></draw:frame>"#, size(1)))
}

/// A `meta.xml` whose `office:meta` holds `fields`.
fn meta_of(fields: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta {} office:version=\"1.3\"><office:meta><meta:generator>TestWriter/1</meta:generator>{fields}</office:meta></office:document-meta>",
        crate::drawing::NS
    )
}

/// The drawing of `n` units of `kind`. A kind and its count of nothing is the drawing the units are measured against.
fn drawing_of(kind: &str, n: usize) -> Drawing {
    match kind {
        // Shapes, each of its own size.
        "rect" => page_of(&many(n, |k| format!("<draw:rect {}/>", size(k)))),
        "rect_text" => page_of(&many(n, |k| format!("<draw:rect {}><text:p>Text {k}</text:p></draw:rect>", size(k)))),
        "ellipse" => page_of(&many(n, |k| format!("<draw:ellipse {}/>", size(k)))),
        "circle" => page_of(&many(n, |k| format!("<draw:circle {}/>", size(k)))),
        "line" => page_of(&many(n, |k| format!("<draw:line {}/>", ends(k)))),
        "polygon" => page_of(&many(n, |k| {
            format!(r#"<draw:polygon {} svg:viewBox="0 0 1000 1000" draw:points="0,0 {},10 20,20 30,{}"/>"#, size(k), k % 1000, k % 999)
        })),
        "polyline" => page_of(&many(n, |k| {
            format!(r#"<draw:polyline {} svg:viewBox="0 0 1000 1000" draw:points="0,0 {},10 20,20 30,{}"/>"#, size(k), k % 1000, k % 999)
        })),
        "path" => page_of(&many(n, |k| {
            format!(r#"<draw:path {} svg:viewBox="0 0 1000 1000" svg:d="M0 0 L{} 10 C10 20 30 40 {} 60 Z"/>"#, size(k), k % 1000, k % 999)
        })),
        "custom_shape" => page_of(&many(n, |k| {
            format!(
                r#"<draw:custom-shape {}><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 Z N"/></draw:custom-shape>"#,
                size(k)
            )
        })),
        // A custom shape whose path reads two equations of its own: the equations, compiled, and the rows of the path.
        "custom_equations" => page_of(&many(n, |k| {
            format!(
                r#"<draw:custom-shape {}><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:enhanced-path="M ?f0 0 L 21600 ?f1 0 21600 Z N"><draw:equation draw:name="f0" draw:formula="{} * 2"/><draw:equation draw:name="f1" draw:formula="?f0 + 100"/></draw:enhanced-geometry></draw:custom-shape>"#,
                size(k),
                k % 10_000
            )
        })),
        // A rectangle with a glue point of its own: a row of its `Connection` section.
        "glue_point" => page_of(&many(n, |k| {
            format!(r#"<draw:rect {}><draw:glue-point draw:id="4" svg:x="0cm" svg:y="0.5cm" draw:escape-direction="horizontal"/></draw:rect>"#, size(k))
        })),
        "text_box" => page_of(&many(n, |k| format!("<draw:frame {}><draw:text-box><text:p>Hello {k}</text:p></draw:text-box></draw:frame>", size(k)))),
        "picture" => {
            let mut d = page_of(&many(n, |k| format!(r#"<draw:frame {}><draw:image xlink:href="Pictures/p{k}.png"/></draw:frame>"#, size(k))));
            d.files.extend((0..n).map(|k| (format!("Pictures/p{k}.png"), png(k))));
            d
        }
        "object" => {
            let mut d = page_of(&many(n, |k| {
                format!(r#"<draw:frame {}><draw:object xlink:href="./Object {k}"/><draw:image xlink:href="./ObjectReplacements/Object {k}"/></draw:frame>"#, size(k))
            }));
            for k in 0..n {
                d.files.push((format!("Object {k}/content.xml"), format!("<office:document-content k=\"{k}\"/>").into_bytes()));
                d.files.push((format!("ObjectReplacements/Object {k}"), png(k)));
            }
            d
        }
        "connector" => page_of(&many(n, |k| format!(r#"<draw:connector draw:type="standard" {}/>"#, ends(k)))),
        // Connectors glued at both ends to two shapes the drawing of none holds too.
        "connector_glued" => {
            let shapes = format!(r#"<draw:rect draw:id="a" {}/><draw:rect draw:id="b" {}/>"#, size(500), size(501));
            page_of(&format!(
                "{shapes}{}",
                many(n, |k| format!(r#"<draw:connector draw:type="standard" {} draw:start-shape="a" draw:start-glue-point="1" draw:end-shape="b" draw:end-glue-point="3"/>"#, ends(k)))
            ))
        }
        "group" => page_of(&many(n, |k| format!("<draw:g><draw:rect {}/><draw:rect {}/></draw:g>", size(2 * k), size(2 * k + 1)))),
        "turned" => page_of(&many(n, |k| {
            format!(
                r#"<draw:rect svg:width="{}mm" svg:height="10mm" draw:transform="rotate (0.{}) translate ({}mm 1cm)"/>"#,
                1 + k % 997,
                1 + k % 9,
                k % 1000
            )
        })),
        // What the reader keeps without modelling it: a hyperlink's wrapper round a shape, and a shape of a kind the model has no slot for.
        "link" => page_of(&many(n, |k| format!(r#"<draw:a xlink:href="http://example.org/{k}"><draw:rect {}/></draw:a>"#, size(k)))),
        "measure" => page_of(&many(n, |k| format!(r#"<draw:measure svg:x1="{k}mm" svg:y1="1cm" svg:x2="{}mm" svg:y2="1cm"/>"#, k + 5))),
        // A shape with an attribute and an element no field reads, kept in its slot.
        "foreign_attr" => page_of(&many(n, |k| format!(r#"<draw:rect {} draw:zz-unread="{k}"/>"#, size(k)))),
        "foreign_child" => page_of(&many(n, |k| format!(r#"<draw:rect {}><draw:zz-unread draw:a="{k}"/></draw:rect>"#, size(k)))),
        // Texts: paragraphs of one text box, a span on a text style, a list, a paragraph on a paragraph style of its own.
        "paragraph" => text_box("", &many(n, |k| format!("<text:p>Paragraph {k}</text:p>"))),
        "span" => text_box(
            r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#,
            &many(n, |k| format!(r#"<text:p>Hello <text:span text:style-name="T1">world {k}</text:span></text:p>"#)),
        ),
        "list" => text_box("", &many(n, |k| format!("<text:list><text:list-item><text:p>One {k}</text:p></text:list-item></text:list>"))),
        "paragraph_style" => text_box(
            &many(n, |k| format!(r#"<style:style style:name="P{k}" style:family="paragraph"><style:paragraph-properties fo:margin-left="{k}mm"/></style:style>"#)),
            &many(n, |k| format!(r#"<text:p text:style-name="P{k}">p</text:p>"#)),
        ),
        // Styles: a common style named by a rectangle, an automatic style of a rectangle's own, and one that casts a shadow.
        "style_common" => Drawing::a4()
            .common(&many(n, |k| format!(r##"<style:style style:name="s{k}" style:family="graphic"><style:graphic-properties draw:fill-color="#{k:06x}"/></style:style>"##)))
            .page(&many(n, |k| format!(r#"<draw:rect draw:style-name="s{k}" {}/>"#, size(k)))),
        "style_auto" => Drawing::a4()
            .auto(&many(n, |k| {
                format!(
                    r##"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#{k:06x}" draw:stroke="solid" svg:stroke-color="#3465a4"/></style:style>"##
                )
            }))
            .page(&many(n, |k| format!(r#"<draw:rect draw:style-name="gr{k}" draw:layer="layout" {}/>"#, size(k)))),
        "shadow" => Drawing::a4()
            .auto(&many(n, |k| {
                format!(
                    r##"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:shadow="visible" draw:shadow-offset-x="0.{}cm" draw:shadow-offset-y="0.2cm" draw:shadow-color="#{k:06x}"/></style:style>"##,
                    1 + k % 9
                )
            }))
            .page(&many(n, |k| format!(r#"<draw:rect draw:style-name="gr{k}" {}/>"#, size(k)))),
        // Definitions of `office:styles`, which the model holds whether or not a style names them.
        "gradient" => {
            Drawing::a4().common(&many(n, |k| format!(r#"<draw:gradient draw:name="g{k}" draw:style="linear" draw:angle="{}"/>"#, k % 3600))).page("")
        }
        "hatch" => Drawing::a4()
            .common(&many(n, |k| {
                format!(r##"<draw:hatch draw:name="H{k}" draw:style="single" draw:color="#000000" draw:distance="0.1cm" draw:rotation="{}"/>"##, k % 3600)
            }))
            .page(""),
        "dash" => Drawing::a4()
            .common(&many(n, |k| {
                format!(r#"<draw:stroke-dash draw:name="D{k}" draw:style="rect" draw:dots1="1" draw:dots1-length="0.{}cm" draw:distance="0.2cm"/>"#, 1 + k % 9)
            }))
            .page(""),
        "marker" => Drawing::a4().common(&many(n, |k| format!(r#"<draw:marker draw:name="M{k}" svg:viewBox="0 0 20 30" svg:d="M10 0L0 30H{} 0z"/>"#, k % 1000))).page(""),
        // The document: pages, each with a rectangle; master pages, each named by a page; layers; font faces; package entries; user-defined fields.
        "page" => (0..n).fold(Drawing::a4(), |d, k| d.page(&format!("<draw:rect {}/>", size(k)))),
        "page_empty" => (0..n).fold(Drawing::a4(), |d, _| d.page("")),
        "master_page" => {
            let mut d = Drawing::a4();
            d.masters.push_str(&many(n, |k| format!(r#"<style:master-page style:name="M{k}" style:page-layout-name="PM1"/>"#)));
            d.pages.push_str(&many(n, |k| format!(r#"<draw:page draw:name="page{}" draw:master-page-name="M{k}"/>"#, k + 1)));
            if n == 0 {
                d = d.page("");
            }
            d
        }
        "layer" => {
            let mut d = Drawing::a4().page("");
            let layers = many(n, |k| format!(r#"<draw:layer draw:name="L{k}"/>"#));
            d.masters = d.masters.replace("</draw:layer-set>", &format!("{layers}</draw:layer-set>"));
            d
        }
        "font_face" => Drawing {
            fonts: many(n, |k| format!(r#"<style:font-face style:name="F{k}" svg:font-family="'Family {k}'"/>"#)),
            ..Drawing::a4().page("")
        },
        "package_entry" => {
            let mut d = Drawing::a4().page("");
            d.files.extend((0..n).map(|k| (format!("Basic/Standard/Module{k}.xml"), format!("<script:module k=\"{k}\"/>").into_bytes())));
            d
        }
        // Kinds made mostly of long text: a rectangle on a style of its own whose DiaCraft data carries a cell of a formula that is one string (its tree holds the
        // string again beside the source); a text box of one paragraph; a rectangle with a kept child of one text.
        "formula_long" => Drawing::a4()
            .auto(&many(n, |k| {
                let sheet = format!("<Sheet><Cell N=\"Note\" F=\"&quot;{}&quot;\"/></Sheet>", long_text(10 * 1024 - 2, k));
                format!(
                    r#"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:fill="none" diacraft:version="1" diacraft:cells="{}"/></style:style>"#,
                    attr_value(&sheet)
                )
            }))
            .page(&many(n, |k| format!(r#"<draw:rect draw:style-name="gr{k}" {}/>"#, size(k)))),
        "formula_nodes" => Drawing::a4()
            .auto(&many(n, |k| {
                let args = vec!["1"; 5_000].join(",");
                let sheet = format!("<Sheet><Cell N=\"Note\" F=\"SUM({args},{k})\"/></Sheet>");
                format!(
                    r#"<style:style style:name="gr{k}" style:family="graphic"><style:graphic-properties draw:fill="none" diacraft:version="1" diacraft:cells="{}"/></style:style>"#,
                    attr_value(&sheet)
                )
            }))
            .page(&many(n, |k| format!(r#"<draw:rect draw:style-name="gr{k}" {}/>"#, size(k)))),
        "text_long" => page_of(&many(n, |k| {
            format!("<draw:frame {}><draw:text-box><text:p>{}</text:p></draw:text-box></draw:frame>", size(k), long_text(64 * 1024, k))
        })),
        "kept_long" => page_of(&many(n, |k| format!(r#"<draw:rect {}><draw:zz-unread>{}</draw:zz-unread></draw:rect>"#, size(k), long_text(1024 * 1024, k)))),
        "user_defined" => Drawing {
            meta: Some(meta_of(&many(n, |k| format!(r#"<meta:user-defined meta:name="Field {k}" meta:value-type="string">value {k}</meta:user-defined>"#)))),
            ..Drawing::a4().page("")
        },
        other => panic!("no kind {other}"),
    }
}

/// What a child counted of one read.
#[derive(Clone, Copy, Debug)]
struct Counted {
    /// The bytes the document and its index hold, as the allocator was asked for them, and the allocations that hold them.
    kept: u64,
    blocks: u64,
    /// The items the read charged, and its figure for the model ([`diacraft_odg::ReadCost::model_bytes`]).
    items: u64,
    charged: u64,
}

impl Counted {
    fn need(&self) -> u64 {
        need(self.kept, self.blocks)
    }
}

/// Reads `bytes`, and counts what the document and its index keep: whether it read, the bytes and the allocations kept, the items and the figure.
fn count_read(bytes: &[u8]) -> (bool, i128, i128, u64, u64) {
    let signed = |n: usize| i128::try_from(n).unwrap_or(i128::MAX);
    let region = Region::new(crate::GLOBAL);
    let (result, report) = read_partial(bytes, &Limits::default());
    // The engine keeps the index beside the document (`OdgState`); the rest of the report (the warnings, the counts by kind) is let go.
    let ReadReport { index, .. } = report;
    let cost = *index.read_cost();
    let change = region.change();
    // A reallocation's growth is in the bytes allocated and its shrinking in the bytes given back (`bytes_reallocated` is the two again, as one sum).
    let kept = signed(change.bytes_allocated) - signed(change.bytes_deallocated);
    let blocks = signed(change.allocations) - signed(change.deallocations);
    let ok = result.is_ok();
    drop(result);
    drop(index);
    (ok, kept, blocks, cost.items, cost.model_bytes())
}

#[test]
#[ignore = "run by the tests below in a process of its own; alone it needs the kind in the environment"]
fn counted_child() {
    let Ok(spec) = std::env::var(KIND) else { return };
    let (kind, n) = spec.split_once(':').expect("a kind and a count");
    let n: usize = n.parse().expect("a count");
    // The code a read runs, and what it makes once for the process, is made by a small read first.
    let _ = count_read(&drawing_of(kind, n.min(3)).bytes());
    let probe = Region::new(crate::GLOBAL);
    drop(std::hint::black_box(Vec::<u8>::with_capacity(4096)));
    let counting = probe.change().bytes_allocated >= 4096;
    for (label, units) in [("base", 0), ("full", n)] {
        let bytes = drawing_of(kind, units).bytes();
        let (ok, kept, blocks, items, charged) = count_read(&bytes);
        println!("COUNTED {label} ok={ok} counting={counting} n={units} kept={kept} blocks={blocks} items={items} charged={charged}");
    }
}

/// The reads of a kind with none and with `n` units, as the child counted them.
fn measure(kind: &str, n: usize) -> (Counted, Counted) {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(["--exact", "memory_counted::counted_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(KIND, format!("{kind}:{n}"))
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = |label: &str| -> Counted {
        let key = format!("COUNTED {label} ");
        let line = text.find(&key).map(|at| text[at..].lines().next().unwrap_or_default()).unwrap_or_else(|| panic!("{kind}: no count in {text}"));
        assert!(line.contains("ok=true"), "{kind}: the read failed: {line}");
        assert!(line.contains("counting=true"), "{kind}: the binary's allocator does not count what is allocated: {line}");
        let field = |key: &str| -> u64 {
            let value: i128 = line.split_whitespace().find_map(|w| w.strip_prefix(key)).and_then(|v| v.parse().ok()).expect("a number");
            u64::try_from(value).unwrap_or_else(|_| panic!("{kind}: {key} is not a count: {line}"))
        };
        Counted { kept: field("kept="), blocks: field("blocks="), items: field("items="), charged: field("charged=") }
    };
    (line("base"), line("full"))
}

/// The units of `kind` at `n`: **the figure for them covers what they keep**, and so does the figure for the whole drawing.
fn check(kind: &str, n: usize) {
    let (base, full) = measure(kind, n);
    let units = Counted {
        kept: full.kept.saturating_sub(base.kept),
        blocks: full.blocks.saturating_sub(base.blocks),
        items: full.items.saturating_sub(base.items),
        charged: full.charged.saturating_sub(base.charged),
    };
    assert!(units.items > 0, "{kind}: {n} units were charged no items: the drawing does not hold what the kind says");
    let per = |x: u64| x as f64 / n as f64;
    eprintln!(
        "{kind:<17} {n:>6} units: kept {:>8.1} bytes in {:>6.2} allocations each ({:>8.1} by the law), {:>7.2} items each ({:>5.1} bytes an item by the law), charged {:>8.1}: {:.3} of the need; the whole drawing {:.3}",
        per(units.kept),
        per(units.blocks),
        per(units.need()),
        per(units.items),
        units.need() as f64 / units.items as f64,
        per(units.charged),
        units.charged as f64 / units.need().max(1) as f64,
        full.charged as f64 / full.need().max(1) as f64,
    );
    let mut problems = Vec::new();
    if units.charged < units.need() {
        problems.push(format!(
            "{kind}: {n} units keep {} bytes in {} allocations ({} with what the allocators add) and the read's figure for them is {} ({} items)",
            units.kept,
            units.blocks,
            units.need(),
            units.charged,
            units.items
        ));
    }
    if full.charged < full.need() {
        problems.push(format!(
            "{kind}: the whole drawing keeps {} bytes in {} allocations ({} with what the allocators add) and the read's figure for it is {} ({} items)",
            full.kept,
            full.blocks,
            full.need(),
            full.charged,
            full.items
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

macro_rules! kinds {
    ($($test:ident: $kind:literal, $count:expr;)*) => {
        $(
            #[test]
            fn $test() {
                check($kind, $count);
            }
        )*
    };
}

kinds! {
    a_rectangle_is_charged_what_it_keeps: "rect", 16_385;
    a_rectangle_with_a_paragraph_is_charged_what_it_keeps: "rect_text", 16_385;
    an_ellipse_is_charged_what_it_keeps: "ellipse", 16_385;
    a_circle_is_charged_what_it_keeps: "circle", 16_385;
    a_line_is_charged_what_it_keeps: "line", 16_385;
    a_polygon_is_charged_what_it_keeps: "polygon", 16_385;
    a_polyline_is_charged_what_it_keeps: "polyline", 16_385;
    a_path_is_charged_what_it_keeps: "path", 16_385;
    a_custom_shape_is_charged_what_it_keeps: "custom_shape", 16_385;
    a_custom_shape_of_equations_is_charged_what_it_keeps: "custom_equations", 16_385;
    a_glue_point_is_charged_what_it_keeps: "glue_point", 16_385;
    a_text_box_is_charged_what_it_keeps: "text_box", 16_385;
    a_picture_is_charged_what_it_keeps: "picture", 2_049;
    an_object_is_charged_what_it_keeps: "object", 513;
    a_connector_is_charged_what_it_keeps: "connector", 16_385;
    a_glued_connector_is_charged_what_it_keeps: "connector_glued", 16_385;
    a_group_of_two_rectangles_is_charged_what_it_keeps: "group", 16_385;
    a_turned_rectangle_is_charged_what_it_keeps: "turned", 16_385;
    a_kept_hyperlink_wrapper_is_charged_what_it_keeps: "link", 16_385;
    a_kept_measure_is_charged_what_it_keeps: "measure", 16_385;
    a_kept_attribute_of_a_shape_is_charged_what_it_keeps: "foreign_attr", 16_385;
    a_kept_child_of_a_shape_is_charged_what_it_keeps: "foreign_child", 16_385;
    a_paragraph_is_charged_what_it_keeps: "paragraph", 16_385;
    a_span_on_a_text_style_is_charged_what_it_keeps: "span", 16_385;
    a_list_is_charged_what_it_keeps: "list", 16_385;
    a_paragraph_style_is_charged_what_it_keeps: "paragraph_style", 16_385;
    a_common_style_is_charged_what_it_keeps: "style_common", 16_385;
    an_automatic_style_is_charged_what_it_keeps: "style_auto", 16_385;
    a_shadow_is_charged_what_it_keeps: "shadow", 16_385;
    a_gradient_is_charged_what_it_keeps: "gradient", 16_385;
    a_hatch_is_charged_what_it_keeps: "hatch", 16_385;
    a_dash_is_charged_what_it_keeps: "dash", 16_385;
    a_marker_is_charged_what_it_keeps: "marker", 16_385;
    a_page_is_charged_what_it_keeps: "page", 513;
    an_empty_page_is_charged_what_it_keeps: "page_empty", 513;
    a_master_page_is_charged_what_it_keeps: "master_page", 513;
    a_layer_is_charged_what_it_keeps: "layer", 16_385;
    a_font_face_is_charged_what_it_keeps: "font_face", 16_385;
    a_package_entry_is_charged_what_it_keeps: "package_entry", 2_049;
    a_user_defined_field_is_charged_what_it_keeps: "user_defined", 16_385;
    a_cell_of_a_long_formula_is_charged_what_it_keeps: "formula_long", 2_049;
    a_cell_of_a_formula_of_many_nodes_is_charged_what_it_keeps: "formula_nodes", 129;
    a_long_paragraph_is_charged_what_it_keeps: "text_long", 257;
    a_long_kept_text_is_charged_what_it_keeps: "kept_long", 17;
}
