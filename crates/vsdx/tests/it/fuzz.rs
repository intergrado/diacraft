//! Fuzzing the reader (AGENTS.md: importers are fuzzed): a built package, mutated at two levels, must read or be refused, never panic, never hang,
//! and read the same twice.
//!
//! Mutating the archive's bytes is stopped by the container's checksums nearly always, which is the container's own test (`opc`'s fuzz). To reach
//! the reader above it the XML of each part is mutated and the archive written again with right checksums: the reader meets what a hostile file
//! made by hand would be. **A clean run of this finds nothing a crafted input does not** (25 million clean runs missed a 972 KB file that hung
//! the container): the cost of a file's size is in `timing.rs`, built by hand.

use std::time::{Duration, Instant};

use diacraft_opc::Package;
use diacraft_testkit::package::{DIR, MasterSpec, OD, PageSpec, Pkg, VsdxBuilder};
use diacraft_vsdx::{Limits, ReadReport, read, read_partial};
use proptest::prelude::*;

/// A package that has a little of everything the reader reads.
pub(crate) fn seed_package() -> Pkg {
    seed_builder().pkg()
}

/// The seed with its theme part listed in the content types and no relationship to it: the reader finds the theme by its content type.
pub(crate) fn orphan_seed_package() -> Pkg {
    seed_package().without_rels_of_type(&format!("{DIR}/document.xml"), &format!("{OD}theme"))
}

/// The builder of [`seed_package`], for a test that changes the variant or adds to it.
pub(crate) fn seed_builder() -> VsdxBuilder {
    let shapes = r##"<Shape ID="1" Type="Group" Master="1" LineStyle="1"><Cell N="Width" V="2" F="Height*2" U="IN"/><Cell N="X" V="1" F="Inh"/><Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/></Row></Section><Section N="User"><Row N="a"><Cell N="Value" V="x" E="#N/A"/></Row></Section><Text><cp IX="0"/>ab<fld IX="0">3</fld></Text><Data1>d</Data1><Shapes><Shape ID="2" MasterShape="4"/><Shape ID="3" Type="Foreign"><ForeignData ForeignType="Bitmap"><Rel r:id="rId1"/></ForeignData></Shape></Shapes><Trigger N="RecalcColor"/><x:n xmlns:x="urn:z"/></Shape>"##;
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:srgbClr val="000000"/></a:dk1></a:clrScheme><a:fontScheme name="F"><a:majorFont><a:latin typeface="X"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
    VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .face_names(r#"<FaceName NameU="Sans" Panose="1"/>"#)
        .more_styles(r#"<StyleSheet ID="1" LineStyle="0"><Cell N="LineWeight" V="1" F="THEMEVAL()"/></StyleSheet>"#)
        .document_sheet(r#"<Cell N="DocLangID" V="1033"/>"#)
        .document_extra("<DocumentSettings TopPage=\"0\"/><x:y xmlns:x=\"urn:q\"/>")
        .theme(theme.as_bytes())
        .solution("DiaCraft", "<SolutionXML Name=\"DiaCraft\"><dc:Data xmlns:dc=\"urn:app.diacraft:extension\" dc:version=\"1\"/></SolutionXML>")
        .master(MasterSpec::new(1, "M").shapes(r#"<Shape ID="4"><Cell N="PinX" V="1" F="Width/2"/></Shape>"#).icon("AAAB"))
        .page(
            PageSpec::new(1, "P")
                .shapes(shapes)
                .connects(r#"<Connect FromSheet="1" FromCell="BeginX" FromPart="9" ToSheet="2" ToCell="PinX" ToPart="3"/>"#)
                .rel("rId1", &format!("{OD}image"), "../media/i.png"),
        )
        .page(PageSpec::new(2, "Q").attrs(r#" BackPage="1""#).without_part())
        .with(Pkg::new().part("/visio/media/i.png", "image/png", [1u8, 2, 3]))
}

/// Fragments that change what an XML reader sees: a tag, an entity, a namespace, a quote, a number at an edge.
const SNIPPETS: &[&str] = &[
    "<",
    ">",
    "</",
    "/>",
    "&",
    "&amp;",
    "&#0;",
    "&#x110000;",
    "&bogus;",
    "\"",
    "'",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "<?",
    "?>",
    "<!DOCTYPE x>",
    "xmlns=\"urn:z\"",
    "xmlns:r=\"urn:z\"",
    " ID=\"4294967296\"",
    " ID=\"-1\"",
    " IX=\"",
    "<Cell N=\"X\"/>",
    "<Shape ID=\"1\">",
    "</Shape>",
    "<Shapes>",
    "</Shapes>",
    "<Section N=\"Geometry\"/>",
    "<Row/>",
    "\u{0}",
    "\u{FFFE}",
    "\u{1F600}",
    "\r\n",
    "1.#INF",
    "Themed",
    "Inh",
    "No Formula",
    "N=\"\"",
];

/// A tiny generator: the seed makes the mutation, so a failing seed reproduces.
pub(crate) struct Gen(pub(crate) u64);

impl Gen {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

pub(crate) fn mutate(g: &mut Gen, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.extend_from_slice(b"<");
        return;
    }
    for _ in 0..1 + g.below(4) {
        let at = g.below(bytes.len());
        match g.below(7) {
            0 => bytes[at] ^= 1 << g.below(8),
            1 => {
                let end = (at + 1 + g.below(40)).min(bytes.len());
                bytes.drain(at..end);
            }
            2 => {
                let s = SNIPPETS[g.below(SNIPPETS.len())];
                bytes.splice(at..at, s.bytes());
            }
            3 => {
                let end = (at + 1 + g.below(200)).min(bytes.len());
                let chunk: Vec<u8> = bytes[at..end].to_vec();
                let to = g.below(bytes.len());
                bytes.splice(to..to, chunk);
            }
            4 => bytes.truncate(at),
            5 => {
                let other = g.below(bytes.len());
                bytes.swap(at, other);
            }
            _ => {
                let s = SNIPPETS[g.below(SNIPPETS.len())];
                let end = (at + 1 + g.below(10)).min(bytes.len());
                bytes.splice(at..end, s.bytes());
            }
        }
        if bytes.is_empty() {
            bytes.push(b'<');
        }
    }
}

/// What a document that read holds of its package (the plan's D5): **the bytes of each held part are the package's, exactly**, no two held parts are one
/// part (ASCII case folded), the report says how many and how many bytes, a held part is never the document part or a part the model names as a page's or a master's, the
/// rows held come from parts of the package that are not held, and nothing is skipped.
fn held_is_the_packages(bytes: &[u8], doc: &diacraft_doc::Document, report: &ReadReport) {
    let package = Package::read(bytes, &Limits::default().opc).expect("a package that read opens again");
    let mut names = std::collections::HashSet::new();
    let mut sum = 0u64;
    // The parts of the other format that DiaCraft's part carries are the model's, and are no part of this package: they came from the items, and the parts that held their
    // bytes are not held.
    for part in doc.parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx) {
        let original = package.part(&part.name).unwrap_or_else(|| panic!("a held part, {:?}, that the package does not have", part.name));
        assert_eq!(&*part.bytes, original, "the held bytes are the package's");
        assert!(names.insert(part.name.to_ascii_lowercase()), "two held parts are one part: {:?}", part.name);
        assert!(!part.name.eq_ignore_ascii_case(&report.document_part), "the document part is held");
        assert!(
            !diacraft_opc::name::is_rels_part(&part.name) || part.rels.is_empty(),
            "a relationships part held as bytes has no relationships of its own"
        );
        sum += part.bytes.len() as u64;
    }
    assert_eq!((report.held_parts, report.held_bytes), (doc.parts.iter().filter(|p| p.format == diacraft_doc::Format::Vsdx).count(), sum));
    assert!(report.skipped_counts.is_empty(), "{:?}", report.skipped_counts);
    for named in doc.pages.iter().filter_map(|p| p.part_name.as_ref()).chain(doc.masters.iter().filter_map(|m| m.part_name.as_ref())) {
        assert!(package.part(named).is_some(), "a page or master names a part the package has: {named:?}");
        assert!(!names.contains(&named.to_ascii_lowercase()), "a part that is a page's or a master's is not held: {named:?}");
    }
    if let Some(held) = &doc.vsdx {
        for row in &held.rels {
            assert!(&*row.source == "/" || package.part(&row.source).is_some(), "a held row comes from a part of the package: {:?}", row.source);
            assert!(!names.contains(&row.source.to_ascii_lowercase()), "the rows of a held part are its own: {:?}", row.source);
        }
    }
}

/// What must hold of the model writer's answer for any model (the plan's D5): **it writes a package that reads, or refuses in words**, never panics, and **writes the same bytes
/// twice**. (What it writes is compared with what was read by `model_writer.rs`; a mutated package holds what no honest one does, and its model is written as it is or refused.)
fn model_writer_holds(doc: &diacraft_doc::Document, variant: Option<diacraft_vsdx::Variant>) {
    let options =
        diacraft_vsdx::WriteOptions { variant, now: Some(1_700_000_000), content_unchanged: true, ..diacraft_vsdx::WriteOptions::default() };
    let started = Instant::now();
    let first = diacraft_vsdx::write_with(doc, None, &options);
    assert!(started.elapsed() < Duration::from_secs(60), "a write took {:?}", started.elapsed());
    match first {
        Ok(written) => {
            read(&written.bytes, &Limits::default()).expect("a package the model writer made reads");
            let again = diacraft_vsdx::write_with(doc, None, &options).expect("a model that was written is written again");
            assert!(written.bytes == again.bytes, "one model, two writes, two packages");
        }
        Err(e) => assert!(!e.to_string().chars().any(char::is_control), "{e:?}"),
    }
}

/// Reads twice, within a bound, and checks what must hold of any outcome. (The harness over what reads is fuzzed in `diacraft-cli`.)
fn check(bytes: &[u8]) {
    let limits = Limits::default();
    let started = Instant::now();
    let first = read(bytes, &limits);
    let took = started.elapsed();
    // A hang guard and no measure of cost: a package the fuzz makes reads in milliseconds, and a minute is generous for a machine at load 50.
    assert!(took < Duration::from_secs(60), "a package of {} bytes took {took:?}", bytes.len());
    let second = read(bytes, &limits);
    match (first, second) {
        (Ok((a, ra)), Ok((b, rb))) => {
            assert!(a == b && ra == rb, "reading twice gave two documents");
            a.check_limits().expect("a document that read passes the model's own caps");
            held_is_the_packages(bytes, &a, &ra);
            model_writer_holds(&a, ra.variant);
        }
        (Err(a), Err(b)) => {
            assert_eq!(a, b);
            // The message never holds a control character, whatever the file said.
            assert!(!a.to_string().chars().any(char::is_control), "{a:?}");
        }
        (a, b) => panic!("reading twice gave {} and {}", a.is_ok(), b.is_ok()),
    }
}

/// Reads with a pool of any size beside the file and the package: **a read or a refusal for memory in words, and every tree the read made was held in the pool** (
/// the parse is bounded before the tree is made, so a tree that was made, and counted, was charged, and a refusal is never a tree made and then found not to fit). The pool is
/// small enough that the parse of a part, a relationships part, a theme or the solutions index is refused on the way in some of the cases.
fn check_pool(bytes: &[u8], pool: u64) {
    let (_, first) = read_partial(bytes, &Limits::default());
    let room = first.stats.file_bytes + first.stats.uncompressed_bytes + diacraft_vsdx::cost::GROWTH_BYTES;
    let (result, report) = read_partial(bytes, &Limits { max_memory: room + pool, ..Limits::default() });
    assert!(
        report.stats.max_tree_bytes <= report.stats.memory_peak,
        "a tree of {} bytes was made and the pool held {} at the most (a pool of {pool}): {:?}",
        report.stats.max_tree_bytes,
        report.stats.memory_peak,
        report.stats
    );
    assert!(report.stats.memory_peak <= room + pool, "the pool was passed: {:?}", report.stats);
    if let Err(e) = result {
        assert!(!e.to_string().chars().any(char::is_control), "{e:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, failure_persistence: None, ..ProptestConfig::default() })]

    /// The XML of the parts, mutated, read with a pool of any size beside the file: a read, or a refusal in words, and no tree made that the pool did not hold.
    #[test]
    fn mutated_xml_read_in_a_pool_of_any_size_makes_no_tree_the_pool_does_not_hold(seed in any::<u64>(), pool in 0u64..600_000) {
        let mut g = Gen(seed | 1);
        let names = seed_package().part_names();
        let chosen = names[g.below(names.len())].clone();
        let bytes = seed_package().map_parts(|name, part| {
            if *name == chosen {
                let mut inner = Gen(g.next() | 1);
                mutate(&mut inner, part);
            }
        }).build();
        check_pool(&bytes, pool);
    }

    /// The XML of the parts, mutated and written again with right checksums.
    #[test]
    fn mutated_xml_in_a_package_with_right_checksums_never_panics_or_hangs(seed in any::<u64>()) {
        let mut g = Gen(seed | 1);
        let names = seed_package().part_names();
        let mut chosen = vec![names[g.below(names.len())].clone()];
        if g.below(3) == 0 {
            chosen.push(names[g.below(names.len())].clone());
        }
        let bytes = seed_package().map_parts(|name, part| {
            if chosen.iter().any(|c| c == name) {
                let mut inner = Gen(g.next() | 1);
                mutate(&mut inner, part);
            }
        }).build();
        check(&bytes);
    }

    /// The same, where the theme part has no relationship and is found by its content type: the part is mutated like any other, and so are the content types.
    #[test]
    fn mutated_xml_where_the_theme_has_no_relationship_never_panics_or_hangs(seed in any::<u64>()) {
        let mut g = Gen(seed | 1);
        let names = orphan_seed_package().part_names();
        let mut chosen = vec![format!("{DIR}/theme/theme1.xml"), names[g.below(names.len())].clone()];
        if g.below(3) == 0 {
            chosen.push(names[g.below(names.len())].clone());
        }
        let bytes = orphan_seed_package().map_parts(|name, part| {
            if chosen.iter().any(|c| c == name) {
                let mut inner = Gen(g.next() | 1);
                mutate(&mut inner, part);
            }
        }).build();
        check(&bytes);
    }

    /// The same, read in a pool of any size.
    #[test]
    fn mutated_orphan_theme_read_in_a_pool_of_any_size_makes_no_tree_the_pool_does_not_hold(seed in any::<u64>(), pool in 0u64..600_000) {
        let mut g = Gen(seed | 1);
        let bytes = orphan_seed_package().map_parts(|name, part| {
            if *name == format!("{DIR}/theme/theme1.xml") {
                let mut inner = Gen(g.next() | 1);
                mutate(&mut inner, part);
            }
        }).build();
        check_pool(&bytes, pool);
    }

    /// A package that carries the data of the other format, one of its parts mutated (the solution part, its relationships, a part of DiaCraft's own, the types) and packed
    /// again with right checksums: it reads, or is refused, never panics or hangs, reads the same twice, and what reads is written again.
    #[test]
    fn mutated_carried_data_in_a_package_with_right_checksums_never_panics_or_hangs(seed in any::<u64>()) {
        check(&crate::carry::mutated_carried(seed));
    }

    /// The same, read in a pool of any size: a read or a refusal for memory in words, and no tree the pool did not hold.
    #[test]
    fn mutated_carried_data_read_in_a_pool_of_any_size_makes_no_tree_the_pool_does_not_hold(seed in any::<u64>(), pool in 0u64..600_000) {
        check_pool(&crate::carry::mutated_carried(seed), pool);
    }

    /// The archive's own bytes, mutated: nearly always the container refuses, and it must say so and not fail.
    #[test]
    fn mutated_archive_bytes_never_panic_or_hang(seed in any::<u64>()) {
        let mut g = Gen(seed | 1);
        let mut bytes = seed_package().build();
        mutate(&mut g, &mut bytes);
        check(&bytes);
    }

    /// Anything at all.
    #[test]
    fn arbitrary_bytes_are_never_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
        check(&bytes);
    }

    /// Any model, written by the model writer: a package that reads, or a refusal in words, and the same bytes twice. The characters a part cannot hold, the names no part may have and the
    /// ids that repeat are in the models the strategy makes.
    #[test]
    fn any_model_is_written_by_the_model_writer_or_refused_in_words(doc in diacraft_testkit::model::any_document(diacraft_testkit::model::ModelSize::Small)) {
        let mut doc = doc;
        doc.vsdx = Some(std::sync::Arc::new(diacraft_doc::VsdxHeld::default()));
        model_writer_holds(&doc, None);
    }

    /// Any sheet edit of rows and sections of the seed package, and a second one after it (the property of `vsdx_roundtrip`'s kind 5): each save is a package that reads or a
    /// refusal in words that is not the writer's own check and says nothing of unknown nodes.
    #[test]
    fn any_edit_of_the_rows_and_sections_of_the_seed_package_is_saved_twice_or_refused_in_words(data in proptest::collection::vec(any::<u8>(), 0..8)) {
        let bytes = seed_package().build();
        let (doc, _) = crate::read_ok(&bytes);
        if let Err(why) = fuzz_edits::save_after_edit(&bytes, &doc, &data) {
            panic!("{why}");
        }
    }
}

/// The edits and the two saves of the fuzz target `vsdx_roundtrip` (kind 5), the very source it is built from.
#[path = "../../fuzz/src/edits.rs"]
mod fuzz_edits;

#[test]
fn the_orphan_seed_reads_its_theme_and_says_it_had_no_relationship() {
    let (doc, report) = crate::read_ok(&orphan_seed_package().build());
    assert_eq!((doc.pages.len(), doc.masters.len(), doc.themes.len()), (2, 1, 1));
    assert_eq!(report.warning_count("theme_without_relationship"), 1, "{:?}", report.warnings);
    assert_eq!(report.warnings.len(), 1, "that, and nothing else: {:?}", report.warnings);
}

#[test]
fn the_seed_package_reads_clean_so_the_mutations_start_from_something() {
    let (doc, report) = crate::read_ok(&seed_package().build());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!((doc.pages.len(), doc.masters.len(), doc.themes.len()), (2, 1, 1));
    assert!(report.stats.cells > 5 && report.stats.shapes >= 4);
}
