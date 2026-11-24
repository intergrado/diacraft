//! Fuzzing the reader (AGENTS.md: importers are fuzzed): bytes of any kind, drawings this file's grammar builds from the ODF vocabulary
//! with values of any shape, and real drawings damaged byte by byte, give a document or an error with a known class, never a panic; and a
//! document they give holds the model's caps and the index's promises. These run under `proptest` with a fixed budget of cases; the long
//! run is the cargo-fuzz target `odg_read` (`crates/odg/fuzz`). Random input finds crashes and not hangs: the shapes that cost the most
//! are built by hand and counted in `read_hostile.rs`.

use diacraft_odg::{Limits, read_partial};
use proptest::prelude::*;

use crate::drawing::Drawing;

/// The error classes a read may give.
pub const CLASSES: &[&str] = &[
    "not_zip",
    "zip_corrupt",
    "path_traversal",
    "invalid_part_name",
    "zip_ratio_exceeded",
    "part_too_large",
    "package_too_large",
    "too_many_parts",
    "unsupported_compression",
    "encrypted_package",
    "duplicate_part",
    "out_of_memory",
    "xml_malformed",
    "xml_entity_refused",
    "xml_too_complex",
    "missing_manifest",
    "manifest_malformed",
    "not_a_drawing",
    "no_drawing_content",
    "too_many_manifest_entries",
    "too_much_xml",
    "too_many_pages",
    "too_many_styles",
    "too_many_shapes_in_a_page",
    "too_many_shapes",
    "too_many_cells",
    "too_many_items",
    "model_limit",
];

/// What every read must hold.
fn check(bytes: &[u8], limits: &Limits) {
    let (result, report) = read_partial(bytes, limits);
    match result {
        Err(e) => {
            assert!(CLASSES.contains(&e.class()), "an unknown class: {e:?}");
            let _shown = e.to_string();
        }
        Ok(doc) => {
            assert!(doc.check_limits().is_ok());
            assert_eq!(report.index.pages(), doc.pages.len(), "one index page per page");
            for (at, page) in doc.pages.iter().enumerate() {
                let index = report.index.page(at).expect("an index page");
                // A group's members are modelled shapes too: the page's shapes and theirs, found with a stack of their own.
                let mut all: Vec<&diacraft_doc::Shape> = Vec::new();
                let mut stack: Vec<&diacraft_doc::Shape> = page.shapes.iter().map(|s| s.as_ref()).collect();
                while let Some(shape) = stack.pop() {
                    all.push(shape);
                    stack.extend(shape.children.iter().map(|s| s.as_ref()));
                }
                assert_eq!(index.modelled(), all.len(), "every modelled shape indexed");
                for shape in all {
                    assert!(index.ordinal(shape.id).is_some_and(|o| o >= 1 && o <= index.elements()));
                    assert!(shape.id <= index.high_water());
                    odf_data_holds(shape);
                    // The rows of a `Connection` section have one `IX` each.
                    if let Some(section) = shape.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)) {
                        let mut seen = std::collections::HashSet::new();
                        assert!(section.rows.iter().all(|r| seen.insert(r.ix)), "two connection rows of one IX");
                    }
                }
                // A glued end's `ToPart` is 3 or 100 plus a row a section holds, never a saturated number.
                for connect in page.connects.iter() {
                    assert!(connect.to_part.is_none_or(|p| p == 3 || (100..100 + diacraft_doc::MAX_ROWS_PER_SECTION as u32).contains(&p)));
                }
            }
            assert!(report.work.items <= limits.max_items as u64);
            // The document's `.odg` data: at most the cap of a family of each, and a page that holds a master page's name holds the stamp of its cells.
            if let Some(data) = doc.ext.get() {
                assert!(data.odf.definitions.len() <= 6 * diacraft_doc::MAX_DEFINITIONS);
            }
            for page in doc.pages.iter() {
                if let Some(data) = page.ext.get() {
                    assert!(data.odf.master_page.is_none() || data.odf.stamp.is_some());
                }
            }
            check_package_data(bytes, limits, &doc, &report);
            the_model_holds_the_constructs(&doc);
        }
    }
}

/// What the model holds of the constructs its cells do not say, for any drawing (the fuzz target `odg_read` asserts the same): a style sheet's text values are
/// stamped by their group of its row 0 and its other values are of the groups a style gives; the container markup is a chain of elements, one inside the other, with the
/// attributes of the containers among the roots; a paragraph holds at most the cap of levels, the first paragraph of a text opens every list it is in, an element is held
/// without its children, and the runs of a paragraph are in the order they begin (an outer before an inner).
fn the_model_holds_the_constructs(doc: &diacraft_doc::Document) {
    use diacraft_doc::{HeldGroup, RawNode, RowKey, SectionKey, stamp_text_group};
    for style in doc.styles.iter() {
        let Some(data) = style.ext.get() else { continue };
        let row = |section: &str| {
            style
                .sheet
                .section(&SectionKey::new(section, None))
                .and_then(|s| s.row(&RowKey::Ix(0)))
                .map_or_else(diacraft_doc::Row::default, |r| (**r).clone())
        };
        let (paragraph, run) = (row("Paragraph"), row("Character"));
        for value in &data.odf.text_held {
            let row = if value.group.is_paragraph() { &paragraph } else { &run };
            assert_eq!(value.stamp, Some(stamp_text_group(row, value.group)), "a text value is stamped by its row");
        }
        for held in &data.odf.held {
            assert!(
                !matches!(held.group, HeldGroup::Frame | HeldGroup::Glue | HeldGroup::Geometry),
                "a style holds no value of a shape's pose, glue or geometry: {:?}",
                held.group
            );
        }
    }
    if let Some(data) = doc.ext.get() {
        for placed in data.odf.kept.kept.iter() {
            assert!(
                matches!(placed.node, RawNode::Element(_)),
                "a container's markup is an element: the root of its part, with the next container or the child inside it"
            );
        }
        for (name, _) in &data.odf.roots {
            assert!(name.matches('/').count() <= 4, "a root is a part, or a container below it: {name}");
        }
    }
    for page in doc.pages.iter().map(|p| p.as_ref()) {
        let mut stack: Vec<&diacraft_doc::Shape> = page.shapes.iter().map(|s| s.as_ref()).collect();
        while let Some(shape) = stack.pop() {
            stack.extend(shape.children.iter().map(|s| s.as_ref()));
            let Some(text) = shape.text.as_ref().and_then(|t| t.ext.get().map(|d| d.odf.clone())) else { continue };
            for (at, paragraph) in text.paragraphs.iter().enumerate() {
                assert!(paragraph.levels.len() <= diacraft_doc::MAX_LIST_LEVELS);
                if at == 0 {
                    assert!(paragraph.levels.iter().all(|l| l.list.is_some() && l.item.is_some()), "the first paragraph opens every list it is in");
                }
                assert!(paragraph.element.as_ref().is_none_or(|e| e.children.is_empty()));
                assert!(paragraph.runs.iter().all(|r| r.element.as_ref().is_none_or(|e| e.children.is_empty())));
                assert!(
                    paragraph.runs.windows(2).all(|w| w[0].from < w[1].from || (w[0].from == w[1].from && w[0].to >= w[1].to)),
                    "runs are in the order they begin, an outer one first"
                );
            }
        }
    }
}

/// What the `.odg` data of a shape holds for any drawing (the fuzz target `odg_read` asserts the same): a value of the Frame group is judged by the pose and has no stamp, no glue
/// point is held on the shape, the units are the six a length is written in, a glue point is stamped by its row whole, and a text's paragraphs hold a stamp and runs that lie in them.
fn odf_data_holds(shape: &diacraft_doc::Shape) {
    if let Some(data) = shape.ext.get() {
        for held in &data.odf.held {
            if held.group == diacraft_doc::HeldGroup::Frame {
                assert!(held.stamp.is_none());
            }
            assert!(held.group != diacraft_doc::HeldGroup::Glue, "a glue point is held on its row");
        }
        for (_, unit) in data.odf.units.iter() {
            assert!(["cm", "mm", "in", "pt", "pc", "px"].contains(&unit), "{unit}");
        }
    }
    if let Some(section) = shape.sheet.section(&diacraft_doc::SectionKey::new("Connection", None)) {
        for row in section.rows.iter() {
            if let Some(held) = row.ext.get().and_then(|d| d.odf.held.clone()) {
                assert_eq!(held.stamp, Some(diacraft_doc::stamp_row(row)));
            }
        }
    }
    if let Some(text) = shape.text.as_ref().and_then(|t| t.ext.get().map(|d| d.odf.clone())) {
        for paragraph in &text.paragraphs {
            assert!(paragraph.style.stamp.is_some() && paragraph.characters.is_some());
            assert!(paragraph.runs.iter().all(|run| run.from <= run.to));
        }
    }
}

/// What the model holds of the package, the font faces and the document's own parts: the entries are parts of the model once each, none of the
/// ones the model reads as something else, each with the row that names it and every row of the manifest held exactly once (on a part, or by name), the bytes within the media
/// budget, the faces within their cap, and a flat drawing, which has no package, holds none of it.
fn check_package_data(bytes: &[u8], limits: &Limits, doc: &diacraft_doc::Document, report: &diacraft_odg::ReadReport) {
    use std::collections::HashSet;
    assert!(doc.parts.len() <= diacraft_doc::MAX_PARTS);
    assert!(doc.fonts.len() <= 65_536, "the cap of the faces");
    let mut names: HashSet<&str> = HashSet::new();
    let mut held = 0u64;
    for part in doc.parts.iter() {
        assert!(names.insert(&part.name), "a part is held once: {}", part.name);
        assert_eq!(part.format, diacraft_doc::Format::Odf);
        assert!(
            !matches!(&*part.name, "mimetype" | "META-INF/manifest.xml" | "content.xml" | "styles.xml"),
            "{} is read as something else",
            part.name
        );
        assert!(!part.name.ends_with('/') || part.bytes.is_empty(), "a folder holds no bytes");
        assert!(part.rels.is_empty());
        held += part.bytes.len() as u64;
    }
    assert!(held <= limits.max_media_bytes, "the parts are within the media budget");
    assert!(report.work.media_bytes <= limits.max_media_bytes);
    if report.form == diacraft_odg::Form::Flat {
        assert!(
            doc.parts.is_empty()
                && doc.ext.get().is_none_or(|d| d.odf.roots.is_empty() && d.odf.manifest_rows.is_empty() && d.odf.mimetype.is_none())
        );
        return;
    }
    let odf = doc.ext.get().map(|d| &d.odf);
    // `meta.xml` and `settings.xml` are read into fields or are parts, never both and never neither.
    for (name, fields) in [
        ("meta.xml", odf.is_some_and(|d| d.roots.iter().any(|(n, _)| &**n == "meta.xml"))),
        ("settings.xml", odf.is_some_and(|d| d.settings.is_some())),
    ] {
        if report.work.package.inflated > 0
            && diacraft_odg::package::Package::read_kept(bytes, limits, &mut diacraft_odg::Work::default())
                .ok()
                .is_some_and(|p| p.part(name).is_some())
        {
            assert_ne!(names.contains(name), fields, "{name} is read into fields or kept as an entry");
        }
    }
    // Every row of the manifest is held once: on the part it names (the first row of a name) or by name.
    if let Ok(manifest) = diacraft_odg::Manifest::read(bytes, limits) {
        let on_parts = doc.parts.iter().filter(|p| manifest.entry(&p.name).is_some()).count();
        let by_name = odf.map_or(0, |d| d.manifest_rows.len());
        // The rows that name no file are held under the empty name.
        assert_eq!(manifest.entries.len() + manifest.unnamed_rows.len(), on_parts + by_name, "every row of the manifest is held once");
    }
}

/// A length of any shape: units right and wrong, signs, exponents, blanks.
fn length() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just(""), Just("-"), Just("+")],
        0u32..5000,
        prop_oneof![Just("cm"), Just("mm"), Just("in"), Just("pt"), Just("pc"), Just("px"), Just(""), Just("%"), Just("e9cm")],
    )
        .prop_map(|(sign, n, unit)| format!("{sign}{}.{}{unit}", n / 100, n % 100))
}

fn number() -> impl Strategy<Value = String> {
    prop_oneof![
        (-100_000i64..100_000).prop_map(|n| n.to_string()),
        Just("1e308".to_owned()),
        Just("-0".to_owned()),
        Just(".".to_owned()),
        Just("nan".to_owned()),
        "[0-9.eE+-]{0,6}"
    ]
}

/// SVG path data of any shape: commands, numbers and garbage.
fn path_data() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            "[MmLlHhVvCcSsQqTtAaZzX]".prop_map(|c| format!(" {c}")),
            number().prop_map(|n| format!(" {n}")),
            Just(",".to_owned()),
            Just("0 0 1 1".to_owned())
        ],
        0..30,
    )
    .prop_map(|parts| parts.concat())
}

fn transform() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            number().prop_map(|n| format!("rotate ({n})")),
            (length(), length()).prop_map(|(a, b)| format!("translate ({a} {b})")),
            (number(), number()).prop_map(|(a, b)| format!("scale ({a} {b})")),
            number().prop_map(|n| format!("skewX ({n})")),
            (number(), number(), length(), length()).prop_map(|(a, b, e, f)| format!("matrix ({a} 0 0 {b} {e} {f})")),
            "[a-z( )0-9]{0,8}"
        ],
        0..4,
    )
    .prop_map(|terms| terms.join(" "))
}

fn colour() -> impl Strategy<Value = String> {
    prop_oneof!["#[0-9a-fA-F]{6}", "#[0-9a-z]{0,8}", Just("red".to_owned())]
}

/// An ODF formula of any shape: the grammar's pieces in any order, and garbage.
fn formula() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            number(),
            Just("+".to_owned()),
            Just("-".to_owned()),
            Just("*".to_owned()),
            Just("/".to_owned()),
            Just("(".to_owned()),
            Just(")".to_owned()),
            Just(",".to_owned()),
            "[?][a-f][0-3]".prop_map(|n| n),
            "[$][0-4]".prop_map(|n| n),
            "(pi|left|top|right|bottom|width|height|logwidth|logheight|xstretch|ystretch|hasfill|hasstroke)".prop_map(|n| n),
            "(abs|sqrt|sin|cos|tan|atan|min|max|atan2|if)[(]".prop_map(|n| n),
            "[a-z]{1,4}"
        ],
        0..14,
    )
    .prop_map(|parts| parts.join(" "))
}

/// A path of an enhanced geometry: commands, numbers, modifiers, equation names and garbage.
fn enhanced_path() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            "[MLCQZNFSTUABVWXYGmx]".prop_map(|c| format!(" {c}")),
            number().prop_map(|n| format!(" {n}")),
            "[?][a-f][0-3]".prop_map(|n| format!(" {n}")),
            "[$][0-4]".prop_map(|n| format!(" {n}")),
            "(left|top|right|bottom|width|height)".prop_map(|n| format!(" {n}")),
            Just(" 0 0 100 100 0 90".to_owned())
        ],
        0..40,
    )
    .prop_map(|parts| parts.concat())
}

/// A `draw:enhanced-geometry` with attributes and children of any value.
fn geometry() -> impl Strategy<Value = String> {
    (
        enhanced_path(),
        prop::collection::vec(formula(), 0..6),
        prop::collection::vec(number(), 0..5),
        prop::collection::vec(number(), 0..8),
        (number(), number(), "(true|false|x)?", "(none|path)?", "(ooxml-rect|ooxml-roundRect|ooxml-star5|mso-spt1|non-primitive)"),
    )
        .prop_map(|(path, formulas, mods, glue, (a, b, mirror, glue_type, ty))| {
            let equations: String = formulas.iter().enumerate().map(|(i, f)| format!(r#"<draw:equation draw:name="{}" draw:formula="{f}"/>"#, ["f0", "f1", "f2", "f3", "a", "b"][i % 6])).collect();
            let handles = format!(r#"<draw:handle draw:handle-position-x="$0" draw:handle-position-y="{a}"/><draw:handle draw:handle-position="?f0 $1" draw:handle-polar="{b} {a}"/><draw:handle draw:handle-position="$0 top" draw:handle-range-x-minimum="{a}" draw:handle-radius-range-maximum="{b}"/>"#);
            format!(
                r#"<draw:enhanced-geometry svg:viewBox="0 0 {a} {b}" draw:type="{ty}" draw:modifiers="{}" draw:glue-points="{}" draw:glue-point-type="{glue_type}" draw:text-areas="{a} {b} ?f0 ?f1" draw:text-rotate-angle="{a}" draw:mirror-horizontal="{mirror}" draw:enhanced-path="{path}">{equations}{handles}</draw:enhanced-geometry>"#,
                mods.join(" "),
                glue.join(" ")
            )
        })
}

/// One shape element of the vocabulary, with attributes of any value.
fn shape() -> impl Strategy<Value = String> {
    let frame = (length(), length(), length(), length(), transform(), "(gr[0-3])?", "(P[0-2])?", "(layout|Notes|L[0-9])?", "[0-9-]{0,3}");
    let extra = ("(s[0-3])?", geometry(), "(s[0-3]|none)?", "([0-9a-z-]{0,2}|4294967295|320000|319999)");
    (frame, 0usize..19, path_data(), number(), number(), prop::collection::vec(text_piece(), 0..5), extra).prop_map(
        |((x, y, w, h, t, style, text_style, layer, z), kind, d, a, b, text, (id, geo, target, point))| {
            let ident = if id.is_empty() { String::new() } else { format!(r#" xml:id="{id}""#) };
            let common = format!(
                r#"{ident} svg:x="{x}" svg:y="{y}" svg:width="{w}" svg:height="{h}" draw:transform="{t}" draw:style-name="{style}" draw:text-style-name="{text_style}" draw:layer="{layer}" draw:z-index="{z}""#
            );
            let body = if a.len() % 3 == 0 { format!("<text:p>{}</text:p>", text.concat()) } else { format!("<text:h text:outline-level=\"2\">{}</text:h><text:list><text:list-item><text:p>{}</text:p></text:list-item></text:list>", text.concat(), text.concat()) };
            match kind {
                0 => format!(r#"<draw:rect {common} draw:corner-radius="{x}">{body}</draw:rect>"#),
                1 => format!(r#"<draw:ellipse {common} draw:kind="section" draw:start-angle="{a}" draw:end-angle="{b}">{body}</draw:ellipse>"#),
                2 => format!(r#"<draw:circle svg:cx="{x}" svg:cy="{y}" svg:r="{w}" draw:kind="arc" draw:start-angle="{a}"/>"#),
                3 => format!(r#"<draw:line svg:x1="{x}" svg:y1="{y}" svg:x2="{w}" svg:y2="{h}" draw:transform="{t}"/>"#),
                4 => format!(r#"<draw:polyline {common} svg:viewBox="0 0 {a} {b}" draw:points="{a},{b} 1,1 {b},{a}"/>"#),
                5 => format!(r#"<draw:polygon {common} svg:viewBox="{a} {b} 10 10" draw:points="0,0 {a},{b}"/>"#),
                6 => format!(r#"<draw:path {common} svg:viewBox="0 0 {a} {b}" svg:d="{d}"/>"#),
                7 => format!(r#"<draw:regular-polygon {common} draw:corners="{a}" draw:concave="true" draw:sharpness="{b}%"/>"#),
                8 => format!(r#"<draw:frame {common}><draw:text-box>{body}</draw:text-box></draw:frame>"#),
                9 => format!(r#"<draw:frame {common}><draw:image xlink:href="Pictures/{a}.png"><office:binary-data>{d}</office:binary-data></draw:image></draw:frame>"#),
                10 => format!(r#"<draw:g>{body}<draw:rect {common}/></draw:g>"#),
                11 => format!(r#"<draw:custom-shape {common}>{geo}{body}</draw:custom-shape>"#),
                12 => format!(
                    r#"<draw:custom-shape {common}><draw:glue-point draw:id="{a}" svg:x="{x}" svg:y="{y}" draw:align="top-left"/><draw:glue-point draw:id="4" svg:x="{y}" svg:y="{x}"/><draw:glue-point draw:id="4" svg:x="{x}" svg:y="{x}"/><draw:glue-point draw:id="4294967295" svg:x="{x}" svg:y="{y}"/>{geo}</draw:custom-shape>"#
                ),
                13 => format!(
                    r#"<draw:connector {common} draw:start-shape="{target}" draw:start-glue-point="{point}" draw:end-shape="{target}" draw:end-glue-point="{point}" draw:type="curve" svg:d="{d}" svg:x1="{x}" svg:y1="{y}" svg:x2="{w}" svg:y2="{h}">{body}</draw:connector>"#
                ),
                14 => format!(r#"<draw:measure svg:x1="{x}" svg:y1="{y}" svg:x2="{w}" svg:y2="{h}"/><draw:caption {common}/><dr3d:scene {common}/><draw:control {common}/>"#),
                15 => format!(r#"<draw:frame {common}><draw:object xlink:href="./O"/><draw:image xlink:href="Pictures/{a}.png"/></draw:frame>"#),
                16 => format!(r#"<draw:g {common}>{geo}<draw:g><draw:custom-shape {common}/></draw:g><draw:connector {common}/></draw:g>"#),
                17 => format!(r#"<draw:custom-shape {common}><draw:enhanced-geometry draw:extrusion="true"/></draw:custom-shape><draw:custom-shape {common}><draw:enhanced-geometry draw:text-path="true"/></draw:custom-shape>"#),
                _ => format!(r#"<text:list text:style-name="L1"><text:list-item>{body}</text:list-item></text:list>"#),
            }
        },
    )
}

fn text_piece() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z \t\n]{0,6}",
        number().prop_map(|n| format!(r#"<text:s text:c="{n}"/>"#)),
        Just("<text:tab/>".to_owned()),
        Just("<text:line-break/>".to_owned()),
        "[a-z]{0,4}".prop_map(|t| format!(r#"<text:span text:style-name="T1">{t}</text:span>"#)),
        "[0-9]{0,3}".prop_map(|t| format!("<text:page-number>{t}</text:page-number>")),
        "[a-z]{0,3}".prop_map(|t| format!(r#"<text:a xlink:href="x">{t}</text:a>"#)),
        // A mark, fields the text has no cell for, a span with an attribute of its own, a hyperlink holding a span, a field in a hyperlink.
        Just(r#"<text:bookmark text:name="b"/>"#.to_owned()),
        "[a-z]{0,3}".prop_map(|t| format!("<text:title>{t}</text:title>")),
        "[a-z]{0,3}".prop_map(|t| format!(r#"<text:date text:fixed="true" style:data-style-name="N1">{t}</text:date>"#)),
        "[a-z]{0,3}".prop_map(|t| format!(r#"<text:span text:style-name="T1" text:class-names="k">{t}</text:span>"#)),
        "[a-z]{0,3}".prop_map(|t| format!(r#"<text:a xlink:href="x"><text:span text:style-name="T1">{t}</text:span><text:page-number/></text:a>"#)),
    ]
}

/// Styles of any properties, with parents that may name anything, themselves included.
fn styles() -> impl Strategy<Value = (String, String)> {
    let style = (
        0usize..4,
        "(gr[0-3]|S[0-2]|P[0-2])?",
        length(),
        colour(),
        number(),
        prop::sample::select(vec!["none", "solid", "dash", "gradient", "hatch", "bitmap", "x"]),
    );
    prop::collection::vec(style, 0..8).prop_map(|list| {
        let (mut common, mut auto) = (String::new(), String::new());
        for (i, (kind, parent, len, col, n, fill)) in list.into_iter().enumerate() {
            let props = format!(
                r#"<style:graphic-properties draw:fill="{fill}" draw:fill-color="{col}" svg:stroke-width="{len}" draw:stroke="{fill}" draw:stroke-dash="D" draw:marker-end="DiaCraft_20_end_20_{n}" draw:marker-end-width="{len}" draw:opacity="{n}%" fo:padding="{len}" draw:fill-gradient-name="G" diacraft:id="{n}" diacraft:links="S0 S1 {parent}"/><style:paragraph-properties fo:text-align="end" fo:line-height="{n}%" fo:margin-left="{len}"><style:tab-stops><style:tab-stop style:position="{len}"/></style:tab-stops></style:paragraph-properties><style:text-properties fo:font-size="{n}%" fo:font-weight="{n}" fo:color="{col}" style:font-name="F"/>"#
            );
            let parent = if parent.is_empty() { String::new() } else { format!(r#" style:parent-style-name="{parent}""#) };
            match kind {
                0 => common.push_str(&format!(r#"<style:style style:name="S{i}" style:family="graphic"{parent}>{props}</style:style>"#)),
                1 => common.push_str(&format!(r#"<style:style style:name="P{i}" style:family="paragraph"{parent}>{props}</style:style>"#)),
                2 => auto.push_str(&format!(r#"<style:style style:name="gr{i}" style:family="graphic"{parent}>{props}</style:style>"#)),
                _ => common.push_str(&format!(r#"<style:default-style style:family="graphic">{props}</style:default-style><draw:stroke-dash draw:name="D" draw:dots1="{n}" draw:dots1-length="{len}" draw:distance="{len}"/><draw:marker draw:name="DiaCraft_20_end_20_{n}"/><draw:gradient draw:name="G" draw:style="axial" draw:start-color="{col}" draw:border="{n}%"/>"#)),
            }
        }
        (common, auto)
    })
}

/// A drawing of any small shape the grammar makes.
fn drawing() -> impl Strategy<Value = Vec<u8>> {
    (styles(), prop::collection::vec(prop::collection::vec(shape(), 0..6), 1..3), any::<bool>()).prop_map(|((common, auto), pages, no_styles)| {
        let mut d = Drawing::a4();
        d.common = common;
        d.auto = format!(
            r#"{auto}<style:style style:name="T1" style:family="text"><style:text-properties fo:font-style="italic"/></style:style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style>"#
        );
        d.no_styles = no_styles;
        // What the containers hold that no field reads: a child of the master styles and of a master page, a script and an attribute of the body.
        d.ns.push_str(r#" xmlns:x="urn:example:extension""#);
        d.masters.push_str(r#"<x:m a="1"/><style:master-page style:name="M2" style:page-layout-name="PM1"><x:p/></style:master-page>"#);
        d.common.push_str(concat!(
            r##"<style:style style:name="Z0" style:family="paragraph"><style:paragraph-properties fo:text-align="start"><style:tab-stops><style:tab-stop style:position="1cm" style:leader-text="."/></style:tab-stops></style:paragraph-properties>"##,
            r##"<style:text-properties style:text-underline-style="wave" fo:text-transform="lowercase"/></style:style>"##
        ));
        for shapes in pages {
            d = d.page(&shapes.concat());
        }
        d.pages.push_str(r#"<x:k/>"#);
        d.bytes()
    })
}

/// A field of `meta.xml` of any kind: the ones both formats say, a user-defined one, a comment, one of a producer's, and garbage.
fn meta_field() -> impl Strategy<Value = String> {
    prop_oneof![
        ("(title|description|subject|language|date|creator)", "[a-z ]{0,6}").prop_map(|(n, t)| format!("<dc:{n}>{t}</dc:{n}>")),
        ("(keyword|initial-creator|creation-date|print-date|generator|editing-cycles)", "[a-z0-9 ]{0,6}")
            .prop_map(|(n, t)| format!("<meta:{n}>{t}</meta:{n}>")),
        ("[a-zA-Z0-9]{0,3}", "(string|float|date|boolean|)", "[a-z0-9]{0,4}")
            .prop_map(|(n, t, v)| format!(r#"<meta:user-defined meta:name="{n}" meta:value-type="{t}" x:k="v">{v}</meta:user-defined>"#)),
        Just("<meta:user-defined>no name</meta:user-defined>".to_owned()),
        Just("<meta:user-defined meta:name=\"n\"><x:child/></meta:user-defined>".to_owned()),
        Just("<!-- a comment --><?pi data?>".to_owned()),
        Just("<dc:title xml:lang=\"en\">with an attribute</dc:title><dc:title/>".to_owned()),
        Just(r#"<meta:document-statistic meta:page-count="1"/><x:unknown a="1"><y>text</y></x:unknown>"#.to_owned()),
        "[a-z <>/&;]{0,8}",
    ]
}

/// A `settings.xml` of any small tree of configuration items.
fn settings_items() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            ("[A-Za-z]{1,6}", "(int|boolean|string|short)", "[a-z0-9 ]{0,4}")
                .prop_map(|(n, t, v)| format!(r#"<config:config-item config:name="{n}" config:type="{t}">{v}</config:config-item>"#)),
            Just(r#"<config:config-item-set config:name="ooo:view-settings"><config:config-item-map-indexed config:name="Views"><config:config-item-map-entry><config:config-item config:name="ScaleNumerator" config:type="int">1</config:config-item></config:config-item-map-entry></config:config-item-map-indexed></config:config-item-set>"#.to_owned()),
            Just(r#"<config:config-item-set config:name="ooo:configuration-settings"><config:config-item config:name="ScaleNumerator" config:type="int">3</config:config-item><config:config-item config:name="ScaleDenominator" config:type="int">0</config:config-item></config:config-item-set>"#.to_owned()),
        ],
        0..5,
    )
    .prop_map(|items| items.concat())
}

/// The rows of a manifest, any of the names a package holds and some it lacks, any attributes, some with no name.
fn manifest_rows() -> impl Strategy<Value = String> {
    let name = prop::sample::select(vec![
        "/",
        "content.xml",
        "styles.xml",
        "meta.xml",
        "settings.xml",
        "Fonts/a.ttf",
        "Fonts/b/",
        "Basic/m.xml",
        "Object 1/",
        "Thumbnails/thumbnail.png",
        "Pictures/p.png",
        "gone",
        "",
        "x y",
    ]);
    prop::collection::vec((name, "(text/xml|image/png|)", "[a-z0-9]{0,3}"), 0..10).prop_map(|rows| {
        rows.into_iter()
            .map(|(n, t, v)| {
                let path = if n.is_empty() { String::new() } else { format!(r#" manifest:full-path="{n}""#) };
                format!(r#"<manifest:file-entry{path} manifest:media-type="{t}" manifest:size="{v}" x:k="{v}"/>"#)
            })
            .collect()
    })
}

/// A package of an honest drawing with `meta.xml` and `settings.xml` of any fields, font faces of any kind, some entries and a manifest of any rows; `None` when a name repeats.
fn package_data() -> impl Strategy<Value = Option<Vec<u8>>> {
    let entry = (
        prop::sample::select(vec![
            "Fonts/a.ttf",
            "Fonts/b/",
            "Basic/m.xml",
            "Object 1/content.xml",
            "Thumbnails/thumbnail.png",
            "Pictures/p.png",
            "Pictures/q.png",
            "x/y",
            "A",
            "a",
        ]),
        prop::collection::vec(any::<u8>(), 0..24),
    );
    let faces = prop::collection::vec(("[A-C]", "(fixed|variable|)", any::<bool>()), 0..5);
    (
        prop::collection::vec(meta_field(), 0..8),
        any::<bool>(),
        settings_items(),
        any::<bool>(),
        faces,
        prop::collection::vec(entry, 0..5),
        manifest_rows(),
    )
        .prop_map(|(fields, with_meta, settings, with_settings, faces, entries, rows)| {
            let mut d = Drawing::a4().page(r#"<draw:frame svg:width="1cm" svg:height="1cm"><draw:image xlink:href="Pictures/p.png"/></draw:frame>"#);
            if with_meta {
                d.meta = Some(format!(
                    "<?xml version=\"1.0\"?>\n<office:document-meta {} xmlns:x=\"urn:x\" office:version=\"1.3\"><office:meta>{}</office:meta></office:document-meta>",
                    crate::drawing::NS,
                    fields.concat()
                ));
            }
            if with_settings {
                d.settings = Some(crate::drawing::settings(&settings));
            }
            d.fonts = faces.iter().map(|(n, p, src)| {
                let src = if *src { r#"<svg:font-face-src><svg:font-face-uri xlink:href="Fonts/a.ttf"/></svg:font-face-src>"# } else { "" };
                format!(r#"<style:font-face style:name="{n}" svg:font-family="{n}" style:font-pitch="{p}">{src}</style:font-face>"#)
            }).collect();
            let mut files = d.files();
            let mut seen: std::collections::HashSet<String> = files.iter().map(|(n, _)| n.clone()).collect();
            for (name, bytes) in entries {
                if !seen.insert(name.to_owned()) {
                    return None;
                }
                files.push((name.to_owned(), bytes));
            }
            let manifest = format!(
                "<?xml version=\"1.0\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" xmlns:x=\"urn:x\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{}\"/>{rows}</manifest:manifest>",
                crate::build::DRAWING
            );
            files.push(("META-INF/manifest.xml".to_owned(), manifest.into_bytes()));
            Some(crate::build::write(Some(crate::build::DRAWING), &files))
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn packages_of_any_data_give_a_document_or_an_error(bytes in package_data()) {
        if let Some(bytes) = bytes {
            check(&bytes, &Limits::default());
        }
    }

    /// The budget of the media is every part's and every picture's: a package read within a small one is refused or holds no more bytes than it.
    #[test]
    fn tight_media_budgets_refuse_with_a_class_and_never_half_read(bytes in package_data(), media in 0u64..80, items in 0usize..2_000) {
        if let Some(bytes) = bytes {
            check(&bytes, &Limits { max_media_bytes: media, max_items: items, ..Limits::default() });
        }
    }

    #[test]
    fn any_bytes_give_a_document_or_an_error(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        check(&bytes, &Limits::default());
    }

    #[test]
    fn drawings_of_any_small_shape_give_a_document_or_an_error(bytes in drawing()) {
        check(&bytes, &Limits::default());
    }

    #[test]
    fn a_damaged_content_part_gives_a_document_or_an_error(at in 0usize..4096, byte in any::<u8>(), cut in any::<bool>()) {
        let d = Drawing::a4().page(r##"<draw:rect draw:name="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm" draw:transform="rotate (0.5) translate (1cm 2cm)"><text:p>a<text:s text:c="2"/>b</text:p></draw:rect><draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="M0 0 A 5 5 30 1 1 10 10 Z"/>"##);
        let mut content = d.content_xml().into_bytes();
        let at = at % content.len();
        if cut { content.truncate(at) } else if let Some(b) = content.get_mut(at) { *b = byte }
        let mut files = d.files();
        if let Some(first) = files.first_mut() {
            first.1 = content;
        }
        check(&crate::build::package(crate::build::DRAWING, files), &Limits::default());
    }

    #[test]
    fn tight_limits_refuse_with_a_class_and_never_half_read(bytes in drawing(), items in 0usize..400, cells in 0usize..200) {
        let limits = Limits { max_items: items, max_cells: cells, ..Limits::default() };
        check(&bytes, &limits);
    }
}

/// Writes the seeds of the cargo-fuzz target `odg_read` into the folder `ODG_FUZZ_SEEDS` names: drawings the grammar makes, each as its
/// `content.xml` behind a first byte of 0 (the target's switch for wrapping it in a package), and a few whole packages. Run by hand.
#[test]
#[ignore = "writes the fuzz seeds when ODG_FUZZ_SEEDS names a folder; run by hand"]
fn write_fuzz_seeds() {
    let Some(dir) = std::env::var_os("ODG_FUZZ_SEEDS") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let tree = |runner: &mut proptest::test_runner::TestRunner| drawing().new_tree(runner).unwrap();
    for k in 0..64 {
        use proptest::strategy::ValueTree;
        let bytes = tree(&mut runner).current();
        std::fs::write(dir.join(format!("package-{k}")), &bytes).unwrap();
    }
    let mut d = Drawing::a4().page(r#"<draw:rect draw:name="a" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2.5cm" draw:transform="rotate (0.5) translate (1cm 2cm)"><text:p>a<text:s text:c="2"/>b</text:p></draw:rect><draw:path svg:width="1cm" svg:height="1cm" svg:viewBox="0 0 10 10" svg:d="M0 0 A 5 5 30 1 1 10 10 Z"/><draw:frame svg:width="2cm" svg:height="1cm"><draw:text-box><text:list text:style-name="L1"><text:list-item><text:p>x<text:span text:style-name="T1">y</text:span></text:p></text:list-item></text:list></draw:text-box></draw:frame>"#);
    d.auto = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><text:list-style style:name="L1"><text:list-level-style-bullet text:level="1" text:bullet-char="x"/></text:list-style>"#.to_owned();
    let mut seed = vec![0u8];
    seed.extend(d.content_xml().into_bytes());
    std::fs::write(dir.join("content-0"), seed).unwrap();
}
